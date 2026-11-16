use leptos::prelude::*;

use crate::live::reply;
use rules::{ToastTimers, arrival};

// The toast's arrival and lifetime decisions are host-testable; the DOM
// glue around them runs in a browser via the gallery's @ui scenarios. Only
// the gallery shows a toast yet, and the host tests use these decisions, so
// only a build of neither leaves them unused.
#[cfg_attr(
    all(not(feature = "gallery"), not(test)),
    expect(dead_code, reason = "only the gallery shows a toast yet")
)]
mod rules;

thread_local! {
    /// The generations of the toasts shown so far, so an older toast's expiry
    /// no longer clears the text a newer toast is showing.
    static TOAST_TIMERS: ToastTimers = ToastTimers::default();
}

/// A toast: shown as plain text (§4.5 renders toasts through the announcer,
/// so the element itself is not a second live region), announced politely
/// once through the announcer by [`show_toast`], and gone after [`TOAST_MS`](rules::TOAST_MS)
/// without any announcement of its going.
#[component]
pub fn Toast(text: RwSignal<Option<String>>) -> impl IntoView {
    view! { {move || { text.get().map(|message| view! { <p class="toast">{message}</p> }) }} }
}

/// Shows `message` as a toast for the lifetime [`rules::arrival`]
/// picks, announcing its arrival politely; its disappearance is not
/// announced, and an older toast's expiry no longer clears it.
pub(crate) fn show_toast(text: RwSignal<Option<String>>, message: String) {
    let plan = arrival();
    let generation = TOAST_TIMERS.with(ToastTimers::arrive);
    text.set(Some(message.clone()));
    reply(plan.politeness, &message);
    set_timeout(
        move || {
            if TOAST_TIMERS.with(|timers| timers.still_current(generation)) {
                text.set(None);
            }
        },
        plan.shown_for,
    );
}
