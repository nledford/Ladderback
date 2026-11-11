//! A backup's source that a cancel cuts short.

use std::sync::Arc;

use rustic_core::{ReadSource, RusticResult};

use super::Cancel;

/// A source that, once the cancel is asked for, lists nothing more, opens
/// nothing more, reads nothing more from a file already open, and gives up
/// the scan of its size.
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
    type Open = S::Open;
    type Iter = S::Iter;

    fn size(&self) -> RusticResult<Option<u64>> {
        todo!()
    }

    fn entries(&self) -> Self::Iter {
        todo!()
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
