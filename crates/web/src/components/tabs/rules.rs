//! What a set of tabs does (components §4.5, "Tabs"; the WAI-ARIA tabs
//! pattern), decided on the host: which tab a key moves focus to in the
//! layout shown, whether arriving there selects it, which tab is the list's
//! one Tab stop, the ids and ARIA attributes that tie each tab to its panel,
//! name the list and say its orientation, and whether a panel is a Tab stop
//! of its own. `components/tabs.rs`'s `Tabs` is its DOM glue, which the `@ui`
//! scenarios run in a browser.

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
        match self {
            Self::Automatic => selection_on_press(selected, arrived),
            Self::Manual => None,
        }
    }
}

/// The tab newly selected by a press (a click, Enter or Space) on the tab
/// `pressed` while the tab `selected` is, or `None` for a press on the tab
/// already selected, so a caller following the selection (`Tabs`'s
/// `selected`) hears of each change once and of nothing else.
pub fn selection_on_press(selected: usize, pressed: usize) -> Option<usize> {
    (pressed != selected).then_some(pressed)
}

/// The tab selected when the tabs are drawn, of tabs whose callers marked
/// each as the one to start on or not (`marked`, in the order of the list):
/// the first one marked, or the first tab when none is, so a caller can't
/// start the tabs on a tab the set doesn't hold.
pub fn selected_at_start(marked: impl IntoIterator<Item = bool>) -> usize {
    let _ = marked.into_iter();
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
        match self {
            Self::Label(label) => (Some(label.clone()), None),
            Self::LabelledBy(id) => (None, Some(id.clone())),
        }
    }
}

/// How a set of tabs lays out its list beside its panels (§4.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// A row of tabs above the panel: the default.
    #[default]
    Horizontal,
    /// A column of tabs beside the panel, at its start.
    Vertical,
    /// A column beside the panel while the tabs' container has room for
    /// both, a row above it below that: a container query decides, so the
    /// same tabs suit a wide page and a narrow column.
    Responsive,
}

impl Orientation {
    /// The layout a list of this orientation always shows, or `None` for
    /// `Responsive`, whose layout is the one its container's room draws.
    pub fn fixed(self) -> Option<Layout> {
        match self {
            Self::Horizontal => Some(Layout::Horizontal),
            Self::Vertical => Some(Layout::Vertical),
            Self::Responsive => None,
        }
    }
}

/// The layout a tab list is shown in, which its arrow keys and its
/// `aria-orientation` follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// A row: the left and right arrows move between the tabs.
    Horizontal,
    /// A column: the up and down arrows move between the tabs.
    Vertical,
}

impl Layout {
    /// The layout of a list drawn with the CSS `flex-direction` `drawn`, its
    /// computed value: a column is vertical, anything else a row.
    pub fn drawn(drawn: &str) -> Self {
        if drawn.starts_with("column") {
            Self::Vertical
        } else {
            Self::Horizontal
        }
    }

    /// The list's `aria-orientation`.
    pub fn aria_orientation(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// The tab focus moves to from the tab `at` of a list whose last tab is
/// `last` (a list holds at least one, so there is always a last) when `key`
/// (a `KeyboardEvent.key` pressed without Ctrl, Alt or Meta) is pressed in a
/// list shown in `layout` whose text runs in `direction`, or `None` for a key
/// the list leaves to the page. In a row, the arrow pointing along the line
/// goes to the next tab, the other to the one before, so in a list that runs
/// right to left the left arrow goes to the next tab; in a column, the down
/// arrow goes to the next and the up arrow to the one before, whatever the
/// direction. Both wrap, from the last tab to the first and back. Home and
/// End go to the first and the last.
pub fn arrival(
    at: usize,
    last: usize,
    key: &str,
    direction: Direction,
    layout: Layout,
) -> Option<usize> {
    let (next, previous) = match (layout, direction) {
        (Layout::Horizontal, Direction::Ltr) => ("ArrowRight", "ArrowLeft"),
        (Layout::Horizontal, Direction::Rtl) => ("ArrowLeft", "ArrowRight"),
        (Layout::Vertical, _) => ("ArrowDown", "ArrowUp"),
    };
    match key {
        "Home" => Some(0),
        "End" => Some(last),
        _ if key == next => Some(if at >= last { 0 } else { at.saturating_add(1) }),
        _ if key == previous => Some(at.checked_sub(1).unwrap_or(last)),
        _ => None,
    }
}

/// A tab's `tabindex`: the selected tab is the list's one Tab stop, and the
/// arrow keys reach the others.
pub fn tab_stop(selected: bool) -> &'static str {
    if selected { "0" } else { "-1" }
}

/// A tab's `aria-selected`, on every tab of the list.
pub fn aria_selected(selected: bool) -> &'static str {
    if selected { "true" } else { "false" }
}

/// A panel's `tabindex`: `0` for a panel nothing in which takes focus, none
/// for one whose controls do.
pub fn panel_stop(holds: Holds) -> Option<&'static str> {
    match holds {
        Holds::Text => Some("0"),
        Holds::Controls => None,
    }
}

/// The `id` of the tab at `index` of the list whose ids start with `prefix`,
/// which its panel's `aria-labelledby` names.
pub fn tab_id(prefix: &str, index: usize) -> String {
    format!("{prefix}-tab-{index}")
}

/// The `id` of the panel of the tab at `index`, which the tab's
/// `aria-controls` names.
pub fn panel_id(prefix: &str, index: usize) -> String {
    format!("{prefix}-panel-{index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LTR: Direction = Direction::Ltr;
    const RTL: Direction = Direction::Rtl;

    /// Where `key` moves focus in a row of tabs.
    fn in_a_row(at: usize, last: usize, key: &str, direction: Direction) -> Option<usize> {
        arrival(at, last, key, direction, Layout::Horizontal)
    }

    /// Where `key` moves focus in a column of tabs.
    fn in_a_column(at: usize, last: usize, key: &str, direction: Direction) -> Option<usize> {
        arrival(at, last, key, direction, Layout::Vertical)
    }

    #[test]
    fn the_tabs_start_on_the_first_tab_marked_to_start_on() {
        assert_eq!(selected_at_start([false, false, true]), 2);
        assert_eq!(selected_at_start([false, true, false]), 1);
        assert_eq!(selected_at_start([false, true, true]), 1);
        assert_eq!(selected_at_start([true, true]), 0);
    }

    #[test]
    fn tabs_none_of_which_is_marked_start_on_the_first() {
        assert_eq!(selected_at_start([false, false, false]), 0);
        assert_eq!(selected_at_start([false]), 0);
    }

    #[test]
    fn in_a_list_that_runs_left_to_right_the_right_arrow_goes_to_the_next_tab_and_the_left_to_the_one_before()
     {
        assert_eq!(in_a_row(0, 2, "ArrowRight", LTR), Some(1));
        assert_eq!(in_a_row(1, 2, "ArrowRight", LTR), Some(2));
        assert_eq!(in_a_row(2, 2, "ArrowLeft", LTR), Some(1));
        assert_eq!(in_a_row(1, 2, "ArrowLeft", LTR), Some(0));
    }

    #[test]
    fn the_arrows_wrap_from_the_last_tab_to_the_first_and_back() {
        assert_eq!(in_a_row(2, 2, "ArrowRight", LTR), Some(0));
        assert_eq!(in_a_row(0, 2, "ArrowLeft", LTR), Some(2));
        assert_eq!(in_a_row(3, 3, "ArrowLeft", RTL), Some(0));
        assert_eq!(in_a_row(0, 3, "ArrowRight", RTL), Some(3));
    }

    #[test]
    fn in_a_list_that_runs_right_to_left_the_left_arrow_goes_to_the_next_tab_and_the_right_to_the_one_before()
     {
        assert_eq!(in_a_row(0, 2, "ArrowLeft", RTL), Some(1));
        assert_eq!(in_a_row(1, 2, "ArrowLeft", RTL), Some(2));
        assert_eq!(in_a_row(2, 2, "ArrowRight", RTL), Some(1));
        assert_eq!(in_a_row(1, 2, "ArrowRight", RTL), Some(0));
    }

    #[test]
    fn in_a_column_the_down_arrow_goes_to_the_next_tab_and_the_up_to_the_one_before_in_either_direction()
     {
        for direction in [LTR, RTL] {
            assert_eq!(
                in_a_column(0, 2, "ArrowDown", direction),
                Some(1),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(1, 2, "ArrowDown", direction),
                Some(2),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(2, 2, "ArrowUp", direction),
                Some(1),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(1, 2, "ArrowUp", direction),
                Some(0),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn in_a_column_the_up_and_down_arrows_wrap_from_the_last_tab_to_the_first_and_back() {
        for direction in [LTR, RTL] {
            assert_eq!(
                in_a_column(2, 2, "ArrowDown", direction),
                Some(0),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(0, 2, "ArrowUp", direction),
                Some(2),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(3, 3, "ArrowDown", direction),
                Some(0),
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(0, 3, "ArrowUp", direction),
                Some(3),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn home_and_end_go_to_the_first_and_the_last_tab_in_either_direction() {
        for direction in [LTR, RTL] {
            for layout in [Layout::Horizontal, Layout::Vertical] {
                let case = format!("{direction:?} {layout:?}");
                assert_eq!(arrival(1, 2, "Home", direction, layout), Some(0), "{case}");
                assert_eq!(arrival(1, 2, "End", direction, layout), Some(2), "{case}");
                assert_eq!(arrival(0, 2, "Home", direction, layout), Some(0), "{case}");
                assert_eq!(arrival(2, 2, "End", direction, layout), Some(2), "{case}");
            }
        }
    }

    #[test]
    fn a_list_of_one_tab_keeps_focus_on_it() {
        for key in ["ArrowRight", "ArrowLeft", "Home", "End"] {
            assert_eq!(in_a_row(0, 0, key, LTR), Some(0), "{key}");
        }
        for key in ["ArrowDown", "ArrowUp", "Home", "End"] {
            assert_eq!(in_a_column(0, 0, key, LTR), Some(0), "{key}");
        }
    }

    // Up and Down belong to a column and Left and Right to a row, the other
    // pair left to the page as the WAI-ARIA tabs pattern does; Enter and
    // Space press the tab itself, a native button; Tab leaves the list.
    #[test]
    fn every_other_key_is_left_to_the_page() {
        for key in ["Enter", " ", "Tab", "Escape", "PageDown", "a"] {
            for direction in [LTR, RTL] {
                assert_eq!(in_a_row(1, 3, key, direction), None, "{key}");
                assert_eq!(in_a_column(1, 3, key, direction), None, "{key}");
            }
        }
        for direction in [LTR, RTL] {
            assert_eq!(in_a_row(1, 3, "ArrowUp", direction), None, "{direction:?}");
            assert_eq!(
                in_a_row(1, 3, "ArrowDown", direction),
                None,
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(1, 3, "ArrowLeft", direction),
                None,
                "{direction:?}"
            );
            assert_eq!(
                in_a_column(1, 3, "ArrowRight", direction),
                None,
                "{direction:?}"
            );
        }
    }

    #[test]
    fn horizontal_and_vertical_tabs_always_show_their_own_layout_and_responsive_ones_the_one_drawn()
    {
        assert_eq!(Orientation::default(), Orientation::Horizontal);
        assert_eq!(Orientation::Horizontal.fixed(), Some(Layout::Horizontal));
        assert_eq!(Orientation::Vertical.fixed(), Some(Layout::Vertical));
        assert_eq!(Orientation::Responsive.fixed(), None);
    }

    #[test]
    fn a_list_drawn_as_a_column_is_vertical_and_any_other_is_horizontal() {
        assert_eq!(Layout::drawn("column"), Layout::Vertical);
        assert_eq!(Layout::drawn("column-reverse"), Layout::Vertical);
        assert_eq!(Layout::drawn("row"), Layout::Horizontal);
        assert_eq!(Layout::drawn("row-reverse"), Layout::Horizontal);
        assert_eq!(Layout::drawn(""), Layout::Horizontal);
    }

    #[test]
    fn the_list_says_the_layout_it_is_shown_in() {
        assert_eq!(Layout::Horizontal.aria_orientation(), "horizontal");
        assert_eq!(Layout::Vertical.aria_orientation(), "vertical");
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
