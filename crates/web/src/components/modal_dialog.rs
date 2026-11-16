use leptos::prelude::*;

use super::button::Content;
use super::{Button, trap_focus};
use crate::live::{close_modal_when_gone, modal_gone, open_modal};

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
    // page while it is open closes it and ends the count however it went away.
    let counted = RwSignal::new(false);
    close_modal_when_gone(dialog, counted);
    let body = children();
    view! {
        <Button
            content=Content::Text(open_label.into())
            on_press=Callback::new(move |()| {
                if let Some(dialog) = dialog.get() {
                    open_modal(&dialog, counted);
                }
            })
        />
        <dialog
            node_ref=dialog
            aria-label=label.clone()
            on:close=move |_| modal_gone(counted)
            on:keydown=move |ev: web_sys::KeyboardEvent| trap_focus(&dialog, &ev)
        >
            <h3>{label.clone()}</h3>
            {body}
            <Button
                content=Content::Text(close_label.into())
                on_press=Callback::new(move |()| {
                    if let Some(dialog) = dialog.get() {
                        dialog.close();
                    }
                })
            />
        </dialog>
    }
}
