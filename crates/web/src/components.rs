//! The shared component set (technical design §4.5): native HTML first, each
//! component shown in the gallery with demo state (loading, empty, error,
//! success) and driven by keyboard-only journeys.
//!
//! Native elements carry the accessibility work: `<dialog>` with `showModal`
//! traps focus and returns it on close, table headers `scope` their columns,
//! and `aria-describedby` ties a form field's error to its input. The
//! announcer ([`crate::live`]) speaks what §4.5 says is spoken, and nothing
//! else: a progress bar's value is never announced.

use std::rc::Rc;
use std::sync::Arc;

use l10n::Localizer;
use leptos::prelude::*;
use leptos::wasm_bindgen::closure::Closure;
use leptos::wasm_bindgen::{JsCast, JsValue};

use crate::announcer::Politeness;
use crate::focus::{FocusWrap, focus_wrap};
use crate::live::reply;
use crate::repositories::{Run, Sentence, address_parts, runs};
use alert::rules::AlertTone;

pub(crate) mod alert;
pub(crate) mod badge;
pub(crate) mod banner;
pub(crate) mod button;
mod checkbox;
mod error_summary;
pub(crate) mod icon;
pub(crate) mod link;
mod modal_dialog;
mod pause_updates;
mod progress;
mod radio_group;
mod review_dialog;
pub(crate) mod skeleton;
pub(crate) mod spinner;
pub(crate) mod status_badge;
mod step_up_field;
pub(crate) mod steps;
pub(crate) mod table;
pub(crate) mod tabs;
pub(crate) mod text_field;
mod toast;
pub(crate) mod tooltip;
pub(crate) mod tree;

pub use alert::{Alert, Dismissal};
pub use badge::Badge;
pub use banner::{Banner, BannerDismissal};
pub use button::Button;
pub use checkbox::Checkbox;
pub(crate) use error_summary::{ErrorSummary, SummaryItem};
pub use icon::Icon;
pub use link::Link;
pub use modal_dialog::{ModalDialog, TitleLevel};
pub(crate) use pause_updates::PauseUpdates;
pub use progress::Progress;
pub use radio_group::{LabelLanguage, RadioGroup};
pub use review_dialog::{Confirmation, ReviewDialog, Working};
pub use skeleton::Skeleton;
pub use spinner::Spinner;
pub use status_badge::StatusBadge;
pub use step_up_field::{StepUp, StepUpField};
pub use table::{Align, Column, Footer, Name, Sentences, Sort, SortKey, State, Table};
pub use tabs::{Tab, Tabs};
pub use text_field::TextField;
pub use toast::{Toast, ToastTone, Toasts};
pub use tooltip::{Tip, Tooltip};
pub(crate) use tree::{FolderLoader, FolderPage, Tree};

/// Wraps Tab focus from the dialog's last focusable back to its first (and
/// Shift-Tab from the first to the last), keeping keyboard focus inside the
/// open dialog however the Tab key was pressed. The wrap decision is
/// [`crate::focus::focus_wrap`], tested there; this is the DOM glue.
pub(crate) fn trap_focus(dialog: &NodeRef<leptos::html::Dialog>, event: &web_sys::KeyboardEvent) {
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
    let Some((active, first, last)) = active.zip(first).zip(last).map(|((a, f), l)| (a, f, l))
    else {
        return;
    };
    let single = first.is_same_node(Some(&last));
    let on_last = active.is_same_node(Some(&last));
    let on_first = active.is_same_node(Some(&first));
    match focus_wrap(
        event.key() == "Tab",
        event.shift_key(),
        single,
        on_last,
        on_first,
    ) {
        Some(FocusWrap::First) => {
            event.prevent_default();
            if let Some(el) = first.dyn_ref::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        }
        Some(FocusWrap::Last) => {
            event.prevent_default();
            if let Some(el) = last.dyn_ref::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        }
        None => {}
    }
}

/// Writes `copied_text` to the clipboard, then says in `said`, a polite
/// region of the dialog's own (the announcer waits while a modal is open),
/// whether it is there. A page not served over HTTPS or from this computer
/// has no clipboard, and a browser may refuse it: then it says `refused`,
/// which tells the person to copy it by hand.
pub(crate) fn copy_text(
    copied_text: &str,
    said: RwSignal<Option<String>>,
    copied: String,
    refused: String,
) {
    said.set(None);
    write_clipboard(copied_text, move |done| {
        said.try_set(Some(if done {
            copied.clone()
        } else {
            refused.clone()
        }));
    });
}

/// Writes `text` to the clipboard, then calls `then` once with whether it is
/// there, for a button that says so in a region of its own choosing. A page
/// not served over HTTPS or from this computer has no clipboard, and a
/// browser may refuse it: then `then` hears `false`, at once when there is no
/// clipboard, otherwise once the browser has answered.
pub(crate) fn write_clipboard(text: &str, then: impl Fn(bool) + 'static) {
    let clipboard = web_sys::window()
        .and_then(|window| {
            js_sys::Reflect::get(window.navigator().as_ref(), &"clipboard".into()).ok()
        })
        .and_then(|clipboard| clipboard.dyn_into::<web_sys::Clipboard>().ok());
    let Some(clipboard) = clipboard else {
        then(false);
        return;
    };
    let then = Rc::new(then);
    let on_done = Rc::clone(&then);
    let done = Closure::once(move |_: JsValue| on_done(true));
    let failed = Closure::once(move |_: JsValue| then(false));
    let _ = clipboard.write_text(text).then2(&done, &failed);
    // The promise calls one of them once, after this returns; each is a few
    // bytes, left to the page rather than dropped before it is called.
    done.forget();
    failed.forget();
}

/// A sentence, with the engine's own words in it marked as English.
pub(crate) fn sentence_view(sentence: Sentence) -> impl IntoView {
    let Sentence { before, detail } = sentence;
    let detail = detail.map(|(detail, after)| {
        view! {
            <span lang="en">{detail}</span>
            {after}
        }
    });
    view! {
        {before}
        {detail}
    }
}

/// A refusal shown on a page (§4.5, #929): an Error [`Alert`] holding its
/// sentence, the sender's words marked as English (3.1.2). The alert is no
/// live region: the page says the refusal with [`say_refusals`], or, inside
/// a dialog, draws it with [`dialog_refusal_alert`].
pub(crate) fn refusal_alert(
    localizer: &Arc<Localizer>,
    sentence: Sentence,
) -> impl IntoView + use<> {
    view! {
        <Alert localizer=Arc::clone(localizer) tone=AlertTone::Error>
            {sentence_view(sentence)}
        </Alert>
    }
}

/// A refusal shown inside a modal dialog: its [`refusal_alert`] inside a
/// `role="alert"` of the dialog's own (the Dialog contract, §4.5). The
/// modal makes the announcer's regions inert, so the dialog says it itself,
/// and the page doesn't say it again.
pub(crate) fn dialog_refusal_alert(
    localizer: &Arc<Localizer>,
    sentence: Sentence,
) -> impl IntoView + use<> {
    view! { <div role="alert">{refusal_alert(localizer, sentence)}</div> }
}

/// Says each refusal `refused` comes to hold through the announcer's
/// assertive region, as the page's answer (4.1.3), since its
/// [`refusal_alert`] is no live region. Each is said once: a page that reads
/// again and is refused in the same words keeps its alert and says nothing
/// new, and one cleared, then refused again, is said again. `refused` may be
/// derived, as a panel's refusal put into words is. Called once, where the
/// page makes the signal; it stops with the page.
pub(crate) fn say_refusals(refused: impl Into<Signal<Option<Sentence>>>) {
    let refused = refused.into();
    let shown = Memo::new(move |_| refused.get());
    Effect::new(move |_| {
        if let Some(sentence) = shown.get() {
            reply(Politeness::Assertive, &sentence.plain());
        }
    });
}

/// A sentence in a table's cell, as [`sentence_view`] says it, with the
/// catalog's placeables in it kept whole as [`runs_view`] keeps them.
pub(crate) fn cell_sentence_view(sentence: Sentence) -> impl IntoView {
    let Sentence { before, detail } = sentence;
    let detail = detail.map(|(detail, after)| {
        view! {
            <span lang="en">{detail}</span>
            {runs_view(&after)}
        }
    });
    view! {
        {runs_view(&before)}
        {detail}
    }
}

/// A catalog's text in a table's cell, each placeable in it (a name, a path,
/// an address) a `placeable` of its own: the cell moves one whole to the next
/// line rather than breaking it where a line runs out, and breaks one longer
/// than a line only after a separator in it (#430, #482).
pub(crate) fn runs_view(text: &str) -> impl IntoView + use<> {
    runs(text)
        .into_iter()
        .map(|run| match run {
            Run::Words(words) => words.to_owned().into_any(),
            Run::Placeable(placeable) => {
                view! { <span class="placeable">{address_view(placeable)}</span> }.into_any()
            }
        })
        .collect_view()
}

/// An address, or any text that splits like one, which may break only
/// between its parts ([`address_parts`]) where a line has no room for it
/// whole (#482).
pub(crate) fn address_view(address: &str) -> impl IntoView + use<> {
    address_parts(address)
        .into_iter()
        .enumerate()
        .map(|(at, part)| {
            view! {
                {(at > 0).then(|| view! { <wbr /> })}
                {part.to_owned()}
            }
        })
        .collect_view()
}

/// Moves focus to the element with `id`, when the page has one that takes it.
/// Whether focus is on it now.
pub(crate) fn focus_element(id: &str) -> bool {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return false;
    };
    let Some(element) = document
        .get_element_by_id(id)
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return false;
    };
    // A failed focus leaves focus where it was.
    let _ = element.focus();
    let node: &web_sys::Node = &element;
    document
        .active_element()
        .is_some_and(|active| active.is_same_node(Some(node)))
}

/// Returns focus to the control `to` that asked for a dialog, or to `fallback`
/// when that control is gone, so that focus never falls to the page.
fn return_focus(to: &str, fallback: Option<&str>) {
    if !focus_element(to)
        && let Some(fallback) = fallback
    {
        focus_element(fallback);
    }
}

/// Moves focus to the element with `id` when focus has fallen to the page
/// itself, as it does when the control that held it leaves the page. Focus
/// anywhere else stays where the reader put it.
pub(crate) fn recover_focus(id: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let body: Option<web_sys::Node> = document.body().map(Into::into);
    let lost = document
        .active_element()
        .is_none_or(|active| active.is_same_node(body.as_ref()));
    if lost {
        focus_element(id);
    }
}

/// Whether `element` holds focus now.
fn holds_focus(element: &web_sys::Element) -> bool {
    let node: &web_sys::Node = element;
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .is_some_and(|active| active.is_same_node(Some(node)))
}

/// Hands focus to the element with the ID `then_focus` when the element
/// `node` refers to goes while it holds focus, as a run's progress bar and
/// its Cancel button do when the run ends, so focus does not fall to the page
/// (WCAG 2.4.3; §16, decision 83). It is a cleanup of the reactive view being
/// built, which runs before that view takes the element off the page, so it
/// still knows whether the element holds focus. A keyed list that removes a
/// whole item takes it off first, and the row's header with it.
pub(crate) fn hand_focus_when_gone<E>(node: NodeRef<E>, then_focus: String)
where
    E: leptos::html::ElementType,
    E::Output: JsCast + Clone + AsRef<web_sys::Element> + 'static,
{
    on_cleanup(move || {
        if node
            .get_untracked()
            .is_some_and(|element| holds_focus(element.as_ref()))
        {
            focus_element(&then_focus);
        }
    });
}
