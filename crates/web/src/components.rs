//! The shared component set (technical design §4.5): native HTML first, each
//! component shown in the gallery with demo state (loading, empty, error,
//! success) and driven by keyboard-only journeys. Stubbed for the red commit
//! (§13.1): the bodies and the gallery wiring arrive with the implementation
//! commits.

// Unwired until the gallery shows them, and `todo!()` is the point of this
// commit.
#![allow(clippy::todo)]

use leptos::prelude::*;

/// A modal dialog (`<dialog>` + `showModal`): focus is trapped inside while it
/// is open, Escape closes it, and focus returns to the element that opened it.
#[component]
pub fn ModalDialog(label: String, children: Children) -> AnyView {
    let _ = (label, children);
    todo!()
}

/// A data table: a caption, `scope`d headers, and columns sortable through
/// buttons in the headers, with the sort state announced politely.
#[component]
pub fn DataTable(caption: String, children: Children) -> AnyView {
    let _ = (caption, children);
    todo!()
}

/// A form field: a label, a description wired with `aria-describedby`, and
/// error text that describes the input and is announced politely.
#[component]
pub fn FormField(label: String, children: Children) -> AnyView {
    let _ = (label, children);
    todo!()
}

/// A status badge: a short labeled state, colored by tokens whose contrast the
/// token script checks.
#[component]
pub fn StatusBadge(label: String, children: Children) -> AnyView {
    let _ = (label, children);
    todo!()
}

/// A progress bar (`role="progressbar"` with `aria-valuenow` and friends). Its
/// value changes are never announced.
#[component]
pub fn Progress(label: String, value: ReadSignal<u8>) -> AnyView {
    let _ = (label, value);
    todo!()
}

/// A toast: rendered visually and spoken once through the announcer, politely;
/// assertively only for destructive results. Its disappearance is not
/// announced.
#[component]
pub fn Toast(label: String, children: ChildrenFn) -> AnyView {
    let _ = (label, children);
    todo!()
}
