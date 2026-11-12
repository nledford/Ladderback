//! The decisions behind the live pages (technical design §4.4): how the
//! REST history and the events stream come together into the state one page
//! shows, and when the stream is opened again. Host-tested here; the DOM
//! glue that fetches and holds the socket is `ws.rs`, which only a browser
//! exercises.
//!
//! The state model is one signal's whole value: after the initial REST
//! fetch, each event is applied to it here, so a missed event can't be
//! represented wrongly — applying an event is a pure function of the state
//! and the event, and the reconnect path re-fetches the whole history
//! instead of guessing what was lost.

use std::collections::HashMap;
use std::time::Duration;

use api::{OperationSummaryDto, OperationsPageDto, UiEvent};

/// The live progress of a running operation. Transient: never part of the
/// stored history, and gone the moment the operation ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    pub(crate) current: u64,
    pub(crate) total: Option<u64>,
}

/// One row of the operations table: what the daemon's history says about an
/// operation, with the live progress of a run that is still going.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationRow {
    pub(crate) summary: OperationSummaryDto,
    pub(crate) progress: Option<Progress>,
}

/// The state of the operations page: the history the daemon holds, newest
/// first, plus what live events have said since it was read. One
/// `RwSignal<OperationsState>` holds all of it (§4.4), so a render can't
/// tear between the history and the live part of a row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OperationsState {
    pub(crate) rows: Vec<OperationRow>,
    /// The keyset cursor older operations follow, when any remain.
    pub(crate) next: Option<String>,
}

impl OperationsState {
    pub(crate) fn from_history(operations: Vec<OperationSummaryDto>, next: Option<String>) -> Self {
        Self {
            rows: operations
                .into_iter()
                .map(|summary| OperationRow {
                    summary,
                    progress: None,
                })
                .collect(),
            next,
        }
    }

    /// Applies one live event. An event about an operation the state doesn't
    /// hold is ignored: the missed part of the story is what the next
    /// initial fetch brings back, never a half-invented row. Nor does an
    /// event take a row back: the page reads the history again once its
    /// stream is open (#305), so news told before that read can arrive after
    /// it. A start moves only a queued row, and progress and warnings reach
    /// only a row that has not ended.
    pub(crate) fn apply(&mut self, event: &UiEvent) {
        match event {
            UiEvent::OperationStarted {
                operation,
                started_at,
                ..
            } => {
                if let Some(row) = self.row_mut(operation)
                    && rank(&row.summary.status) == QUEUED
                {
                    row.summary.status = "running".to_owned();
                    row.summary.started_at = Some(started_at.clone());
                }
            }
            UiEvent::OperationProgress {
                operation,
                current,
                total,
            } => {
                if let Some(row) = self.live_row_mut(operation) {
                    row.progress = Some(Progress {
                        current: *current,
                        total: *total,
                    });
                }
            }
            UiEvent::OperationWarning {
                operation, code, ..
            } => {
                if let Some(row) = self.live_row_mut(operation) {
                    row.summary.warning_codes.push(*code);
                }
            }
            UiEvent::OperationSucceeded {
                operation,
                finished_at,
            } => {
                if let Some(row) = self.row_mut(operation) {
                    // A run that warned its way to the end is its own status,
                    // not a plain success the warnings merely accompany.
                    row.summary.status = if row.summary.warning_codes.is_empty() {
                        "succeeded".to_owned()
                    } else {
                        "succeeded-with-warnings".to_owned()
                    };
                    row.summary.finished_at = Some(finished_at.clone());
                    row.progress = None;
                }
            }
            UiEvent::OperationFailed {
                operation,
                code,
                finished_at,
                ..
            } => {
                if let Some(row) = self.row_mut(operation) {
                    row.summary.status = "failed".to_owned();
                    row.summary.failure_code = Some(*code);
                    row.summary.finished_at = Some(finished_at.clone());
                    row.progress = None;
                }
            }
            // The fleet's comings and goings are no row of this table.
            UiEvent::AgentOnline { .. } | UiEvent::AgentOffline { .. } => {}
        }
    }

    /// Merges a page the page just re-read: the settle-poll after a cancel
    /// (a cancelled run emits no `UiEvent`), the re-read a run's end asks for,
    /// the one each opened stream starts and the refetch a reconnect starts
    /// from. The page is the source of truth for what it holds, but it never
    /// regresses a status an event advanced — the store can answer with what
    /// it held before the news the stream told since — and a live row the
    /// page doesn't hold yet stays, newest: dropping it would unshow a run
    /// the reader is watching.
    pub(crate) fn merge(&mut self, page: OperationsPageDto) {
        let mut held: HashMap<String, OperationRow> = self
            .rows
            .drain(..)
            .map(|row| (row.summary.id.clone(), row))
            .collect();
        let mut rows = Vec::with_capacity(page.operations.len());
        for summary in page.operations {
            let live = held.remove(&summary.id);
            rows.push(match live {
                // The held row is at least as far along as the page says:
                // what the events said wins, progress included. An end that
                // came by an event carries no result, so the page's is kept.
                Some(mut row) if rank(&row.summary.status) >= rank(&summary.status) => {
                    row.summary.result = row.summary.result.take().or(summary.result);
                    row
                }
                _ => OperationRow {
                    summary,
                    progress: None,
                },
            });
        }
        // What is still held is not in the page: keep only the runs it could
        // be ahead of, and keep them first, because a run the page has not
        // written yet is the newest thing on the page.
        let mut live: Vec<OperationRow> = held
            .into_values()
            .filter(|row| rank(&row.summary.status) < TERMINAL)
            .collect();
        // The order of a map is nobody's: a run the page has not written
        // yet is newest, and between two of them the queue decides.
        live.sort_by(|a, b| {
            (a.summary.queued_at.as_str(), a.summary.id.as_str())
                .cmp(&(b.summary.queued_at.as_str(), b.summary.id.as_str()))
        });
        live.reverse();
        rows.splice(0..0, live);
        self.rows = rows;
        self.next = page.next;
    }

    /// The row of `operation`, when the state holds it.
    fn row_mut(&mut self, operation: &str) -> Option<&mut OperationRow> {
        self.rows.iter_mut().find(|row| row.summary.id == operation)
    }

    /// The row of `operation`, when the state holds it and it has not ended.
    fn live_row_mut(&mut self, operation: &str) -> Option<&mut OperationRow> {
        self.row_mut(operation)
            .filter(|row| rank(&row.summary.status) < TERMINAL)
    }

    /// The row of `operation`, when the state holds it: how a row's view reads
    /// what it shows, so that an update to another row does not reach it.
    pub(crate) fn row(&self, operation: &str) -> Option<&OperationRow> {
        self.rows.iter().find(|row| row.summary.id == operation)
    }

    /// How many operations this state shows otherwise than `shown` does: a
    /// row that changed, a row `shown` lacks and a row this one lacks each
    /// count once, however many events changed it. What a paused page's
    /// banner counts (§4.5): the rows that change when it resumes.
    pub(crate) fn changes_since(&self, shown: &Self) -> usize {
        let mut before: HashMap<&str, &OperationRow> = shown
            .rows
            .iter()
            .map(|row| (row.summary.id.as_str(), row))
            .collect();
        let changed = self
            .rows
            .iter()
            .filter(|row| before.remove(row.summary.id.as_str()) != Some(*row))
            .count();
        changed.saturating_add(before.len())
    }
}

/// Where a status stands in an operation's life: still `QUEUED`, running, or
/// ended. Every ended status is `TERMINAL`; only their common rank matters
/// here, because an operation ends once.
const QUEUED: u8 = 0;
const TERMINAL: u8 = 2;

fn rank(status: &str) -> u8 {
    match status {
        "queued" => QUEUED,
        "running" => 1,
        _ => TERMINAL,
    }
}

/// How long to wait before trying the events stream again after the
/// `attempt`-th failure, counting from 1: doubling from one second, capped
/// at thirty (§4.4). A cap keeps an unreachable daemon from backing off into
/// silence while keeping the load on it bounded.
pub(crate) fn reconnect_backoff(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << (attempt - 1).min(SHIFTS_TO_THE_CAP)).min(30))
}

/// The doublings it takes to pass the thirty-second cap: five of them, so
/// the sixth attempt and every later one wait the cap.
const SHIFTS_TO_THE_CAP: u32 = 5;

/// The events stream's URL: the page's own origin, over WebSocket. `http` is
/// the prefix of `https` too, so both schemes become their WebSocket one.
pub(crate) fn events_url(base: &str) -> String {
    format!("{}/api/v1/events", base.trim_end_matches('/')).replacen("http", "ws", 1)
}

/// Whether `status` is an operation's last: anything but queued or running.
/// The announcement effect and the settle-poll both ask, so the answer lives
/// here, host-tested, and not twice.
pub(crate) fn ended(status: &str) -> bool {
    rank(status) >= TERMINAL
}

/// Whether a POST's answer is a success: any 2xx. An operation is accepted
/// with 202, a repository that was tested with 200 and one that was connected
/// or created with 201; anything else is a refusal, with a typed body.
pub(crate) fn succeeded(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Whether `event` asks the page to read its operations again: the end of a
/// run. The event carries no result, and the run tells of its end only after
/// its last write to the log (§4.4), so the re-read finds the end there with
/// what the run produced, and the row's Result shows without a reload. A run
/// started elsewhere, which the page did not hold, comes in with it.
pub(crate) fn asks_for_a_reread(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::OperationSucceeded { .. } | UiEvent::OperationFailed { .. }
    )
}

/// How long a re-read waits for the history before it counts as no answer: a
/// fetch that hangs would otherwise keep the one re-read out for good.
pub(crate) const REREAD_PATIENCE: Duration = Duration::from_secs(10);

/// How many times a re-read that got no history is tried again before the
/// page waits for the next end, or the stream's re-fetch, to read it.
const REREAD_RETRIES: u32 = 3;

/// The page's re-reads of its operations, one at a time. An end, or the
/// events stream opening, that arrives while one is out may be newer than
/// what that one reads, so it is owed one more, after it; any number of them
/// then are owed that same one, because a read finds everything told before
/// it. A re-read that gets no history is tried again, later each time, a few
/// times: the end it was for carries no result, so without it the row would
/// show none until the page is reloaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rereads {
    /// A re-read is out, or waits to be tried again.
    reading: bool,
    /// An end, or a stream opening, arrived while it was out: one more is owed.
    owed: bool,
    /// How many tries in a row got no history.
    failed: u32,
}

impl Rereads {
    /// An end arrived, or the events stream opened: whether to start a
    /// re-read now. With one out, the answer is no, and one more is owed
    /// after it.
    pub(crate) fn ask(&mut self) -> bool {
        if self.reading {
            self.owed = true;
            return false;
        }
        self.reading = true;
        true
    }

    /// The re-read that was out is done, `answered` with the history or not:
    /// how long to wait before the next one, or `None` for no next one. A try
    /// again after no answer reads every end told before it, so it is the one
    /// owed too; once the tries run out, only an end owed meanwhile reads again.
    pub(crate) fn finish(&mut self, answered: bool) -> Option<Duration> {
        if answered {
            self.failed = 0;
        } else if self.failed < REREAD_RETRIES {
            self.failed += 1;
            self.owed = false;
            return Some(Duration::from_secs(1 << (self.failed - 1)));
        } else {
            self.failed = 0;
        }
        self.reading = std::mem::take(&mut self.owed);
        self.reading.then_some(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    // The decisions of §4.4, one test each; commit 1 wrote the red ones
    // against the stubs, and the merge and failed/warning tests came with
    // the code that passes them.

    use std::time::Duration;

    use super::{
        OperationsState, Progress, Rereads, asks_for_a_reread, ended, events_url,
        reconnect_backoff, succeeded,
    };
    use api::{
        ErrorCode, OperationResultDto, OperationSummaryDto, OperationsPageDto, UiEvent, WarningCode,
    };

    /// The full operation ID of a test operation.
    fn id(short: &str) -> String {
        format!("018f0000-0000-7000-8000-{short:>012}")
    }

    /// A summary the daemon could have sent: `status` is the operation's
    /// life so far.
    fn summary(short: &str, status: &str, started: bool, finished: bool) -> OperationSummaryDto {
        OperationSummaryDto::new(
            id(short),
            "demo-home".to_owned(),
            "backup".to_owned(),
            status.to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            started.then(|| "2026-11-09T10:00:01Z".to_owned()),
            finished.then(|| "2026-11-09T10:05:00Z".to_owned()),
            Vec::new(),
            None,
            None,
            None,
        )
        .unwrap_or_else(|problem| panic!("a valid summary: {problem:?}"))
    }

    /// A finished backup of three files, as the history holds it.
    fn succeeded_backup(short: &str) -> OperationSummaryDto {
        OperationSummaryDto::new(
            id(short),
            "demo-home".to_owned(),
            "backup".to_owned(),
            "succeeded".to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            Some("2026-11-09T10:00:01Z".to_owned()),
            Some("2026-11-09T10:05:00Z".to_owned()),
            Vec::new(),
            None,
            Some(OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
                snapshot: Some("cd".repeat(32)),
                bytes_processed: Some(65_536),
            }),
            None,
        )
        .unwrap_or_else(|problem| panic!("a valid summary: {problem:?}"))
    }

    const FIRST: &str = "000000000001";
    const SECOND: &str = "000000000002";

    #[test]
    fn a_state_shows_no_change_since_itself() {
        let state = OperationsState::from_history(
            vec![
                summary(SECOND, "running", true, false),
                succeeded_backup(FIRST),
            ],
            None,
        );
        assert_eq!(state.changes_since(&state.clone()), 0);
    }

    #[test]
    fn a_row_that_changed_is_one_change_however_many_events_changed_it() {
        let shown = OperationsState::from_history(
            vec![
                summary(SECOND, "running", true, false),
                succeeded_backup(FIRST),
            ],
            None,
        );
        let mut live = shown.clone();
        for current in 1..=5 {
            live.apply(&UiEvent::OperationProgress {
                operation: id(SECOND),
                current,
                total: Some(5),
            });
        }
        assert_eq!(live.changes_since(&shown), 1);
        live.apply(&UiEvent::OperationSucceeded {
            operation: id(SECOND),
            finished_at: "2026-11-09T10:05:00Z".to_owned(),
        });
        assert_eq!(live.changes_since(&shown), 1);
    }

    #[test]
    fn a_row_that_came_and_a_row_that_went_are_a_change_each() {
        let shown = OperationsState::from_history(
            vec![
                summary(SECOND, "running", true, false),
                succeeded_backup(FIRST),
            ],
            None,
        );
        let came = OperationsState::from_history(
            vec![
                summary("000000000003", "queued", false, false),
                summary(SECOND, "running", true, false),
                succeeded_backup(FIRST),
            ],
            None,
        );
        assert_eq!(came.changes_since(&shown), 1);
        let went =
            OperationsState::from_history(vec![summary(SECOND, "running", true, false)], None);
        assert_eq!(went.changes_since(&shown), 1);
        let both = OperationsState::from_history(
            vec![
                summary("000000000003", "queued", false, false),
                summary(SECOND, "succeeded", true, true),
            ],
            None,
        );
        assert_eq!(both.changes_since(&shown), 3);
    }

    #[test]
    fn the_history_arrives_newest_first_with_its_cursor() {
        let state = OperationsState::from_history(
            vec![succeeded_backup(SECOND), succeeded_backup(FIRST)],
            Some(id(SECOND)),
        );
        assert_eq!(
            state
                .rows
                .iter()
                .map(|row| row.summary.id.as_str())
                .collect::<Vec<_>>(),
            [id(SECOND).as_str(), id(FIRST).as_str()],
            "the newest operation is the first row"
        );
        assert_eq!(state.rows[0].progress, None);
        assert_eq!(state.next, Some(id(SECOND)));
    }

    #[test]
    fn an_empty_history_is_an_empty_state() {
        let state = OperationsState::from_history(Vec::new(), None);
        assert!(state.rows.is_empty());
        assert_eq!(state.next, None);
    }

    #[test]
    fn a_started_event_marks_its_row_running() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "queued", false, false)], None);
        state.apply(&UiEvent::OperationStarted {
            operation: id(FIRST),
            repository: Some("demo".to_owned()),
            started_at: "2026-11-09T10:00:01Z".to_owned(),
        });
        let row = &state.rows[0];
        assert_eq!(row.summary.status, "running");
        assert_eq!(
            row.summary.started_at.as_deref(),
            Some("2026-11-09T10:00:01Z")
        );
    }

    // The page reads the history once its events stream is open (#305), so
    // news told before that read can reach the page after it: what the read
    // already has, the news must not take back or count twice.

    #[test]
    fn a_start_heard_after_a_read_that_has_the_run_further_along_changes_nothing() {
        for (status, finished) in [
            ("running", false),
            ("succeeded", true),
            ("failed", true),
            ("cancelled", true),
        ] {
            let mut state =
                OperationsState::from_history(vec![summary(FIRST, status, true, finished)], None);
            let read = state.clone();
            state.apply(&UiEvent::OperationStarted {
                operation: id(FIRST),
                repository: Some("demo".to_owned()),
                started_at: "2026-11-09T10:00:09Z".to_owned(),
            });
            assert_eq!(state, read, "a {status} row");
        }
    }

    #[test]
    fn progress_and_warnings_heard_after_a_read_that_has_the_end_change_nothing() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "succeeded", true, true)], None);
        state.rows[0].summary.status = "succeeded-with-warnings".to_owned();
        state.rows[0].summary.warning_codes = vec![WarningCode::SourceUnreadable];
        let read = state.clone();
        state.apply(&UiEvent::OperationProgress {
            operation: id(FIRST),
            current: 3,
            total: Some(10),
        });
        state.apply(&UiEvent::OperationWarning {
            operation: id(FIRST),
            code: WarningCode::SourceUnreadable,
            args: Vec::new(),
        });
        assert_eq!(state, read);
    }

    #[test]
    fn a_progress_event_updates_only_the_live_part_of_the_row() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.apply(&UiEvent::OperationProgress {
            operation: id(FIRST),
            current: 3,
            total: Some(10),
        });
        assert_eq!(
            state.rows[0].progress,
            Some(Progress {
                current: 3,
                total: Some(10)
            })
        );
        assert_eq!(state.rows[0].summary.status, "running");
    }

    #[test]
    fn a_succeeded_event_ends_the_row_and_its_progress_with_it() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.rows[0].progress = Some(Progress {
            current: 9,
            total: Some(10),
        });
        state.apply(&UiEvent::OperationSucceeded {
            operation: id(FIRST),
            finished_at: "2026-11-09T10:05:00Z".to_owned(),
        });
        let row = &state.rows[0];
        assert_eq!(row.summary.status, "succeeded");
        assert_eq!(
            row.summary.finished_at.as_deref(),
            Some("2026-11-09T10:05:00Z")
        );
        assert_eq!(row.progress, None, "progress is transient");
    }

    #[test]
    fn an_event_about_an_operation_the_state_does_not_hold_is_ignored() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "queued", false, false)], None);
        let before = state.clone();
        for event in [
            UiEvent::OperationStarted {
                operation: id(SECOND),
                repository: None,
                started_at: "2026-11-09T10:00:01Z".to_owned(),
            },
            UiEvent::OperationProgress {
                operation: id(SECOND),
                current: 1,
                total: None,
            },
        ] {
            state.apply(&event);
        }
        assert_eq!(state, before, "no half-invented row");
    }

    #[test]
    fn a_row_is_found_by_its_operations_id() {
        let state = OperationsState::from_history(
            vec![succeeded_backup(SECOND), succeeded_backup(FIRST)],
            None,
        );
        assert_eq!(state.row(&id(SECOND)), Some(&state.rows[0]));
        assert_eq!(state.row(&id(FIRST)), Some(&state.rows[1]));
        assert_eq!(
            state.row(&id("3")),
            None,
            "a run the state does not hold has no row"
        );
    }

    #[test]
    fn a_failed_event_ends_the_row_with_its_code() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.apply(&UiEvent::OperationFailed {
            operation: id(FIRST),
            code: ErrorCode::EngineUnavailable,
            args: Vec::new(),
            finished_at: "2026-11-09T10:05:00Z".to_owned(),
        });
        let row = &state.rows[0];
        assert_eq!(row.summary.status, "failed");
        assert_eq!(row.summary.failure_code, Some(ErrorCode::EngineUnavailable));
        assert_eq!(
            row.summary.finished_at.as_deref(),
            Some("2026-11-09T10:05:00Z")
        );
        assert_eq!(row.progress, None, "progress is transient");
    }

    #[test]
    fn a_warning_is_recorded_and_a_success_after_warnings_is_its_own_status() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.apply(&UiEvent::OperationWarning {
            operation: id(FIRST),
            code: WarningCode::SourceUnreadable,
            args: Vec::new(),
        });
        assert_eq!(
            state.rows[0].summary.warning_codes,
            [WarningCode::SourceUnreadable]
        );
        state.apply(&UiEvent::OperationSucceeded {
            operation: id(FIRST),
            finished_at: "2026-11-09T10:05:00Z".to_owned(),
        });
        assert_eq!(state.rows[0].summary.status, "succeeded-with-warnings");
    }

    #[test]
    fn a_merged_page_never_regresses_a_status_an_event_advanced() {
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "queued", false, false)], None);
        state.apply(&UiEvent::OperationStarted {
            operation: id(FIRST),
            repository: Some("demo".to_owned()),
            started_at: "2026-11-09T10:00:01Z".to_owned(),
        });
        state.apply(&UiEvent::OperationProgress {
            operation: id(FIRST),
            current: 3,
            total: Some(10),
        });
        // The store's write of the start can lag the event: the re-read still
        // says queued.
        state.merge(page(vec![summary(FIRST, "queued", false, false)], None));
        let row = &state.rows[0];
        assert_eq!(row.summary.status, "running");
        assert_eq!(
            row.summary.started_at.as_deref(),
            Some("2026-11-09T10:00:01Z")
        );
        assert_eq!(
            row.progress,
            Some(Progress {
                current: 3,
                total: Some(10)
            }),
            "the live part of the row survives the merge"
        );
    }

    #[test]
    fn a_merged_page_adopts_the_end_the_store_has_and_the_page_does_not_hold() {
        // The settle-poll after a cancel: no event ends a cancelled run, the
        // re-read is where the end comes from.
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.merge(page(vec![summary(FIRST, "cancelled", true, true)], None));
        assert_eq!(state.rows[0].summary.status, "cancelled");

        // A run the events stream holds that the page has not written yet
        // stays, first, because it is the newest thing on the page.
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.merge(page(vec![succeeded_backup(SECOND)], Some(id(SECOND))));
        assert_eq!(
            state
                .rows
                .iter()
                .map(|row| row.summary.id.as_str())
                .collect::<Vec<_>>(),
            [id(FIRST).as_str(), id(SECOND).as_str()],
            "the run the page does not hold yet stays, newest"
        );
        assert_eq!(state.rows[0].summary.status, "running");
        assert_eq!(state.next, Some(id(SECOND)));
    }

    /// A page of the history, as the daemon answers it.
    fn page(operations: Vec<OperationSummaryDto>, next: Option<String>) -> OperationsPageDto {
        OperationsPageDto { operations, next }
    }

    #[test]
    fn the_backoff_doubles_from_one_second_and_stops_at_thirty() {
        assert_eq!(reconnect_backoff(1), Duration::from_secs(1));
        assert_eq!(reconnect_backoff(2), Duration::from_secs(2));
        assert_eq!(reconnect_backoff(3), Duration::from_secs(4));
        assert_eq!(reconnect_backoff(6), Duration::from_secs(30));
        assert_eq!(
            reconnect_backoff(60),
            Duration::from_secs(30),
            "the cap holds"
        );
    }
    #[test]
    fn the_events_url_is_the_origin_over_websocket() {
        assert_eq!(
            events_url("http://127.0.0.1:8080"),
            "ws://127.0.0.1:8080/api/v1/events"
        );
        assert_eq!(
            events_url("https://demo.example"),
            "wss://demo.example/api/v1/events"
        );
        assert_eq!(
            events_url("https://demo.example/"),
            "wss://demo.example/api/v1/events",
            "a trailing slash is not doubled"
        );
    }

    #[test]
    fn ended_names_the_statuses_a_run_ends_with() {
        for status in ["queued", "running"] {
            assert!(!ended(status), "{status} is still going");
        }
        for status in [
            "succeeded",
            "succeeded-with-warnings",
            "cancelled",
            "failed",
        ] {
            assert!(ended(status), "{status} is an end");
        }
    }

    #[test]
    fn a_reconnects_refetch_corrects_a_stale_row_and_keeps_an_advanced_one() {
        // FIRST was read before the disconnect and ended while the page was
        // away; SECOND was advanced by an event the store had not written
        // when the socket dropped. The re-fetch adopts FIRST's end and
        // leaves SECOND ahead of the store, newest.
        let mut state = OperationsState::from_history(
            vec![
                summary(SECOND, "queued", false, false),
                summary(FIRST, "running", true, false),
            ],
            None,
        );
        state.apply(&UiEvent::OperationStarted {
            operation: id(SECOND),
            repository: Some("demo".to_owned()),
            started_at: "2026-11-09T10:00:02Z".to_owned(),
        });
        state.apply(&UiEvent::OperationProgress {
            operation: id(SECOND),
            current: 4,
            total: Some(9),
        });
        let mut failed = summary(FIRST, "failed", true, true);
        failed.failure_code = Some(ErrorCode::RepositoryUnknown);
        state.merge(page(vec![failed], None));
        let (second, first) = (&state.rows[0], &state.rows[1]);
        assert_eq!(
            first.summary.status, "failed",
            "the re-read corrects the row that went stale"
        );
        assert_eq!(
            second.summary.status, "running",
            "an event's advance survives the re-read"
        );
        assert_eq!(
            second.summary.started_at.as_deref(),
            Some("2026-11-09T10:00:02Z")
        );
        assert_eq!(
            second.progress,
            Some(Progress {
                current: 4,
                total: Some(9)
            })
        );
    }

    #[test]
    fn a_merged_page_drops_a_run_that_already_ended_and_it_does_not_hold() {
        // The re-read is the truth for what it holds: a run that ended while
        // the page was away and has already left the store's first page is
        // gone, not kept forever as a zombie.
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "succeeded", true, true)], None);
        state.merge(page(vec![succeeded_backup(SECOND)], None));
        assert_eq!(state.rows.len(), 1);
        assert_eq!(state.rows[0].summary.id, id(SECOND));
    }

    #[test]
    fn a_merged_page_gives_a_run_that_ended_live_the_result_the_store_has() {
        // The end came by an event, which carries no result; the re-read
        // that follows is where the row learns what the run produced.
        let mut state =
            OperationsState::from_history(vec![summary(FIRST, "running", true, false)], None);
        state.apply(&UiEvent::OperationSucceeded {
            operation: id(FIRST),
            finished_at: "2026-11-09T10:05:00Z".to_owned(),
        });
        assert_eq!(state.rows[0].summary.result, None);
        state.merge(page(vec![succeeded_backup(FIRST)], None));
        assert_eq!(
            state.rows[0].summary.result,
            succeeded_backup(FIRST).result,
            "the row shows what the store says the run produced"
        );
        assert_eq!(state.rows[0].summary.status, "succeeded");
    }

    #[test]
    fn every_2xx_answer_is_a_success_and_nothing_else_is() {
        for status in [200, 201, 202, 204, 299] {
            assert!(succeeded(status), "{status}");
        }
        for status in [0, 100, 199, 300, 400, 409, 422, 500, 503] {
            assert!(!succeeded(status), "{status}");
        }
    }

    #[test]
    fn the_end_of_a_run_asks_for_a_reread_and_nothing_else_does() {
        let operation = id(FIRST);
        let finished_at = "2026-11-09T10:05:00Z".to_owned();
        assert!(asks_for_a_reread(&UiEvent::OperationSucceeded {
            operation: operation.clone(),
            finished_at: finished_at.clone(),
        }));
        assert!(asks_for_a_reread(&UiEvent::OperationFailed {
            operation: operation.clone(),
            code: ErrorCode::EngineUnavailable,
            args: Vec::new(),
            finished_at,
        }));
        for event in [
            UiEvent::OperationStarted {
                operation: operation.clone(),
                repository: None,
                started_at: "2026-11-09T10:00:01Z".to_owned(),
            },
            UiEvent::OperationProgress {
                operation: operation.clone(),
                current: 1,
                total: Some(3),
            },
            UiEvent::OperationWarning {
                operation,
                code: WarningCode::SourceUnreadable,
                args: Vec::new(),
            },
            UiEvent::AgentOnline {
                agent: "demo-agent".to_owned(),
                at: "2026-11-09T10:00:00Z".to_owned(),
            },
            UiEvent::AgentOffline {
                agent: "demo-agent".to_owned(),
                at: "2026-11-09T10:00:00Z".to_owned(),
            },
        ] {
            assert!(!asks_for_a_reread(&event), "{event:?}");
        }
    }

    /// A `Rereads::finish` that starts the next re-read at once.
    const AT_ONCE: Option<Duration> = Some(Duration::ZERO);

    #[test]
    fn an_end_starts_a_reread_when_none_is_out() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask(), "the first end reads at once");
        assert_eq!(
            rereads.finish(true),
            None,
            "an end told before it is in what it read"
        );
        assert!(rereads.ask(), "a later end reads again");
    }

    #[test]
    fn ends_during_a_reread_are_owed_one_more_and_only_one() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask());
        assert!(!rereads.ask(), "one re-read is out at a time");
        assert!(!rereads.ask(), "a third end shares the one owed");
        assert_eq!(
            rereads.finish(true),
            AT_ONCE,
            "the owed re-read starts as the first ends"
        );
        assert!(!rereads.ask(), "and it is out in turn");
        assert_eq!(
            rereads.finish(true),
            AT_ONCE,
            "an end during it is owed one more"
        );
        assert_eq!(
            rereads.finish(true),
            None,
            "with nothing owed, the reading stops"
        );
        assert!(rereads.ask(), "and the next end starts it again");
    }

    #[test]
    fn a_reread_that_gets_no_history_is_tried_again_three_times_later_each_time() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask());
        assert_eq!(rereads.finish(false), Some(Duration::from_secs(1)));
        assert!(!rereads.ask(), "a try that waits is still out");
        assert_eq!(rereads.finish(false), Some(Duration::from_secs(2)));
        assert_eq!(rereads.finish(false), Some(Duration::from_secs(4)));
        assert_eq!(rereads.finish(false), None, "the tries ran out");
        assert!(rereads.ask(), "and the next end reads again");
    }

    #[test]
    fn a_reread_that_gets_the_history_starts_the_tries_over() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask());
        assert_eq!(rereads.finish(false), Some(Duration::from_secs(1)));
        assert_eq!(rereads.finish(true), None);
        assert!(rereads.ask());
        assert_eq!(
            rereads.finish(false),
            Some(Duration::from_secs(1)),
            "the first try again of a new re-read waits the least"
        );
    }

    #[test]
    fn a_try_again_reads_the_ends_told_during_the_try_that_got_nothing() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask());
        assert!(!rereads.ask(), "an end during the read is owed");
        assert_eq!(rereads.finish(false), Some(Duration::from_secs(1)));
        assert_eq!(
            rereads.finish(true),
            None,
            "the try again came after that end, so nothing more is owed"
        );
    }

    #[test]
    fn once_the_tries_run_out_an_end_told_during_the_last_reads_again_with_tries_of_its_own() {
        let mut rereads = Rereads::default();
        assert!(rereads.ask());
        for _ in 0..3 {
            assert!(rereads.finish(false).is_some());
        }
        assert!(!rereads.ask(), "an end during the last try");
        assert_eq!(rereads.finish(false), AT_ONCE);
        assert_eq!(
            rereads.finish(false),
            Some(Duration::from_secs(1)),
            "its own tries start over"
        );
    }
}
