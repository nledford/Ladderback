use leptos::prelude::*;
use leptos::task::{spawn_local, tick};
use leptos::wasm_bindgen::JsCast;

use super::button::{Appearance, Content};
use super::{ACTIONS, Button, ModalDialog, TitleLevel};
use crate::variants::Tone;

/// What a review's confirm button says, the tone it is drawn in (`Error`
/// for one that deletes or ends something, `Primary` otherwise), and what
/// pressing it does. The confirm button is the review's one filled button;
/// a second way to go ahead (`also`) is outlined in its tone.
#[derive(Clone)]
pub struct Confirmation {
    pub label: String,
    pub tone: Tone,
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
/// person's word asked for. It is a [`ModalDialog`] titled at level 3, under
/// the section of the page that asked for it, and opened by whoever owns the
/// run: the page sets `open` once it knows what the run would do, and the
/// dialog clears it however it closes (Escape, the dismiss button, or the
/// owner clearing it first). It opens with focus on its first button, the
/// dismiss one, so a stray Enter backs out and never confirms. Closing returns
/// focus to the control with the id `returns_to`, the one that asked for the
/// review, or to the element with the id `fallback` when that control is gone,
/// so that focus never falls to the page.
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
    let dismiss = NodeRef::<leptos::html::Button>::new();
    let (busy, status) = match working {
        Some(Working { active, status }) => (active, status),
        None => (Signal::stored(false), String::new()),
    };
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
    let body = children();
    // Which button that goes ahead was pressed, so only it shows the wait:
    // the confirm one (1) or `also` (2).
    let pressed_choice = RwSignal::new(0_u8);
    let (confirm_button, also_button) = (
        NodeRef::<leptos::html::Button>::new(),
        NodeRef::<leptos::html::Button>::new(),
    );
    let choice = {
        let held_id = held_id.clone();
        move |Confirmation {
                  label,
                  tone,
                  action,
              }: Confirmation,
              which: u8,
              appearance: Appearance,
              node_ref: NodeRef<leptos::html::Button>| {
            let held_id = held_id.clone();
            view! {
                <Button
                    content=Content::Text(label.into())
                    appearance=appearance
                    tone=tone
                    node_ref=node_ref
                    busy=Signal::derive(move || busy.get() && pressed_choice.get() == which)
                    disabled=Signal::derive(move || busy.get() || held.with(Option::is_some))
                    described_by=Signal::derive(move || {
                        held.with(Option::is_some).then(|| held_id.clone())
                    })
                    on_press=Callback::new(move |()| {
                        pressed_choice.set(which);
                        action.run(());
                    })
                />
            }
        }
    };
    let confirm = confirm
        .map(|confirmation| choice.clone()(confirmation, 1, Appearance::Filled, confirm_button));
    let also = also.map(|confirmation| choice(confirmation, 2, Appearance::Outlined, also_button));
    // A held button that goes ahead takes no press, as an unavailable button
    // doesn't, but its press still bubbles here, where it says the reason.
    let press_held = move |event: web_sys::MouseEvent| {
        if busy.get_untracked() || held.with_untracked(Option::is_none) {
            return;
        }
        let target = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok());
        let goes_ahead = [confirm_button, also_button].into_iter().any(|button| {
            button
                .get_untracked()
                .is_some_and(|button| button.contains(target.as_ref()))
        });
        if goes_ahead {
            pressed_held.set(false);
            spawn_local(async move {
                tick().await;
                pressed_held.try_set(true);
            });
        }
    };
    // Escape dismisses the review as its dismiss button does, unless the run
    // is being started: the review then holds still.
    let escape = Callback::new(move |()| {
        if !busy.get_untracked() {
            open.set(false);
        }
    });
    view! {
        <ModalDialog
            open=open
            id=returns_to.clone()
            title=title
            title_level=TitleLevel::Three
            described_by=described_by
            starts_on=starts_on.unwrap_or(dismiss)
            on_cancel=escape
            returns_to=returns_to
            fallback=fallback
        >
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
            <p class=ACTIONS on:click=press_held>
                <Button
                    content=Content::Text(dismiss_label.into())
                    node_ref=dismiss
                    disabled=busy
                    on_press=Callback::new(move |()| open.set(false))
                />
                {confirm}
                {also}
            </p>
        </ModalDialog>
    }
}
