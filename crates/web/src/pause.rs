//! Pausing live updates (technical design §4.5, "Live updates"; A3, WCAG
//! 2.2.2): what a live page shows while the reader has paused it, how many
//! changes wait, and what is said when it resumes. Decided here, on the host;
//! the toggle button and its banner are `components::PauseUpdates`, and the
//! announcer's side is `announcer.rs`.
//!
//! The page keeps listening while it is paused: the events stream, its
//! reconnects and the re-reads an end asks for go on filling the live state,
//! and the page shows a copy of it taken when the reader paused. Resuming
//! drops the copy, so the page shows the live state at once: nothing to
//! replay, nothing missed, and no row twice.

use api::MsgArg;
use l10n::msg;
use leptos::prelude::*;

use crate::backend::OperationsState;
use crate::wording::Words;
use crate::ws::Phase;

/// The page as it was when the reader paused it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Held {
    phase: Phase,
    state: OperationsState,
}

/// What the Operations page shows: the live state and the phase of its
/// connection, or the copy of them taken when the reader paused. Reading it
/// while paused reads only the copy, so an update to the live state reaches
/// nothing the page shows.
#[derive(Clone, Copy)]
pub(crate) struct Pausable {
    state: RwSignal<OperationsState>,
    phase: RwSignal<Phase>,
    held: RwSignal<Option<Held>>,
}

impl Pausable {
    pub(crate) fn new(state: RwSignal<OperationsState>, phase: RwSignal<Phase>) -> Self {
        Self {
            state,
            phase,
            held: RwSignal::new(None),
        }
    }

    /// Holds the page as it is now. A page already paused keeps the copy it
    /// took then.
    pub(crate) fn pause(self) {
        if self.paused_untracked() {
            return;
        }
        self.held.set(Some(Held {
            phase: self.phase.get_untracked(),
            state: self.state.get_untracked(),
        }));
    }

    /// Shows the live state again.
    pub(crate) fn resume(self) {
        self.held.set(None);
    }

    /// Whether the reader has paused the page.
    pub(crate) fn paused(self) -> bool {
        self.held.with(Option::is_some)
    }

    /// Whether the page is paused, without following it: for the code that
    /// decides what to say of an update, which is no view of the page.
    pub(crate) fn paused_untracked(self) -> bool {
        self.held.with_untracked(Option::is_some)
    }

    /// The phase of the connection the page shows.
    pub(crate) fn phase(self) -> Phase {
        self.held
            .with(|held| held.as_ref().map(|held| held.phase))
            .unwrap_or_else(|| self.phase.get())
    }

    /// Reads the state the page shows.
    pub(crate) fn with<R>(self, read: impl FnOnce(&OperationsState) -> R) -> R {
        self.held.with(|held| match held {
            Some(held) => read(&held.state),
            None => self.state.with(read),
        })
    }

    /// The live state, whatever the page shows: what a cancel waits on for
    /// its run's end.
    pub(crate) fn live(self) -> RwSignal<OperationsState> {
        self.state
    }

    /// How many changes wait for the resume: each operation whose row will
    /// change, and the connection when its phase has changed. None when the
    /// page is not paused.
    pub(crate) fn pending(self) -> usize {
        self.held.with(|held| {
            held.as_ref().map_or(0, |held| {
                let rows = self.state.with(|state| state.changes_since(&held.state));
                rows.saturating_add(usize::from(self.phase.get() != held.phase))
            })
        })
    }
}

/// The runs that ended while the page was paused, which the resume tells in
/// one sentence rather than one for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ends {
    ended: u64,
    failed: u64,
}

impl Ends {
    /// Counts a run that ended with `status`.
    pub(crate) fn count(&mut self, status: &str) {
        self.ended = self.ended.saturating_add(1);
        if status == "failed" {
            self.failed = self.failed.saturating_add(1);
        }
    }
}

/// The banner of a paused page, with `pending` changes waiting.
pub(crate) fn banner(pending: usize) -> Words {
    Words {
        id: msg::UI_LIVE_PAUSED_BANNER,
        args: vec![(
            "count",
            MsgArg::Count(u64::try_from(pending).unwrap_or(u64::MAX)),
        )],
    }
}

/// What is said when the page resumes, after `ends`.
pub(crate) fn resumed(ends: Ends) -> Words {
    Words {
        id: msg::UI_LIVE_RESUMED,
        args: vec![
            ("ended", MsgArg::Count(ends.ended)),
            ("failed", MsgArg::Count(ends.failed)),
        ],
    }
}

#[cfg(test)]
mod tests {
    use api::{OperationSummaryDto, UiEvent};
    use l10n::Localizer;

    use super::*;
    use crate::routes::text_with;

    fn id(short: u32) -> String {
        format!("018f0000-0000-7000-8000-{short:012}")
    }

    fn summary(short: u32, status: &str) -> OperationSummaryDto {
        OperationSummaryDto::new(
            id(short),
            "demo-home".to_owned(),
            "backup".to_owned(),
            status.to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            Some("2026-11-09T10:00:01Z".to_owned()),
            None,
            Vec::new(),
            None,
            None,
            None,
        )
        .unwrap_or_else(|problem| panic!("a valid summary: {problem:?}"))
    }

    /// A ready page with two runs going.
    fn page() -> (Pausable, RwSignal<OperationsState>, RwSignal<Phase>) {
        let state = RwSignal::new(OperationsState::from_history(
            vec![summary(2, "running"), summary(1, "running")],
            None,
        ));
        let phase = RwSignal::new(Phase::Ready);
        (Pausable::new(state, phase), state, phase)
    }

    fn statuses(page: Pausable) -> Vec<String> {
        page.with(|state| {
            state
                .rows
                .iter()
                .map(|row| row.summary.status.clone())
                .collect()
        })
    }

    fn succeed(state: RwSignal<OperationsState>, short: u32) {
        state.update(|state| {
            state.apply(&UiEvent::OperationSucceeded {
                operation: id(short),
                finished_at: "2026-11-09T10:05:00Z".to_owned(),
            });
        });
    }

    /// The words' text, isolation marks aside.
    fn said(words: &Words) -> String {
        text_with(&Localizer::new(&[]), words.id, &words.args).replace(['\u{2066}', '\u{2069}'], "")
    }

    #[test]
    fn a_page_not_paused_shows_the_live_state() {
        let (page, state, phase) = page();
        assert!(!page.paused());
        assert!(!page.paused_untracked());
        succeed(state, 2);
        phase.set(Phase::Unreachable);
        assert_eq!(statuses(page), ["succeeded", "running"]);
        assert_eq!(page.phase(), Phase::Unreachable);
        assert_eq!(page.pending(), 0);
    }

    #[test]
    fn a_paused_page_shows_what_it_showed_when_it_paused() {
        let (page, state, phase) = page();
        page.pause();
        assert!(page.paused());
        assert!(page.paused_untracked());
        succeed(state, 2);
        phase.set(Phase::Unreachable);
        assert_eq!(statuses(page), ["running", "running"]);
        assert_eq!(page.phase(), Phase::Ready);
    }

    #[test]
    fn what_the_page_s_own_requests_change_while_paused_is_the_live_state() {
        let (page, state, _) = page();
        page.pause();
        succeed(page.live(), 2);
        assert_eq!(
            state.with_untracked(|state| state.row(&id(2)).map(|row| row.summary.status.clone())),
            Some("succeeded".to_owned())
        );
        assert_eq!(statuses(page), ["running", "running"]);
        assert_eq!(page.pending(), 1);
    }

    #[test]
    fn a_resumed_page_shows_the_live_state_at_once() {
        let (page, state, phase) = page();
        page.pause();
        succeed(state, 2);
        phase.set(Phase::Unreachable);
        page.resume();
        assert!(!page.paused());
        assert_eq!(statuses(page), ["succeeded", "running"]);
        assert_eq!(page.phase(), Phase::Unreachable);
        assert_eq!(page.pending(), 0);
    }

    #[test]
    fn pausing_a_paused_page_keeps_the_copy_it_took_first() {
        let (page, state, _) = page();
        page.pause();
        succeed(state, 2);
        page.pause();
        assert_eq!(statuses(page), ["running", "running"]);
    }

    #[test]
    fn a_paused_page_counts_each_row_that_will_change_once() {
        let (page, state, _) = page();
        page.pause();
        assert_eq!(page.pending(), 0);
        for current in 1..=3 {
            state.update(|state| {
                state.apply(&UiEvent::OperationProgress {
                    operation: id(1),
                    current,
                    total: Some(3),
                });
            });
        }
        assert_eq!(page.pending(), 1);
        succeed(state, 2);
        assert_eq!(page.pending(), 2);
    }

    #[test]
    fn a_paused_page_counts_a_change_of_its_connection_as_one() {
        let (page, _, phase) = page();
        page.pause();
        phase.set(Phase::Unreachable);
        assert_eq!(page.pending(), 1);
        phase.set(Phase::Ready);
        assert_eq!(page.pending(), 0);
    }

    #[test]
    fn ends_count_every_end_and_the_failures_among_them() {
        let mut ends = Ends::default();
        for status in [
            "succeeded",
            "failed",
            "succeeded-with-warnings",
            "cancelled",
            "failed",
        ] {
            ends.count(status);
        }
        assert_eq!(
            ends,
            Ends {
                ended: 5,
                failed: 2
            }
        );
    }

    #[test]
    fn the_banner_counts_the_changes_waiting() {
        assert_eq!(
            banner(0),
            Words {
                id: msg::UI_LIVE_PAUSED_BANNER,
                args: vec![("count", MsgArg::Count(0))],
            }
        );
        assert_eq!(
            said(&banner(0)),
            "Live updates are paused. Nothing has changed since."
        );
        assert_eq!(
            said(&banner(1)),
            "Live updates are paused. One change is waiting."
        );
        assert_eq!(
            said(&banner(12)),
            "Live updates are paused. 12 changes are waiting."
        );
    }

    #[test]
    fn the_resume_tells_how_many_runs_ended_and_how_many_failed() {
        let ends = |ended, failed| Ends { ended, failed };
        assert_eq!(
            resumed(ends(2, 1)),
            Words {
                id: msg::UI_LIVE_RESUMED,
                args: vec![("ended", MsgArg::Count(2)), ("failed", MsgArg::Count(1))],
            }
        );
        for (ended, failed, sentence) in [
            (
                0,
                0,
                "Live updates are on again. No run ended while they were paused.",
            ),
            (
                1,
                0,
                "Live updates are on again. One run ended while they were paused, and it did not fail.",
            ),
            (
                1,
                1,
                "Live updates are on again. One run ended while they were paused, and it failed.",
            ),
            (
                3,
                0,
                "Live updates are on again. 3 runs ended while they were paused, and none of them failed.",
            ),
            (
                3,
                1,
                "Live updates are on again. 3 runs ended while they were paused, and one of them failed.",
            ),
            (
                3,
                2,
                "Live updates are on again. 3 runs ended while they were paused, and 2 of them failed.",
            ),
        ] {
            assert_eq!(said(&resumed(ends(ended, failed))), sentence);
        }
    }
}
