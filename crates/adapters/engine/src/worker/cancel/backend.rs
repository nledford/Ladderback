//! A repository backend that refuses a snapshot once the job is cancelled,
//! by the daemon or by its deadline.

use std::sync::Arc;

use bytes::Bytes;
use rustic_core::{
    BytesList, ErrorKind, FileType, Id, ReadBackend, RepositoryBackends, RusticError, RusticResult,
    WriteBackend,
};

use super::Cancel;

/// `backends`, refusing to write a snapshot once the cancel is asked for. A
/// backup cancelled, or past its deadline, winds down through rustic's own
/// end of a backup, which saves the index of its packs and then the
/// snapshot: this is what keeps the snapshot out. rustic writes a snapshot
/// to the hot half first and to the cold half only once the hot half has
/// it, so only the half written first refuses it: a cancel between the two
/// writes can't leave the snapshot in one half alone. Everything else is
/// the halves' own.
pub(crate) fn refusing_snapshots_once_cancelled(
    backends: &RepositoryBackends,
    cancel: &Cancel,
) -> RepositoryBackends {
    match backends.repo_hot() {
        Some(hot) => RepositoryBackends::new(backends.repository(), Some(refusing(hot, cancel))),
        None => RepositoryBackends::new(refusing(backends.repository(), cancel), None),
    }
}

fn refusing(inner: Arc<dyn WriteBackend>, cancel: &Cancel) -> Arc<dyn WriteBackend> {
    Arc::new(NoSnapshotOnceCancelled {
        inner,
        cancel: cancel.clone(),
    })
}

struct NoSnapshotOnceCancelled {
    inner: Arc<dyn WriteBackend>,
    cancel: Cancel,
}

impl ReadBackend for NoSnapshotOnceCancelled {
    fn location(&self) -> String {
        self.inner.location()
    }

    fn list_with_size(&self, tpe: FileType) -> RusticResult<Vec<(Id, u32)>> {
        self.inner.list_with_size(tpe)
    }

    fn list(&self, tpe: FileType) -> RusticResult<Vec<Id>> {
        self.inner.list(tpe)
    }

    fn read_full(&self, tpe: FileType, id: &Id) -> RusticResult<Bytes> {
        self.inner.read_full(tpe, id)
    }

    fn read_partial(
        &self,
        tpe: FileType,
        id: &Id,
        cacheable: bool,
        offset: u32,
        length: u32,
    ) -> RusticResult<Bytes> {
        self.inner.read_partial(tpe, id, cacheable, offset, length)
    }

    fn warmup_path(&self, tpe: FileType, id: &Id) -> String {
        self.inner.warmup_path(tpe, id)
    }

    fn needs_warm_up(&self) -> bool {
        self.inner.needs_warm_up()
    }

    fn warm_up(&self, tpe: FileType, id: &Id) -> RusticResult<()> {
        self.inner.warm_up(tpe, id)
    }
}

impl WriteBackend for NoSnapshotOnceCancelled {
    fn create(&self) -> RusticResult<()> {
        self.inner.create()
    }

    fn write_bytes(
        &self,
        tpe: FileType,
        id: &Id,
        cacheable: bool,
        content: BytesList,
    ) -> RusticResult<()> {
        if tpe == FileType::Snapshot && !self.cancel.admit_snapshot() {
            return Err(RusticError::new(
                ErrorKind::Other,
                "the backup was stopped: its snapshot is not written",
            ));
        }
        self.inner.write_bytes(tpe, id, cacheable, content)
    }

    fn remove(&self, tpe: FileType, id: &Id, cacheable: bool) -> RusticResult<()> {
        self.inner.remove(tpe, id, cacheable)
    }
}

#[cfg(test)]
mod tests {
    use rustic_backend::BackendOptions;
    use rustic_core::{FileType, Id};

    use super::*;

    fn local(dir: &std::path::Path) -> Arc<dyn WriteBackend> {
        let backend = BackendOptions::default()
            .repository(dir.to_string_lossy().into_owned())
            .to_backends()
            .unwrap()
            .repository();
        backend.create().unwrap();
        backend
    }

    fn id(n: u8) -> Id {
        format!("{n:064x}").parse().unwrap()
    }

    fn write_snapshot(backend: &Arc<dyn WriteBackend>) -> RusticResult<()> {
        backend.write_bytes(
            FileType::Snapshot,
            &id(1),
            false,
            b"snapshot".to_vec().into(),
        )
    }

    #[test]
    fn a_snapshot_is_written_until_the_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let inner = local(dir.path());
        let backend = refusing(Arc::clone(&inner), &Cancel::default());
        backend
            .write_bytes(
                FileType::Snapshot,
                &id(1),
                false,
                b"snapshot".to_vec().into(),
            )
            .unwrap();
        assert_eq!(inner.list(FileType::Snapshot).unwrap(), [id(1)]);
    }

    #[test]
    fn once_cancelled_no_snapshot_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let inner = local(dir.path());
        let cancel = Cancel::default();
        let backend = refusing(Arc::clone(&inner), &cancel);
        cancel.request();
        assert!(
            backend
                .write_bytes(
                    FileType::Snapshot,
                    &id(1),
                    false,
                    b"snapshot".to_vec().into()
                )
                .is_err()
        );
        assert!(inner.list(FileType::Snapshot).unwrap().is_empty());
    }

    /// I3: from the deadline on, no snapshot write starts.
    #[test]
    fn once_the_deadline_passed_no_snapshot_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let inner = local(dir.path());
        let cancel = Cancel::default();
        let backend = refusing(Arc::clone(&inner), &cancel);
        cancel.pass_deadline();
        assert!(write_snapshot(&backend).is_err());
        assert!(inner.list(FileType::Snapshot).unwrap().is_empty());
    }

    /// What a cancelled backup still writes as it winds down: its last
    /// packs, and the index of them all.
    #[test]
    fn once_cancelled_packs_and_index_files_are_still_written() {
        let dir = tempfile::tempdir().unwrap();
        let inner = local(dir.path());
        let cancel = Cancel::default();
        let backend = refusing(Arc::clone(&inner), &cancel);
        cancel.request();
        backend
            .write_bytes(FileType::Pack, &id(2), false, b"pack".to_vec().into())
            .unwrap();
        backend
            .write_bytes(FileType::Index, &id(3), true, b"index".to_vec().into())
            .unwrap();
        assert_eq!(inner.list(FileType::Pack).unwrap(), [id(2)]);
        assert_eq!(inner.list(FileType::Index).unwrap(), [id(3)]);
        assert_eq!(
            &backend.read_full(FileType::Pack, &id(2)).unwrap()[..],
            b"pack"
        );
    }

    /// rustic writes the cold half's copy only once the hot half has it: a
    /// snapshot the hot half took before the cancel reaches the cold half
    /// after it, and one the hot half refused never gets there.
    #[test]
    fn in_a_hot_cold_repository_only_the_hot_half_refuses_a_snapshot() {
        let (cold, hot) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let cancel = Cancel::default();
        let backends = refusing_snapshots_once_cancelled(
            &RepositoryBackends::new(local(cold.path()), Some(local(hot.path()))),
            &cancel,
        );
        cancel.request();
        assert!(write_snapshot(&backends.repo_hot().unwrap()).is_err());
        write_snapshot(&backends.repository()).unwrap();
    }

    #[test]
    fn without_a_hot_half_the_repository_refuses_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = Cancel::default();
        let backends = refusing_snapshots_once_cancelled(
            &RepositoryBackends::new(local(dir.path()), None),
            &cancel,
        );
        cancel.request();
        assert!(write_snapshot(&backends.repository()).is_err());
    }
}
