//! The navigation bar (components §4.5): the brand, the pages' links and a
//! group of rows behind a disclosure menu, on one line where they fit and
//! behind a hamburger button where they don't. [`rules`] decides when its
//! panel shows, host-tested; this module is the glue that draws it.

use std::sync::Arc;

use l10n::Localizer;
use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos_router::hooks::use_location;

use super::button::{Appearance, Content};
use super::disclosure_menu::rules::{self as menu_rules, FocusMoved};
use super::link::rules::{Destination, LinkPlacement, LinkTone};
use super::{Button, DisclosureMenu, Link, Tip, Tooltip, focus_element};
use crate::variants::{Side, Size};

// When the panel shows and what its button tells, decided on the host;
// `NavigationBar` is its glue.
pub(crate) mod rules;

/// A page of the navigation bar: the words of its link and where it goes.
/// Its link is current wherever `to` says it is (`Destination::current`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationPage {
    pub words: String,
    pub to: Destination,
}

/// The navigation bar (§4.5, decisions 143, 152 and 160): `children`, the
/// brand, at the start of its line, then a `<nav>` named `label` holding
/// `pages` as links standing alone in the color of the text around them,
/// and after them a [`DisclosureMenu`] whose words are `group_label`,
/// holding `group_pages` and then `group_end` under a hairline (the
/// header's Settings menu: Settings, Log, Sign out). The current page is
/// told by its weight and a bar at its start, never by color alone; the
/// group's button is marked while the reader is on one of its pages.
///
/// - **From 54rem up** it is one line: the brand, then the links and the
///   group's button at the line's end (`style.css`).
/// - **Below 54rem** a hamburger button, the shared `Button`, Ghost, XLarge,
///   icon-only and named `menu_label` in both states, ends the brand's line,
///   and every link and the group's rows sit in a panel (`panel_id`) it
///   shows and hides: `aria-expanded` says whether it shows, `aria-controls`
///   names it. Shown, the panel stands over the page under the brand's line,
///   so opening and closing it moves nothing on the page. Its rows stand in
///   as many columns of 10em as its width holds, each at least 2.75rem
///   tall; the group's button isn't drawn there and its rows are rows of
///   the panel.
/// - **The panel closes** on Escape, which returns focus to the hamburger
///   (only while it is drawn); on a link chosen, the current page's link
///   returning focus to the hamburger since it changes no route; on focus
///   moving to an element outside the bar; on a press of the pointer outside
///   it; on a route change; and whenever `shows_pages` doesn't hold. How it
///   closes on focus moving, on a press and on the current page's link is
///   the disclosure menu's own rule, so the bar's two disclosures close
///   alike.
///
/// While `shows_pages` doesn't hold (the sign-in page) the bar is the brand
/// alone.
///
/// What the caller owes WCAG 2.2 AA, since the bar does none of it:
///
/// - **A `label`** naming the navigation (2.4.6), unique among the page's
///   `navigation` landmarks, a **`group_label`** naming the group and a
///   **`menu_label`** naming the hamburger, the same whether the panel
///   shows or not and unique among the page's buttons.
/// - **A `panel_id` unique on the page**: the hamburger's `id` and the
///   group's panel's are made from it ([`rules::button_id`],
///   [`rules::group_id`]).
/// - **A brand** that is a link to the start page, or nothing a reader
///   would take for a page of the bar.
#[component]
pub fn NavigationBar(
    localizer: Arc<Localizer>,
    panel_id: String,
    label: String,
    menu_label: String,
    #[prop(into)] shows_pages: Signal<bool>,
    pages: Vec<NavigationPage>,
    group_label: String,
    group_pages: Vec<NavigationPage>,
    #[prop(optional, into)] group_end: ViewFn,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let root = NodeRef::<html::Div>::new();
    let button_id = rules::button_id(&panel_id);
    let group_id = rules::group_id(&panel_id);
    let location = use_location();

    // A route change closes the panel: a page chosen from it shows it
    // closed, so the header is as it was.
    Effect::new(move |previous: Option<()>| {
        location.pathname.track();
        if previous.is_some() {
            open.set(false);
        }
    });
    // The pages go with a sign-out, and the button's state with them: the
    // next sign-in finds the panel hidden, whatever it was.
    Effect::new(move |_| {
        if rules::hidden_without_pages(shows_pages.get()) {
            open.set(false);
        }
    });
    let group_routes: Vec<Destination> = group_pages.iter().map(|page| page.to.clone()).collect();
    let in_group = Signal::derive(move || {
        let (path, query) = (location.pathname.get(), location.search.get());
        group_routes
            .iter()
            .any(|route| route.current(&path, &query).is_some())
    });

    // Escape hides the panel and returns focus to the hamburger, while the
    // hamburger is drawn. An Escape that hides a tooltip has done its job
    // (the tooltip's own rule, WCAG 1.4.13): it leaves the panel, and focus,
    // where they are.
    let escape_button = button_id.clone();
    let on_keydown = move |event: web_sys::KeyboardEvent| {
        if !rules::escape_closes(open.get_untracked(), &event.key(), is_drawn(&escape_button))
            || tooltip_shown()
        {
            return;
        }
        open.set(false);
        focus_element(&escape_button);
    };
    // Focus leaving for an element outside the bar closes the panel, which
    // would cover it (2.4.11); focus going nowhere, or inside, leaves it.
    let on_focusout = move |event: web_sys::FocusEvent| {
        let moved = match event
            .related_target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
        {
            None => FocusMoved::Nowhere,
            Some(target) if contains(root, &target) => FocusMoved::Inside,
            Some(_) => FocusMoved::Outside,
        };
        if menu_rules::closes_when_focus_moves(open.get_untracked(), moved) {
            open.set(false);
        }
    };
    // A press of the pointer outside the bar closes the panel, as the
    // disclosure menu's does (#1582): on a touch screen, with no Escape, it
    // is the way back.
    let pressed = window_event_listener(leptos::ev::pointerdown, move |event| {
        let inside = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|target| contains(root, &target));
        if menu_rules::closes_on_press(open.get_untracked(), inside) {
            open.set(false);
        }
    });
    on_cleanup(move || pressed.remove());
    // A link chosen in the panel hides it; the current page's changes no
    // route, so focus goes back to the hamburger, which is drawn, rather
    // than staying on a link that no longer is (2.4.3).
    let choice_button = button_id.clone();
    let on_panel_click = move |event: web_sys::MouseEvent| {
        let link = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|target| target.closest("a").ok().flatten());
        if let Some(link) = link {
            let was_open = open.get_untracked();
            open.set(false);
            if was_open
                && is_drawn(&choice_button)
                && menu_rules::focus_returns_on_choice(
                    link.get_attribute("aria-current").as_deref(),
                )
            {
                focus_element(&choice_button);
            }
        }
    };

    let controls = panel_id.clone();
    let hamburger = move || {
        shows_pages.get().then(|| {
            view! {
                <MenuButton
                    name=menu_label.clone()
                    id=button_id.clone()
                    controls=controls.clone()
                    open=open
                />
            }
        })
    };
    let rows = Arc::new((pages, group_pages, label, group_label));
    let panel = move || {
        shows_pages.get().then(|| {
            let (pages, group_pages, label, group_label) = &*rows;
            let (label, group_label) = (label.clone(), group_label.clone());
            let page_rows = page_links(&localizer, pages);
            let group_rows = page_links(&localizer, group_pages);
            let group_end = group_end.clone();
            view! {
                <div
                    id=panel_id.clone()
                    class=move || rules::panel_class(open.get())
                    on:click=on_panel_click.clone()
                >
                    <nav class="navigation-bar-nav" aria-label=label>
                        <ul>
                            {page_rows} <li class="navigation-bar-group">
                                <DisclosureMenu
                                    label=group_label
                                    panel_id=group_id.clone()
                                    current=in_group
                                >
                                    <ul>{group_rows}</ul>
                                    <div class="navigation-bar-end empty:hidden">
                                        {group_end.run()}
                                    </div>
                                </DisclosureMenu>
                            </li>
                        </ul>
                    </nav>
                </div>
            }
            // Erased, as is the bar's own view below: their types nest too
            // deep for the compiler's query limit in the wasm test build.
            .into_any()
        })
    };

    view! {
        <div class="navigation-bar" node_ref=root on:keydown=on_keydown on:focusout=on_focusout>
            // The brand's line: the brand takes the room the navigation
            // leaves, so the navigation stands at the line's end, and below
            // 54rem the hamburger ends it, level with it.
            <div class="flex flex-wrap items-center gap-x-4 grow">{children()} {hamburger}</div>
            {panel}
        </div>
    }
    .into_any()
}

/// One list item for each page, its link standing alone in the color of
/// the text around it.
fn page_links(localizer: &Arc<Localizer>, pages: &[NavigationPage]) -> impl IntoView + use<> {
    pages
        .iter()
        .map(|page| {
            let (to, words) = (page.to.clone(), page.words.clone());
            view! {
                <li>
                    <Link
                        to=to
                        localizer=Arc::clone(localizer)
                        tone=LinkTone::Inherit
                        placement=LinkPlacement::Alone
                    >
                        {words}
                    </Link>
                </li>
            }
        })
        .collect_view()
}

/// The hamburger, which below 54rem ends the brand's line and shows the
/// panel, and is drawn only there (`style.css`). Icon-only, so its name
/// ("Menu", the same in both states) is its label and its tooltip, as every
/// icon-only button's is; the icon is a hamburger while the panel is hidden
/// and an X while it shows. XLarge, 3em, so it is a finger's target (44 px)
/// at the page's own text size. The tooltip stands at the button's inline
/// start, where it covers neither the panel nor the page under the bar.
#[component]
fn MenuButton(name: String, id: String, controls: String, open: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="navigation-bar-button">
            <Tooltip tip=Tip::Names(name.clone()) side=Side::Start>
                <Button
                    id=id
                    content=Content::IconOnly {
                        glyph: Signal::derive(move || rules::glyph(open.get())),
                        label: Signal::stored(name),
                    }
                    appearance=Appearance::Ghost
                    size=Size::XLarge
                    expanded=open
                    controls=controls
                    on_press=Callback::new(move |()| {
                        open.update(|open| *open = rules::toggled(*open))
                    })
                />
            </Tooltip>
        </div>
    }
}

/// Whether the element `id` is drawn now: the hamburger is below 54rem
/// alone, where `style.css` draws it (a `display: none` box has no size).
/// Read from the box, not from the breakpoint, so the glue never repeats
/// the CSS.
fn is_drawn(id: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .is_some_and(|button| {
            let drawn = button.get_bounding_client_rect();
            menu_rules::is_drawn(drawn.width(), drawn.height())
        })
}

/// Whether a tooltip shows now, which an Escape hides first.
fn tooltip_shown() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            document
                .query_selector(".tooltip:popover-open")
                .ok()
                .flatten()
        })
        .is_some()
}

/// Whether `node` is the bar's or inside it.
fn contains(root: NodeRef<html::Div>, node: &web_sys::Node) -> bool {
    root.get_untracked()
        .is_some_and(|root| root.contains(Some(node)))
}
