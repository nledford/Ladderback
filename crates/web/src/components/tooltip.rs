//! The tooltip (components §4.5): a short note beside a control, shown on
//! keyboard focus at once and on a resting pointer after a moment. [`rules`]
//! decides what a screen reader meets, when it shows and where it stands,
//! host-tested; this module is the glue that measures the page and draws it.

use std::cell::Cell;
use std::time::Duration;

use leptos::ev;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos::wasm_bindgen::closure::Closure;

use crate::variants::Side;

pub(crate) mod rules;

pub use rules::Tip;
use rules::{Event, Rect, Room, Shown, Wait};

thread_local! {
    /// The number the next tooltip's `id` takes, so no two on a page share one.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

/// How the note is drawn, in Tailwind utilities (decision 117), logical ones
/// only: in the top layer at the `left` and `top` the page sets (in the
/// viewport, `position: fixed`), so no container clips it. The page's
/// colors swapped, the small corner, flat (a popover draws no shadow of its
/// own, and `shadow-none` would compute as five clear ones), no arrow; a
/// step smaller than its control's text, in em, so it grows with the
/// reader's text (1.4.4), and wrapping at about 20 of its em, or at the
/// room the page gives it
/// (`--room`: the viewport's width, without its scroll bar, less an edge at
/// each end), never cut. It fades in over 100 ms and out over 60 ms, sooner,
/// since what the reader turned from should not linger (a transition takes
/// the duration of the state it goes to: the open one's is `open:`'s, which
/// on this `<span>` is only `:popover-open`), with `display` and `overlay`
/// held until the fade ends, for a reader who asks for less motion too (a
/// fade, which the reduced-motion rule keeps); it never takes the pointer
/// while hidden. Under forced colors it is the page's own colors, edged in
/// its text color, so it stands apart from what it lies over.
const NOTE: &str = "tooltip fixed inset-auto m-0 border-0 rounded-(--radius-small) \
    bg-fg text-bg text-[0.875em] leading-[1.4] py-[0.25em] px-[0.5em] \
    max-inline-[min(20em,var(--room,100vw))] wrap-anywhere opacity-100 \
    transition-[opacity,display,overlay] duration-60 ease-[ease] transition-discrete \
    open:duration-100 starting:open:opacity-0 not-open:opacity-0 not-open:pointer-events-none \
    forced-colors:border forced-colors:border-[color:CanvasText] \
    forced-colors:bg-[color:Canvas] forced-colors:text-[color:CanvasText]";

/// A tooltip (§4.5): `tip`, a short note beside the one control its
/// `children` are, standing on `side` of it (Above unless the caller names
/// another) where there is room, flipping where there isn't ([`rules::place`]).
///
/// Keyboard focus on the control shows it at once (`:focus-visible`: a
/// mouse's press doesn't, since the pointer resting there already does); a
/// pointer resting on the control shows it after [`rules::HOVER_DELAY_MS`],
/// and it stays while the pointer is on the control or on the tooltip, with
/// [`rules::GRACE_MS`] to cross the gap between them. Escape hides it
/// without moving focus or the pointer. A finger shows it only as the focus
/// it gives does. It is a `popover="manual"` in the top layer, so no
/// container clips it, placed in the viewport and moved as the page scrolls
/// or the window changes size; it is never over its control (2.4.11).
///
/// A [`Tip::Describes`] is the control's description: the control's
/// `aria-describedby` names the tooltip, a `role="tooltip"`. A [`Tip::Names`]
/// is an icon-only control's name: it becomes the control's `aria-label`,
/// and the tooltip is hidden from a screen reader, so the name is heard once.
///
/// What the caller owes WCAG 2.2 AA, since the tooltip does none of it:
///
/// - **Never a control's only name,** except as [`Tip::Names`] on an
///   icon-only control, where the name lives on the control too.
/// - **Short text,** a sentence or two, and nothing the reader needs to do
///   the task: a tooltip is found only by focusing or hovering its control.
/// - **No interactive content inside:** it is never a tab stop, and a link
///   or a button in it couldn't be reached; that is a popover's (#784).
/// - **One control as its children,** the element it describes: the first
///   element inside the tooltip's wrapper.
///
/// It takes no tone and no size: it is the page's colors swapped, a step
/// smaller than its control's text, a note rather than a status.
#[component]
pub fn Tooltip(tip: Tip, #[prop(optional)] side: Side, children: Children) -> impl IntoView {
    let id = NEXT_ID.with(|next| {
        let number = next.get();
        next.set(number.wrapping_add(1));
        format!("tooltip-{number}")
    });
    let anchor = NodeRef::<leptos::html::Span>::new();
    let note = NodeRef::<leptos::html::Span>::new();
    let machine = Machine {
        shown: StoredValue::new(Shown::default()),
        open: RwSignal::new(false),
        wait: StoredValue::new(None),
    };

    // The control takes its description or its name from the tooltip, once
    // it is on the page.
    let described_by = tip.described_by(&id).map(str::to_owned);
    let label = tip.label().map(str::to_owned);
    Effect::new(move |_| {
        let Some(control) = anchor.get().and_then(|anchor| anchor.first_element_child()) else {
            return;
        };
        if let Some(id) = &described_by {
            describe(&control, id);
        }
        if let Some(label) = &label {
            let _ = control.set_attribute("aria-label", label);
        }
    });

    Effect::new(move |_| {
        let open = machine.open.get();
        let Some(note) = note.get() else {
            return;
        };
        if open {
            let _ = note.show_popover();
            stand(anchor, &note, side);
        } else {
            let _ = note.hide_popover();
        }
    });

    // Where the page scrolls (any scroller, so the capture) or the window
    // changes size, an open tooltip moves with its control.
    let follow = move || {
        if machine.open.try_get_untracked() == Some(true)
            && let Some(note) = note.try_get_untracked().flatten()
        {
            stand(anchor, &note, side);
        }
    };
    let resized = window_event_listener(ev::resize, move |_| follow());
    let scrolled = Closure::<dyn Fn()>::new(follow).into_js_value();
    let _ = window().add_event_listener_with_callback_and_bool(
        "scroll",
        scrolled.unchecked_ref(),
        true,
    );
    let scrolled = StoredValue::new_local(scrolled);
    let escaped = window_event_listener(ev::keydown, move |event| {
        if event.key() != "Escape" {
            return;
        }
        let was_open = machine.is_open();
        machine.send(Event::Escape);
        // Escape that hid a tooltip did its work; a dialog around the
        // control stays open until the next.
        if was_open {
            event.prevent_default();
        }
    });
    on_cleanup(move || {
        machine.call_off();
        resized.remove();
        escaped.remove();
        scrolled.with_value(|scrolled| {
            let _ = window().remove_event_listener_with_callback_and_bool(
                "scroll",
                scrolled.unchecked_ref(),
                true,
            );
        });
    });

    let on_focus = move |event: web_sys::FocusEvent| {
        let shows = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|control| control.matches(":focus-visible").unwrap_or(false));
        if shows {
            machine.send(Event::Focus);
        }
    };
    let on_pointer = move |event: web_sys::PointerEvent, then: Event| {
        if rules::hovers(&event.pointer_type()) {
            machine.send(then);
        }
    };
    let (role, hidden, text) = (tip.role(), tip.hidden(), tip.text().to_owned());
    view! {
        <span
            class="tooltip-anchor"
            node_ref=anchor
            on:focusin=on_focus
            on:focusout=move |_| machine.send(Event::Blur)
            on:pointerenter=move |event| on_pointer(event, Event::PointerIn)
            on:pointerleave=move |event| on_pointer(event, Event::PointerOut)
        >
            {children()}
            <span class=NOTE node_ref=note id=id popover="manual" role=role aria-hidden=hidden>
                {text}
            </span>
        </span>
    }
}

/// Adds `id` to the control's `aria-describedby`, after any description it
/// has already.
fn describe(control: &web_sys::Element, id: &str) {
    let ids = control
        .get_attribute("aria-describedby")
        .unwrap_or_default();
    if !ids.split_whitespace().any(|known| known == id) {
        let ids = if ids.trim().is_empty() {
            id.to_owned()
        } else {
            format!("{ids} {id}")
        };
        let _ = control.set_attribute("aria-describedby", &ids);
    }
}

/// Places the open tooltip `note` beside the control `anchor` holds, as
/// [`rules::place`] decides from what the page measures: the control's box,
/// the tooltip's size, the viewport (without its scroll bars), the
/// control's direction and text size. All of it is in the viewport's own
/// coordinates, `left` and `top` from its top-left corner in every direction
/// of writing; the tooltip is `position: fixed`, so they are its own.
fn stand(anchor: NodeRef<leptos::html::Span>, note: &web_sys::HtmlElement, side: Side) {
    let Some(control) = anchor
        .try_get_untracked()
        .flatten()
        .and_then(|anchor| anchor.first_element_child())
    else {
        return;
    };
    let window = window();
    let Some(viewport) = window
        .document()
        .and_then(|document| document.document_element())
    else {
        return;
    };
    let Ok(Some(computed)) = window.get_computed_style(&control) else {
        return;
    };
    let Some(em) = computed
        .get_property_value("font-size")
        .ok()
        .and_then(|size| {
            size.strip_suffix("px")
                .and_then(|px| px.parse::<f64>().ok())
        })
    else {
        return;
    };
    let direction =
        l10n::Direction::of_computed(&computed.get_property_value("direction").unwrap_or_default());
    let room = Room {
        width: f64::from(viewport.client_width()),
        height: f64::from(viewport.client_height()),
        direction,
        gap: rules::GAP_EM * em,
        edge: rules::EDGE_EM * em,
    };
    let style = note.style();
    // Measured in the viewport's corner, where it has the whole width to
    // wrap in, so its size is its own, not what room is left where it was;
    // it wraps at the viewport's width less an edge at each end, a width
    // CSS can't give it, since `100vw` counts the scroll bar.
    let _ = style.set_property("--room", &format!("{}px", room.width - 2.0 * room.edge));
    let _ = style.set_property("left", "0px");
    let _ = style.set_property("top", "0px");
    let control = control.get_bounding_client_rect();
    let size = note.get_bounding_client_rect();
    let spot = rules::place(
        Rect {
            left: control.left(),
            top: control.top(),
            width: control.width(),
            height: control.height(),
        },
        size.width(),
        size.height(),
        room,
        side,
    );
    let _ = style.set_property("left", &format!("{}px", spot.left));
    let _ = style.set_property("top", &format!("{}px", spot.top));
}

/// A tooltip's state ([`Shown`]), whether it shows (a signal the drawing
/// follows), and its one wait.
#[derive(Clone, Copy)]
struct Machine {
    shown: StoredValue<Shown>,
    open: RwSignal<bool>,
    wait: StoredValue<Option<TimeoutHandle>>,
}

impl Machine {
    /// Takes in `event`, does what it says of the wait, and shows or hides
    /// the tooltip. Nothing, once the tooltip has gone.
    fn send(self, event: Event) {
        let Some(wait) = self.shown.try_update_value(|shown| shown.on(event)) else {
            return;
        };
        match wait {
            Wait::Keep => {}
            Wait::CallOff => self.call_off(),
            Wait::Start(ms, due) => {
                self.call_off();
                let ended = move || {
                    self.wait.try_set_value(None);
                    self.send(due);
                };
                if let Ok(handle) = set_timeout_with_handle(ended, Duration::from_millis(ms.into()))
                {
                    self.wait.try_set_value(Some(handle));
                }
            }
        }
        let open = self.is_open();
        if self.open.try_get_untracked() != Some(open) {
            self.open.try_set(open);
        }
    }

    /// Whether the tooltip shows now.
    fn is_open(self) -> bool {
        self.shown.try_with_value(Shown::open).unwrap_or(false)
    }

    /// Calls off the wait, if one is running.
    fn call_off(self) {
        if let Some(Some(handle)) = self.wait.try_update_value(Option::take) {
            handle.clear();
        }
    }
}
