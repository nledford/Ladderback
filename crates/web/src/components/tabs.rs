//! The tabs (components §4.5, "Tabs"): the WAI-ARIA tabs pattern on native
//! buttons. [`rules`] decides which tab a key moves to, which is selected
//! and which is the list's Tab stop, and each element's ids and ARIA
//! attributes, host-tested; this module is the glue that draws them.

use l10n::Direction;
use leptos::prelude::*;

use super::focus_element;

// What a set of tabs does, decided on the host; `Tabs` is its glue.
pub(crate) mod rules;

pub use rules::{Activation, Holds, TabsName};

/// One tab: its words, what its panel holds and the panel. A caller builds
/// the panel once, with the tabs, and it stays drawn while another shows.
pub struct Tab {
    label: String,
    holds: Holds,
    panel: AnyView,
}

impl Tab {
    /// A tab named `label`, its words, whose panel is `panel`. `holds` says
    /// whether anything in the panel takes focus, which decides whether the
    /// panel is a Tab stop of its own.
    pub fn new(label: String, holds: Holds, panel: impl IntoView + 'static) -> Self {
        Self {
            label,
            holds,
            panel: panel.into_any(),
        }
    }
}

/// The tabs of a set, in the order they stand in the row: at least one, so
/// a set with no tab, a `tablist` without a tab to select, can't be built.
pub struct TabSet {
    first: Tab,
    rest: Vec<Tab>,
}

impl TabSet {
    /// The set of `first` and then each of `rest`.
    pub fn new(first: Tab, rest: Vec<Tab>) -> Self {
        Self { first, rest }
    }
}

/// A set of tabs (§4.5): a `tablist` named by `name`, a native
/// `<button type="button">` for each tab, then every tab's panel, built
/// once, the unselected ones `hidden`, so what a reader typed or scrolled
/// to in one stays while another shows. The first tab is selected.
///
/// The selected tab is the list's one Tab stop, and Tab from it goes into
/// its panel. The left and right arrows move between the tabs in the
/// direction the focused tab's text runs, read from its computed style (so
/// a part of the page set right to left mirrors them), wrapping at either
/// end; Home and End go to the first and the last. With
/// `Activation::Automatic`, the default, the tab arrived at is selected;
/// with `Activation::Manual` only Enter or Space (the button's own press)
/// selects, as a click does. Nothing is announced: the screen reader says
/// the selected tab.
///
/// What the caller owes WCAG 2.2 AA, since the tabs do none of it:
///
/// - **A name that says what the tabs choose between (4.1.2, 2.4.6),**
///   and tab words that say what each panel shows.
/// - **`Holds::Controls` for a panel whose content takes focus,** and
///   `Holds::Text` for one that holds none, so a keyboard reader reaches
///   every panel from its tab.
/// - **An `id` unique on the page,** which starts every tab's and panel's.
/// - **Words that say how `Manual` tabs are chosen,** shown beside them and
///   named by `described_by`, since a reader expects the arrows to select.
#[component]
pub fn Tabs(
    /// The start of each tab's and each panel's `id`, unique on the page.
    #[prop(into)]
    id: String,
    name: TabsName,
    tabs: TabSet,
    #[prop(optional)] activation: Activation,
    /// The `id` of words on the page that say how the tabs are used, the
    /// list's `aria-describedby`.
    #[prop(optional, into)]
    described_by: Option<String>,
    /// Follows the selected tab, its index in `tabs` (0, the first, to
    /// begin with), set once for each change: a caller that reads a panel's
    /// content only once its tab is chosen watches it. The tabs set it; a
    /// caller only reads it.
    #[prop(optional)]
    selected: Option<RwSignal<usize>>,
) -> impl IntoView {
    let TabSet { first, rest } = tabs;
    let last = rest.len();
    let selection = RwSignal::new(0_usize);
    if let Some(selected) = selected {
        // Written only when it differs, so a caller watching it hears of
        // each change once and of the first tab not at all when it already
        // holds 0.
        Effect::new(move |_| {
            let now = selection.get();
            if selected.get_untracked() != now {
                selected.set(now);
            }
        });
    }
    let (label, labelled_by) = name.attributes();
    let mut buttons = Vec::with_capacity(last.saturating_add(1));
    let mut panels = Vec::with_capacity(last.saturating_add(1));
    for (index, tab) in std::iter::once(first).chain(rest).enumerate() {
        let tab_id = rules::tab_id(&id, index);
        let panel_id = rules::panel_id(&id, index);
        let is_selected = move || selection.get() == index;
        let prefix = id.clone();
        let keydown = move |event: web_sys::KeyboardEvent| {
            if event.ctrl_key() || event.alt_key() || event.meta_key() {
                return;
            }
            let running = window()
                .get_computed_style(&event_target::<web_sys::Element>(&event))
                .ok()
                .flatten()
                .and_then(|style| style.get_property_value("direction").ok())
                .unwrap_or_default();
            let Some(to) =
                rules::arrival(index, last, &event.key(), Direction::of_computed(&running))
            else {
                return;
            };
            event.prevent_default();
            if let Some(chosen) = activation.selection_on_arrival(selection.get_untracked(), to) {
                selection.set(chosen);
            }
            focus_element(&rules::tab_id(&prefix, to));
        };
        let press = move |_| {
            if let Some(chosen) = rules::selection_on_press(selection.get_untracked(), index) {
                selection.set(chosen);
            }
        };
        let (label, words) = (tab.label.clone(), tab.label);
        buttons.push(view! {
            <button
                type="button"
                role="tab"
                id=tab_id.clone()
                class="tab"
                aria-selected=move || rules::aria_selected(is_selected())
                aria-controls=panel_id.clone()
                tabindex=move || rules::tab_stop(is_selected())
                on:click=press
                on:keydown=keydown
            >
                // An unseen bold copy of the words keeps the tab as wide
                // selected as not (style.css, `.tab-label`).
                <span class="tab-label" data-label=label>
                    {words}
                </span>
            </button>
        });
        panels.push(view! {
            <div
                role="tabpanel"
                id=panel_id
                class="tab-panel"
                aria-labelledby=tab_id
                tabindex=rules::panel_stop(tab.holds)
                hidden=move || !is_selected()
            >
                {tab.panel}
            </div>
        });
    }
    view! {
        <div class="tabs">
            <div
                role="tablist"
                class="tab-list"
                aria-label=label
                aria-labelledby=labelled_by
                aria-describedby=described_by
            >
                {buttons}
            </div>
            {panels}
        </div>
    }
    .into_any()
}
