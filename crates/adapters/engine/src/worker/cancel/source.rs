//! A backup's source that a cancel cuts short.

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use rustic_core::{
    ErrorKind, ReadSource, ReadSourceEntry, ReadSourceOpen, RusticError, RusticResult,
};

use super::Cancel;

/// How often a size scan still running looks for the cancel.
const SIZE_POLL: Duration = Duration::from_millis(50);

const CANCELLED: &str = "the backup was cancelled";

/// A source that, once the cancel is asked for, lists nothing more, opens
/// nothing more, reads nothing more from a file already open, and gives up
/// the scan of its size. rustic then ends the backup as it ends any: it
/// saves the packs and the index of what it read.
pub(crate) struct CancellableSource<S> {
    inner: Arc<S>,
    cancel: Cancel,
}

impl<S> CancellableSource<S> {
    pub(crate) fn new(inner: S, cancel: Cancel) -> Self {
        Self {
            inner: Arc::new(inner),
            cancel,
        }
    }
}

impl<S: ReadSource + 'static> ReadSource for CancellableSource<S> {
    type Open = CancellableOpen<S::Open>;
    type Iter = CancellableEntries<S::Iter>;

    /// The inner source's size, scanned on a thread of its own: rustic
    /// waits for the scan before it saves the index, and a scan of a large
    /// source must not hold a cancelled backup's wind-down. A scan given up
    /// is left to run until the worker exits; the size only feeds progress.
    fn size(&self) -> RusticResult<Option<u64>> {
        let inner = Arc::clone(&self.inner);
        let (sender, size) = mpsc::channel();
        let scan = std::thread::Builder::new()
            .name("source-size".to_owned())
            .spawn(move || {
                let _ = sender.send(inner.size());
            });
        if scan.is_err() {
            return Ok(None);
        }
        loop {
            match size.recv_timeout(SIZE_POLL) {
                Ok(size) => return size,
                Err(RecvTimeoutError::Timeout) if !self.cancel.is_requested() => {}
                Err(_) => return Ok(None),
            }
        }
    }

    fn entries(&self) -> Self::Iter {
        CancellableEntries {
            inner: self.inner.entries(),
            cancel: self.cancel.clone(),
        }
    }
}

/// The entries of a [`CancellableSource`]: none once the cancel is asked for.
pub(crate) struct CancellableEntries<I> {
    inner: I,
    cancel: Cancel,
}

impl<I, O> Iterator for CancellableEntries<I>
where
    I: Iterator<Item = RusticResult<ReadSourceEntry<O>>>,
{
    type Item = RusticResult<ReadSourceEntry<CancellableOpen<O>>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel.is_requested() {
            return None;
        }
        let entry = self.inner.next()?;
        Some(entry.map(|entry| ReadSourceEntry {
            path: entry.path,
            node: entry.node,
            open: entry.open.map(|open| CancellableOpen {
                inner: open,
                cancel: self.cancel.clone(),
            }),
        }))
    }
}

/// How to open a file of a [`CancellableSource`].
pub(crate) struct CancellableOpen<O> {
    inner: O,
    cancel: Cancel,
}

impl<O: ReadSourceOpen> ReadSourceOpen for CancellableOpen<O> {
    type Reader = CancellableReader<O::Reader>;

    fn open(self) -> RusticResult<Self::Reader> {
        if self.cancel.is_requested() {
            return Err(RusticError::new(ErrorKind::InputOutput, CANCELLED));
        }
        Ok(CancellableReader {
            inner: self.inner.open()?,
            cancel: self.cancel,
        })
    }
}

/// A file of a [`CancellableSource`], which fails its next read once the
/// cancel is asked for. rustic then drops the file from the backup; what
/// it had chunked of it is saved and indexed with the rest.
pub(crate) struct CancellableReader<R> {
    inner: R,
    cancel: Cancel,
}

impl<R: Read> Read for CancellableReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_requested() {
            return Err(io::Error::other(CANCELLED));
        }
        self.inner.read(buf)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use rustic_core::{
        Excludes, LocalSource, LocalSourceFilterOptions, LocalSourceSaveOptions, ReadSourceEntry,
        ReadSourceOpen,
    };

    use super::*;

    fn local(dir: &Path) -> LocalSource {
        LocalSource::new(
            LocalSourceSaveOptions::default(),
            &Excludes::default(),
            &LocalSourceFilterOptions::default(),
            &[dir],
        )
        .unwrap()
    }

    /// A folder of three files of 1 KiB each.
    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.bin", "b.bin", "c.bin"] {
            std::fs::write(dir.path().join(name), [7_u8; 1024]).unwrap();
        }
        dir
    }

    fn paths<O>(entries: impl Iterator<Item = RusticResult<ReadSourceEntry<O>>>) -> Vec<PathBuf> {
        entries.map(|entry| entry.unwrap().path).collect()
    }

    /// The first entry of `source` that is a file, to open.
    fn first_file<S: ReadSource>(source: &S) -> S::Open {
        source
            .entries()
            .find_map(|entry| entry.unwrap().open)
            .expect("a file in the folder")
    }

    #[test]
    fn until_the_cancel_the_source_lists_and_reads_as_its_own() {
        let dir = folder();
        let source = CancellableSource::new(local(dir.path()), Cancel::default());
        assert_eq!(paths(source.entries()), paths(local(dir.path()).entries()));
        let mut content = Vec::new();
        first_file(&source)
            .open()
            .unwrap()
            .read_to_end(&mut content)
            .unwrap();
        assert_eq!(content, [7_u8; 1024]);
        assert_eq!(source.size().unwrap(), local(dir.path()).size().unwrap());
    }

    #[test]
    fn a_cancel_ends_the_listing_where_it_stands() {
        let dir = folder();
        let cancel = Cancel::default();
        let source = CancellableSource::new(local(dir.path()), cancel.clone());
        let mut entries = source.entries();
        assert!(entries.next().is_some());
        cancel.request();
        assert!(entries.next().is_none());
        assert!(source.entries().next().is_none());
    }

    #[test]
    fn a_file_open_when_the_cancel_comes_is_read_no_further() {
        let dir = folder();
        let cancel = Cancel::default();
        let source = CancellableSource::new(local(dir.path()), cancel.clone());
        let mut reader = first_file(&source).open().unwrap();
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte).unwrap();
        cancel.request();
        assert!(reader.read(&mut byte).is_err());
    }

    #[test]
    fn no_file_is_opened_after_the_cancel() {
        let dir = folder();
        let cancel = Cancel::default();
        let source = CancellableSource::new(local(dir.path()), cancel.clone());
        let file = first_file(&source);
        cancel.request();
        assert!(file.open().is_err());
    }

    /// A source whose size scan lasts until it is released.
    struct SlowToSize {
        inner: LocalSource,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl ReadSource for SlowToSize {
        type Open = <LocalSource as ReadSource>::Open;
        type Iter = <LocalSource as ReadSource>::Iter;

        fn size(&self) -> RusticResult<Option<u64>> {
            let _ = self
                .release
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .recv();
            self.inner.size()
        }

        fn entries(&self) -> Self::Iter {
            self.inner.entries()
        }
    }

    /// rustic waits for the size scan before it saves the index, so a scan
    /// of a large source must not hold the wind-down.
    #[test]
    fn a_cancel_gives_up_a_size_scan_that_is_still_running() {
        let dir = folder();
        let cancel = Cancel::default();
        let (release, released) = mpsc::channel();
        let source = CancellableSource::new(
            SlowToSize {
                inner: local(dir.path()),
                release: Mutex::new(released),
            },
            cancel.clone(),
        );
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            cancel.request();
            // A scan that waits for its end anyway ends, with its size,
            // rather than hanging the test.
            std::thread::sleep(Duration::from_secs(5));
            let _ = release.send(());
        });
        assert_eq!(source.size().unwrap(), None);
    }
}
