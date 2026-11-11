//! A cancel: the daemon's SIGTERM, or the job's deadline (§4.1). A backup
//! that is saving data winds down on it rather than being cut off: it stops
//! reading its source, lets rustic save the index of the packs it wrote, and
//! starts no snapshot write. One that had begun finishes, in both halves of
//! a hot/cold repository.

mod backend;
mod source;

pub(crate) use backend::refusing_snapshots_once_cancelled;
pub(crate) use source::CancellableSource;

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What the SIGTERM handler, or the deadline watchdog, does once it has
/// asked for the cancel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterCancel {
    /// Nothing is being saved: the worker exits at once.
    ExitNow,
    /// A backup is saving data: it winds down and ends the worker itself.
    WindDown,
}

/// Why the cancel was asked for, which decides how the worker exits. The
/// first cause stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The daemon's SIGTERM: the operation ends cancelled.
    Cancel,
    /// The job's deadline passed (I3): the operation ends `deadline-passed`.
    Deadline,
}

impl Cause {
    fn code(self) -> u8 {
        match self {
            Self::Cancel => 1,
            Self::Deadline => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Cancel),
            2 => Some(Self::Deadline),
            _ => None,
        }
    }
}

/// The worker's cancel, shared by the SIGTERM handler, the deadline
/// watchdog, the job, its source and its backends.
#[derive(Clone, Default)]
pub(crate) struct Cancel(Arc<Shared>);

#[derive(Default)]
struct Shared {
    /// 0 until the cancel is asked for, then the code of its [`Cause`].
    cause: AtomicU8,
    /// Whether a backup is archiving, its snapshot write included. Asking
    /// for the cancel and starting or ending the archiving take this lock,
    /// so the handler, the watchdog and the job agree on which of them ends
    /// the worker.
    saving: Mutex<bool>,
}

impl Cancel {
    /// Whether the cancel was asked for. It is never taken back.
    pub(crate) fn is_requested(&self) -> bool {
        self.cause().is_some()
    }

    /// Why the cancel was asked for; `None` until it is.
    pub(crate) fn cause(&self) -> Option<Cause> {
        Cause::from_code(self.0.cause.load(Ordering::SeqCst))
    }

    /// Asks for the cancel, and says what the handler does next.
    pub(crate) fn request(&self) -> AfterCancel {
        self.ask(Cause::Cancel)
    }

    /// Asks for the cancel because the deadline passed (I3), and says what
    /// the watchdog does next. The snapshot is refused from here on. A
    /// backup that archives, or whose snapshot write began, winds down as a
    /// cancelled one does; otherwise the worker exits at once.
    pub(crate) fn pass_deadline(&self) -> AfterCancel {
        self.ask(Cause::Deadline)
    }

    /// Records `cause` unless another came first, and says what comes
    /// next. A backup that archives winds down, and that includes a
    /// snapshot write that began: rustic makes it to the cold half once the
    /// hot half has it, so ending the worker during it could leave the
    /// snapshot in one half alone (decision 50).
    fn ask(&self, cause: Cause) -> AfterCancel {
        let saving = self.lock_saving();
        let _ = self
            .0
            .cause
            .compare_exchange(0, cause.code(), Ordering::SeqCst, Ordering::SeqCst);
        if *saving {
            AfterCancel::WindDown
        } else {
            AfterCancel::ExitNow
        }
    }

    /// Runs `save`, a backup's archiving, as the part of the job a cancel
    /// winds down rather than cuts off. `None` when the cancel came before
    /// or during it: the job is cancelled, whatever `save` returned.
    pub(crate) fn saving<T>(&self, save: impl FnOnce() -> T) -> Option<T> {
        {
            let mut saving = self.lock_saving();
            if self.is_requested() {
                return None;
            }
            *saving = true;
        }
        let saved = save();
        let mut saving = self.lock_saving();
        *saving = false;
        (!self.is_requested()).then_some(saved)
    }

    /// Whether the snapshot may be written: not once the cancel is asked
    /// for. A snapshot write admitted before it runs to its end, within
    /// the archiving the cancel winds down.
    pub(crate) fn admit_snapshot(&self) -> bool {
        !self.is_requested()
    }

    fn lock_saving(&self) -> MutexGuard<'_, bool> {
        self.0.saving.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_cancel_while_nothing_is_saved_exits_at_once() {
        let cancel = Cancel::default();
        assert!(!cancel.is_requested());
        assert_eq!(cancel.request(), AfterCancel::ExitNow);
        assert!(cancel.is_requested());
    }

    #[test]
    fn a_cancel_while_a_backup_saves_lets_it_wind_down_and_cancels_the_job() {
        let cancel = Cancel::default();
        let saved = cancel.saving(|| {
            assert_eq!(cancel.request(), AfterCancel::WindDown);
            "saved"
        });
        assert_eq!(saved, None);
        assert!(cancel.is_requested());
    }

    #[test]
    fn a_backup_cancelled_before_it_saves_anything_does_not_start() {
        let cancel = Cancel::default();
        cancel.request();
        let ran = Cell::new(false);
        assert_eq!(cancel.saving(|| ran.set(true)), None);
        assert!(!ran.get());
    }

    #[test]
    fn a_backup_that_saves_uncancelled_keeps_its_result_and_a_later_cancel_exits_at_once() {
        let cancel = Cancel::default();
        assert_eq!(cancel.saving(|| "saved"), Some("saved"));
        assert_eq!(cancel.request(), AfterCancel::ExitNow);
    }

    #[test]
    fn a_deadline_while_a_backup_archives_lets_it_wind_down_and_ends_the_job() {
        let cancel = Cancel::default();
        let saved = cancel.saving(|| {
            assert_eq!(cancel.pass_deadline(), AfterCancel::WindDown);
            "saved"
        });
        assert_eq!(saved, None);
        assert!(cancel.is_requested());
        assert_eq!(cancel.cause(), Some(Cause::Deadline));
    }

    #[test]
    fn a_deadline_while_nothing_is_archived_exits_at_once() {
        let cancel = Cancel::default();
        assert_eq!(cancel.pass_deadline(), AfterCancel::ExitNow);
        assert!(cancel.is_requested());
        assert_eq!(cancel.cause(), Some(Cause::Deadline));
    }

    /// A deadline waits for a snapshot write that began, as a cancel does:
    /// the margin covers it (I3), and rustic makes it to the cold half once
    /// the hot half has it (decision 50).
    #[test]
    fn a_deadline_once_the_snapshot_write_began_lets_it_finish() {
        let cancel = Cancel::default();
        cancel.saving(|| {
            assert!(cancel.admit_snapshot());
            assert_eq!(cancel.pass_deadline(), AfterCancel::WindDown);
        });
    }

    /// A cancel waits for a snapshot write that began, which rustic makes
    /// to the cold half once the hot half has it (decision 50).
    #[test]
    fn a_cancel_once_the_snapshot_write_began_still_winds_down() {
        let cancel = Cancel::default();
        cancel.saving(|| {
            assert!(cancel.admit_snapshot());
            assert_eq!(cancel.request(), AfterCancel::WindDown);
        });
    }

    #[test]
    fn no_snapshot_write_is_admitted_once_the_cancel_or_the_deadline_came() {
        let cancelled = Cancel::default();
        assert!(cancelled.admit_snapshot());
        cancelled.request();
        assert!(!cancelled.admit_snapshot());
        let past = Cancel::default();
        past.pass_deadline();
        assert!(!past.admit_snapshot());
    }

    #[test]
    fn the_first_cause_stands() {
        let cancel = Cancel::default();
        assert_eq!(cancel.cause(), None);
        cancel.request();
        cancel.pass_deadline();
        assert_eq!(cancel.cause(), Some(Cause::Cancel));
        let past = Cancel::default();
        past.pass_deadline();
        past.request();
        assert_eq!(past.cause(), Some(Cause::Deadline));
    }
}
