//! The deadline watchdog (invariant I3, technical design §4.1).
//!
//! `rustic_core`'s `backup()` writes the snapshot itself and offers no hook
//! before that write, and nothing in it can be cancelled. So the worker
//! watches the wall clock from a thread of its own, and once the deadline has
//! passed it reports the failure and exits the process.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use api::ErrorCode;
use domain::Timestamp;
use domain::execution::deadline::Deadline;

use super::Process;
use super::events::{Ending, Events};
use crate::job::{WorkerEvent, exit_code};

/// The wall clock. Injected so a test can move it, say across a suspend.
pub(crate) type WallClock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// What to do once the failure is reported: exit the process, with this code.
pub(crate) type Terminate = Arc<dyn Fn(i32) + Send + Sync>;

/// How long the watchdog waits to write its failure before it exits without
/// one: not at all, it tries the stream once. A writer stuck on a full pipe
/// must not keep a worker past its deadline, and the exit code tells a
/// daemon that finds no failure line what happened.
const WRITE_PATIENCE: Duration = Duration::ZERO;

/// A running watchdog. Dropping it stops the watching.
pub(crate) struct Watchdog {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Starts watching `deadline`. The first look at the clock happens before
    /// this returns, so a deadline that has already passed stops the job
    /// before it does anything. After that the thread looks again at least
    /// every `interval`: `thread::sleep` doesn't count the time a machine
    /// spends suspended, so one long sleep to the deadline could overshoot it
    /// by hours, but a short sleep ends within `interval` of waking.
    pub(crate) fn start(
        deadline: Deadline,
        interval: Duration,
        events: Events,
        process: &Process,
    ) -> Self {
        // The first look is here, so a deadline that has passed already
        // stops the job before it does anything.
        if deadline.has_passed((process.clock)()) {
            expire(&events, process);
            return Self {
                stop: None,
                thread: None,
            };
        }
        let (stop, stopped) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("deadline-watchdog".to_owned())
            .spawn({
                let (events, process) = (events.clone(), process.clone());
                move || watch(deadline, interval, &events, &process, &stopped)
            });
        match spawned {
            Ok(thread) => Self {
                stop: Some(stop),
                thread: Some(thread),
            },
            Err(error) => {
                // A job that can't be held to its deadline must not run.
                end_process(
                    &events,
                    &process.terminate,
                    &WorkerEvent::Failed {
                        code: ErrorCode::OperationFailed,
                        args: Vec::new(),
                        detail: Some(format!("cannot start the deadline watchdog: {error}")),
                    },
                    1,
                );
                Self {
                    stop: None,
                    thread: None,
                }
            }
        }
    }
}

/// Looks at the clock until the deadline passes or the watchdog is dropped.
fn watch(
    deadline: Deadline,
    interval: Duration,
    events: &Events,
    process: &Process,
    stopped: &mpsc::Receiver<()>,
) {
    loop {
        let now = (process.clock)();
        if deadline.has_passed(now) {
            expire(events, process);
            return;
        }
        // Wake at the deadline or after `interval`, whichever comes first.
        let until_deadline =
            Duration::try_from(deadline.at().duration_since(now)).unwrap_or(interval);
        let wait = until_deadline.min(interval).max(Duration::from_millis(1));
        match stopped.recv_timeout(wait) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The deadline passed: report it as the last line and exit.
fn expire(events: &Events, process: &Process) {
    let failed = WorkerEvent::Failed {
        code: ErrorCode::DeadlinePassed,
        args: Vec::new(),
        detail: None,
    };
    end_process(
        events,
        &process.terminate,
        &failed,
        exit_code::DEADLINE_PASSED,
    );
}

/// Writes `last` as the stream's last line and exits with `code`. When a
/// result was written already the job is over and nothing is wrong, so the
/// worker is left to exit by itself.
fn end_process(events: &Events, terminate: &Terminate, last: &WorkerEvent, code: i32) {
    if events.end(Some(last), WRITE_PATIENCE) != Ending::AlreadyClosed {
        terminate(code);
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Disconnecting wakes the thread at once.
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            // A watchdog that panicked has nothing left to stop.
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::Instant;

    use domain::SignedDuration;

    use super::*;
    use crate::job::OpResult;
    use crate::worker::cancel::{AfterCancel, Cancel};
    use crate::worker::events::testing::{Capture, capture};

    const START: i64 = 1_800_000_000;

    struct Harness {
        events: Events,
        capture: Capture,
        seconds: Arc<AtomicI64>,
        terminated: mpsc::Receiver<i32>,
        /// Whether the snapshot was refused already when the worker was
        /// ended, once it was.
        refusing_at_exit: Arc<Mutex<Option<bool>>>,
        process: Process,
    }

    impl Harness {
        /// A wind-down that only the job's end stops.
        fn new() -> Self {
            Self::with_wind_down_limit(Duration::from_secs(3600))
        }

        fn with_wind_down_limit(wind_down_limit: Duration) -> Self {
            let (events, capture) = capture();
            let seconds = Arc::new(AtomicI64::new(START));
            let clock: WallClock = {
                let seconds = Arc::clone(&seconds);
                Arc::new(move || Timestamp::from_second(seconds.load(Ordering::SeqCst)).unwrap())
            };
            let cancel = Cancel::default();
            let refusing_at_exit = Arc::new(Mutex::new(None));
            let (tx, terminated) = mpsc::channel();
            let tx = Mutex::new(tx);
            let terminate: Terminate = {
                let (cancel, refusing_at_exit) = (cancel.clone(), Arc::clone(&refusing_at_exit));
                Arc::new(move |code| {
                    *refusing_at_exit.lock().unwrap() = Some(cancel.is_requested());
                    let _ = tx.lock().unwrap().send(code);
                })
            };
            Self {
                events,
                capture,
                seconds,
                terminated,
                refusing_at_exit,
                process: Process {
                    clock,
                    terminate,
                    cancel,
                    wind_down_limit,
                },
            }
        }

        fn now(&self) -> Timestamp {
            (self.process.clock)()
        }

        fn cancel(&self) -> &Cancel {
            &self.process.cancel
        }

        fn watch(&self, budget: SignedDuration, interval: Duration) -> Watchdog {
            Watchdog::start(
                Deadline::after_index_read(self.now(), budget),
                interval,
                self.events.clone(),
                &self.process,
            )
        }

        /// Waits for the deadline's failure to be written.
        fn wait_for_the_failure(&self) {
            let began = Instant::now();
            while self.capture.parsed().is_empty() {
                assert!(began.elapsed() < PATIENT, "no failure was written");
                thread::sleep(Duration::from_millis(1));
            }
        }

        fn move_clock(&self, seconds: i64) {
            self.seconds.fetch_add(seconds, Ordering::SeqCst);
        }

        fn deadline_failure() -> WorkerEvent {
            WorkerEvent::Failed {
                code: ErrorCode::DeadlinePassed,
                args: Vec::new(),
                detail: None,
            }
        }
    }

    const TICK: Duration = Duration::from_millis(10);
    const PATIENT: Duration = Duration::from_secs(10);

    #[test]
    fn a_deadline_that_has_passed_stops_the_job_before_the_watchdog_returns() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::ZERO, TICK);
        // No waiting: the first look happens inside `start`.
        assert_eq!(
            harness.terminated.try_recv(),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    #[test]
    fn a_negative_budget_has_passed_already() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::from_hours(-3), TICK);
        assert_eq!(
            harness.terminated.try_recv(),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    #[test]
    fn a_deadline_in_the_future_leaves_the_job_alone() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::from_hours(6), TICK);
        assert!(harness.terminated.try_recv().is_err());
        assert_eq!(harness.capture.text(), "");
    }

    #[test]
    fn a_deadline_that_passes_later_stops_the_job_and_nothing_follows() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        harness.move_clock(60);
        assert_eq!(
            harness.terminated.recv_timeout(PATIENT),
            Ok(exit_code::DEADLINE_PASSED)
        );
        // The job keeps running until the process exits; none of it is reported.
        harness
            .events
            .emit(&WorkerEvent::Result(OpResult::Fake { steps_completed: 9 }))
            .unwrap();
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    /// A writer stuck on a full pipe at the deadline must not hold the exit
    /// back, not even for the old quarter of a second: the watchdog tries the
    /// stream once and exits. The exit code tells the daemon what happened.
    #[test]
    fn a_stream_that_is_stuck_at_the_deadline_does_not_hold_the_exit_back() {
        let harness = Harness::new();
        let (stuck, entered, release) = crate::worker::events::testing::stuck();
        let writer = {
            let stuck = stuck.clone();
            thread::spawn(move || {
                stuck.emit(&WorkerEvent::Log {
                    level: crate::job::Level::Info,
                    message: "blocked".to_owned(),
                })
            })
        };
        entered.recv().unwrap();
        let _watchdog = Watchdog::start(
            Deadline::after_index_read(harness.now(), SignedDuration::from_secs(60)),
            TICK,
            stuck,
            &harness.process,
        );
        harness.move_clock(60);
        assert_eq!(
            harness.terminated.recv_timeout(Duration::from_millis(200)),
            Ok(exit_code::DEADLINE_PASSED)
        );
        release.send(()).unwrap();
        writer.join().unwrap().unwrap();
    }

    #[test]
    fn a_clock_that_jumped_while_the_machine_slept_is_noticed_within_one_interval() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::from_hours(6), TICK);
        // A suspend: the wall clock moved hours, no sleeping thread noticed.
        harness.move_clock(8 * 3600);
        assert_eq!(
            harness.terminated.recv_timeout(PATIENT),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    #[test]
    fn stopping_the_watchdog_ends_its_thread_without_waiting_out_the_interval() {
        let harness = Harness::new();
        let watchdog = harness.watch(SignedDuration::from_hours(6), Duration::from_secs(3600));
        let began = Instant::now();
        drop(watchdog);
        assert!(began.elapsed() < Duration::from_secs(30), "stopping hung");
        harness.move_clock(10 * 3600);
        thread::sleep(Duration::from_millis(50));
        assert!(harness.terminated.try_recv().is_err());
        assert_eq!(harness.capture.text(), "");
    }

    #[test]
    fn a_job_that_already_reported_its_result_is_not_failed_by_a_late_deadline() {
        let harness = Harness::new();
        let watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        let result = WorkerEvent::Result(OpResult::Fake { steps_completed: 1 });
        harness.events.emit(&result).unwrap();
        harness.move_clock(120);
        thread::sleep(Duration::from_millis(100));
        drop(watchdog);
        assert!(
            harness.terminated.try_recv().is_err(),
            "the process would exit with a reported result"
        );
        assert_eq!(harness.capture.parsed(), [result]);
    }

    /// I3: the snapshot is refused from the deadline on, before the worker
    /// is ended and whether or not anything winds down.
    #[test]
    fn the_deadline_refuses_the_snapshot_before_it_ends_the_worker() {
        let at_once = Harness::new();
        let _watchdog = at_once.watch(SignedDuration::ZERO, TICK);
        assert_eq!(
            at_once.terminated.try_recv(),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(*at_once.refusing_at_exit.lock().unwrap(), Some(true));

        let later = Harness::new();
        let _watchdog = later.watch(SignedDuration::from_secs(60), TICK);
        later.move_clock(60);
        assert_eq!(
            later.terminated.recv_timeout(PATIENT),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(*later.refusing_at_exit.lock().unwrap(), Some(true));
    }

    /// I3 again: the snapshot is refused before the failure goes out, so a
    /// daemon that has read the failure never sees a snapshot write begin
    /// after it. Whether the worker ends at once or winds down.
    #[test]
    fn the_deadline_refuses_the_snapshot_before_it_reports_the_failure() {
        use std::io::{self, Write};

        /// A stream that notes, at each write, whether the snapshot was
        /// refused already.
        struct Refusing {
            cancel: Cancel,
            refused: Arc<Mutex<Vec<bool>>>,
        }

        impl Write for Refusing {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.refused
                    .lock()
                    .unwrap()
                    .push(self.cancel.is_requested());
                Ok(buf.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let watch = |harness: &Harness| {
            let refused = Arc::new(Mutex::new(Vec::new()));
            let events = Events::new(Refusing {
                cancel: harness.cancel().clone(),
                refused: Arc::clone(&refused),
            });
            let watchdog = Watchdog::start(
                Deadline::after_index_read(harness.now(), SignedDuration::from_secs(60)),
                TICK,
                events,
                &harness.process,
            );
            (watchdog, refused)
        };

        let at_once = Harness::new();
        let (_watchdog, refused) = watch(&at_once);
        at_once.move_clock(60);
        assert_eq!(
            at_once.terminated.recv_timeout(PATIENT),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(*refused.lock().unwrap(), [true]);

        let winding_down = Harness::new();
        let (watchdog, refused) = watch(&winding_down);
        winding_down.cancel().saving(|| {
            winding_down.move_clock(60);
            let passed = Instant::now();
            while refused.lock().unwrap().is_empty() {
                assert!(passed.elapsed() < PATIENT, "no failure was written");
                thread::sleep(Duration::from_millis(1));
            }
        });
        drop(watchdog);
        assert_eq!(*refused.lock().unwrap(), [true]);
    }

    /// A backup that archives at its deadline winds down as a cancelled one
    /// does (decision 50): the failure goes out at once, the snapshot is
    /// refused, and the job, not the watchdog, ends the worker once rustic
    /// has saved the index.
    #[test]
    fn a_deadline_that_passes_while_a_backup_archives_lets_it_wind_down() {
        let harness = Harness::new();
        let watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        let saved = harness.cancel().saving(|| {
            harness.move_clock(60);
            harness.wait_for_the_failure();
            assert!(harness.cancel().is_requested(), "the snapshot is allowed");
            assert!(
                harness
                    .terminated
                    .recv_timeout(Duration::from_millis(100))
                    .is_err(),
                "the worker was ended while its backup wound down"
            );
        });
        assert_eq!(saved, None);
        drop(watchdog);
        assert!(
            harness.terminated.try_recv().is_err(),
            "the worker was ended after its backup had wound down"
        );
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    /// The wind-down has a limit of its own, for a worker whose daemon is
    /// gone and can't end it after its grace period.
    #[test]
    fn a_backup_still_winding_down_at_the_limit_is_ended() {
        let limit = Duration::from_millis(300);
        let harness = Harness::with_wind_down_limit(limit);
        let _watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        harness.cancel().saving(|| {
            let passed = Instant::now();
            harness.move_clock(60);
            assert_eq!(
                harness.terminated.recv_timeout(PATIENT),
                Ok(exit_code::DEADLINE_PASSED)
            );
            assert!(
                passed.elapsed() >= limit,
                "ended {:?} after the deadline, within the wind-down limit",
                passed.elapsed()
            );
        });
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    /// A deadline during the snapshot write lets it finish, as a cancel
    /// does: rustic writes the snapshot to the hot half and then to the
    /// cold half, and a worker ended between the two would leave it in one
    /// half alone (decision 50). The margin covers the write (I3). The job,
    /// cancelled by the deadline, then ends the worker with the deadline's
    /// code, as `a_backup_whose_deadline_passes_mid_archive_…` shows.
    #[test]
    fn a_deadline_during_the_snapshot_write_lets_it_finish_in_both_halves() {
        use rustic_backend::BackendOptions;
        use rustic_core::{FileType, Id, ReadBackend, RepositoryBackends, WriteBackend};

        use crate::worker::cancel::{Cause, refusing_snapshots_once_cancelled};

        let local = |dir: &std::path::Path| {
            let backend = BackendOptions::default()
                .repository(dir.to_string_lossy().into_owned())
                .to_backends()
                .unwrap()
                .repository();
            backend.create().unwrap();
            backend
        };
        let (cold_dir, hot_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let harness = Harness::new();
        let backends = refusing_snapshots_once_cancelled(
            &RepositoryBackends::new(local(cold_dir.path()), Some(local(hot_dir.path()))),
            harness.cancel(),
        );
        let (hot, cold) = (backends.repo_hot().unwrap(), backends.repository());
        let snapshot: Id = format!("{:064x}", 1).parse().unwrap();
        let write = |half: &Arc<dyn WriteBackend>| {
            half.write_bytes(
                FileType::Snapshot,
                &snapshot,
                false,
                b"snapshot".to_vec().into(),
            )
        };
        let watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        let saved = harness.cancel().saving(|| {
            write(&hot).unwrap();
            harness.move_clock(60);
            harness.wait_for_the_failure();
            assert!(
                harness
                    .terminated
                    .recv_timeout(Duration::from_millis(100))
                    .is_err(),
                "the worker was ended between the hot half's snapshot and the cold half's"
            );
            write(&cold).unwrap();
        });
        assert_eq!(saved, None, "the job was not stopped by its deadline");
        assert_eq!(harness.cancel().cause(), Some(Cause::Deadline));
        drop(watchdog);
        assert!(
            harness.terminated.try_recv().is_err(),
            "the worker was ended after its snapshot write had finished"
        );
        assert_eq!(hot.list(FileType::Snapshot).unwrap(), [snapshot]);
        assert_eq!(cold.list(FileType::Snapshot).unwrap(), [snapshot]);
        assert_eq!(harness.capture.parsed(), [Harness::deadline_failure()]);
    }

    #[test]
    fn a_deadline_before_the_archiving_ends_the_worker_at_once() {
        let harness = Harness::new();
        let _watchdog = harness.watch(SignedDuration::from_secs(60), TICK);
        harness.move_clock(60);
        assert_eq!(
            harness.terminated.recv_timeout(PATIENT),
            Ok(exit_code::DEADLINE_PASSED)
        );
        assert_eq!(harness.cancel().saving(|| ()), None);
        assert_eq!(harness.cancel().request(), AfterCancel::ExitNow);
    }
}
