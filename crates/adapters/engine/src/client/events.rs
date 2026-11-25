//! The events of one job, read from the worker's stdout a line at a time,
//! with a repository's listing gathered whole and the worker's end told.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use app::ports::{
    Deletion, DeletionAnswer, DeletionAsk, EngineEvent, EngineResult, SnapshotSummary,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::execution::FailureReason;
use domain::restore::download::PIECE_BYTES;
use domain::secrets::Redactor;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, watch};

use super::DRAIN_AFTER_EXIT;
use super::stop::{Exited, ForEvents, Order, deadline_after};
use super::translate::translate;
use crate::job::{
    DeleteAnswer, MAX_LISTED_BYTES, MAX_LISTED_SNAPSHOTS, OpResult, WorkerEvent, exit_code,
};

/// The events of one job, read from the worker's stdout.
pub(super) struct Reading {
    pub(super) stdout: BufReader<ForEvents>,
    pub(super) line: Vec<u8>,
    pub(super) exited: watch::Receiver<Exited>,
    pub(super) commands: mpsc::UnboundedSender<Order>,
    pub(super) stopped: Arc<AtomicBool>,
    /// Whether the daemon's stop of its workers stopped the job, not its
    /// control.
    pub(super) daemon_stopped: Arc<AtomicBool>,
    pub(super) done: bool,
    /// When the output of a worker that has exited is no longer waited for.
    pub(super) drain_until: Option<tokio::time::Instant>,
    /// What masks the job's secrets in what the worker says.
    pub(super) redactor: Redactor,
    /// How long the exit of a worker that closed its output is waited for.
    pub(super) exit_timeout: Duration,
    /// When the worker's first complete line is due, until it has come: the
    /// start timeout counted from the start of the write of the job.
    /// `None` once a line has come, or for a timeout that can't be
    /// represented.
    pub(super) first_line_by: Option<tokio::time::Instant>,
    /// The listing the worker has sent so far, when the job is one.
    pub(super) listing: Listing,
    /// A prune's stdin, where the one answer to its question goes: `None`
    /// for any other job, and once the prune has asked (decision 161).
    pub(super) answer_to: Option<ChildStdin>,
}

/// A repository's listing as it arrives: the `snapshots` events of a worker
/// until the result that closes it. The list is held up to
/// [`MAX_LISTED_SNAPSHOTS`] and no further, and it is handed on only once
/// the closing result's count agrees with it: a list that lost a batch is
/// never taken for the whole.
#[derive(Default)]
pub(super) struct Listing {
    snapshots: Vec<SnapshotSummary>,
    /// What the snapshots held so far weigh, as [`weight`] counts it.
    bytes: usize,
}

/// What a listed snapshot costs the daemon to hold: its texts, and a fixed
/// amount for the rest.
fn weight(snapshot: &SnapshotSummary) -> usize {
    let texts = snapshot.hostname.len()
        + snapshot.label.len()
        + snapshot.tags.iter().map(String::len).sum::<usize>()
        + snapshot.paths.iter().map(String::len).sum::<usize>();
    texts.saturating_add(256)
}

/// The offset a worker reported, in seconds east of UTC, or `None` when no
/// clock can run at it (beyond ±25:59:59).
fn offset_of(seconds: i32) -> Option<domain::Offset> {
    domain::Offset::from_seconds(seconds).ok()
}

/// What a [`Listing`] made of one event of the worker's.
enum Absorbed {
    /// A batch, kept; nothing to tell the job's owner yet.
    Kept,
    /// The listing's end: the whole list.
    Complete(Vec<SnapshotSummary>),
    /// A listing that can't be taken, and why.
    Refused(String),
    /// Not part of a listing.
    Passed(WorkerEvent),
}

impl Listing {
    fn absorb(&mut self, event: WorkerEvent) -> Absorbed {
        match event {
            WorkerEvent::Snapshots { snapshots } => {
                if self.snapshots.len().saturating_add(snapshots.len()) > MAX_LISTED_SNAPSHOTS {
                    return Absorbed::Refused(format!(
                        "worker listing longer than {MAX_LISTED_SNAPSHOTS} snapshots"
                    ));
                }
                let mut batch = Vec::with_capacity(snapshots.len());
                for s in snapshots {
                    let Some(offset) = offset_of(s.offset_seconds) else {
                        return Absorbed::Refused(format!(
                            "worker listed a snapshot at an offset of {} seconds, which no clock has",
                            s.offset_seconds
                        ));
                    };
                    batch.push(SnapshotSummary {
                        id: s.id,
                        time: s.time,
                        offset,
                        hostname: s.hostname,
                        label: s.label,
                        tags: s.tags,
                        paths: s.paths,
                        paths_truncated: s.paths_truncated,
                        tags_truncated: s.tags_truncated,
                    });
                }
                let bytes = batch
                    .iter()
                    .fold(self.bytes, |all, s| all.saturating_add(weight(s)));
                if bytes > MAX_LISTED_BYTES {
                    return Absorbed::Refused(format!(
                        "worker listing larger than {MAX_LISTED_BYTES} bytes"
                    ));
                }
                self.bytes = bytes;
                self.snapshots.extend(batch);
                Absorbed::Kept
            }
            WorkerEvent::Result(OpResult::Snapshots { count }) => {
                let snapshots = std::mem::take(&mut self.snapshots);
                self.bytes = 0;
                if u64::try_from(snapshots.len()).is_ok_and(|held| held == count) {
                    Absorbed::Complete(snapshots)
                } else {
                    Absorbed::Refused(format!(
                        "worker listing ended at {count} snapshots after sending {}",
                        snapshots.len()
                    ))
                }
            }
            other => Absorbed::Passed(other),
        }
    }
}

impl Reading {
    pub(super) async fn next(mut self) -> Option<(EngineEvent, Self)> {
        if self.done {
            return None;
        }
        loop {
            self.line.clear();
            let read = self.read_next_line().await;
            if self.stopped.load(Ordering::SeqCst) {
                return self.stopped_end().await;
            }
            match read {
                Ok(Line::Complete) => self.first_line_by = None,
                // No line, not even a `log` one, by the time it was due: the
                // worker is killed without waiting for the job's control,
                // which may be held for as long as its owner likes.
                Ok(Line::Silent) => {
                    self.done = true;
                    let _ = self.commands.send(Order::Kill);
                    return Some((
                        EngineEvent::Failed(FailureReason::EngineStartTimedOut),
                        self,
                    ));
                }
                // A line without its newline was cut off by a worker that
                // died mid-write, say at its deadline.
                Ok(Line::Cut) => return self.exited().await,
                Ok(Line::TooLong) => {
                    return Some((
                        self.broken(format!("worker event longer than {MAX_EVENT_LINE} bytes")),
                        self,
                    ));
                }
                Err(error) => {
                    return Some((self.broken(format!("reading worker output: {error}")), self));
                }
            }
            let event = match serde_json::from_slice::<WorkerEvent>(&self.line) {
                Ok(event) => event,
                Err(error) => {
                    return Some((
                        self.broken(format!("malformed worker event: {error}")),
                        self,
                    ));
                }
            };
            let event = match self.listing.absorb(event) {
                Absorbed::Kept => continue,
                Absorbed::Complete(snapshots) => {
                    self.done = true;
                    let _ = self.commands.send(Order::ResultReported);
                    return Some((
                        EngineEvent::Succeeded(EngineResult::Snapshots(snapshots)),
                        self,
                    ));
                }
                Absorbed::Refused(detail) => return Some((self.broken(detail), self)),
                Absorbed::Passed(event) => event,
            };
            if let WorkerEvent::Piece { bytes } = &event {
                return Some(match piece(bytes) {
                    Ok(piece) => (EngineEvent::Piece(piece), self),
                    Err(detail) => (self.broken(detail), self),
                });
            }
            if matches!(event, WorkerEvent::ReadyToDelete) {
                let Some(stdin) = self.answer_to.take() else {
                    let detail = "the worker asked to delete outside a prune, or twice";
                    return Some((self.broken(detail.to_owned()), self));
                };
                let (ask, answer) = DeletionAsk::new();
                tokio::spawn(tell_deletion(stdin, answer, self.exited.clone()));
                return Some((EngineEvent::DeletionAsked(ask), self));
            }
            if let Some(event) = translate(event, &self.redactor) {
                if matches!(event, EngineEvent::Succeeded(_) | EngineEvent::Failed(_)) {
                    self.done = true;
                    let _ = self.commands.send(Order::ResultReported);
                }
                return Some((event, self));
            }
        }
    }

    /// Reads the next line of the worker's output into `self.line`.
    ///
    /// While the worker runs, the wait is as long as it takes, but for the
    /// first line, which is due by [`Reading::first_line_by`]. Once the
    /// worker has exited, what it wrote is in the pipe already, so reading
    /// goes on for that, but for [`DRAIN_AFTER_EXIT`] in all: nothing is
    /// waited for on a pipe that another process holds open.
    async fn read_next_line(&mut self) -> std::io::Result<Line> {
        let Self {
            stdout,
            line,
            exited,
            drain_until,
            first_line_by,
            ..
        } = self;
        let reading = read_line(stdout, line);
        tokio::pin!(reading);
        let silent = async {
            match *first_line_by {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            // What is in the pipe goes before the worker's exit, and a
            // worker that has exited is not a silent one: its exit is told
            // by what follows.
            biased;
            read = &mut reading => return read,
            () = worker_exited(exited) => {}
            () = silent => return Ok(Line::Silent),
        }
        let until = *drain_until
            .get_or_insert_with(|| deadline_after(tokio::time::Instant::now(), DRAIN_AFTER_EXIT));
        // A pipe that stays quiet is an ended one: the cut line, if any, is
        // reported as a worker that died mid-write.
        match tokio::time::timeout_at(until, reading).await {
            Ok(read) => read,
            Err(_) => Ok(Line::Cut),
        }
    }

    /// The end of the events of a job that was stopped. A cancel ends them
    /// without a failure, as ever. The daemon's stop of its workers never
    /// ends them: what runs the job, which would record and announce the end
    /// it read and run the hooks of one, ends with the daemon instead, and
    /// the daemon's stop records the operation as ended `daemon-shut-down`,
    /// announcing nothing (decision 92).
    async fn stopped_end(self) -> Option<(EngineEvent, Self)> {
        if self.daemon_stopped.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        None
    }

    /// The worker closed stdout without a result: it crashed, was killed, its
    /// deadline watchdog stopped it, or it lives on. Its exit is waited for
    /// the exit timeout and no longer: a worker still there then is killed,
    /// and the failure says so.
    async fn exited(mut self) -> Option<(EngineEvent, Self)> {
        self.done = true;
        // `None`: the worker was still there when the time was up. The wait
        // is a scope of its own: what it borrows from `self.exited` is let go
        // before `self` moves on.
        let exit = {
            let waited =
                tokio::time::timeout(self.exit_timeout, self.exited.wait_for(Option::is_some))
                    .await;
            match waited {
                Ok(Ok(exit)) => Some((*exit).flatten()),
                // The task that owns the worker is gone, and the worker with it.
                Ok(Err(_)) => return None,
                Err(_) => None,
            }
        };
        if self.stopped.load(Ordering::SeqCst) {
            return self.stopped_end().await;
        }
        let reason = match exit {
            Some(Some(exit_code::DEADLINE_PASSED)) => FailureReason::DeadlinePassed,
            Some(exit_code) => FailureReason::EngineExited { exit_code },
            None => {
                // The worker is killed without waiting for the job's control,
                // which may be held for as long as its owner likes.
                let _ = self.commands.send(Order::Kill);
                FailureReason::EngineExitTimedOut
            }
        };
        Some((EngineEvent::Failed(reason), self))
    }

    fn broken(&mut self, detail: String) -> EngineEvent {
        // Dropping the job kills the worker. What a parser says of the line
        // it couldn't read can quote it.
        self.done = true;
        EngineEvent::Failed(FailureReason::EngineFailed {
            detail: self.redactor.redact(&detail),
        })
    }
}

/// A dump's piece as the worker wrote it, as base64: its bytes, which are
/// at most [`PIECE_BYTES`].
fn piece(text: &str) -> Result<Vec<u8>, String> {
    let bytes = STANDARD
        .decode(text)
        .map_err(|error| format!("worker sent a piece that isn't base64: {error}"))?;
    if bytes.len() > PIECE_BYTES {
        return Err(format!(
            "worker sent a piece of {} bytes, more than {PIECE_BYTES}",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// The longest line of events a worker may write: the protocol's message
/// cap. A longer line is a worker gone wrong, and is never read into memory.
const MAX_EVENT_LINE: usize = crate::job::MAX_MESSAGE_BYTES;

enum Line {
    Complete,
    /// The output ended before the line's newline.
    Cut,
    TooLong,
    /// No line was complete by the time the first was due.
    Silent,
}

/// Reads one line, newline included, into `line`, holding at most
/// [`MAX_EVENT_LINE`] bytes of it before the newline.
async fn read_line(reader: &mut BufReader<ForEvents>, line: &mut Vec<u8>) -> std::io::Result<Line> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(Line::Cut);
        }
        let (taken, complete) = match available.iter().position(|&byte| byte == b'\n') {
            Some(newline) => (newline.saturating_add(1), true),
            None => (available.len(), false),
        };
        let content = if complete {
            taken.saturating_sub(1)
        } else {
            taken
        };
        if line.len().saturating_add(content) > MAX_EVENT_LINE {
            return Ok(Line::TooLong);
        }
        line.extend_from_slice(available.get(..taken).unwrap_or(available));
        reader.consume(taken);
        if complete {
            return Ok(Line::Complete);
        }
    }
}

/// Writes the answer a prune's question gets to its stdin as one line, and
/// closes it; nothing once the worker has exited. A write that fails leaves
/// the prune with no answer, on which it deletes nothing.
async fn tell_deletion(
    mut stdin: ChildStdin,
    answer: DeletionAnswer,
    mut exited: watch::Receiver<Exited>,
) {
    let deletion = tokio::select! {
        deletion = answer => deletion,
        () = worker_exited(&mut exited) => return,
    };
    let answer = match deletion {
        Deletion::Permitted => DeleteAnswer::Delete,
        Deletion::Refused => DeleteAnswer::DeleteNothing,
    };
    let Ok(mut line) = serde_json::to_vec(&answer) else {
        return;
    };
    line.push(b'\n');
    let _ = stdin.write_all(&line).await;
}

/// Resolves once the worker has exited.
pub(super) async fn worker_exited(exited: &mut watch::Receiver<Exited>) {
    let _ = exited.wait_for(Option::is_some).await;
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::time::Instant;

    use api::WarningCode;
    use app::ports::RunningJob;
    use domain::{OperationId, Uuid};
    use futures::StreamExt;

    use super::*;
    use crate::client::WorkerEngine;
    use crate::client::test_support::*;
    use crate::job::{Level, MAX_MESSAGE_BYTES};

    #[tokio::test]
    async fn a_worker_that_exits_without_a_result_is_reported_with_its_code() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; exit 3"]);
        let RunningJob { mut events, .. } = run(&engine).await;
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            }))
        );
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn a_worker_cut_off_mid_line_at_its_deadline_is_a_passed_deadline() {
        let script = format!(
            "cat >/dev/null; printf '{{\"event\":\"log\"'; exit {}",
            exit_code::DEADLINE_PASSED
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob { mut events, .. } = run(&engine).await;
        assert_eq!(
            events.next().await,
            Some(EngineEvent::Failed(FailureReason::DeadlinePassed))
        );
    }

    /// A process the worker started, rclone say, can outlive it and keep the
    /// pipe open. The events end with the worker all the same, after what
    /// the worker had written, and the process is ended with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_events_end_with_the_worker_even_if_another_process_holds_the_pipe() {
        let warning = serde_json::to_string(&WorkerEvent::Warning {
            item: None,
            during: "backup".to_owned(),
            code: WarningCode::Unknown,
            detail: "slow disk".to_owned(),
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        // The `sleep` inherits the worker's stdout and outlives the shell.
        let script = format!(
            "cat >/dev/null; echo '{warning}'; sleep 8 & echo $! >'{}'; exit 3",
            pid_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let events = run(&engine).await.events;
        let events = tokio::time::timeout(Duration::from_secs(4), events.collect::<Vec<_>>())
            .await
            .expect("the events ended while another process held the pipe");
        assert_matches!(
            events.as_slice(),
            [
                EngineEvent::Warning(_),
                EngineEvent::Failed(FailureReason::EngineExited { exit_code: Some(3) })
            ],
            "{events:?}"
        );
        let child = pid_from(&pid_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker left running outlived it"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_line_that_cannot_be_read_and_quotes_a_secret_is_masked_in_the_failure() {
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "garbling",
            &[r#"{"event":"hunter2-demo"}"#.to_owned()],
        )
        .await;
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events.as_slice()
        else {
            panic!("expected one engine failure, got {events:?}");
        };
        assert!(detail.starts_with("malformed worker event: "), "{detail}");
        assert!(detail.contains("***"), "{detail}");
        assert!(!detail.contains("hunter2-demo"), "{detail}");
    }

    /// Runs `script` as the worker and collects its events; the test fails if
    /// they don't end within ten seconds.
    async fn events_from(script: &str) -> Vec<EngineEvent> {
        let engine = WorkerEngine::new("sh", ["-c", script]);
        let events = run(&engine).await.events;
        tokio::time::timeout(Duration::from_secs(10), events.collect::<Vec<_>>())
            .await
            .expect("the worker's events ended")
    }

    fn assert_over_the_cap(events: &[EngineEvent]) {
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events else {
            panic!("expected exactly one engine failure, got {events:?}");
        };
        assert!(
            detail.contains(&MAX_MESSAGE_BYTES.to_string()),
            "the failure should name the cap: {detail}"
        );
    }

    #[tokio::test]
    async fn an_event_line_past_the_cap_fails_the_job() {
        // 3 MiB of `x`, then the newline.
        let events =
            events_from("cat >/dev/null; head -c 3145728 /dev/zero | tr '\\0' x; echo").await;
        assert_over_the_cap(&events);
    }

    #[tokio::test]
    async fn a_line_that_never_ends_fails_the_job_instead_of_growing_forever() {
        // 64 MiB without a newline, then silence: the worker is still alive.
        let events =
            events_from("cat >/dev/null; head -c 67108864 /dev/zero | tr '\\0' x; exec sleep 30")
                .await;
        assert_over_the_cap(&events);
    }

    fn listed(n: u128) -> crate::job::SnapshotResult {
        crate::job::SnapshotResult {
            id: domain::SnapshotId::parse(&format!("{n:064x}")).unwrap(),
            time: domain::Timestamp::from_second(i64::try_from(n).unwrap()).unwrap(),
            offset_seconds: 0,
            hostname: "demo-host".to_owned(),
            label: format!("label {n}"),
            tags: vec!["tag".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: n.is_multiple_of(2),
            tags_truncated: n.is_multiple_of(3),
        }
    }

    fn batch(from: u128, to: u128) -> WorkerEvent {
        WorkerEvent::Snapshots {
            snapshots: (from..to).map(listed).collect(),
        }
    }

    /// The script of a worker that prints these events, one line each.
    fn printing(events: &[WorkerEvent]) -> String {
        let lines: String = events
            .iter()
            .map(|event| format!("echo '{}'; ", serde_json::to_string(event).unwrap()))
            .collect();
        format!("cat >/dev/null; {lines}")
    }

    #[tokio::test]
    async fn a_worker_that_lists_in_batches_gives_the_job_one_whole_list() {
        let events = events_from(&printing(&[
            batch(0, 2),
            batch(2, 3),
            WorkerEvent::Result(OpResult::Snapshots { count: 3 }),
        ]))
        .await;
        let [EngineEvent::Succeeded(EngineResult::Snapshots(all))] = events.as_slice() else {
            panic!("one list, whole: {events:?}");
        };
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn a_worker_whose_listing_comes_up_short_fails_the_job() {
        let events = events_from(&printing(&[
            batch(0, 2),
            WorkerEvent::Result(OpResult::Snapshots { count: 3 }),
        ]))
        .await;
        let [EngineEvent::Failed(FailureReason::EngineFailed { detail })] = events.as_slice()
        else {
            panic!("the job fails: {events:?}");
        };
        assert!(detail.contains('3') && detail.contains('2'), "{detail}");
    }

    #[test]
    fn a_listing_arrives_in_batches_and_is_handed_on_whole_at_its_end() {
        let mut listing = Listing::default();
        assert!(matches!(listing.absorb(batch(0, 2)), Absorbed::Kept));
        assert!(matches!(listing.absorb(batch(2, 5)), Absorbed::Kept));
        let Absorbed::Complete(all) =
            listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count: 5 }))
        else {
            panic!("the listing is complete");
        };
        let ids: Vec<String> = all.iter().map(|s| s.id.as_str().to_owned()).collect();
        let expected: Vec<String> = (0..5).map(|n| listed(n).id.as_str().to_owned()).collect();
        assert_eq!(ids, expected, "in the order sent");
        // What was cut from a snapshot goes with it.
        assert!(all[0].paths_truncated && all[0].tags_truncated);
        assert!(!all[1].paths_truncated && !all[1].tags_truncated);
        assert_eq!(all[3].label, "label 3");
        assert_eq!(all[3].paths, ["/home/demo"]);
        assert_eq!(all[3].tags, ["tag"]);
        assert!(all[3].tags_truncated && !all[3].paths_truncated);
    }

    #[test]
    fn an_empty_listing_is_complete_with_nothing_in_it() {
        let mut listing = Listing::default();
        let Absorbed::Complete(all) =
            listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count: 0 }))
        else {
            panic!("the listing is complete");
        };
        assert!(all.is_empty());
    }

    #[test]
    fn a_listing_that_lost_a_batch_is_refused_not_taken_for_the_whole() {
        for count in [4, 6] {
            let mut listing = Listing::default();
            let _ = listing.absorb(batch(0, 5));
            let Absorbed::Refused(detail) =
                listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count }))
            else {
                panic!("a listing that disagrees with its count is refused");
            };
            assert!(
                detail.contains(&count.to_string()) && detail.contains('5'),
                "{detail}"
            );
        }
    }

    #[test]
    fn a_listing_past_the_most_the_daemon_holds_is_refused() {
        let mut listing = Listing::default();
        let most = MAX_LISTED_SNAPSHOTS;
        let fill = |from: usize, to: usize| WorkerEvent::Snapshots {
            snapshots: (from..to).map(|_| listed(1)).collect(),
        };
        assert!(matches!(listing.absorb(fill(0, most - 1)), Absorbed::Kept));
        assert!(matches!(listing.absorb(fill(0, 1)), Absorbed::Kept));
        let Absorbed::Refused(detail) = listing.absorb(fill(0, 1)) else {
            panic!("one past the most is refused");
        };
        assert!(detail.contains(&most.to_string()), "{detail}");
    }

    #[test]
    fn a_listing_past_the_bytes_the_daemon_holds_is_refused_whatever_its_count() {
        let mut listing = Listing::default();
        let fat = || crate::job::SnapshotResult {
            label: "x".repeat(130 * 1024),
            tags: Vec::new(),
            paths: Vec::new(),
            ..listed(1)
        };
        let per = 130 * 1024 + "demo-host".len() + 256;
        let fits = MAX_LISTED_BYTES / per;
        let batch = |n: usize| WorkerEvent::Snapshots {
            snapshots: (0..n).map(|_| fat()).collect(),
        };
        assert!(matches!(listing.absorb(batch(fits)), Absorbed::Kept));
        let Absorbed::Refused(detail) = listing.absorb(batch(2)) else {
            panic!("past the bytes held is refused");
        };
        assert!(detail.contains(&MAX_LISTED_BYTES.to_string()), "{detail}");
    }

    #[test]
    fn what_is_not_a_listing_passes_through_untouched() {
        let mut listing = Listing::default();
        let event = WorkerEvent::Result(OpResult::Fake { steps_completed: 3 });
        assert!(matches!(
            listing.absorb(event.clone()),
            Absorbed::Passed(passed) if passed == event
        ));
    }

    #[test]
    fn a_listed_snapshot_keeps_the_offset_of_the_clock_that_took_it() {
        let mut listing = Listing::default();
        let at = |n: u128, seconds: i32| crate::job::SnapshotResult {
            offset_seconds: seconds,
            ..listed(n)
        };
        let batch = WorkerEvent::Snapshots {
            snapshots: vec![
                at(0, -6 * 3600),
                at(1, 13 * 3600),
                at(2, 93_599),
                at(3, -93_599),
            ],
        };
        assert!(matches!(listing.absorb(batch), Absorbed::Kept));
        let Absorbed::Complete(all) =
            listing.absorb(WorkerEvent::Result(OpResult::Snapshots { count: 4 }))
        else {
            panic!("the listing is complete");
        };
        let offsets: Vec<i32> = all.iter().map(|s| s.offset.seconds()).collect();
        assert_eq!(offsets, [-21_600, 46_800, 93_599, -93_599]);
    }

    #[test]
    fn a_listing_with_an_offset_no_clock_has_is_refused() {
        for seconds in [93_600, -93_600, i32::MAX, i32::MIN] {
            let mut listing = Listing::default();
            let batch = WorkerEvent::Snapshots {
                snapshots: vec![
                    listed(0),
                    crate::job::SnapshotResult {
                        offset_seconds: seconds,
                        ..listed(1)
                    },
                ],
            };
            let Absorbed::Refused(detail) = listing.absorb(batch) else {
                panic!("an offset of {seconds} seconds is refused");
            };
            assert!(detail.contains(&seconds.to_string()), "{detail}");
        }
    }

    #[tokio::test]
    async fn an_event_line_of_exactly_the_cap_is_read() {
        let started = serde_json::to_string(&WorkerEvent::Started {
            op_id: OperationId::from_uuid(Uuid::from_u128(1)),
            pid: 1,
            index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
            repository: None,
        })
        .unwrap();
        let head =
            r#"{"event":"warning","item":null,"during":"backup","code":"unknown","detail":""#;
        let tail = r#""}"#;
        let detail_len = MAX_MESSAGE_BYTES - head.len() - tail.len();
        let result =
            serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 }))
                .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{started}'; printf '%s' '{head}'; \
             head -c {detail_len} /dev/zero | tr '\\0' x; echo '{tail}'; echo '{result}'"
        );
        let events = events_from(&script).await;
        assert_matches!(
            events.as_slice(),
            [EngineEvent::Started { .. }, EngineEvent::Warning(warning), EngineEvent::Succeeded(_)]
                if warning.detail.len() == detail_len,
            "unexpected events: {} events",
            events.len()
        );
    }

    // The first-line deadline: a worker that took its job and then says
    // nothing is as hung as one that never read it, and the daemon can't
    // tell it from one that is slowly opening a repository, except by the
    // time that passed since the job was written.

    /// A worker that read its job (the script says so with `cat`) and then
    /// stays silent: nothing it could still do makes it a worker to wait for.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_reads_its_job_but_stays_silent_is_failed_when_the_start_timeout_passes()
    {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!(
            "cat >/dev/null; echo $$ >'{}'; exec sleep 30",
            pid_file.display()
        );
        // Long enough for a loaded machine to start the shell.
        let timeout = Duration::from_secs(1);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(timeout);
        let began = Instant::now();
        let RunningJob {
            events,
            mut control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` has no first event to wait for, and must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "gave up before the start timeout: {took:?}"
        );
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the silent worker outlived the start timeout"
        );
        // Nobody stopped the job, and its control is still here: the worker
        // is gone all the same.
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("stopping a worker that was killed already hung");
    }

    /// As everywhere a worker ends, what it started goes with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_of_a_silent_worker_also_kills_the_processes_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        // The child's output goes elsewhere, so that the worker's own
        // descriptor is the last one on the pipe.
        let script = format!(
            "cat >/dev/null; sleep 30 >/dev/null & echo $! >'{}'; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_start_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the start timeout"
        );
    }

    /// One deadline covers the write of the job and the wait for the first
    /// line: a worker that is slow to read its job has that much less time to
    /// answer. The job is larger than a pipe, so the write waits for the
    /// worker's reading.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_start_timeout_counts_from_the_start_of_the_write_of_the_job() {
        let script = "sleep 3; cat >/dev/null; exec sleep 30";
        let timeout = Duration::from_secs(4);
        let engine = WorkerEngine::new("sh", ["-c", script]).with_start_timeout(timeout);
        let began = Instant::now();
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, job_larger_than_a_pipe())
            .await
            .expect("a worker that read its job in time was timed out");
        let events = collected_within(Duration::from_secs(10), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
        assert!(
            took < Duration::from_millis(5500),
            "the wait for the first line started over after the write: {took:?}"
        );
    }

    /// A line is not there before its newline: a worker that starts one and
    /// stalls has not said anything yet.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_stalls_in_its_first_line_is_failed_when_the_start_timeout_passes() {
        let script = r#"cat >/dev/null; printf '{"event":"star'; exec sleep 30"#;
        let engine =
            WorkerEngine::new("sh", ["-c", script]).with_start_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(6), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineStartTimedOut)]
        );
    }

    fn started_line() -> String {
        serde_json::to_string(&WorkerEvent::Started {
            op_id: OperationId::from_uuid(Uuid::from_u128(1)),
            pid: 1,
            index_read_at: domain::Timestamp::from_second(1_800_000_000).unwrap(),
            repository: None,
        })
        .unwrap()
    }

    fn log_line() -> String {
        serde_json::to_string(&WorkerEvent::Log {
            level: Level::Info,
            message: "opening the repository".to_owned(),
        })
        .unwrap()
    }

    /// Runs `script`, which reads its job first, under a start timeout of
    /// `timeout`, and returns the events.
    #[cfg(unix)]
    async fn events_under_a_start_timeout(timeout: Duration, script: &str) -> Vec<EngineEvent> {
        let engine = WorkerEngine::new("sh", ["-c", script]).with_start_timeout(timeout);
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        collected_within(Duration::from_secs(15), events).await
    }

    /// Once the worker has been heard from it is not on the clock any more:
    /// a repository can be slow to open, and then a backup slow to finish.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_sent_its_first_event_in_time_may_go_quiet_for_longer_than_the_start_timeout()
     {
        let script = format!(
            "cat >/dev/null; echo '{}'; sleep 2.5; echo '{}'",
            started_line(),
            result_line()
        );
        let events = events_under_a_start_timeout(Duration::from_secs(1), &script).await;
        assert_matches!(
            events.as_slice(),
            [EngineEvent::Started { .. }, EngineEvent::Succeeded(_)],
            "{events:?}"
        );
    }

    /// A `log` line is a line like any other: it shows that the worker lives,
    /// and the `started` that follows may be slow, for it comes once the
    /// repository is open.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_whose_first_line_is_a_log_line_may_go_quiet_for_longer_than_the_start_timeout()
     {
        let script = format!(
            "cat >/dev/null; echo '{}'; sleep 2.5; echo '{}'",
            log_line(),
            result_line()
        );
        let events = events_under_a_start_timeout(Duration::from_secs(1), &script).await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)], "{events:?}");
    }

    /// The timeout bounds the wait for the first line, not the worker: one
    /// that answers in time is reported as it always was.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_answers_within_the_start_timeout_is_not_timed_out() {
        let script = format!("cat >/dev/null; sleep 0.3; echo '{}'", result_line());
        let events = events_under_a_start_timeout(Duration::from_secs(5), &script).await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)], "{events:?}");
    }

    /// A timeout too long to be a time never passes, and doesn't panic
    /// where it is added to the clock.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_start_timeout_too_long_to_represent_never_passes() {
        let script = format!("cat >/dev/null; sleep 0.3; echo '{}'", result_line());
        let engine =
            WorkerEngine::new("sh", ["-c", script.as_str()]).with_start_timeout(Duration::MAX);
        let RunningJob {
            events,
            control: _control,
        } = run_within(Duration::from_secs(10), &engine, fake_job())
            .await
            .expect("`run` must not fail");
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)], "{events:?}");
    }

    /// A worker that closes its output produces no more events, and the
    /// daemon waits for its exit to tell what became of it. One that lives on
    /// would be waited for forever: the exit timeout kills it, and says why.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_closes_its_output_but_lives_on_is_killed_when_the_exit_timeout_passes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!(
            "cat >/dev/null; echo $$ >'{}'; exec >&-; exec sleep 30",
            pid_file.display()
        );
        let timeout = Duration::from_millis(300);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_exit_timeout(timeout);
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let began = Instant::now();
        let events = collected_within(Duration::from_secs(5), events).await;
        let took = began.elapsed();
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExitTimedOut)]
        );
        assert!(
            took >= timeout.saturating_sub(Duration::from_millis(50)),
            "the exit timeout fired early: {took:?}"
        );
        // Nobody stopped the job, and its control is still here: the worker
        // is gone all the same.
        let worker = pid_from(&pid_file).await;
        assert!(
            gone_soon(worker).await,
            "the worker outlived the exit timeout"
        );
        tokio::time::timeout(Duration::from_secs(5), control.stop())
            .await
            .expect("stopping a worker that was killed already hung");
    }

    /// Neither does the kill leave behind what the worker started.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_exit_timeout_also_kills_the_processes_the_worker_started() {
        let dir = tempfile::tempdir().unwrap();
        let child_file = dir.path().join("child.pid");
        // The child's output goes elsewhere, so that the worker's own
        // descriptor is the last one on the pipe.
        let script = format!(
            "cat >/dev/null; sleep 30 >/dev/null & echo $! >'{}'; exec >&-; exec sleep 30",
            child_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()])
            .with_exit_timeout(Duration::from_millis(300));
        let RunningJob {
            events,
            control: _control,
        } = run(&engine).await;
        let events = collected_within(Duration::from_secs(5), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExitTimedOut)]
        );
        let child = pid_from(&child_file).await;
        assert!(
            gone_soon(child).await,
            "the process the worker started outlived the exit timeout"
        );
    }

    /// The exit timeout bounds the wait, not the worker: one that closes its
    /// output and exits in time is reported as it always was.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_closes_its_output_and_exits_within_the_exit_timeout_keeps_its_code() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; exec >&-; sleep 0.2; exit 3"])
            .with_exit_timeout(Duration::from_secs(5));
        let RunningJob {
            events,
            control: _control,
        } = run(&engine).await;
        let events = collected_within(Duration::from_secs(10), events).await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::EngineExited {
                exit_code: Some(3)
            })]
        );
    }

    /// A stop that is under way ends the events silently, as it always does:
    /// the exit timeout passing meanwhile adds no failure to them.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_requested_while_the_exit_is_awaited_ends_the_events_without_a_failure() {
        // The worker ignores SIGTERM, so the stop is still waiting out its
        // grace period when the exit timeout passes.
        let script = "trap '' TERM; cat >/dev/null; exec >&-; exec sleep 30";
        let engine = WorkerEngine::new("sh", ["-c", script])
            .with_stop_grace(Duration::from_secs(2))
            .with_exit_timeout(Duration::from_secs(1));
        let RunningJob {
            events,
            mut control,
        } = run(&engine).await;
        let stopping = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            control.stop().await;
        };
        let (events, ()) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(events.collect::<Vec<_>>(), stopping)
        })
        .await
        .expect("the stop or the events never ended");
        assert!(
            events.is_empty(),
            "a stopped job's events ended with {events:?}"
        );
    }
}
