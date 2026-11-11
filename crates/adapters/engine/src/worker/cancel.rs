//! A cancel: the daemon's SIGTERM (§4.1). A backup that is saving data
//! winds down on it rather than being cut off: it stops reading its source,
//! lets rustic save the index of the packs it wrote, and writes no snapshot.

mod backend;
mod source;

pub(crate) use backend::refusing_snapshots_once_cancelled;
pub(crate) use source::CancellableSource;

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
pub(crate) struct Cancel;

impl Cancel {
    /// Whether the cancel was asked for. It is never taken back.
    pub(crate) fn is_requested(&self) -> bool {
        todo!()
    }

    /// Asks for the cancel, and says what the handler does next.
    pub(crate) fn request(&self) -> AfterCancel {
        todo!()
    }

    /// Runs `save`, a backup's archiving, as the part of the job a cancel
    /// winds down rather than cuts off. `None` when the cancel came before
    /// or during it: the job is cancelled, whatever `save` returned.
    pub(crate) fn saving<T>(&self, _save: impl FnOnce() -> T) -> Option<T> {
        todo!()
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
