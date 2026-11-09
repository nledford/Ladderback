//! The shared component set (technical design §4.5): native HTML first, each
//! component shown in the gallery with demo state (loading, empty, error,
//! success) and driven by keyboard-only journeys.
//!
//! Native elements carry the accessibility work: `<dialog>` with `showModal`
//! traps focus and returns it on close, table headers `scope` their columns,
//! and `aria-describedby` ties a form field's error to its input. The
//! announcer ([`crate::live`]) speaks what §4.5 says is spoken, and nothing
//! else: a progress bar's value is never announced.

use std::time::Duration;

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

use crate::announcer::Politeness;
use crate::live::{modal_gone, open_modal, say};

/// How long a toast stays on screen before it goes, unannounced.
const TOAST_MS: u64 = 3_000;

/// A modal dialog (`<dialog>` + `showModal`): focus is trapped inside while it
/// is open (the browser's trap, backed by an explicit Tab wrap, since a
/// synthetic Tab press escapes a native dialog), Escape closes it, and focus
/// returns to the invoker. While it is open, polite messages wait
/// ([`crate::live::modal_opened`]).
#[component]
pub fn ModalDialog(
    label: String,
    open_label: String,
    close_label: String,
    children: Children,
) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    // Whether the announcer counts the dialog as an open modal, so leaving the
    // page while it is open ends the count however the dialog went away.
    let counted = RwSignal::new(false);
    on_cleanup(move || modal_gone(counted));
    let body = children();
    view! {
        <button
            type="button"
            on:click=move |_| {
                if let Some(dialog) = dialog.get() {
                    open_modal(&dialog, counted);
                }
            }
        >
            {open_label}
        </button>
        <dialog
            node_ref=dialog
            aria-label=label.clone()
            on:close=move |_| modal_gone(counted)
            on:keydown=move |ev: web_sys::KeyboardEvent| trap_focus(&dialog, &ev)
        >
            <h3>{label.clone()}</h3>
            {body}
            <button type="button" on:click=move |_| if let Some(dialog) = dialog.get() {
                dialog.close();
            }>
                {close_label}
            </button>
        </dialog>
    }
}

/// Wraps Tab focus from the dialog's last focusable back to its first (and
/// Shift-Tab from the first to the last), keeping keyboard focus inside the
/// open dialog however the Tab key was pressed.
fn trap_focus(dialog: &NodeRef<leptos::html::Dialog>, event: &web_sys::KeyboardEvent) {
    if event.key() != "Tab" {
        return;
    }
    let Some(element) = dialog.get() else {
        return;
    };
    let Ok(focusables) = element
        .unchecked_ref::<web_sys::Element>()
        .query_selector_all(
            "button:not([disabled]), a[href], input:not([disabled]), \
         select:not([disabled]), textarea:not([disabled]), \
         [tabindex]:not([tabindex=\"-1\"])",
        )
    else {
        return;
    };
    let first = focusables.item(0);
    let last = focusables
        .length()
        .checked_sub(1)
        .and_then(|i| focusables.item(i));
    let active = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element());
    if let (Some(active), Some(first), Some(last)) = (active, first, last) {
        if first.is_same_node(Some(&last)) {
            return;
        }
        if active.is_same_node(Some(&last)) && !event.shift_key() {
            event.prevent_default();
            if let Some(el) = first.dyn_ref::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        } else if active.is_same_node(Some(&first)) && event.shift_key() {
            event.prevent_default();
            if let Some(el) = last.dyn_ref::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        }
    }
}

/// A data table: a caption, `scope`d headers, and a column sortable through a
/// button in its header, with each sort announced politely.
#[component]
pub fn SortableTable(
    caption: String,
    column: String,
    sort_label: String,
    sorted_message: String,
    rows: Vec<String>,
) -> impl IntoView {
    // Starts descending, so the first sort click is the ascending one the
    // gallery's journey checks.
    let sorted = RwSignal::new(false);
    let message = StoredValue::new(sorted_message);
    let shown = Memo::new(move |_| {
        let mut rows = rows.clone();
        if sorted.get() {
            rows.sort();
        }
        rows
    });
    view! {
        <table>
            <caption class="text-muted">{caption}</caption>
            <thead>
                <tr>
                    <th
                        scope="col"
                        aria-sort=move || {
                            if sorted.get() { "ascending" } else { "descending" }
                        }
                    >
                        {column.clone()}
                        <button
                            type="button"
                            on:click=move |_| {
                                sorted.set(!sorted.get_untracked());
                                say(Politeness::Polite, &message.with_value(String::clone));
                            }
                        >
                            {sort_label}
                        </button>
                    </th>
                </tr>
            </thead>
            <tbody>
                {move || {
                    shown
                        .get()
                        .into_iter()
                        .map(|row| view! { <tr><td>{row}</td></tr> })
                        .collect::<Vec<_>>()
                }}
            </tbody>
        </table>
    }
}

/// A form field: a label, an input, a description and an error, both wired to
/// the input with `aria-describedby`. The caller's check button decides when
/// `error` is `Some`; the caller announces it politely (as the gallery's
/// button does).
#[component]
pub fn FormField(
    label: String,
    description: String,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let field_id = "form-field-input";
    let described_by = format!("{field_id}-description");
    let error_id = format!("{field_id}-error");
    // Clones for the reactive closures, which outlive this scope.
    let described_in_attr = described_by.clone();
    let error_in_error = error_id.clone();
    let error_in_attr = error_id.clone();
    view! {
        <label for=field_id>{label.clone()}</label>
        <input
            id=field_id
            type="text"
            aria-describedby=move || {
                if error.get().is_some() {
                    format!("{described_in_attr} {error_in_attr}")
                } else {
                    described_in_attr.clone()
                }
            }
        />
        <p id=described_by.clone()>{description}</p>
        {move || {
            error
                .get()
                .map(|text| view! { <p id=error_in_error.clone() class="form-error">{text}</p> })
        }}
    }
}

/// A status badge: a short labeled state on a colored pill, one of
/// `badge-loading`, `badge-empty`, `badge-error` or `badge-success`, whose
/// text token on its background token is pair-checked by
/// `scripts/check-contrast.mjs`.
#[component]
pub fn StatusBadge(state: &'static str, label: String) -> impl IntoView {
    view! { <span class=format!("badge badge-{state}")>{label}</span> }
}
/// A progress bar (`role="progressbar"` with `aria-valuenow`, `aria-valuemin`,
/// `aria-valuemax` and `aria-valuetext`). Its value changes are never
/// announced: nothing here calls the announcer.
#[component]
pub fn Progress(label: String, value: RwSignal<u8>) -> impl IntoView {
    view! {
        <div
            role="progressbar"
            aria-label=label.clone()
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow=move || value.get().to_string()
            aria-valuetext=move || format!("{}%", value.get())
        >
            <div class="progress-track">
                <div class="progress-fill" style=move || format!("inline-size: {}%", value.get())></div>
            </div>
        </div>
        <span aria-hidden="true">{move || format!("{}%", value.get())}</span>
    }
}
/// A toast: shown as plain text (§4.5 renders toasts through the announcer,
/// so the element itself is not a second live region), announced politely
/// once through the announcer by [`show_toast`], and gone after [`TOAST_MS`]
/// without any announcement of its going.
#[component]
pub fn Toast(text: RwSignal<Option<String>>) -> impl IntoView {
    view! {
        {move || {
            text.get().map(|message| view! { <p class="toast">{message}</p> })
        }}
    }
}

/// Shows `message` as a toast for [`TOAST_MS`], announcing its arrival
/// politely; its disappearance is not announced.
pub(crate) fn show_toast(text: RwSignal<Option<String>>, message: String) {
    text.set(Some(message.clone()));
    say(Politeness::Polite, &message);
    set_timeout(move || text.set(None), Duration::from_millis(TOAST_MS));
}
