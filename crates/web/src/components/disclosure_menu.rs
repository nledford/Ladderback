//! The disclosure menu (components §4.5): a button that shows and hides a
//! panel of links under it. [`rules`] decides when it opens and closes,
//! host-tested; this module is the glue that draws it.

use leptos::html;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

use super::Icon;
use rules::FocusMoved;

// When the menu opens and closes and what its button tells, decided on the
// host; `DisclosureMenu` is its glue.
pub(crate) mod rules;

/// A disclosure menu (§4.5, decision 152): a native `<button>` whose words
/// are `label`, then a chevron, and under it a panel holding `children`, the
/// rows: links, and buttons where a row does something (Sign out). Not
/// ARIA's `menu` role, which is for an application's commands and makes a
/// screen reader treat links as menu items: its rows are links and buttons
/// Tab reaches in order, with no arrow keys of their own.
///
/// - **The button** says whether the panel shows (`aria-expanded`) and names
///   it (`aria-controls`, `panel_id`). It looks like the links beside it:
///   the text's color, no box, the underline under the pointer. A press, or
///   Enter or Space, shows the panel; the pointer resting on it never does.
///   While `current` holds, the page the reader is on is one of the panel's,
///   so the button carries `aria-current="true"` and is drawn as a current
///   link is, bold with a 3px bar at its inline start: where the reader is
///   shows with the panel closed.
/// - **The panel** stands under the button, its inline end at the button's,
///   in the page's color with an edge in `--field-edge` and the small radius,
///   never wider than the window less 2rem (1.4.10). It covers what lies
///   under it, so it closes whenever focus moves out of the menu (2.4.11);
///   focus is never kept in it.
/// - **It closes** on Escape, which returns focus to the button; on a row's
///   link chosen; on focus moving to an element outside it; and on a press
///   of the pointer outside it. Escape does nothing while the button isn't
///   drawn: a page may show the panel's rows in a menu of its own and hide
///   the button, as the header's hamburger panel does below 54rem
///   (`style.css`), and that menu's Escape is then its own.
///
/// What the caller owes WCAG 2.2 AA, since the menu does none of it:
///
/// - **A `label` that names the group of rows** (2.4.6), the same whether
///   the panel shows or not.
/// - **A `panel_id` unique on the page**, which `aria-controls` names.
#[component]
pub fn DisclosureMenu(
    label: String,
    panel_id: String,
    #[prop(into)] current: Signal<bool>,
    children: Children,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let root = NodeRef::<html::Div>::new();
    let button = NodeRef::<html::Button>::new();

    // Escape closes the panel and returns focus to the button, while the
    // button is drawn: a `display: none` box has no size.
    let on_keydown = move |event: web_sys::KeyboardEvent| {
        let drawn = button.get_untracked().is_some_and(|button| {
            let rect = button.get_bounding_client_rect();
            rules::is_drawn(rect.width(), rect.height())
        });
        if rules::escape_closes(open.get_untracked(), &event.key(), drawn) {
            open.set(false);
            if let Some(button) = button.get_untracked() {
                // A failed focus leaves focus where it was.
                let _ = button.focus();
            }
        }
    };
    // Focus leaving an element of the menu: inside it, outside it, or to
    // nothing at all.
    let on_focusout = move |event: web_sys::FocusEvent| {
        let moved = match event
            .related_target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
        {
            None => FocusMoved::Nowhere,
            Some(target) => {
                let inside = root
                    .get_untracked()
                    .is_some_and(|root| root.contains(Some(&target)));
                if inside {
                    FocusMoved::Inside
                } else {
                    FocusMoved::Outside
                }
            }
        };
        if rules::closes_when_focus_moves(open.get_untracked(), moved) {
            open.set(false);
        }
    };
    // A press of the pointer anywhere on the page: outside the menu it
    // closes it, as focus moving there would.
    let pressed = window_event_listener(leptos::ev::pointerdown, move |event| {
        let inside = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|target| {
                root.get_untracked()
                    .is_some_and(|root| root.contains(Some(&target)))
            });
        if rules::closes_on_press(open.get_untracked(), inside) {
            open.set(false);
        }
    });
    on_cleanup(move || pressed.remove());
    // A row's link chosen closes the panel, the current page's too, which
    // changes no route: focus then goes back to the button, since no page's
    // heading takes it and the link it was on is no longer drawn.
    let on_panel_click = move |event: web_sys::MouseEvent| {
        let link = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|target| target.closest("a").ok().flatten());
        if let Some(link) = link {
            open.set(false);
            if rules::focus_returns_on_choice(link.get_attribute("aria-current").as_deref())
                && let Some(button) = button.get_untracked()
            {
                // A failed focus leaves focus where it was.
                let _ = button.focus();
            }
        }
    };

    view! {
        <div
            class="disclosure-menu relative"
            node_ref=root
            on:keydown=on_keydown
            on:focusout=on_focusout
        >
            <button
                type="button"
                class=rules::BUTTON
                node_ref=button
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-controls=panel_id.clone()
                aria-current=move || rules::current(current.get())
                on:click=move |_| open.update(|open| *open = rules::toggled(*open))
            >
                {label}
                <Icon glyph=Signal::derive(move || rules::chevron(open.get())) />
            </button>
            <div id=panel_id class=move || rules::panel_class(open.get()) on:click=on_panel_click>
                {children()}
            </div>
        </div>
    }
}
