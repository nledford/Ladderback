//! A repository backend that refuses a snapshot once the job is canceled,
//! by the daemon or by its deadline.

use std::sync::{Arc, Mutex, PoisonError};

use bytes::Bytes;
use rustic_core::{
    BytesList, ErrorKind, FileType, Id, ReadBackend, RepositoryBackends, RusticError, RusticResult,
    WriteBackend,
};

use super::Cancel;
use crate::job::DeleteAnswer;

/// `backends`, refusing to write a snapshot once the cancel is asked for. A
/// backup canceled, or past its deadline, winds down through rustic's own
/// end of a backup, which saves the index of its packs and then the
/// snapshot: this is what keeps the snapshot out. rustic writes a snapshot
/// to the hot half first and to the cold half only once the hot half has
/// it, so only the half written first refuses it: a cancel between the two
/// writes can't leave the snapshot in one half alone. Everything else is
/// the halves' own.
pub(crate) fn refusing_snapshots_once_canceled(
    backends: &RepositoryBackends,
    cancel: &Cancel,
) -> RepositoryBackends {
    match backends.repo_hot() {
        Some(hot) => RepositoryBackends::new(backends.repository(), Some(refusing(hot, cancel))),
        None => RepositoryBackends::new(refusing(backends.repository(), cancel), None),
    }
}

fn refusing(inner: Arc<dyn WriteBackend>, cancel: &Cancel) -> Arc<dyn WriteBackend> {
    Arc::new(NoSnapshotOnceCanceled {
        inner,
        cancel: cancel.clone(),
    })
}

struct NoSnapshotOnceCanceled {
    inner: Arc<dyn WriteBackend>,
    cancel: Cancel,
}

impl ReadBackend for NoSnapshotOnceCanceled {
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

impl WriteBackend for NoSnapshotOnceCanceled {
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

/// A prune's question to the daemon, asked once, at its first removal: rustic
/// warms up and repacks before it removes anything, which can take hours, so
/// asking any earlier would let it delete under a lease checked long before
/// (I1; §16, decision 161).
pub(crate) struct DeleteGate {
    state: Mutex<Gate>,
}

enum Gate {
    Unasked(Box<dyn FnOnce() -> Option<DeleteAnswer> + Send>),
    Heard(Option<DeleteAnswer>),
}

impl DeleteGate {
    /// A gate that asks with `ask`, which answers `None` when no answer came.
    pub(crate) fn new(ask: impl FnOnce() -> Option<DeleteAnswer> + Send + 'static) -> Self {
        Self {
            state: Mutex::new(Gate::Unasked(Box::new(ask))),
        }
    }

    /// What the daemon answered: `None` until a removal asked, then the
    /// answer, `None` inside when none came.
    pub(crate) fn heard(&self) -> Option<Option<DeleteAnswer>> {
        match &*self.state.lock().unwrap_or_else(PoisonError::into_inner) {
            Gate::Unasked(_) => None,
            Gate::Heard(answer) => Some(*answer),
        }
    }

    /// Whether a removal may go on: asks on the first call, holding the
    /// lock so removals on other threads wait for the one answer.
    pub(crate) fn admits_removal(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = match std::mem::replace(&mut *state, Gate::Heard(None)) {
            Gate::Unasked(ask) => ask(),
            Gate::Heard(answer) => answer,
        };
        *state = Gate::Heard(answer);
        answer == Some(DeleteAnswer::Delete)
    }
}

/// `backends`, each half asking `gate` before its first removal and
/// removing nothing unless the answer is to delete.
pub(crate) fn asking_before_removal(
    backends: &RepositoryBackends,
    gate: &Arc<DeleteGate>,
) -> RepositoryBackends {
    let asking = |inner: Arc<dyn WriteBackend>| -> Arc<dyn WriteBackend> {
        Arc::new(AskingBeforeRemoval {
            inner,
            gate: Arc::clone(gate),
        })
    };
    RepositoryBackends::new(
        asking(backends.repository()),
        backends.repo_hot().map(asking),
    )
}

struct AskingBeforeRemoval {
    inner: Arc<dyn WriteBackend>,
    gate: Arc<DeleteGate>,
}

impl ReadBackend for AskingBeforeRemoval {
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

impl WriteBackend for AskingBeforeRemoval {
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
        self.inner.write_bytes(tpe, id, cacheable, content)
    }

    fn remove(&self, tpe: FileType, id: &Id, cacheable: bool) -> RusticResult<()> {
        if !self.gate.admits_removal() {
            return Err(RusticError::new(
                ErrorKind::Other,
                "the daemon didn't let this prune delete: nothing is removed",
            ));
        }
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
    fn once_canceled_no_snapshot_is_written() {
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

    /// What a canceled backup still writes as it winds down: its last
    /// packs, and the index of them all.
    #[test]
    fn once_canceled_packs_and_index_files_are_still_written() {
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
        let backends = refusing_snapshots_once_canceled(
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
        let backends = refusing_snapshots_once_canceled(
            &RepositoryBackends::new(local(dir.path()), None),
            &cancel,
        );
        cancel.request();
        assert!(write_snapshot(&backends.repository()).is_err());
    }
}

#[cfg(test)]
mod gate_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rustic_backend::BackendOptions;

    use super::*;

    fn local(dir: &std::path::Path) -> RepositoryBackends {
        let backends = BackendOptions::default()
            .repository(dir.to_string_lossy().into_owned())
            .to_backends()
            .unwrap();
        backends.repository().create().unwrap();
        backends
    }

    fn id(n: u8) -> Id {
        format!("{n:064x}").parse().unwrap()
    }

    fn write_pack(backend: &Arc<dyn WriteBackend>, n: u8) {
        backend
            .write_bytes(FileType::Pack, &id(n), false, b"pack".to_vec().into())
            .unwrap();
    }

    /// A gate answering `answer`, and how many times it was asked.
    fn counting(answer: Option<DeleteAnswer>) -> (Arc<DeleteGate>, Arc<AtomicUsize>) {
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&asked);
        let gate = DeleteGate::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            answer
        });
        (Arc::new(gate), asked)
    }

    /// rustic warms up, reads, writes and lists before it removes anything:
    /// none of that asks, so the answer is as fresh as the first removal.
    #[test]
    fn nothing_is_asked_before_the_first_removal() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, asked) = counting(Some(DeleteAnswer::Delete));
        let repository = asking_before_removal(&local(dir.path()), &gate).repository();
        write_pack(&repository, 1);
        write_pack(&repository, 2);
        repository.list(FileType::Pack).unwrap();
        repository.read_full(FileType::Pack, &id(1)).unwrap();
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        assert_eq!(gate.heard(), None);
        repository.remove(FileType::Pack, &id(1), false).unwrap();
        repository.remove(FileType::Pack, &id(2), false).unwrap();
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        assert_eq!(gate.heard(), Some(Some(DeleteAnswer::Delete)));
        assert!(repository.list(FileType::Pack).unwrap().is_empty());
    }

    #[test]
    fn a_removal_refused_or_unanswered_removes_nothing_and_asks_once() {
        for answer in [Some(DeleteAnswer::DeleteNothing), None] {
            let dir = tempfile::tempdir().unwrap();
            let (gate, asked) = counting(answer);
            let repository = asking_before_removal(&local(dir.path()), &gate).repository();
            write_pack(&repository, 1);
            assert!(repository.remove(FileType::Pack, &id(1), false).is_err());
            assert!(repository.remove(FileType::Pack, &id(1), false).is_err());
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{answer:?}");
            assert_eq!(gate.heard(), Some(answer));
            assert_eq!(repository.list(FileType::Pack).unwrap(), [id(1)]);
        }
    }

    /// rustic removes from many threads; they wait on one question.
    #[test]
    fn removals_on_many_threads_ask_once() {
        let dir = tempfile::tempdir().unwrap();
        let (gate, asked) = counting(Some(DeleteAnswer::Delete));
        let repository = asking_before_removal(&local(dir.path()), &gate).repository();
        for n in 1..=8 {
            write_pack(&repository, n);
        }
        std::thread::scope(|scope| {
            for n in 1..=8 {
                let repository = &repository;
                scope.spawn(move || repository.remove(FileType::Pack, &id(n), false).unwrap());
            }
        });
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    /// A repository with a hot half removes from both; one question covers
    /// the two.
    #[test]
    fn both_halves_share_the_one_question() {
        let cold = tempfile::tempdir().unwrap();
        let hot = tempfile::tempdir().unwrap();
        let backends = RepositoryBackends::new(
            local(cold.path()).repository(),
            Some(local(hot.path()).repository()),
        );
        let (gate, asked) = counting(Some(DeleteAnswer::DeleteNothing));
        let gated = asking_before_removal(&backends, &gate);
        let hot = gated.repo_hot().unwrap();
        write_pack(&gated.repository(), 1);
        write_pack(&hot, 1);
        assert!(
            gated
                .repository()
                .remove(FileType::Pack, &id(1), false)
                .is_err()
        );
        assert!(hot.remove(FileType::Pack, &id(1), false).is_err());
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }
}
