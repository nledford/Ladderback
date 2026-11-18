//! What a list of a process's steps shows and says (components §4.5): each
//! step's state, told by where the process has reached, its icon, a shape of
//! its own for every state, and its words, so the state never rests on its
//! color (A2), which step is the current one, the color of the connector
//! after each step, and what a change of step is said as.
//! `components/steps.rs`'s `Steps` is the glue that draws them and says the
//! change through the announcer.

use api::MsgArg;
use l10n::{MessageId, msg};
use lepticons_data::LucideGlyph;

use crate::variants::Tone;
use crate::wording::Words;

/// Where a process has reached among its steps, counted from 0. The steps
/// before it are done and the steps after it are still to do, so a list can
/// never show two current steps, or a step done after one to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reached {
    /// The step at this place is under way.
    At(usize),
    /// The step at this place failed, and the process stopped there.
    FailedAt(usize),
    /// Every step is done.
    Finished,
}

impl Reached {
    /// Where the process has reached among `count` steps: a place past the
    /// last step is every step done.
    fn within(self, count: usize) -> Self {
        match self {
            Self::At(place) | Self::FailedAt(place) if place >= count => Self::Finished,
            reached => reached,
        }
    }
}

/// What names the list (4.1.2): words of its own, or an element on the page
/// that already shows them, such as a heading above the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepsName {
    /// The list's `aria-label`.
    Label(String),
    /// The `id` of the element that names the list (`aria-labelledby`).
    LabelledBy(String),
}

impl StepsName {
    /// The list's `aria-label` and `aria-labelledby`: one of them, never
    /// both.
    pub fn attributes(&self) -> (Option<String>, Option<String>) {
        match self {
            Self::Label(label) => (Some(label.clone()), None),
            Self::LabelledBy(id) => (None, Some(id.clone())),
        }
    }
}

/// The state of one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    /// The process has gone past it.
    Done,
    /// The process is at it now.
    Current,
    /// The process hasn't reached it yet.
    ToDo,
    /// The process stopped at it, failed.
    Failed,
}

impl StepState {
    /// Every state, in the order a process meets them.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Done, Self::Current, Self::ToDo, Self::Failed];

    /// The state of the step at `place` of `count` steps, when the process
    /// has reached `reached`.
    pub fn of(place: usize, count: usize, reached: Reached) -> Self {
        let (at, stood) = match reached.within(count) {
            Reached::At(at) => (at, Self::Current),
            Reached::FailedAt(at) => (at, Self::Failed),
            Reached::Finished => return Self::Done,
        };
        match place.cmp(&at) {
            std::cmp::Ordering::Less => Self::Done,
            std::cmp::Ordering::Equal => stood,
            std::cmp::Ordering::Greater => Self::ToDo,
        }
    }

    /// The icon before the step's name, its shape the state's own, so a
    /// reader who can't tell the colors apart, or whose forced colors draw
    /// every icon alike, tells the states apart by it.
    pub fn glyph(self) -> LucideGlyph {
        match self {
            Self::Done => LucideGlyph::CircleCheck,
            Self::Current => LucideGlyph::CircleDot,
            Self::ToDo => LucideGlyph::Circle,
            Self::Failed => LucideGlyph::CircleX,
        }
    }

    /// The icon's tone: Ladder Blue for the steps reached, the muted text's
    /// gray for those still to do, red for a failure. The words carry the
    /// state too (1.4.1).
    pub fn tone(self) -> Tone {
        match self {
            Self::Done | Self::Current => Tone::Primary,
            Self::ToDo => Tone::Secondary,
            Self::Failed => Tone::Error,
        }
    }

    /// The state in words, shown under the step's name.
    pub fn word(self) -> MessageId {
        match self {
            Self::Done => msg::UI_STEPS_DONE,
            Self::Current => msg::UI_STEPS_CURRENT,
            Self::ToDo => msg::UI_STEPS_TO_DO,
            Self::Failed => msg::UI_STEPS_FAILED,
        }
    }

    /// The step's `aria-current`: `step` on the step the process stands at,
    /// under way or failed there, and on no other.
    pub fn aria_current(self) -> Option<&'static str> {
        match self {
            Self::Current | Self::Failed => Some("step"),
            Self::Done | Self::ToDo => None,
        }
    }

    /// The color of the connector that leaves this step for the next one:
    /// Ladder Blue once the step is done, so the line runs as far as the
    /// process has, and the muted text's gray before that. Both stand 3:1
    /// against the page and a surface (1.4.11; the contrast gate's link and
    /// muted pairs).
    pub fn connector_class(self) -> &'static str {
        match self {
            Self::Done => "border-accent",
            Self::Current | Self::ToDo | Self::Failed => "border-muted",
        }
    }
}

/// What the announcer says when the process moves from `before` to `now`
/// among the steps named `names`: nothing when the list is first drawn
/// (`before` is `None`), nothing when where it has reached is the same, and
/// otherwise the step it reached, the step it failed at, or that every step
/// is done, one message with its place, the count and the name.
pub fn change(before: Option<Reached>, now: Reached, names: &[String]) -> Option<Words> {
    let count = names.len();
    let now = now.within(count);
    // With no steps, every place is past the end, so nothing ever changes.
    if before?.within(count) == now {
        return None;
    }
    let total = (
        "total",
        MsgArg::Count(u64::try_from(count).unwrap_or(u64::MAX)),
    );
    let (id, at) = match now {
        Reached::At(at) => (msg::UI_STEPS_REACHED, at),
        Reached::FailedAt(at) => (msg::UI_STEPS_FAILED_AT, at),
        Reached::Finished => {
            return Some(Words {
                id: msg::UI_STEPS_FINISHED,
                args: vec![total],
            });
        }
    };
    let number = u64::try_from(at).map_or(u64::MAX, |at| at.saturating_add(1));
    Some(Words {
        id,
        args: vec![
            ("number", MsgArg::Count(number)),
            total,
            ("name", MsgArg::Text(names.get(at)?.clone())),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<String> {
        of.iter().map(|name| (*name).to_owned()).collect()
    }

    fn states(count: usize, reached: Reached) -> Vec<StepState> {
        (0..count)
            .map(|place| StepState::of(place, count, reached))
            .collect()
    }

    #[test]
    fn the_steps_before_where_the_process_stands_are_done_and_those_after_it_to_do() {
        use StepState::{Current, Done, ToDo};
        assert_eq!(states(4, Reached::At(0)), [Current, ToDo, ToDo, ToDo]);
        assert_eq!(states(4, Reached::At(2)), [Done, Done, Current, ToDo]);
        assert_eq!(states(4, Reached::At(3)), [Done, Done, Done, Current]);
    }

    #[test]
    fn a_failed_step_is_failed_the_steps_before_it_done_and_those_after_it_to_do() {
        use StepState::{Done, Failed, ToDo};
        assert_eq!(states(3, Reached::FailedAt(0)), [Failed, ToDo, ToDo]);
        assert_eq!(states(3, Reached::FailedAt(1)), [Done, Failed, ToDo]);
    }

    #[test]
    fn a_finished_process_or_one_past_its_last_step_has_every_step_done() {
        use StepState::Done;
        assert_eq!(states(3, Reached::Finished), [Done, Done, Done]);
        assert_eq!(states(3, Reached::At(3)), [Done, Done, Done]);
        assert_eq!(states(3, Reached::FailedAt(7)), [Done, Done, Done]);
    }

    #[test]
    fn each_state_has_an_icon_of_its_own() {
        let icons = [
            (StepState::Done, LucideGlyph::CircleCheck),
            (StepState::Current, LucideGlyph::CircleDot),
            (StepState::ToDo, LucideGlyph::Circle),
            (StepState::Failed, LucideGlyph::CircleX),
        ];
        for (state, icon) in icons {
            assert_eq!(state.glyph(), icon, "{state:?}");
        }
        let mut shapes: Vec<_> = StepState::ALL.iter().map(|state| state.glyph()).collect();
        shapes.sort();
        shapes.dedup();
        assert_eq!(
            shapes.len(),
            StepState::ALL.len(),
            "no two states share an icon"
        );
    }

    #[test]
    fn each_state_is_said_in_words_of_its_own() {
        let words = [
            (StepState::Done, msg::UI_STEPS_DONE),
            (StepState::Current, msg::UI_STEPS_CURRENT),
            (StepState::ToDo, msg::UI_STEPS_TO_DO),
            (StepState::Failed, msg::UI_STEPS_FAILED),
        ];
        for (state, word) in words {
            assert_eq!(state.word(), word, "{state:?}");
        }
    }

    #[test]
    fn the_steps_reached_are_ladder_blue_those_to_do_gray_and_a_failure_red() {
        let tones = [
            (StepState::Done, Tone::Primary),
            (StepState::Current, Tone::Primary),
            (StepState::ToDo, Tone::Secondary),
            (StepState::Failed, Tone::Error),
        ];
        for (state, tone) in tones {
            assert_eq!(state.tone(), tone, "{state:?}");
        }
    }

    #[test]
    fn only_the_step_the_process_stands_at_is_the_current_step() {
        let current = [
            (StepState::Done, None),
            (StepState::Current, Some("step")),
            (StepState::ToDo, None),
            (StepState::Failed, Some("step")),
        ];
        for (state, aria) in current {
            assert_eq!(state.aria_current(), aria, "{state:?}");
        }
    }

    #[test]
    fn the_connector_runs_in_ladder_blue_only_after_a_step_that_is_done() {
        assert_eq!(StepState::Done.connector_class(), "border-accent");
        for state in [StepState::Current, StepState::ToDo, StepState::Failed] {
            assert_eq!(state.connector_class(), "border-muted", "{state:?}");
        }
    }

    #[test]
    fn a_list_is_named_by_its_own_words_or_by_an_element_never_both() {
        assert_eq!(
            StepsName::Label("Demo hub move".to_owned()).attributes(),
            (Some("Demo hub move".to_owned()), None)
        );
        assert_eq!(
            StepsName::LabelledBy("demo-heading".to_owned()).attributes(),
            (None, Some("demo-heading".to_owned()))
        );
    }

    fn said(id: MessageId, args: Vec<(&'static str, MsgArg)>) -> Option<Words> {
        Some(Words { id, args })
    }

    #[test]
    fn nothing_is_said_when_the_list_is_first_drawn_or_where_it_stands_stays_the_same() {
        let steps = names(&["Check", "Copy", "Commit"]);
        assert_eq!(change(None, Reached::At(1), &steps), None);
        assert_eq!(change(Some(Reached::At(1)), Reached::At(1), &steps), None);
        assert_eq!(
            change(Some(Reached::Finished), Reached::Finished, &steps),
            None
        );
    }

    #[test]
    fn a_step_reached_is_said_with_its_place_from_one_the_count_and_its_name() {
        let steps = names(&["Check", "Copy", "Commit"]);
        assert_eq!(
            change(Some(Reached::At(0)), Reached::At(1), &steps),
            said(
                msg::UI_STEPS_REACHED,
                vec![
                    ("number", MsgArg::Count(2)),
                    ("total", MsgArg::Count(3)),
                    ("name", MsgArg::Text("Copy".to_owned())),
                ]
            )
        );
        assert_eq!(
            change(Some(Reached::Finished), Reached::At(0), &steps),
            said(
                msg::UI_STEPS_REACHED,
                vec![
                    ("number", MsgArg::Count(1)),
                    ("total", MsgArg::Count(3)),
                    ("name", MsgArg::Text("Check".to_owned())),
                ]
            )
        );
    }

    #[test]
    fn a_failure_is_said_with_the_place_the_count_and_the_name_of_the_step_that_failed() {
        let steps = names(&["Check", "Copy", "Commit"]);
        assert_eq!(
            change(Some(Reached::At(2)), Reached::FailedAt(2), &steps),
            said(
                msg::UI_STEPS_FAILED_AT,
                vec![
                    ("number", MsgArg::Count(3)),
                    ("total", MsgArg::Count(3)),
                    ("name", MsgArg::Text("Commit".to_owned())),
                ]
            )
        );
    }

    #[test]
    fn a_process_that_finishes_or_moves_past_its_last_step_says_every_step_is_done() {
        let steps = names(&["Check", "Copy"]);
        let finished = said(msg::UI_STEPS_FINISHED, vec![("total", MsgArg::Count(2))]);
        assert_eq!(
            change(Some(Reached::At(1)), Reached::Finished, &steps),
            finished
        );
        assert_eq!(
            change(Some(Reached::At(1)), Reached::At(2), &steps),
            finished
        );
        // Past the end is finished, so moving between two such places, or
        // from one to finished, isn't a change.
        assert_eq!(
            change(Some(Reached::At(2)), Reached::FailedAt(5), &steps),
            None
        );
        assert_eq!(
            change(Some(Reached::At(9)), Reached::Finished, &steps),
            None
        );
    }

    #[test]
    fn a_list_of_no_steps_says_nothing() {
        assert_eq!(change(Some(Reached::At(0)), Reached::Finished, &[]), None);
    }
}
