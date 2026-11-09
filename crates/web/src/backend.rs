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

use std::time::Duration;

use api::{OperationSummaryDto, UiEvent};

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
    /// The state after the initial REST fetch, which is also the state a
    /// reconnect returns to: the REST history is the source of truth, and
    /// the stream resumes onto it.
    #[allow(clippy::todo)]
    pub(crate) fn from_history(operations: Vec<OperationSummaryDto>, next: Option<String>) -> Self {
        let _ = (operations, next);
        todo!("commit 1 stub: the red tests in this module say what it does")
    }

    /// Applies one live event. An event about an operation the state doesn't
    /// hold is ignored: the missed part of the story is what the next
    /// initial fetch brings back, never a half-invented row.
    #[allow(clippy::todo)]
    pub(crate) fn apply(&mut self, event: &UiEvent) {
        let _ = event;
        todo!("commit 1 stub: the red tests in this module say what it does")
    }
}

/// How long to wait before trying the events stream again after the
/// `attempt`-th failure, counting from 1: doubling from one second, capped
/// at thirty (§4.4). A cap keeps an unreachable daemon from backing off into
/// silence while keeping the load on it bounded.
#[allow(clippy::todo)]
pub(crate) fn reconnect_backoff(attempt: u32) -> Duration {
    let _ = attempt;
    todo!("commit 1 stub: the red tests in this module say what it does")
}

#[cfg(test)]
mod tests {
    // Every test here is red against the `todo!()` stubs above: commit 1 of
    // the live-pages slice. The tests say what the decisions are; the code
    // that passes them is the next commits'.

    use std::time::Duration;

    use super::{OperationsState, Progress, reconnect_backoff};
    use api::{OperationResultDto, OperationSummaryDto, UiEvent};

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
            }),
        )
        .unwrap_or_else(|problem| panic!("a valid summary: {problem:?}"))
    }

    const FIRST: &str = "000000000001";
    const SECOND: &str = "000000000002";

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
}
