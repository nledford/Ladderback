//! The daemon's log, masked while the daemon opens a repository itself.
//!
//! rustic, its backends and OpenDAL report through the `log` crate. A worker
//! masks every record of theirs (`worker::logging`); the daemon opens
//! repositories in-process too, to evaluate retention (`crate::retention`),
//! and their records then reach the daemon's own logger. rustic names a
//! repository by its location (`repository rclone::s3,…: password is
//! correct.`), and an `rclone:` location holds rclone's settings, credentials
//! among them; the rclone it starts writes to rustic's log what it was asked
//! to serve (#330). So while an open runs, [`Masked`] masks every record with
//! the secrets of that open, from before the open starts anything until its
//! backends, the rclone they started and the thread reading its stderr are
//! gone. OpenDAL's runtime outlives them, shared by the whole process: what
//! a task of it might log after that is not masked, and no record of the
//! open's is known to come so late.
//!
//! Every record is masked, whatever crate made it: the daemon's own, rustic's,
//! and those of the crates under rustic (OpenDAL, the HTTP client), which
//! share no target. With two opens running, a record is masked as one
//! redactor of the secrets of both would mask it: more, never less (I4,
//! #287).

use std::cell::Cell;
use std::sync::{Arc, Mutex, PoisonError};

use domain::secrets::Redactor;
use log::{Level, Log, Metadata, Record};

/// The redactor of every open running.
static OPEN: Mutex<Vec<Arc<Redactor>>> = Mutex::new(Vec::new());

thread_local! {
    /// The warnings counted so far while this thread walks for a plan dry
    /// run, or none when it doesn't ([`unlogged`]).
    static WALKING: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Runs `walk` with every record this thread makes held back from the log,
/// and answers its output with the warnings and errors among them counted.
/// A plan dry run's walk meets paths, and its records name them (rustic's
/// xattr warning, the `ignore` crate's debug record of each entry it
/// leaves out), while the daemon's log is served at the `read` scope. It
/// counts only where [`Masked`] is the process's logger, as the daemon's
/// is; the thread logs as before once `walk` returns or panics.
pub(crate) fn unlogged<T>(walk: impl FnOnce() -> T) -> (T, u64) {
    /// Puts back what the thread counted before, when dropped.
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            WALKING.set(self.0);
        }
    }
    let _restore = Restore(WALKING.replace(Some(0)));
    let done = walk();
    (done, WALKING.get().unwrap_or(0))
}

/// Whether this thread walks for a dry run, so a record of `level` is held
/// back: counted when it is a warning or an error.
fn held_back(level: Level) -> bool {
    match WALKING.get() {
        Some(counted) => {
            if level <= Level::Warn {
                WALKING.set(Some(counted.saturating_add(1)));
            }
            true
        }
        None => false,
    }
}

/// A logger whose records are masked with the secrets of every repository
/// the process has open in-process, as they are when it logs them.
pub struct Masked<L> {
    inner: L,
}

impl<L: Log> Masked<L> {
    /// `inner`, its records masked while an open runs.
    pub fn new(inner: L) -> Self {
        Self { inner }
    }
}

impl<L: Log> Log for Masked<L> {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if held_back(record.level()) || !self.inner.enabled(record.metadata()) {
            return;
        }
        // Copied out, so the lock isn't held while the record is written.
        let open: Vec<Arc<Redactor>> = OPEN.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if open.is_empty() {
            self.inner.log(record);
            return;
        }
        // As one redactor of every open's secrets: masked with each in turn,
        // one's mask could break up another's secret that holds it (#287).
        let masked =
            Redactor::redact_together(open.iter().map(Arc::as_ref), &record.args().to_string());
        // The record as it was made, but for its message: its key-values
        // are left out, since they are not masked.
        self.inner.log(
            &Record::builder()
                .metadata(record.metadata().clone())
                .args(format_args!("{masked}"))
                .module_path(record.module_path())
                .file(record.file())
                .line(record.line())
                .build(),
        );
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// An open running: while it is held, every record [`Masked`] logs is
/// masked with its redactor. Hold it from before the open starts anything
/// until its backends are dropped.
pub(crate) struct Masking {
    redactor: Arc<Redactor>,
}

impl Masking {
    /// Masks every record with `redactor` from now on, until this is dropped.
    pub(crate) fn of(redactor: Redactor) -> Self {
        let redactor = Arc::new(redactor);
        OPEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&redactor));
        Self { redactor }
    }

    /// The redactor every record is masked with while this is held.
    pub(crate) fn redactor(&self) -> &Redactor {
        &self.redactor
    }
}

impl Drop for Masking {
    fn drop(&mut self) {
        OPEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|open| !Arc::ptr_eq(open, &self.redactor));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use domain::secrets::{Redactor, SecretValue};
    use log::{Level, Log, Metadata, Record};

    use super::{Masked, Masking};

    /// What a logger was given, as (level, target, message).
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<(Level, String, String)>>>);

    impl Kept {
        fn messages(&self) -> Vec<(Level, String, String)> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    /// Keeps every record up to `info`, as the daemon's logger logs them.
    impl Log for Kept {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() <= Level::Info
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                self.0.lock().unwrap_or_else(PoisonError::into_inner).push((
                    record.level(),
                    record.target().to_owned(),
                    record.args().to_string(),
                ));
            }
        }

        fn flush(&self) {}
    }

    fn redactor(secret: &str) -> Redactor {
        Redactor::new(&[SecretValue::new(secret)])
    }

    fn log(logger: &Masked<Kept>, level: Level, target: &str, message: &str) {
        logger.log(
            &Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    // Each test has secrets of its own: an open another test holds masks
    // its own secrets only, which these texts don't hold.

    #[test]
    fn a_dry_run_walk_s_records_are_held_back_and_its_warnings_and_errors_counted() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let named = "Error reading xattrs for /srv/demo/CANARY-walked.txt, not excluding";
        let ((), counted) = super::unlogged(|| {
            for level in [Level::Error, Level::Warn, Level::Info, Level::Debug] {
                log(&logger, level, "rustic_core::backend::ignore", named);
            }
        });
        assert_eq!(counted, 2);
        assert_eq!(kept.messages(), []);
        let unwound =
            std::panic::catch_unwind(|| super::unlogged(|| -> u8 { panic!("the walk panicked") }));
        assert!(unwound.is_err());
        log(&logger, Level::Warn, "ladderback", "after the walk");
        assert_eq!(
            kept.messages(),
            [(
                Level::Warn,
                "ladderback".to_owned(),
                "after the walk".to_owned()
            )]
        );
    }

    #[test]
    fn while_an_open_runs_its_secrets_are_masked_in_every_record_whatever_its_crate() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let _open = Masking::of(redactor("CANARY-open-secret"));
        let targets = ["rustic_core::repository", "opendal::services", "ladderback"];
        for target in targets {
            log(
                &logger,
                Level::Info,
                target,
                "repository rclone::s3,secret_access_key=CANARY-open-secret:b: password is correct.",
            );
        }
        let messages = kept.messages();
        assert_eq!(
            messages
                .iter()
                .map(|(_, target, _)| target.as_str())
                .collect::<Vec<_>>(),
            targets,
            "{messages:?}"
        );
        for (level, target, message) in messages {
            assert_eq!(level, Level::Info, "{target}");
            assert_eq!(
                message, "repository rclone::s3,secret_access_key=***:b: password is correct.",
                "{target}"
            );
        }
    }

    #[test]
    fn with_two_opens_running_a_record_is_masked_with_the_secrets_of_both() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let _first = Masking::of(redactor("CANARY-first-open"));
        let _second = Masking::of(redactor("CANARY-second-open"));
        log(
            &logger,
            Level::Warn,
            "rustic_backend::rest",
            "CANARY-first-open and CANARY-second-open",
        );
        assert_eq!(kept.messages()[0].2, "*** and ***");
    }

    /// A password reused with a tail in two repositories open at once: the
    /// record is masked as one redactor of both would mask it, so nothing of
    /// the longer one is left beside the mask of the shorter (#287).
    #[test]
    fn with_two_opens_running_a_secret_of_one_holding_one_of_the_other_is_masked_whole() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let _first = Masking::of(redactor("CANARY-reused"));
        let _second = Masking::of(redactor("CANARY-reused-tail"));
        log(
            &logger,
            Level::Info,
            "rustic_core::repository",
            "password CANARY-reused-tail",
        );
        assert_eq!(kept.messages()[0].2, "password ***");
    }

    #[test]
    fn once_an_open_ends_its_secrets_are_no_longer_looked_for() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let ended = Masking::of(redactor("demo-ended-open"));
        let _running = Masking::of(redactor("CANARY-still-running"));
        drop(ended);
        log(
            &logger,
            Level::Info,
            "ladderback",
            "demo-ended-open beside CANARY-still-running",
        );
        assert_eq!(kept.messages()[0].2, "demo-ended-open beside ***");
    }

    #[test]
    fn with_no_open_running_a_record_is_logged_as_it_was_made() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        log(&logger, Level::Info, "ladderback", "demo-nothing-open");
        assert_eq!(
            kept.messages(),
            [(
                Level::Info,
                "ladderback".to_owned(),
                "demo-nothing-open".to_owned()
            )]
        );
    }

    #[test]
    fn a_level_the_daemon_does_not_log_stays_unlogged_while_an_open_runs() {
        let kept = Kept::default();
        let logger = Masked::new(kept.clone());
        let _open = Masking::of(redactor("CANARY-debug-open"));
        log(
            &logger,
            Level::Debug,
            "rustic_backend::rclone",
            "CANARY-debug-open",
        );
        assert!(logger.enabled(&Metadata::builder().level(Level::Info).build()));
        assert!(!logger.enabled(&Metadata::builder().level(Level::Debug).build()));
        assert_eq!(kept.messages(), []);
    }
}
