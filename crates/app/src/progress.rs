//! What a page is shown of a running operation's progress (technical design
//! §4.4; §16, decision 82). The engine reports any number of progress bars,
//! each by a key, and names the step each measures when it knows it; a page
//! shows one: the bar begun last that is still open, with its step and how
//! far it has come. An older bar still open moves on unseen, and shows again
//! once it is the newest open bar and moves.

use api::UiEvent;

use crate::ports::{OperationStep, ProgressEvent};

/// The most bars of one run kept at once. rustic leaves some bars open for
/// good (a counter it never finishes), and a worker's events are input from
/// outside the process, so the oldest open bar goes when one more begins.
pub const MAX_OPEN_BARS: usize = 64;

/// What a page is shown of a run's progress: the step of the bar it sees,
/// and how far that bar has come, `current` of `total` when it knows one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressShown {
    pub step: Option<OperationStep>,
    pub current: u64,
    pub total: Option<u64>,
}

impl ProgressShown {
    /// The live update that tells a page of `operation`'s progress.
    #[must_use]
    pub fn ui(self, operation: String) -> UiEvent {
        todo!()
    }
}

/// The open bars of one run, oldest first: the last is the one shown.
#[derive(Debug, Default)]
pub struct LiveProgress {
    bars: Vec<(u32, ProgressShown)>,
}

impl LiveProgress {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one change of a bar, and returns what the page is to be shown
    /// when the change is to the bar it shows: a bar beginning (at nothing,
    /// with no total), growing, learning its length or naming its step. A
    /// bar finishing shows nothing: the page keeps what it was last shown
    /// until the next bar begins or moves. A change to a bar never begun,
    /// or to an older open bar, shows nothing either.
    pub fn apply(&mut self, event: &ProgressEvent) -> Option<ProgressShown> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ProgressKind;

    fn begin(key: u32, step: Option<OperationStep>) -> ProgressEvent {
        ProgressEvent::Begin {
            key,
            kind: ProgressKind::Counter,
            step,
        }
    }

    fn shown(step: Option<OperationStep>, current: u64, total: Option<u64>) -> ProgressShown {
        ProgressShown {
            step,
            current,
            total,
        }
    }

    #[test]
    fn a_bar_begins_at_nothing_of_no_total_in_its_step() {
        let mut live = LiveProgress::new();
        assert_eq!(
            live.apply(&begin(3, Some(OperationStep::BackingUp))),
            Some(shown(Some(OperationStep::BackingUp), 0, None))
        );
        assert_eq!(
            live.apply(&ProgressEvent::Length { key: 3, len: 10 }),
            Some(shown(Some(OperationStep::BackingUp), 0, Some(10)))
        );
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 3, by: 4 }),
            Some(shown(Some(OperationStep::BackingUp), 4, Some(10)))
        );
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 3, by: 2 }),
            Some(shown(Some(OperationStep::BackingUp), 6, Some(10)))
        );
    }

    /// rustic begins the index read as a counter with no name and names it
    /// with a title (`index.rs`): the title names the step, and the count and
    /// the total stay.
    #[test]
    fn a_title_names_the_step_of_its_bar_and_keeps_its_count() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, None));
        live.apply(&ProgressEvent::Length { key: 0, len: 10 });
        live.apply(&ProgressEvent::Advance { key: 0, by: 3 });
        assert_eq!(
            live.apply(&ProgressEvent::Title {
                key: 0,
                step: Some(OperationStep::ReadingIndex),
            }),
            Some(shown(Some(OperationStep::ReadingIndex), 3, Some(10)))
        );
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 0, by: 1 }),
            Some(shown(Some(OperationStep::ReadingIndex), 4, Some(10)))
        );
    }

    /// A title in words the adapter doesn't know is no step: the step the bar
    /// had is no longer what it measures.
    #[test]
    fn a_title_the_adapter_doesnt_know_takes_the_step_away() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, Some(OperationStep::ReadingIndex)));
        assert_eq!(
            live.apply(&ProgressEvent::Title { key: 0, step: None }),
            Some(shown(None, 0, None))
        );
    }

    /// The index's counter is not the backup's: once the backup's own bar
    /// begins, the page sees it, and the counter moving shows nothing.
    #[test]
    fn the_bar_begun_last_is_the_one_shown() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, Some(OperationStep::ReadingIndex)));
        live.apply(&ProgressEvent::Length { key: 0, len: 10 });
        assert_eq!(
            live.apply(&begin(1, Some(OperationStep::BackingUp))),
            Some(shown(Some(OperationStep::BackingUp), 0, None))
        );
        assert_eq!(live.apply(&ProgressEvent::Advance { key: 0, by: 5 }), None);
        assert_eq!(live.apply(&ProgressEvent::Length { key: 0, len: 20 }), None);
        assert_eq!(
            live.apply(&ProgressEvent::Title {
                key: 0,
                step: Some(OperationStep::GettingSnapshots),
            }),
            None
        );
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 1, by: 8 }),
            Some(shown(Some(OperationStep::BackingUp), 8, None))
        );
    }

    /// A finished bar shows nothing: the page keeps what it last saw. The bar
    /// before it, still open, shows again once it moves, as it moved meanwhile.
    #[test]
    fn a_finished_bar_shows_nothing_and_the_open_bar_before_it_shows_when_it_moves() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, Some(OperationStep::ListingPacks)));
        live.apply(&ProgressEvent::Advance { key: 0, by: 2 });
        live.apply(&begin(1, Some(OperationStep::ReadingIndex)));
        live.apply(&ProgressEvent::Advance { key: 0, by: 3 });
        assert_eq!(live.apply(&ProgressEvent::Finish { key: 1 }), None);
        assert_eq!(live.apply(&ProgressEvent::Advance { key: 1, by: 1 }), None);
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 0, by: 1 }),
            Some(shown(Some(OperationStep::ListingPacks), 6, None))
        );
        assert_eq!(live.apply(&ProgressEvent::Finish { key: 0 }), None);
        assert_eq!(live.apply(&ProgressEvent::Advance { key: 0, by: 1 }), None);
    }

    /// The worker announces a bar before it moves; a change to one it never
    /// announced, as the bars rustic begins before the job starts, is
    /// nothing to show.
    #[test]
    fn a_bar_never_begun_shows_nothing() {
        let mut live = LiveProgress::new();
        assert_eq!(live.apply(&ProgressEvent::Advance { key: 7, by: 1 }), None);
        assert_eq!(live.apply(&ProgressEvent::Length { key: 7, len: 1 }), None);
        assert_eq!(
            live.apply(&ProgressEvent::Title {
                key: 7,
                step: Some(OperationStep::BackingUp),
            }),
            None
        );
        assert_eq!(live.apply(&ProgressEvent::Finish { key: 7 }), None);
    }

    /// A key begun again is a new bar: it starts over, and is the newest.
    #[test]
    fn a_key_begun_again_starts_over_as_the_newest_bar() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, Some(OperationStep::ReadingIndex)));
        live.apply(&ProgressEvent::Advance { key: 0, by: 5 });
        live.apply(&begin(1, Some(OperationStep::ListingPacks)));
        assert_eq!(
            live.apply(&begin(0, Some(OperationStep::CheckingTrees))),
            Some(shown(Some(OperationStep::CheckingTrees), 0, None))
        );
        live.apply(&ProgressEvent::Finish { key: 0 });
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 1, by: 1 }),
            Some(shown(Some(OperationStep::ListingPacks), 1, None))
        );
    }

    /// Counts come from a worker: one that would pass `u64::MAX` stays there.
    #[test]
    fn a_count_never_wraps() {
        let mut live = LiveProgress::new();
        live.apply(&begin(0, None));
        live.apply(&ProgressEvent::Advance {
            key: 0,
            by: u64::MAX,
        });
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 0, by: 2 }),
            Some(shown(None, u64::MAX, None))
        );
    }

    /// At most [`MAX_OPEN_BARS`] are kept: the oldest goes when one more
    /// begins, and so can never show again.
    #[test]
    fn the_oldest_open_bar_goes_when_one_too_many_begins() {
        let mut live = LiveProgress::new();
        let last = u32::try_from(MAX_OPEN_BARS).unwrap();
        for key in 0..=last {
            live.apply(&begin(key, None));
        }
        for key in (2..=last).rev() {
            live.apply(&ProgressEvent::Finish { key });
        }
        assert_eq!(
            live.apply(&ProgressEvent::Advance { key: 1, by: 1 }),
            Some(shown(None, 1, None))
        );
        live.apply(&ProgressEvent::Finish { key: 1 });
        assert_eq!(live.apply(&ProgressEvent::Advance { key: 0, by: 1 }), None);
    }

    #[test]
    fn what_is_shown_goes_to_the_page_with_its_step_on_the_wire() {
        let operation = "018f0000-0000-7000-8000-000000000001";
        assert_eq!(
            shown(Some(OperationStep::ReadingIndex), 3, Some(10)).ui(operation.to_owned()),
            UiEvent::OperationProgress {
                operation: operation.to_owned(),
                step: Some(api::OperationStep::ReadingIndex),
                current: 3,
                total: Some(10),
            }
        );
        assert_eq!(
            shown(None, 0, None).ui(operation.to_owned()),
            UiEvent::OperationProgress {
                operation: operation.to_owned(),
                step: None,
                current: 0,
                total: None,
            }
        );
    }

    /// Each step goes on the wire as its own code.
    #[test]
    fn each_step_has_its_own_code_on_the_wire() {
        use OperationStep as S;
        use api::OperationStep as W;
        for (step, code) in [
            (S::ReadingIndex, W::ReadingIndex),
            (S::GettingSnapshots, W::GettingSnapshots),
            (S::BackingUp, W::BackingUp),
            (S::ListingPacks, W::ListingPacks),
            (S::CheckingCache, W::CheckingCache),
            (S::CheckingHotRepository, W::CheckingHotRepository),
            (S::CheckingTrees, W::CheckingTrees),
            (S::ReadingPackData, W::ReadingPackData),
            (S::FindingUsedData, W::FindingUsedData),
            (S::MarkingUnusedPacks, W::MarkingUnusedPacks),
            (S::Repacking, W::Repacking),
            (S::RebuildingIndex, W::RebuildingIndex),
            (S::DeletingUnusedData, W::DeletingUnusedData),
            (S::DeletingSnapshots, W::DeletingSnapshots),
            (S::SavingSnapshots, W::SavingSnapshots),
        ] {
            let UiEvent::OperationProgress { step: on_wire, .. } =
                shown(Some(step), 0, None).ui(String::new())
            else {
                panic!("not a progress event");
            };
            assert_eq!(on_wire, Some(code), "{step:?}");
        }
    }
}
