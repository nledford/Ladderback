//! The tabs (components §4.5, "Tabs"): the WAI-ARIA tabs pattern on native
//! buttons. [`rules`] decides which tab a key moves to in the layout shown,
//! which is selected and which is the list's Tab stop, and each element's ids
//! and ARIA attributes, host-tested; this module is the glue that draws them.

use l10n::Direction;
use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos::wasm_bindgen::closure::Closure;

use super::focus_element;

// What a set of tabs does, decided on the host; `Tabs` is its glue.
pub(crate) mod rules;

use rules::Layout;
pub use rules::{Activation, Holds, Orientation, TabsName};

/// One tab: its words, what its panel holds and the panel. A caller builds
/// the panel once, with the tabs, and it stays drawn while another shows.
pub struct Tab {
    label: String,
    holds: Signal<Holds>,
    panel: AnyView,
    start: bool,
}

impl Tab {
    /// A tab named `label`, its words, whose panel is `panel`. `holds` says
    /// whether anything in the panel takes focus, which decides whether the
    /// panel is a Tab stop of its own; it may follow what the panel shows,
    /// as for a panel whose controls come only once the daemon answers.
    pub fn new(
        label: String,
        holds: impl Into<Signal<Holds>>,
        panel: impl IntoView + 'static,
    ) -> Self {
        Self {
            label,
            holds: holds.into(),
            panel: panel.into_any(),
            start: false,
        }
    }

    /// The tab, the one its set starts on when `start` holds, as a page
    /// whose address names a tab does. Of several, the first does; of none,
    /// the set starts on its first tab.
    pub fn selected_at_start(self, start: bool) -> Self {
        Self { start, ..self }
    }
}

/// The tabs of a set, in the order they stand in the list: at least one,
/// so a set with no tab, a `tablist` without a tab to select, can't be
/// built.
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
/// to in one stays while another shows. The first tab is selected, or the
/// one its caller starts the set on (`Tab::selected_at_start`).
///
/// The list is a row above the panels, a column at their start, or, with
/// `Orientation::Responsive`, a column while the tabs' own container has
/// room for it beside the panel and a row below that; its
/// `aria-orientation` says the layout shown.
///
/// The selected tab is the list's one Tab stop, and Tab from it goes into
/// its panel. In a row the left and right arrows move between the tabs in
/// the direction the focused tab's text runs, read from its computed style
/// (so a part of the page set right to left mirrors them); in a column the
/// down and up arrows do. Both wrap at either end; Home and End go to the
/// first and the last. With `Activation::Automatic`, the default, the tab
/// arrived at is selected; with `Activation::Manual` only Enter or Space
/// (the button's own press) selects, as a click does. Nothing is
/// announced: the screen reader says the selected tab.
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
/// - **A width for `Responsive` tabs,** as a block has: they are the
///   container their query measures, which takes no width from its content.
#[component]
pub fn Tabs(
    /// The start of each tab's and each panel's `id`, unique on the page.
    #[prop(into)]
    id: String,
    name: TabsName,
    tabs: TabSet,
    #[prop(optional)] activation: Activation,
    /// How the list stands beside the panels: a row above them (the
    /// default), a column at their start, or whichever the tabs' container
    /// has room for.
    #[prop(optional)]
    orientation: Orientation,
    /// The `id` of words on the page that say how the tabs are used, the
    /// list's `aria-describedby`.
    #[prop(optional, into)]
    described_by: Option<String>,
    /// Follows the selected tab, its index in `tabs` (0, the first, to
    /// begin with), set once for each change, and once as the tabs are
    /// drawn when they start on another: a caller that reads a panel's
    /// content only once its tab is chosen watches it. A caller writes it
    /// to choose a tab, as Settings does for an address changed while it is
    /// open: the tab is selected and its panel shown, focus left where it
    /// is; a tab the set doesn't hold changes nothing.
    #[prop(optional)]
    selected: Option<RwSignal<usize>>,
) -> impl IntoView {
    let TabSet { first, rest } = tabs;
    let last = rest.len();
    let selection = RwSignal::new(rules::selected_at_start(
        std::iter::once(first.start).chain(rest.iter().map(|tab| tab.start)),
    ));
    if let Some(selected) = selected {
        // Written only when it differs, so a caller watching it hears of
        // each change once and of the tab the set starts on only when it
        // doesn't already hold it.
        Effect::new(move |_| {
            let now = selection.get();
            if selected.get_untracked() != now {
                selected.set(now);
            }
        });
        // A caller's choice, not the value the caller starts it at: the set
        // starts on its own tab (`selected_at_start`).
        Effect::new(move |seen: Option<()>| {
            let chosen = selected.get();
            if seen.is_none() {
                return;
            }
            if let Some(chosen) =
                rules::selection_on_choice(selection.get_untracked(), chosen, last)
            {
                selection.set(chosen);
            }
        });
    }
    // The layout shown: a horizontal or vertical set's own, a responsive
    // set's as its list is drawn, read once the tabs are and again whenever
    // they or their list change size. The size watch reports in the same
    // rendering update that draws a new layout, before it is painted, so the
    // keys, which read this, follow the layout on the screen.
    let layout = RwSignal::new(orientation.fixed().unwrap_or(Layout::Horizontal));
    let frame = NodeRef::<html::Div>::new();
    let list = NodeRef::<html::Div>::new();
    if orientation.fixed().is_none() {
        follow_drawn_layout(frame, list, layout);
    }
    let (label, labeled_by) = name.attributes();
    let classes = orientation.classes();
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
            let Some(to) = rules::arrival(
                index,
                last,
                &event.key(),
                Direction::of_computed(&running),
                layout.get_untracked(),
            ) else {
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
        let (label, words, holds) = (tab.label.clone(), tab.label, tab.holds);
        buttons.push(view! {
            <button
                type="button"
                role="tab"
                id=tab_id.clone()
                class=classes.tab
                aria-selected=move || rules::aria_selected(is_selected())
                aria-controls=panel_id.clone()
                tabindex=move || rules::tab_stop(is_selected())
                on:click=press
                on:keydown=keydown
            >
                // An unseen bold copy of the words keeps the tab as wide
                // selected as not (`rules::Classes::label`).
                <span class=classes.label data-label=label>
                    {words}
                </span>
            </button>
        });
        panels.push(view! {
            <div
                role="tabpanel"
                id=panel_id
                class=classes.panel
                aria-labelledby=tab_id
                tabindex=move || rules::panel_stop(holds.get())
                hidden=move || !is_selected()
            >
                {tab.panel}
            </div>
        });
    }
    view! {
        <div class=classes.frame node_ref=frame>
            <div class=classes.layout>
                <div
                    role="tablist"
                    class=classes.list
                    node_ref=list
                    aria-label=label
                    aria-labelledby=labeled_by
                    aria-describedby=described_by
                    aria-orientation=move || layout.get().aria_orientation()
                >
                    {buttons}
                </div>
                {panels}
            </div>
        </div>
    }
    .into_any()
}

/// Shows each hidden tab panel `element` is in, as a press on its tab does,
/// so a modal dialog opened there is drawn: one under a hidden panel isn't,
/// and a dialog that opens when the daemon answers can do so after the
/// reader has chosen another tab. The press goes through the tab's own
/// handler, so the tabs stay the one owner of the selection; the panel is
/// shown at once too, before the tabs' next update would, so the dialog can
/// take focus as it opens.
pub(crate) fn show_panel_holding(element: &web_sys::Element) {
    let mut from = element.parent_element();
    while let Some(inside) = from {
        let Ok(Some(panel)) = inside.closest("[role='tabpanel']") else {
            return;
        };
        if panel.has_attribute("hidden")
            && let Some(tab) = panel
                .get_attribute("aria-labelledby")
                .and_then(|id| document().get_element_by_id(&id))
                .and_then(|tab| tab.dyn_into::<web_sys::HtmlElement>().ok())
        {
            tab.click();
            let _ = panel.remove_attribute("hidden");
        }
        from = panel.parent_element();
    }
}

/// Keeps `layout` to the layout a responsive set's `list` is drawn in: read
/// once the tabs are drawn, then whenever the tabs' `frame` (the container
/// its query measures) or the list change size, as a change of layout or of
/// the text's size does, until the tabs go.
fn follow_drawn_layout(
    frame: NodeRef<html::Div>,
    list: NodeRef<html::Div>,
    layout: RwSignal<Layout>,
) {
    let watch = StoredValue::new_local(None::<(web_sys::ResizeObserver, Closure<dyn FnMut()>)>);
    Effect::new(move |_| {
        let (Some(frame), Some(list)) = (frame.get(), list.get()) else {
            return;
        };
        let read = {
            let list = list.clone();
            move || note_layout(&list, layout)
        };
        let resized = Closure::<dyn FnMut()>::new(read);
        let Ok(observer) = web_sys::ResizeObserver::new(resized.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(&frame);
        observer.observe(&list);
        watch.update_value(|watch| {
            if let Some((before, _)) = watch.replace((observer, resized)) {
                before.disconnect();
            }
        });
    });
    on_cleanup(move || {
        watch.try_update_value(|watch| {
            if let Some((observer, _)) = watch.take() {
                observer.disconnect();
            }
        });
    });
}

/// Writes to `layout` the layout `list` is drawn in, read from its computed
/// `flex-direction`, when it differs.
fn note_layout(list: &web_sys::Element, layout: RwSignal<Layout>) {
    let drawn = window()
        .get_computed_style(list)
        .ok()
        .flatten()
        .and_then(|style| style.get_property_value("flex-direction").ok())
        .unwrap_or_default();
    let now = Layout::drawn(&drawn);
    if layout.try_get_untracked().is_some_and(|was| was != now) {
        layout.set(now);
    }
}
