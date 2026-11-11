//! A cancel: the daemon's SIGTERM (§4.1). A backup that is saving data
//! winds down on it rather than being cut off: it stops reading its source,
//! lets rustic save the index of the packs it wrote, and writes no snapshot.

mod backend;
mod source;

pub(crate) use backend::refusing_snapshots_once_cancelled;
pub(crate) use source::CancellableSource;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What the SIGTERM handler does once it has asked for the cancel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterCancel {
    /// Nothing is being saved: the worker exits at once.
    ExitNow,
    /// A backup is saving data: it winds down and ends the worker itself.
    WindDown,
}

/// The worker's cancel, shared by the SIGTERM handler, the job, its source
/// and its backends.
#[derive(Clone, Default)]
pub(crate) struct Cancel(Arc<Shared>);

#[derive(Default)]
struct Shared {
    requested: AtomicBool,
    /// Whether a backup is archiving. Asking for the cancel and starting or
    /// ending the archiving take this lock, so the handler and the job
    /// agree on which of them ends the worker.
    saving: Mutex<bool>,
}

impl Cancel {
    /// Whether the cancel was asked for. It is never taken back.
    pub(crate) fn is_requested(&self) -> bool {
        self.0.requested.load(Ordering::SeqCst)
    }

    /// Asks for the cancel, and says what the handler does next.
    pub(crate) fn request(&self) -> AfterCancel {
        let saving = self.lock_saving();
        self.0.requested.store(true, Ordering::SeqCst);
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
}
