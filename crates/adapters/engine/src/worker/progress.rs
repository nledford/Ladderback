//! rustic's progress, as worker events.
//!
//! `EventProgressBars` plugs into `Repository::new_with_progress`. Every
//! progress rustic starts becomes a keyed series of events, with increments
//! coalesced so a fast loop doesn't flood the pipe.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use api::WarningCode;
use rustic_core::{Progress, ProgressBars, ProgressType, RusticProgress};

use super::events::Events;
use crate::job::{ProgressKind, WorkerEvent};

/// The shortest time between two `ProgressInc` messages of one progress:
/// four a second.
pub(crate) const MIN_INC_INTERVAL: Duration = Duration::from_millis(250);

/// A monotonic clock: the time since some fixed moment.
pub(crate) type Clock = Arc<dyn Fn() -> Duration + Send + Sync>;

/// A clock that starts now.
pub(crate) fn monotonic_clock() -> Clock {
    let start = std::time::Instant::now();
    Arc::new(move || start.elapsed())
}

#[derive(Clone)]
pub(crate) struct EventProgressBars {
    events: Events,
    next_key: Arc<AtomicU32>,
    clock: Clock,
}

impl std::fmt::Debug for EventProgressBars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventProgressBars").finish_non_exhaustive()
    }
}

impl EventProgressBars {
    pub(crate) fn new(events: Events) -> Self {
        Self::with_clock(events, monotonic_clock())
    }

    pub(crate) fn with_clock(events: Events, clock: Clock) -> Self {
        Self {
            events,
            next_key: Arc::new(AtomicU32::new(0)),
            clock,
        }
    }
}

impl ProgressBars for EventProgressBars {
    fn progress(&self, progress_type: ProgressType, prefix: &str) -> Progress {
        todo!()
    }
}

/// When a progress last sent an increment, and what it holds back.
#[derive(Default)]
struct Limiter {
    sent_at: Option<Duration>,
    pending: u64,
}

struct EventProgress {
    key: u32,
    announced: bool,
    events: Events,
    clock: Clock,
    limiter: Mutex<Limiter>,
}

impl std::fmt::Debug for EventProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventProgress")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl EventProgress {
    fn send(&self, event: WorkerEvent) {
        todo!()
    }
}

impl RusticProgress for EventProgress {
    fn is_hidden(&self) -> bool {
        // Hidden progress makes rustic skip work that only feeds it.
        false
    }

    fn set_length(&self, len: u64) {
        self.send(WorkerEvent::ProgressLength { key: self.key, len });
    }

    fn set_title(&self, title: &str) {
        self.send(WorkerEvent::ProgressTitle {
            key: self.key,
            title: title.to_owned(),
        });
    }

    /// Leading edge: the first increment goes out at once, the rest are
    /// added up and sent when [`MIN_INC_INTERVAL`] has passed since the last.
    fn inc(&self, inc: u64) {
        todo!()
    }

    fn finish(&self) {
        todo!()
    }

    /// Only a source that couldn't be read is a known kind of warning.
    fn error(&self, item: Option<&str>, during: &str, message: &str) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use api::WarningCode;
    use domain::OperationId;

    use super::*;
    use crate::job::{ProgressKind, WorkerEvent};
    use crate::worker::events::testing::{Capture, capture};

    /// A clock the test moves by hand.
    fn manual_clock() -> (Clock, Arc<AtomicU64>) {
        let millis = Arc::new(AtomicU64::new(0));
        let clock: Clock = {
            let millis = Arc::clone(&millis);
            Arc::new(move || Duration::from_millis(millis.load(Ordering::SeqCst)))
        };
        (clock, millis)
    }

    fn started(events: &Events) {
        events
            .emit(&WorkerEvent::Started {
                op_id: OperationId::from_uuid(domain::Uuid::from_u128(1)),
                pid: 1,
                index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
            })
            .unwrap();
    }

    /// A started job, its progress bars and a clock to move by hand.
    fn job() -> (EventProgressBars, Capture, Arc<AtomicU64>) {
        let (events, capture) = capture();
        started(&events);
        let (clock, millis) = manual_clock();
        (
            EventProgressBars::with_clock(events, clock),
            capture,
            millis,
        )
    }

    /// What the stream carries after `Started`.
    fn after_started(capture: &Capture) -> Vec<WorkerEvent> {
        capture.parsed().into_iter().skip(1).collect()
    }

    fn incs(events: &[WorkerEvent], key: u32) -> Vec<u64> {
        events
            .iter()
            .filter_map(|event| match event {
                WorkerEvent::ProgressInc { key: k, inc } if *k == key => Some(*inc),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_progress_announces_its_key_kind_and_prefix() {
        let (bars, capture, _) = job();
        let _first = bars.progress(ProgressType::Bytes, "backing up...");
        let _second = bars.progress(ProgressType::Spinner, "reading index...");
        assert_eq!(
            after_started(&capture),
            [
                WorkerEvent::ProgressStart {
                    key: 0,
                    kind: ProgressKind::Bytes,
                    prefix: "backing up...".to_owned()
                },
                WorkerEvent::ProgressStart {
                    key: 1,
                    kind: ProgressKind::Spinner,
                    prefix: "reading index...".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_progress_is_shown_to_rustic_as_visible_so_it_does_the_work_that_feeds_it() {
        let (bars, _capture, _) = job();
        assert!(!bars.progress(ProgressType::Counter, "x").is_hidden());
    }

    #[test]
    fn length_title_and_finish_pass_straight_through() {
        let (bars, capture, _) = job();
        let progress = bars.progress(ProgressType::Counter, "counting");
        progress.set_length(40);
        progress.set_title("counting files");
        progress.finish();
        assert_eq!(
            after_started(&capture)[1..],
            [
                WorkerEvent::ProgressLength { key: 0, len: 40 },
                WorkerEvent::ProgressTitle {
                    key: 0,
                    title: "counting files".to_owned()
                },
                WorkerEvent::ProgressFinish { key: 0 },
            ]
        );
    }

    #[test]
    fn a_burst_of_increments_becomes_a_few_messages_a_second_that_still_add_up() {
        let (bars, capture, millis) = job();
        let progress = bars.progress(ProgressType::Bytes, "backing up...");
        // One increment every 10 ms for a second.
        for _ in 0..100 {
            progress.inc(3);
            millis.fetch_add(10, Ordering::SeqCst);
        }
        progress.finish();
        let events = after_started(&capture);
        let incs = incs(&events, 0);
        assert!(
            incs.len() <= 5,
            "a second of increments sent {} messages: {incs:?}",
            incs.len()
        );
        assert_eq!(
            incs.iter().sum::<u64>(),
            300,
            "increments were lost: {incs:?}"
        );
    }

    #[test]
    fn the_first_increment_goes_out_at_once() {
        let (bars, capture, _) = job();
        let progress = bars.progress(ProgressType::Counter, "x");
        progress.inc(5);
        assert_eq!(incs(&after_started(&capture), 0), [5]);
    }

    #[test]
    fn the_pending_increment_is_sent_before_the_progress_finishes() {
        let (bars, capture, millis) = job();
        let progress = bars.progress(ProgressType::Counter, "x");
        progress.inc(1);
        millis.fetch_add(10, Ordering::SeqCst);
        progress.inc(2);
        millis.fetch_add(10, Ordering::SeqCst);
        progress.inc(4);
        progress.finish();
        let events = after_started(&capture);
        assert_eq!(incs(&events, 0), [1, 6]);
        let [.., last_inc, finish] = events.as_slice() else {
            panic!("too few events: {events:?}");
        };
        assert_eq!(*last_inc, WorkerEvent::ProgressInc { key: 0, inc: 6 });
        assert_eq!(*finish, WorkerEvent::ProgressFinish { key: 0 });
    }

    #[test]
    fn an_increment_after_the_interval_goes_out_with_what_was_pending() {
        let (bars, capture, millis) = job();
        let progress = bars.progress(ProgressType::Counter, "x");
        progress.inc(1);
        millis.fetch_add(100, Ordering::SeqCst);
        progress.inc(2);
        millis.fetch_add(150, Ordering::SeqCst);
        progress.inc(4);
        assert_eq!(incs(&after_started(&capture), 0), [1, 6]);
    }

    #[test]
    fn each_progress_is_limited_on_its_own() {
        let (bars, capture, _) = job();
        let a = bars.progress(ProgressType::Counter, "a");
        let b = bars.progress(ProgressType::Counter, "b");
        a.inc(1);
        b.inc(1);
        a.inc(1);
        b.inc(1);
        let events = after_started(&capture);
        assert_eq!(incs(&events, 0), [1]);
        assert_eq!(incs(&events, 1), [1]);
    }

    #[test]
    fn a_source_that_could_not_be_read_becomes_a_warning() {
        let (bars, capture, _) = job();
        let progress = bars.progress(ProgressType::Bytes, "backing up...");
        progress.error(Some("/home/demo/locked"), "archival", "permission denied");
        progress.error(None, "scan", "gone");
        assert_eq!(
            after_started(&capture)[1..],
            [
                WorkerEvent::Warning {
                    item: Some("/home/demo/locked".to_owned()),
                    during: "archival".to_owned(),
                    code: WarningCode::SourceUnreadable,
                    detail: "permission denied".to_owned()
                },
                WorkerEvent::Warning {
                    item: None,
                    during: "scan".to_owned(),
                    code: WarningCode::SourceUnreadable,
                    detail: "gone".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_problem_during_something_else_is_a_warning_of_unknown_kind() {
        let (bars, capture, _) = job();
        let progress = bars.progress(ProgressType::Counter, "x");
        progress.error(Some("item"), "repacking", "odd");
        let events = after_started(&capture);
        let Some(WorkerEvent::Warning { code, .. }) = events.last() else {
            panic!("no warning in {events:?}");
        };
        assert_eq!(*code, WarningCode::Unknown);
    }

    #[test]
    fn a_progress_that_began_before_the_job_started_never_reports() {
        let (events, capture) = capture();
        let (clock, _) = manual_clock();
        let bars = EventProgressBars::with_clock(events.clone(), clock);
        let early = bars.progress(ProgressType::Counter, "getting snapshots...");
        started(&events);
        early.set_length(3);
        early.inc(1);
        early.finish();
        let late = bars.progress(ProgressType::Counter, "reading index...");
        late.inc(1);
        // Only the progress that began after `Started` shows up.
        let events = after_started(&capture);
        assert!(
            events.iter().all(|event| match event {
                WorkerEvent::ProgressStart { key, .. }
                | WorkerEvent::ProgressLength { key, .. }
                | WorkerEvent::ProgressInc { key, .. }
                | WorkerEvent::ProgressTitle { key, .. }
                | WorkerEvent::ProgressFinish { key } => *key == 1,
                _ => true,
            }),
            "{events:?}"
        );
        assert_eq!(incs(&events, 1), [1]);
    }

    /// rustic's callbacks can't return an error, so a stream that nobody
    /// reads must end the worker from inside them.
    #[test]
    fn a_progress_whose_events_cannot_be_written_gives_the_worker_up() {
        let (events, gave_up) = crate::worker::events::testing::breaking_after(1);
        started(&events);
        let (clock, _) = manual_clock();
        let bars = EventProgressBars::with_clock(events, clock);
        let progress = bars.progress(ProgressType::Counter, "files");
        assert_eq!(
            gave_up.load(Ordering::SeqCst),
            1,
            "the start went unnoticed"
        );
        progress.set_length(3);
        progress.set_title("files");
        progress.inc(1);
        progress.error(Some("item"), "scan", "odd");
        progress.finish();
        assert_eq!(gave_up.load(Ordering::SeqCst), 6);
    }
}
