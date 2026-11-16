//! What a set of tabs does (components §4.5, "Tabs"; the WAI-ARIA tabs
//! pattern), decided on the host: which tab a key moves focus to, whether
//! arriving there selects it, which tab is the list's one Tab stop, the ids
//! and ARIA attributes that tie each tab to its panel and name the list, and
//! whether a panel is a Tab stop of its own. `components/tabs.rs`'s `Tabs` is
//! its DOM glue, which the `@ui` scenarios run in a browser.

use l10n::Direction;

/// When a tab focus arrives at is selected (the WAI-ARIA tabs pattern,
/// "Deciding When to Make Selection Automatically").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activation {
    /// A tab the arrow keys, Home or End arrive at is selected at once and
    /// its panel shown: the default, for panels that are drawn at once.
    #[default]
    Automatic,
    /// The arrow keys, Home and End move focus only, and Enter or Space
    /// selects the tab focus is on: for a panel that costs something to
    /// show, such as a read from the daemon.
    Manual,
}

impl Activation {
    /// The tab newly selected once focus arrives at the tab `arrived` while
    /// the tab `selected` is, or `None` when the selection stays: with
    /// `Automatic` the tab arrived at, as a press on it would select; with
    /// `Manual`, none, since only a press selects.
    pub fn selection_on_arrival(self, selected: usize, arrived: usize) -> Option<usize> {
        let _ = (self, selected, arrived);
        todo!()
    }
}

/// The tab newly selected by a press (a click, Enter or Space) on the tab
/// `pressed` while the tab `selected` is, or `None` for a press on the tab
/// already selected, so a caller following the selection (`Tabs`'s
/// `selected`) hears of each change once and of nothing else.
pub fn selection_on_press(selected: usize, pressed: usize) -> Option<usize> {
    let _ = (selected, pressed);
    todo!()
}

/// What a panel holds, as its caller says, which decides whether the panel
/// is a Tab stop of its own: Tab from the selected tab goes into its panel,
/// to the panel itself or to the first control in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    /// Nothing that takes focus, such as text alone: the panel is the stop
    /// after its tab (`tabindex="0"`), so a keyboard reader reaches it.
    Text,
    /// Something that takes focus (a field, a button, a link): its first
    /// control is the stop after the tab, and the panel takes none.
    Controls,
}

/// What names the tab list (4.1.2): words of its own, or an element on the
/// page that already shows them, such as a heading above the tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabsName {
    /// The list's `aria-label`.
    Label(String),
    /// The `id` of the element that names the list (`aria-labelledby`).
    LabelledBy(String),
}

impl TabsName {
    /// The list's `aria-label` and `aria-labelledby`: one of them, never
    /// both.
    pub fn attributes(&self) -> (Option<String>, Option<String>) {
        todo!()
    }
}

/// The tab focus moves to from the tab `at` of a list whose last tab is
/// `last` (a list holds at least one, so there is always a last) when `key`
/// (a `KeyboardEvent.key` pressed without Ctrl, Alt or Meta) is pressed in a
/// list that runs in `direction`, or `None` for a key the list leaves to the
/// page. The arrow pointing along the line goes to the next tab, the other
/// to the one before, so in a list that runs right to left the left arrow
/// goes to the next tab; both wrap, from the last tab to the first and back.
/// Home and End go to the first and the last.
pub fn arrival(at: usize, last: usize, key: &str, direction: Direction) -> Option<usize> {
    let _ = (at, last, key, direction);
    todo!()
}

/// A tab's `tabindex`: the selected tab is the list's one Tab stop, and the
/// arrow keys reach the others.
pub fn tab_stop(selected: bool) -> &'static str {
    let _ = selected;
    todo!()
}

/// A tab's `aria-selected`, on every tab of the list.
pub fn aria_selected(selected: bool) -> &'static str {
    let _ = selected;
    todo!()
}

/// A panel's `tabindex`: `0` for a panel nothing in which takes focus, none
/// for one whose controls do.
pub fn panel_stop(holds: Holds) -> Option<&'static str> {
    let _ = holds;
    todo!()
}

/// The `id` of the tab at `index` of the list whose ids start with `prefix`,
/// which its panel's `aria-labelledby` names.
pub fn tab_id(prefix: &str, index: usize) -> String {
    let _ = (prefix, index);
    todo!()
}

/// The `id` of the panel of the tab at `index`, which the tab's
/// `aria-controls` names.
pub fn panel_id(prefix: &str, index: usize) -> String {
    let _ = (prefix, index);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: Direction = Direction::Ltr;
    const RTL: Direction = Direction::Rtl;

    #[test]
    fn in_a_list_that_runs_left_to_right_the_right_arrow_goes_to_the_next_tab_and_the_left_to_the_one_before()
     {
        assert_eq!(arrival(0, 2, "ArrowRight", LTR), Some(1));
        assert_eq!(arrival(1, 2, "ArrowRight", LTR), Some(2));
        assert_eq!(arrival(2, 2, "ArrowLeft", LTR), Some(1));
        assert_eq!(arrival(1, 2, "ArrowLeft", LTR), Some(0));
    }

    #[test]
    fn the_arrows_wrap_from_the_last_tab_to_the_first_and_back() {
        assert_eq!(arrival(2, 2, "ArrowRight", LTR), Some(0));
        assert_eq!(arrival(0, 2, "ArrowLeft", LTR), Some(2));
        assert_eq!(arrival(3, 3, "ArrowLeft", RTL), Some(0));
        assert_eq!(arrival(0, 3, "ArrowRight", RTL), Some(3));
    }

    #[test]
    fn in_a_list_that_runs_right_to_left_the_left_arrow_goes_to_the_next_tab_and_the_right_to_the_one_before()
     {
        assert_eq!(arrival(0, 2, "ArrowLeft", RTL), Some(1));
        assert_eq!(arrival(1, 2, "ArrowLeft", RTL), Some(2));
        assert_eq!(arrival(2, 2, "ArrowRight", RTL), Some(1));
        assert_eq!(arrival(1, 2, "ArrowRight", RTL), Some(0));
    }

    #[test]
    fn home_and_end_go_to_the_first_and_the_last_tab_in_either_direction() {
        for direction in [LTR, RTL] {
            assert_eq!(arrival(1, 2, "Home", direction), Some(0), "{direction:?}");
            assert_eq!(arrival(1, 2, "End", direction), Some(2), "{direction:?}");
            assert_eq!(arrival(0, 2, "Home", direction), Some(0), "{direction:?}");
            assert_eq!(arrival(2, 2, "End", direction), Some(2), "{direction:?}");
        }
    }

    #[test]
    fn a_list_of_one_tab_keeps_focus_on_it() {
        for key in ["ArrowRight", "ArrowLeft", "Home", "End"] {
            assert_eq!(arrival(0, 0, key, LTR), Some(0), "{key}");
        }
    }

    // Up and Down belong to a vertical list, which tabs never are; Enter
    // and Space press the tab itself, a native button; Tab leaves the list.
    #[test]
    fn every_other_key_is_left_to_the_page() {
        for key in [
            "ArrowUp",
            "ArrowDown",
            "Enter",
            " ",
            "Tab",
            "Escape",
            "PageDown",
            "a",
        ] {
            assert_eq!(arrival(1, 3, key, LTR), None, "{key}");
            assert_eq!(arrival(1, 3, key, RTL), None, "{key}");
        }
    }

    #[test]
    fn a_tab_arrived_at_is_selected_with_it_unless_the_tabs_activate_by_hand() {
        assert_eq!(Activation::default(), Activation::Automatic);
        assert_eq!(Activation::Automatic.selection_on_arrival(0, 2), Some(2));
        assert_eq!(Activation::Automatic.selection_on_arrival(2, 1), Some(1));
        assert_eq!(Activation::Manual.selection_on_arrival(0, 2), None);
        assert_eq!(Activation::Manual.selection_on_arrival(2, 1), None);
    }

    // A caller that reads a panel from the daemon once its tab is chosen
    // reads it once: arriving at, or pressing, the tab already selected
    // changes nothing it follows.
    #[test]
    fn only_a_tab_that_isnt_selected_yet_is_newly_selected_by_a_press_or_an_arrival() {
        assert_eq!(selection_on_press(0, 2), Some(2));
        assert_eq!(selection_on_press(2, 0), Some(0));
        assert_eq!(selection_on_press(1, 1), None);
        assert_eq!(Activation::Automatic.selection_on_arrival(1, 1), None);
    }

    #[test]
    fn only_the_selected_tab_is_a_tab_stop_and_every_tab_says_whether_it_is_selected() {
        assert_eq!(tab_stop(true), "0");
        assert_eq!(tab_stop(false), "-1");
        assert_eq!(aria_selected(true), "true");
        assert_eq!(aria_selected(false), "false");
    }

    #[test]
    fn a_panel_of_text_is_a_tab_stop_and_one_holding_controls_is_not() {
        assert_eq!(panel_stop(Holds::Text), Some("0"));
        assert_eq!(panel_stop(Holds::Controls), None);
    }

    #[test]
    fn each_tab_and_each_panel_has_an_id_of_its_own_under_the_lists_prefix() {
        assert_eq!(tab_id("demo", 0), "demo-tab-0");
        assert_eq!(tab_id("demo", 2), "demo-tab-2");
        assert_eq!(panel_id("demo", 0), "demo-panel-0");
        assert_eq!(panel_id("other", 2), "other-panel-2");
    }

    #[test]
    fn the_list_is_named_by_its_label_or_by_the_element_it_names_never_both() {
        assert_eq!(
            TabsName::Label("Demo agent".to_owned()).attributes(),
            (Some("Demo agent".to_owned()), None)
        );
        assert_eq!(
            TabsName::LabelledBy("demo-heading".to_owned()).attributes(),
            (None, Some("demo-heading".to_owned()))
        );
    }
}
