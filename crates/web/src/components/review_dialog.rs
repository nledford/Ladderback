use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use super::{return_focus, trap_focus};
use crate::live::{close_modal_when_gone, modal_gone, open_modal};

/// What a review's confirm button says, and what pressing it does.
#[derive(Clone)]
pub struct Confirmation {
    pub label: String,
    pub action: Callback<()>,
}

/// What a review shows while the run it confirmed is being started, which
/// is a request to the daemon and can take a while to answer.
#[derive(Clone)]
pub struct Working {
    /// Whether the start is out. The review then holds still: its buttons stay
    /// where a keyboard and a screen reader find them, `aria-disabled`, and
    /// Escape does not dismiss it.
    pub active: Signal<bool>,
    /// What the review says while it holds still. It is said in a polite live
    /// region of the dialog's own, since the page's announcer waits while a
    /// modal is open.
    pub status: String,
}

/// A review dialog (§4.5): what a run would do, shown before it runs, with the
/// person's word asked for. It is a native `<dialog>` opened with `showModal`,
/// as [`ModalDialog`](super::ModalDialog) is, but opened by whoever owns the run: the page sets
/// `open` once it knows what the run would do, and the dialog clears it
/// however it closes (Escape, the dismiss button, or the owner clearing it
/// first). It opens with focus on its first button, the dismiss one, so a
/// stray Enter backs out and never confirms. Closing returns focus to the
/// control with the id `returns_to`, the one that asked for the review, or to
/// the element with the id `fallback` when that control is gone, so that
/// focus never falls to the page.
///
/// A review with something to confirm has a [`Confirmation`]: the dismiss
/// button and then the confirm one, whose action leaves the dialog open until
/// the owner closes it. A review with two ways to go ahead, such as a restart
/// once the running operations finish or now, passes the second as `also`,
/// which comes after the confirm button, so the dismiss button still comes
/// first. A review with nothing to do has none, and offers only
/// the dismiss button. `described_by` is the id of the element in `children`
/// that says what the review is about, which a screen reader reads as the
/// dialog's description. While the dialog is open, polite messages wait
/// ([`crate::live::modal_opened`]).
///
/// While the owner is starting the run ([`Working`]), no button does
/// anything and Escape does not close the dialog, so a review that is being
/// confirmed is never dismissed from under the request it sent.
///
/// While `held` says why the run can't be confirmed yet, such as a dry run
/// still reading, the buttons that go ahead stay where a keyboard and a
/// screen reader find them, `aria-disabled` and described by that reason,
/// shown beside them; pressing one does nothing but say the reason in the
/// dialog's polite region. Dismissing still works.
///
/// `starts_on` is a button in `children` focus starts on instead of the
/// dismiss button, for a dialog whose dismissal loses something, such as a
/// secret shown once, which a stray Enter would then throw away.
#[component]
pub fn ReviewDialog(
    open: RwSignal<bool>,
    title: String,
    dismiss_label: String,
    confirm: Option<Confirmation>,
    #[prop(optional_no_strip)] also: Option<Confirmation>,
    returns_to: String,
    described_by: String,
    #[prop(optional)] working: Option<Working>,
    #[prop(optional)] held: Option<Signal<Option<String>>>,
    #[prop(optional)] fallback: Option<String>,
    #[prop(optional)] starts_on: Option<NodeRef<leptos::html::Button>>,
    children: Children,
) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let dismiss = NodeRef::<leptos::html::Button>::new();
    // Whether the announcer counts the dialog as an open modal, so leaving the
    // page while it is open closes it and ends the count however it went away.
    let counted = RwSignal::new(false);
    close_modal_when_gone(dialog, counted);
    let (busy, status) = match working {
        Some(Working { active, status }) => (active, status),
        None => (Signal::stored(false), String::new()),
    };
    let title_id = format!("{returns_to}-title");
    let held_id = format!("{returns_to}-held");
    let held = held.unwrap_or_else(|| Signal::stored(None));
    // Whether a button that goes ahead was pressed while held, so the polite
    // region says why; cleared first on each press, so a second press is said
    // again.
    let pressed_held = RwSignal::new(false);
    Effect::new(move |_| {
        if held.with(Option::is_none) {
            pressed_held.set(false);
        }
    });
    let (opener, opener_fallback) = (returns_to.clone(), fallback.clone());
    Effect::new(move |_| {
        let wanted = open.get();
        let Some(dialog) = dialog.get() else {
            return;
        };
        if wanted {
            // The browser queues a dialog's `close` event rather than raising
            // it as the dialog closes, and Chromium has been seen to raise it
            // after the reader's next key press: a dialog closed with Escape
            // and opened again at once still waits for it. That closing is
            // over, so it is counted gone now, and its event, come late, finds
            // the dialog open and is left alone, or it would close the dialog
            // under the reader.
            if !dialog.open() {
                modal_gone(counted);
            }
            open_modal(&dialog, counted);
            // The browser puts focus on the first control it finds; the
            // dismiss button is the one a review means to start on, unless
            // the owner named another.
            if let Some(first) = starts_on.unwrap_or(dismiss).get_untracked() {
                let _ = first.focus();
            }
        } else if dialog.open() {
            // Closed by its owner, which no `close` handler can be counted on
            // to follow once the owner has taken the dialog off the page.
            dialog.close();
            return_focus(&opener, opener_fallback.as_deref());
        }
    });
    let body = children();
    let choice = {
        let held_id = held_id.clone();
        move |Confirmation { label, action }| {
            let held_id = held_id.clone();
            view! {
                <button
                    type="button"
                    aria-disabled=move || {
                        (busy.get() || held.with(Option::is_some)).then_some("true")
                    }
                    aria-describedby=move || { held.with(Option::is_some).then(|| held_id.clone()) }
                    on:click=move |_| {
                        if busy.get_untracked() {
                            return;
                        }
                        if held.with_untracked(Option::is_some) {
                            pressed_held.set(false);
                            spawn_local(async move {
                                tick().await;
                                pressed_held.try_set(true);
                            });
                        } else {
                            action.run(());
                        }
                    }
                >
                    {label}
                </button>
            }
        }
    };
    let confirm = confirm.map(choice.clone());
    let also = also.map(choice);
    view! {
        // A `close` event that finds the dialog open is late, for a closing
        // the dialog was opened again since (see above): that closing was
        // already counted gone, so the event is left alone.
        <dialog
            node_ref=dialog
            aria-labelledby=title_id.clone()
            aria-describedby=described_by
            on:cancel=move |ev: web_sys::Event| {
                if busy.get_untracked() {
                    ev.prevent_default();
                }
            }
            on:close=move |_| {
                if dialog.get_untracked().is_some_and(|dialog| dialog.open()) {
                    return;
                }
                modal_gone(counted);
                open.try_set(false);
                return_focus(&returns_to, fallback.as_deref());
            }
            on:keydown=move |ev: web_sys::KeyboardEvent| trap_focus(&dialog, &ev)
        >
            <h3 id=title_id.clone()>{title}</h3>
            {body}
            {move || {
                held.get()
                    .map(|reason| {
                        view! {
                            <p id=held_id.clone() class="text-muted">
                                {reason}
                            </p>
                        }
                    })
            }}
            <p aria-live="polite" aria-atomic="true">
                {move || {
                    if busy.get() {
                        Some(status.clone())
                    } else {
                        held.get().filter(|_| pressed_held.get())
                    }
                }}
            </p>
            <p class="actions">
                <button
                    type="button"
                    node_ref=dismiss
                    aria-disabled=move || busy.get().then_some("true")
                    on:click=move |_| {
                        if !busy.get_untracked() && let Some(dialog) = dialog.get() {
                            dialog.close();
                        }
                    }
                >
                    {dismiss_label}
                </button>
                {confirm}
                {also}
            </p>
        </dialog>
    }
}
