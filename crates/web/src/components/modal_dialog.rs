use leptos::either::Either;
use leptos::prelude::*;

use super::button::Content;
use super::{Button, recover_focus, return_focus, trap_focus};
use crate::live::{close_modal_when_gone, modal_gone, open_modal};

/// The level of a modal dialog's title: `Two` for a dialog of the page's
/// own, `Three` for one titled under the section of the page that asked for
/// it, as a review is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleLevel {
    #[default]
    Two,
    Three,
}

/// A modal dialog (`<dialog>` + `showModal`), open while `open` is `true`:
/// its owner sets `open` to open it and clears it to close it, and the dialog
/// clears it however else it closes. Focus is trapped inside while it is open
/// (the browser's trap, backed by an explicit Tab wrap, since a synthetic Tab
/// press escapes a native dialog), and returns to the invoker when it closes,
/// or to the element with the id `fallback` when focus would fall to the
/// page. While it is open, polite messages wait
/// ([`crate::live::modal_opened`]).
///
/// With `returns_to`, focus returns instead to the control with that id
/// however the dialog closes, its owner clearing `open` too, so it finds a
/// control the page drew anew while the dialog was open; `fallback` then
/// names where focus goes when that control is gone.
///
/// It is named by its title, an `<h2>` (or an `<h3>`, by `title_level`) with
/// the id `{id}-title`, and described by the element in `children` with the
/// id `described_by`, so a screen reader reads both as focus moves in. Focus
/// starts on its first control, or on `starts_on`. With a `close_label` it
/// ends on a button that closes it.
///
/// Escape closes it, unless the owner takes Escape with `on_cancel`: the
/// dialog then stays open, and the owner closes it by clearing `open` once it
/// has done what Escape asks. A browser may still close it on a second
/// Escape (Chromium's close watcher), which clears `open` as any close does.
/// `on_close` runs each time it closes while its owner still shows it,
/// before `open` is cleared; taking the dialog away closes it without.
#[component]
pub fn ModalDialog(
    open: RwSignal<bool>,
    #[prop(into)] id: String,
    title: String,
    #[prop(optional)] title_level: TitleLevel,
    #[prop(optional, into)] described_by: Option<String>,
    #[prop(optional)] close_label: Option<String>,
    #[prop(optional)] starts_on: Option<NodeRef<leptos::html::Button>>,
    #[prop(optional)] on_cancel: Option<Callback<()>>,
    #[prop(optional)] on_close: Option<Callback<()>>,
    #[prop(optional, into)] returns_to: Option<String>,
    // Not stripped, so a component drawn with it passes on its own optional
    // fallback.
    #[prop(optional_no_strip, into)] fallback: Option<String>,
    children: Children,
) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    // Whether the announcer counts the dialog as an open modal, so leaving the
    // page while it is open closes it and ends the count however it went away.
    let counted = RwSignal::new(false);
    close_modal_when_gone(dialog, counted);
    let (owner_returns_to, owner_fallback) = (returns_to.clone(), fallback.clone());
    Effect::new(move |_| {
        let wanted = open.get();
        let Some(dialog) = dialog.get() else {
            return;
        };
        if wanted {
            // The browser queues a dialog's `close` event rather than raising
            // it as the dialog closes, and Chromium has been seen to raise it
            // after the reader's next key press: a dialog closed with Escape
            // and opened again at once may still wait for it. That closing is
            // over, so it is counted gone now, and its event, come late, finds
            // the dialog open and is left alone.
            if !dialog.open() {
                modal_gone(counted);
            }
            open_modal(&dialog, counted);
            // The browser puts focus on the first control it finds.
            if let Some(first) = starts_on.and_then(|button| button.get_untracked()) {
                let _ = first.focus();
            }
        } else if dialog.open() {
            dialog.close();
            // Closed through `open`, by its owner or a control of the
            // dialog's own, which no `close` event can be counted on to follow
            // once the owner has taken the dialog off the page.
            if let Some(to) = &owner_returns_to {
                return_focus(to, owner_fallback.as_deref());
            }
        }
    });
    let title_id = format!("{id}-title");
    let heading = match title_level {
        TitleLevel::Two => Either::Left(view! { <h2 id=title_id.clone()>{title}</h2> }),
        TitleLevel::Three => Either::Right(view! { <h3 id=title_id.clone()>{title}</h3> }),
    };
    let body = children();
    let close = close_label.map(|label| {
        view! {
            <Button
                content=Content::Text(label.into())
                on_press=Callback::new(move |()| {
                    if let Some(dialog) = dialog.get() {
                        dialog.close();
                    }
                })
            />
        }
    });
    view! {
        // A `close` event comes after the dialog has closed, and may come
        // once it has opened again, its closing already counted gone: it is
        // then left alone, or it would close the dialog under the reader.
        <dialog
            node_ref=dialog
            aria-labelledby=title_id
            aria-describedby=described_by
            on:cancel=move |event: web_sys::Event| {
                if let Some(on_cancel) = on_cancel {
                    event.prevent_default();
                    on_cancel.try_run(());
                }
            }
            on:close=move |_| {
                let Some(Some(closed)) = dialog.try_get_untracked() else {
                    return;
                };
                if closed.open() {
                    return;
                }
                modal_gone(counted);
                if let Some(on_close) = on_close {
                    on_close.try_run(());
                }
                open.try_set(false);
                match (&returns_to, &fallback) {
                    (Some(to), fallback) => return_focus(to, fallback.as_deref()),
                    (None, Some(fallback)) => recover_focus(fallback),
                    (None, None) => {}
                }
            }
            on:keydown=move |event: web_sys::KeyboardEvent| trap_focus(&dialog, &event)
        >
            {heading}
            {body}
            {close}
        </dialog>
    }
}
