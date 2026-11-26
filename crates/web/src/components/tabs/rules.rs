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

/// The tab newly selected when the tabs' caller chooses the tab `chosen`
/// (writing `Tabs`'s `selected`) while the tab `selected` is, in a set whose
/// last tab is `last`: the tab chosen, as a press on it would select, or
/// `None` for the tab already selected and for one the set doesn't hold,
/// which changes nothing.
pub fn selection_on_choice(selected: usize, chosen: usize, last: usize) -> Option<usize> {
    (chosen <= last)
        .then_some(chosen)
        .and_then(|chosen| selection_on_press(selected, chosen))
}

/// The tab selected when the tabs are drawn, of tabs whose callers marked
/// each as the one to start on or not (`marked`, in the order of the list):
/// the first one marked, or the first tab when none is, so a caller can't
/// start the tabs on a tab the set doesn't hold.
pub fn selected_at_start(marked: impl IntoIterator<Item = bool>) -> usize {
    marked.into_iter().position(|marked| marked).unwrap_or(0)
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
    LabeledBy(String),
}

impl TabsName {
    /// The list's `aria-label` and `aria-labelledby`: one of them, never
    /// both.
    pub fn attributes(&self) -> (Option<String>, Option<String>) {
        match self {
            Self::Label(label) => (Some(label.clone()), None),
            Self::LabeledBy(id) => (None, Some(id.clone())),
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

    /// The classes of the set's parts in this orientation.
    pub fn classes(self) -> Classes {
        match self {
            Self::Horizontal => ROW,
            Self::Vertical => COLUMN,
            Self::Responsive => RESPONSIVE,
        }
    }
}

/// The classes of each part of a set of tabs: its own name, which the
/// scenarios find it by, then the Tailwind utilities that draw it in the
/// set's orientation (decision 117), logical ones only. Each part takes
/// one orientation's utilities, so no utility of another orientation is
/// there to override.
///
/// The selected tab is bold with a 3px Ladder Blue bar over the Slate Note
/// rule, told by weight and a shape, never by color alone (1.4.1, The
/// Weight Not Color Rule). Flat: no fill and no box. Each tab draws its own
/// stretch of the rule, edge to edge with the next, so the rule shows
/// beside every tab, and a tab's edge reaches one pixel into the list's
/// rule, so the two are one. The bar is that edge 3px thick, the tab's
/// padding there 2px less, so choosing a tab moves nothing; a hovered tab
/// that isn't selected shows a Slate Note line as thick as the bar (on the
/// selected tab it is the bar's own width). Under forced colors the tabs
/// are text on the reader's page colors, the rule and the bar in the text's
/// color as the focus ring is (`Highlight` need not reach 3:1 on `Canvas`,
/// 1.4.11), no hover line; the bar stands apart from the rule by its
/// thickness and from the ring by its place. The weights stay in
/// `style.css`: the base rule `button { font: inherit }`, outside
/// Tailwind's layers, outranks any utility on the tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classes {
    /// The set.
    pub frame: &'static str,
    /// The box inside the set that lays out the list and the panels: a
    /// container query can't style its own container.
    pub layout: &'static str,
    /// The `tablist`.
    pub list: &'static str,
    /// Each tab, a native button.
    pub tab: &'static str,
    /// The words in each tab, as wide bold as not, so the row stays still as
    /// the selection moves along it: an unseen bold copy of the words, which
    /// a screen reader skips (its alternative text is empty), sets its width.
    pub label: &'static str,
    /// Each panel.
    pub panel: &'static str,
}

/// A row of tabs over the panels, on the rule under the list. On a narrow
/// screen the row wraps onto more lines, each tab whole (The 320 Rule), and
/// the list's own rule runs on under the last line to the row's end. A
/// panel keeps the room of the tabs' row between it and the list.
const ROW: Classes = Classes {
    frame: "tabs",
    layout: "tabs-layout",
    list: "tab-list flex flex-wrap gap-y-1 border-be border-be-muted \
        forced-colors:border-be-[color:CanvasText]",
    tab: "tab min-block-6 -mbe-px py-1 px-3 border-0 border-be border-be-muted bg-transparent \
        text-fg aria-selected:pbe-[calc(0.25rem-2px)] aria-selected:border-be-[3px] \
        not-forced-colors:aria-selected:border-be-accent \
        not-forced-colors:hover:pbe-[calc(0.25rem-2px)] not-forced-colors:hover:border-be-[3px] \
        forced-colors:bg-[color:Canvas] forced-colors:border-be-[color:CanvasText] \
        forced-colors:text-[color:CanvasText]",
    label: "tab-label inline-flex flex-col items-center after:content-[attr(data-label)_/_''] \
        after:block-0 after:overflow-hidden after:invisible after:font-bold",
    panel: "tab-panel py-3",
};

/// A column of tabs at the panels' start, on the rule at the list's end,
/// between it and the panels, where the selected tab's bar lies. The list
/// takes at most half the room, and the panels the rest. Every tab is as
/// wide as the list, its words wrapping and standing at its start, so the
/// labels line up down the column, wrapped lines too (#1011).
const COLUMN: Classes = Classes {
    frame: "tabs tabs-vertical",
    layout: "tabs-layout flex items-start",
    list: "tab-list flex flex-col max-inline-1/2 border-e border-e-muted \
        forced-colors:border-e-[color:CanvasText]",
    tab: "tab min-block-6 mbe-0 -me-px py-1 px-3 border-0 border-e border-e-muted bg-transparent \
        text-fg text-start wrap-anywhere aria-selected:pe-[calc(0.75rem-2px)] \
        aria-selected:border-e-[3px] not-forced-colors:aria-selected:border-e-accent \
        not-forced-colors:hover:pe-[calc(0.75rem-2px)] not-forced-colors:hover:border-e-[3px] \
        forced-colors:bg-[color:Canvas] forced-colors:border-e-[color:CanvasText] \
        forced-colors:text-[color:CanvasText]",
    label: "tab-label inline-flex flex-col items-start after:content-[attr(data-label)_/_''] \
        after:block-0 after:overflow-hidden after:invisible after:font-bold",
    panel: "tab-panel flex-1 min-inline-0 py-1 ps-4",
};

/// A column while the set's own container has room for the list beside the
/// panels, 42rem (in rem, so larger text needs a wider container), and a
/// row below that: the column's utilities under `@min-[42rem]:`, the row's
/// under `@max-[42rem]:`, and what both share bare. The set is the
/// container (`@container`), so a caller sets its width, as a block's.
const RESPONSIVE: Classes = Classes {
    frame: "tabs tabs-responsive @container",
    layout: "tabs-layout @min-[42rem]:flex @min-[42rem]:items-start",
    list: "tab-list flex @max-[42rem]:flex-wrap @max-[42rem]:gap-y-1 @max-[42rem]:border-be \
        @max-[42rem]:border-be-muted @max-[42rem]:forced-colors:border-be-[color:CanvasText] \
        @min-[42rem]:flex-col @min-[42rem]:max-inline-1/2 @min-[42rem]:border-e \
        @min-[42rem]:border-e-muted @min-[42rem]:forced-colors:border-e-[color:CanvasText]",
    tab: "tab min-block-6 py-1 px-3 border-0 bg-transparent text-fg \
        forced-colors:bg-[color:Canvas] forced-colors:text-[color:CanvasText] \
        @max-[42rem]:-mbe-px @max-[42rem]:border-be @max-[42rem]:border-be-muted \
        @max-[42rem]:aria-selected:pbe-[calc(0.25rem-2px)] \
        @max-[42rem]:aria-selected:border-be-[3px] \
        @max-[42rem]:not-forced-colors:aria-selected:border-be-accent \
        @max-[42rem]:not-forced-colors:hover:pbe-[calc(0.25rem-2px)] \
        @max-[42rem]:not-forced-colors:hover:border-be-[3px] \
        @max-[42rem]:forced-colors:border-be-[color:CanvasText] \
        @min-[42rem]:mbe-0 @min-[42rem]:-me-px @min-[42rem]:border-e @min-[42rem]:border-e-muted \
        @min-[42rem]:text-start @min-[42rem]:wrap-anywhere \
        @min-[42rem]:aria-selected:pe-[calc(0.75rem-2px)] \
        @min-[42rem]:aria-selected:border-e-[3px] \
        @min-[42rem]:not-forced-colors:aria-selected:border-e-accent \
        @min-[42rem]:not-forced-colors:hover:pe-[calc(0.75rem-2px)] \
        @min-[42rem]:not-forced-colors:hover:border-e-[3px] \
        @min-[42rem]:forced-colors:border-e-[color:CanvasText]",
    label: "tab-label inline-flex flex-col after:content-[attr(data-label)_/_''] after:block-0 \
        after:overflow-hidden after:invisible after:font-bold @max-[42rem]:items-center \
        @min-[42rem]:items-start",
    panel: "tab-panel @max-[42rem]:py-3 @min-[42rem]:flex-1 @min-[42rem]:min-inline-0 \
        @min-[42rem]:py-1 @min-[42rem]:ps-4",
};

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

    // A caller choosing a tab, as Settings does for an address whose
    // fragment changed, selects it as a press would, and can't select a tab
    // the set doesn't hold.
    #[test]
    fn a_tab_a_caller_chooses_is_newly_selected_only_if_the_set_holds_it_and_it_isnt_selected_yet()
    {
        assert_eq!(selection_on_choice(0, 2, 2), Some(2));
        assert_eq!(selection_on_choice(2, 0, 2), Some(0));
        assert_eq!(selection_on_choice(0, 1, 2), Some(1));
        assert_eq!(selection_on_choice(1, 1, 2), None);
        assert_eq!(selection_on_choice(0, 3, 2), None);
        assert_eq!(selection_on_choice(0, usize::MAX, 2), None);
        assert_eq!(selection_on_choice(0, 0, 0), None);
        assert_eq!(selection_on_choice(0, 1, 0), None);
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
            TabsName::LabeledBy("demo-heading".to_owned()).attributes(),
            (None, Some("demo-heading".to_owned()))
        );
    }

    /// The classes of each part, in the order `Classes` lists them.
    fn parts(classes: Classes) -> [&'static str; 6] {
        [
            classes.frame,
            classes.layout,
            classes.list,
            classes.tab,
            classes.label,
            classes.panel,
        ]
    }

    fn set(class: &str) -> std::collections::BTreeSet<&str> {
        class.split_whitespace().collect()
    }

    #[test]
    fn every_part_keeps_its_name_in_every_orientation() {
        let names = [
            "tabs",
            "tabs-layout",
            "tab-list",
            "tab",
            "tab-label",
            "tab-panel",
        ];
        for orientation in [
            Orientation::Horizontal,
            Orientation::Vertical,
            Orientation::Responsive,
        ] {
            for (class, name) in parts(orientation.classes()).into_iter().zip(names) {
                assert!(set(class).contains(name), "{orientation:?}: {class}");
            }
        }
        assert!(set(Orientation::Vertical.classes().frame).contains("tabs-vertical"));
        assert!(set(Orientation::Responsive.classes().frame).contains("tabs-responsive"));
    }

    #[test]
    fn a_row_draws_its_rule_and_bar_under_the_tabs_and_a_column_at_their_end() {
        let row = Orientation::Horizontal.classes();
        let column = Orientation::Vertical.classes();
        for under in ["border-be", "aria-selected:border-be-[3px]"] {
            assert!(set(row.tab).contains(under), "{under}");
            assert!(!set(column.tab).contains(under), "{under}");
        }
        for end in ["border-e", "aria-selected:border-e-[3px]"] {
            assert!(set(column.tab).contains(end), "{end}");
            assert!(!set(row.tab).contains(end), "{end}");
        }
        assert!(set(row.list).contains("flex-wrap"));
        assert!(set(column.list).contains("flex-col"));
        assert!(set(column.layout).contains("flex"));
        assert!(!set(row.layout).contains("flex"));
    }

    /// A responsive set is a row below 42rem of its own width and a column
    /// from there: each part takes what the row's and the column's share,
    /// the row's own utilities only under `@max-[42rem]:` and the column's
    /// only under `@min-[42rem]:`, so the two never meet, and the set is
    /// the container its query measures.
    #[test]
    fn responsive_tabs_are_a_row_below_42rem_and_a_column_from_there() {
        let (row, column) = (
            Orientation::Horizontal.classes(),
            Orientation::Vertical.classes(),
        );
        let parts = parts(row)
            .into_iter()
            .zip(parts(column))
            .zip(parts(Orientation::Responsive.classes()))
            .skip(1);
        for ((row, column), responsive) in parts {
            let (row, column) = (set(row), set(column));
            let mut wanted: std::collections::BTreeSet<String> =
                row.intersection(&column).map(|&c| c.to_owned()).collect();
            wanted.extend(row.difference(&column).map(|c| format!("@max-[42rem]:{c}")));
            wanted.extend(column.difference(&row).map(|c| format!("@min-[42rem]:{c}")));
            let drawn: std::collections::BTreeSet<String> =
                set(responsive).into_iter().map(str::to_owned).collect();
            assert_eq!(drawn, wanted);
        }
        assert_eq!(
            set(Orientation::Responsive.classes().frame),
            set("tabs tabs-responsive @container")
        );
    }
}
