//! The shared component set (technical design §4.5): native HTML first, each
//! component shown in the gallery with demo state (loading, empty, error,
//! success) and driven by keyboard-only journeys.
//!
//! Native elements carry the accessibility work: `<dialog>` with `showModal`
//! traps focus and returns it on close, table headers `scope` their columns,
//! and `aria-describedby` ties a form field's error to its input. The
//! announcer ([`crate::live`]) speaks what §4.5 says is spoken, and nothing
//! else: a progress bar's value is never announced.

use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use crate::announcer::{Clock, Politeness};
use crate::badge::BadgeState;
use api::{EntryDto, FolderDto, MsgArg};
use l10n::{Direction, Localizer, msg};
use lepticons_data::Glyph;
use leptos::prelude::*;
use leptos::tachys::html::attribute::custom::custom_attribute;
use leptos::task::{spawn_local, tick};
use leptos::wasm_bindgen::closure::Closure;
use leptos::wasm_bindgen::{JsCast, JsValue};

use crate::focus::{FocusWrap, focus_wrap};
use crate::live::{PageClock, close_modal_when_gone, modal_gone, open_modal, pause, reply, resume};
use crate::progress::{
    Clear, Next, Teller, Told, Wake, percent, progress_text, value_text, wait_for,
};
use crate::repositories::{Run, Sentence, address_parts, runs};
use crate::routes::{instant, text_with};
use crate::spinner::{self, LabelShown};
use crate::text;
use crate::toast::{ToastTimers, arrival};
use crate::tree;
use crate::variants::{Placement, Size, Tone};

thread_local! {
    /// The generations of the toasts shown so far, so an older toast's expiry
    /// no longer clears the text a newer toast is showing.
    static TOAST_TIMERS: ToastTimers = ToastTimers::default();
}

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
            <button
                type="button"
                on:click=move |_| {
                    if let Some(dialog) = dialog.get() {
                        dialog.close();
                    }
                }
            >
                {close_label}
            </button>
        </dialog>
    }
}

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
/// as [`ModalDialog`] is, but opened by whoever owns the run: the page sets
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
    let (opener, opener_fallback) = (returns_to.clone(), fallback.clone());
    Effect::new(move |_| {
        let wanted = open.get();
        let Some(dialog) = dialog.get() else {
            return;
        };
        if wanted {
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
    let choice = move |Confirmation { label, action }| {
        view! {
            <button
                type="button"
                aria-disabled=move || busy.get().then_some("true")
                on:click=move |_| {
                    if !busy.get_untracked() {
                        action.run(());
                    }
                }
            >
                {label}
            </button>
        }
    };
    let confirm = confirm.map(choice);
    let also = also.map(choice);
    view! {
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
                modal_gone(counted);
                open.try_set(false);
                return_focus(&returns_to, fallback.as_deref());
            }
            on:keydown=move |ev: web_sys::KeyboardEvent| trap_focus(&dialog, &ev)
        >
            <h3 id=title_id.clone()>{title}</h3>
            {body}
            <p aria-live="polite" aria-atomic="true">
                {move || busy.get().then(|| status.clone())}
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
                        aria-sort=move || { if sorted.get() { "ascending" } else { "descending" } }
                    >
                        <span class="sort-header">
                            {column.clone()}
                            <button
                                type="button"
                                on:click=move |_| {
                                    sorted.set(!sorted.get_untracked());
                                    reply(Politeness::Polite, &message.with_value(String::clone));
                                }
                            >
                                {sort_label}
                            </button>
                        </span>
                    </th>
                </tr>
            </thead>
            <tbody>
                {move || {
                    shown
                        .get()
                        .into_iter()
                        .map(|row| {
                            view! {
                                <tr>
                                    <td>{row}</td>
                                </tr>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </tbody>
        </table>
    }
}

/// A form field: a label, an input, a description and an error, both wired to
/// the input with `aria-describedby`. `id` is the input's (an error summary's
/// link points at it), and `value` holds what is typed, so the caller reads
/// and clears it. The caller decides when `error` is `Some`; the caller
/// announces it politely (as the gallery's button does) or moves focus to an
/// [`ErrorSummary`]. `kind` is the input's `type`: `password` for a secret,
/// which the browser then never shows. `autocomplete` is the input's token
/// (1.3.5): `off` unless the field has a known purpose, such as
/// `current-password` for the admin password, which a password manager then
/// fills in (3.3.8).
///
/// `lines` makes it a multi-line field, a native `<textarea>` that many rows
/// tall, for a list a person types an entry a line, such as a plan's folders:
/// Enter starts a line there, and never submits the form. Its label,
/// description and error are wired as a one-line field's are, and its `kind`
/// does not apply. Focus brings the whole of it into view (2.4.11): a browser
/// scrolls only far enough to show a textarea's caret, its first line, so a
/// field that starts near the viewport's bottom edge would stay mostly below
/// it.
#[component]
pub fn FormField(
    id: &'static str,
    label: String,
    description: String,
    error: RwSignal<Option<String>>,
    value: RwSignal<String>,
    #[prop(default = "text")] kind: &'static str,
    #[prop(default = "off")] autocomplete: &'static str,
    #[prop(optional)] lines: Option<u32>,
) -> impl IntoView {
    let described_by = format!("{id}-description");
    let error_id = format!("{id}-error");
    let error_in_error = error_id.clone();
    // What describes the input: its description, and its error while it
    // shows one.
    let description_ids = {
        let described_by = described_by.clone();
        move || {
            if error.get().is_some() {
                format!("{described_by} {error_id}")
            } else {
                described_by.clone()
            }
        }
    };
    let invalid = move || error.get().is_some().then_some("true");
    let input = match lines {
        Some(rows) => view! {
            <textarea
                id=id
                rows=rows
                autocomplete=autocomplete
                spellcheck="false"
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                on:focus=move |event| {
                    let options = web_sys::ScrollIntoViewOptions::new();
                    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    event_target::<web_sys::Element>(&event)
                        .scroll_into_view_with_scroll_into_view_options(&options);
                }
                aria-describedby=description_ids
                aria-invalid=invalid
            ></textarea>
        }
        .into_any(),
        None => view! {
            <input
                id=id
                type=kind
                autocomplete=autocomplete
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                aria-describedby=description_ids
                aria-invalid=invalid
            />
        }
        .into_any(),
    };
    view! {
        <div class="field">
            <label for=id>{label}</label>
            {input}
            <p id=described_by>{description}</p>
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_in_error.clone() class="form-error">
                                {text}
                            </p>
                        }
                    })
            }}
        </div>
    }
}

/// A checkbox: a native `<input type="checkbox">` with its visible label
/// beside it, which is also its click target, and an optional description
/// wired to it with `aria-describedby`, read after its name, as a
/// [`FormField`]'s is. `checked` holds the choice, so the caller reads and
/// resets it. Space turns it on and off, and the browser tells a screen
/// reader its state, so nothing here announces it. One size and no color of
/// its own: its meaning is in its words (1.4.1). Off and on only: no
/// disabled, indeterminate, error or group state until a page needs one.
#[component]
pub fn Checkbox(
    id: &'static str,
    label: String,
    #[prop(optional)] description: Option<String>,
    checked: RwSignal<bool>,
) -> impl IntoView {
    let described_by = description.as_ref().map(|_| format!("{id}-description"));
    let description = description.map(|text| {
        view! { <p id=described_by.clone()>{text}</p> }
    });
    view! {
        <div class="field checkbox">
            <input
                id=id
                type="checkbox"
                prop:checked=move || checked.get()
                on:change=move |event| checked.set(event_target_checked(&event))
                aria-describedby=described_by.clone()
            />
            <label for=id>{label}</label>
            {description}
        </div>
    }
}

/// A radio group: one choice of a few, named by the `<legend>` of its
/// `<fieldset>` and described by an optional hint under it, wired to the
/// group with `aria-describedby` so a screen reader reads it on entering the
/// group. Each choice is a native `<input type="radio">` sharing `name`, with
/// its visible label beside it as its click target, laid out as a
/// [`Checkbox`] is. The browser makes the group one Tab stop, on the choice
/// made, and the arrow keys move and choose within it, so nothing here
/// handles keys. `options` pairs each choice's value, which also makes its
/// id (`{name}-{value}`), with its label; `chosen` holds the value chosen,
/// so the caller reads it and saves it when it likes: arrowing through the
/// choices sets it at each step. One size and no color of its own (1.4.1).
#[component]
pub fn RadioGroup(
    name: &'static str,
    legend: String,
    #[prop(optional)] description: Option<String>,
    options: Vec<(&'static str, String)>,
    chosen: RwSignal<&'static str>,
) -> impl IntoView {
    let described_by = description.as_ref().map(|_| format!("{name}-description"));
    let description = description.map(|text| {
        view! { <p id=described_by.clone()>{text}</p> }
    });
    let choices = options
        .into_iter()
        .map(|(value, label)| {
            let id = format!("{name}-{value}");
            view! {
                <div class="field checkbox">
                    <input
                        id=id.clone()
                        type="radio"
                        name=name
                        value=value
                        prop:checked=move || chosen.get() == value
                        on:change=move |event| {
                            if event_target_checked(&event) {
                                chosen.set(value);
                            }
                        }
                    />
                    <label for=id>{label}</label>
                </div>
            }
        })
        .collect_view();
    view! {
        <fieldset class="choices" aria-describedby=described_by.clone()>
            <legend>{legend}</legend>
            {description}
            {choices}
        </fieldset>
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
    let clipboard = web_sys::window()
        .and_then(|window| {
            js_sys::Reflect::get(window.navigator().as_ref(), &"clipboard".into()).ok()
        })
        .and_then(|clipboard| clipboard.dyn_into::<web_sys::Clipboard>().ok());
    let Some(clipboard) = clipboard else {
        said.set(Some(refused));
        return;
    };
    said.set(None);
    let done = Closure::once(move |_: JsValue| {
        said.try_set(Some(copied));
    });
    let failed = Closure::once(move |_: JsValue| {
        said.try_set(Some(refused));
    });
    let _ = clipboard.write_text(copied_text).then2(&done, &failed);
    // The promise calls one of them once, after this returns; each is a few
    // bytes, left to the page rather than dropped before it is called.
    done.forget();
    failed.forget();
}

/// One problem of an [`ErrorSummary`]: what is wrong, and the field it is
/// about when it is about one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryItem {
    pub(crate) field: Option<&'static str>,
    pub(crate) sentence: Sentence,
}

/// The error summary of a form that failed to submit (§4.5): a heading that
/// takes focus as it appears, so a screen reader reads it and a keyboard
/// reader is at the top of the list, and one entry per problem that links to
/// its field. The engine's own words in a sentence are marked as English
/// (3.1.2). `advice` follows the list when there is any. It is not a live
/// region: focus is what tells the reader.
#[component]
pub(crate) fn ErrorSummary(
    heading: String,
    items: Vec<SummaryItem>,
    advice: Option<String>,
) -> impl IntoView {
    let heading_ref = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading_ref.get() {
            // A failed focus leaves focus where it was.
            let _ = heading.focus();
        }
    });
    view! {
        <div class="error-summary">
            <h2 tabindex="-1" node_ref=heading_ref>
                {heading}
            </h2>
            <ul>
                {items
                    .into_iter()
                    .map(|item| {
                        let SummaryItem { field, sentence } = item;
                        let shown = sentence_view(sentence);
                        match field {
                            Some(field) => {
                                view! {
                                    <li>
                                        // The router takes a click on any same-origin
                                        // link and would only scroll, so the link moves
                                        // focus itself and stops the click.
                                        <a
                                            href=format!("#{field}")
                                            on:click=move |event| {
                                                event.prevent_default();
                                                focus_element(field);
                                            }
                                        >
                                            {shown}
                                        </a>
                                    </li>
                                }
                                    .into_any()
                            }
                            None => view! { <li>{shown}</li> }.into_any(),
                        }
                    })
                    .collect_view()}
            </ul>
            {advice.map(|advice| view! { <p>{advice}</p> })}
        </div>
    }
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

/// A status badge: a short labeled state on a colored pill, one of
/// `badge-loading`, `badge-empty`, `badge-error` or `badge-success`, whose
/// text token on its background token is pair-checked by
/// `scripts/check-contrast.ts`, with the state's own icon before the label
/// ([`BadgeState::glyph`]), so the state never rests on its color (A2). The
/// icon is decoration beside the text: `aria-hidden`, so a screen reader reads
/// the label once, `focusable="false"`, and stroked in `currentColor`, the
/// text's color, so the contrast gate covers it too. The icon never moves, a
/// running badge's either: an operation can run for hours while the page is
/// in use, and nothing here could stop a spinner (WCAG 2.2.2); the label
/// tells that it runs. The state and the label may be signals: the pill and
/// its icon follow them in place, as a live row's do.
#[component]
pub fn StatusBadge(
    #[prop(into)] state: Signal<BadgeState>,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    view! {
        <span class=move || format!("badge badge-{}", state.get().pill())>
            // The glyph's shapes (`lepticons-data`'s own markup, never outside
            // input) inside an `<svg>` of the badge's own: `1em` keeps it
            // text-sized without the stylesheet.
            <svg
                class="badge-icon"
                xmlns="http://www.w3.org/2000/svg"
                width="1em"
                height="1em"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
                focusable="false"
                inner_html=move || state.get().glyph().svg()
            />
            {move || label.get()}
        </span>
    }
}

/// A spinner (§4.5): a drawing that turns while the page waits for something
/// it can't show yet, always with its `label`, which says what it waits for.
/// The label is required, so a spinner without one can't be built; it must
/// also say something, since an empty one leaves a spinner whose label is
/// hidden an image without a name (1.1.1).
///
/// By default the label is shown beside the drawing as ordinary text, which
/// a screen reader reads, and the drawing is `aria-hidden`; where there is no
/// room for it (`shown=LabelShown::Hidden`), the drawing is `role="img"`
/// named by the label. [`crate::spinner`] decides both. Its `tone`, `size`
/// and `placement` come from the shared vocabulary (decision 108): Primary,
/// Small and Inline unless the caller names others. The tone colors the
/// drawing only, the label keeping the text's color; the size is the
/// drawing's length (Small 1em, Medium 1.5em, Large 2em, XLarge 3em); the
/// placement goes on the spinner's line, an element around it, never on the
/// spinner itself; on a line of its own (Start, Center, End) a long label
/// wraps beside the drawing, never under it. The drawing is a faint track
/// and an arc, which turns once in 0.9 s and stands still when the reader
/// asks for less motion, the label carrying the meaning.
///
/// What the caller owes WCAG 2.2 AA, since the spinner does none of it:
///
/// - **Announcing (4.1.3, Status Messages).** The spinner is never a live
///   region (`role="status"`, `aria-live`): the page keeps one of each kind,
///   the announcer's (A6). When a spinner appears because of something the
///   reader did, such as a search or a save, the page says the wait, and its
///   end, through the announcer ([`crate::live::reply`]).
/// - **Moving (2.2.2, Pause, Stop, Hide).** A spinner that starts by
///   itself, not from something the reader did, beside content the reader
///   can use, and may turn for more than 5 s, would need a way to stop or
///   hide it: show a [`Progress`] bar or words there instead, as a running
///   [`StatusBadge`] does. One the reader started, one that is over within
///   5 s, or one that is all the page shows while nothing on it can be used
///   yet needs nothing. Technical design §4.5 has the rule in full.
/// - **Busy.** The region it stands for, such as the table being read, gets
///   `aria-busy="true"` until it is filled.
/// - **A busy button** keeps focus while the spinner stands in it: it takes
///   `aria-disabled="true"`, not `disabled`, which would drop focus to the
///   page (A1), and its handler ignores presses until the work ends, as
///   `aria-disabled` stops none.
#[component]
pub fn Spinner(
    #[prop(into)] label: Signal<String>,
    #[prop(optional)] shown: LabelShown,
    #[prop(default = spinner::default_size())] size: Size,
    #[prop(optional)] tone: Tone,
    #[prop(optional)] placement: Placement,
) -> impl IntoView {
    view! {
        <span class=placement.class()>
            <span class=spinner::spinner_class(tone, size)>
                // A track and a quarter arc of one circle (r 9: 56.55
                // around), the arc starting at the top, stroked in the
                // drawing's color, its tone. The arc turns inside its group,
                // so the drawing's box never does. The attributes give it
                // the text's height without the stylesheet, which sets each
                // size's length.
                <svg
                    class="spinner-graphic"
                    xmlns="http://www.w3.org/2000/svg"
                    width="1em"
                    height="1em"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="3"
                    stroke-linecap="round"
                    focusable="false"
                    role=shown.graphic_role()
                    aria-hidden=shown.graphic_hidden()
                    aria-label=move || shown.graphic_name(&label.get())
                >
                    <circle class="spinner-track" cx="12" cy="12" r="9" />
                    <g class="spinner-turn">
                        <circle
                            class="spinner-arc"
                            cx="12"
                            cy="12"
                            r="9"
                            stroke-dasharray="14.14 42.41"
                            transform="rotate(-90 12 12)"
                        />
                    </g>
                </svg>
                {shown
                    .shows_label()
                    .then(|| view! { <span class="spinner-label">{move || label.get()}</span> })}
            </span>
        </span>
    }
}

/// A progress bar (§4.5; §16, decision 83): `role="progressbar"`, named
/// `label`, and a stop in the tab order, so a keyboard user reaches it and
/// hears its value whenever she wants. What it draws follows every report
/// of `reading`: the step above the track while there is a percentage, the
/// fill, which moves smoothly to it unless the reader asks for less motion,
/// and the percentage, or the step in its place while there is none. What
/// it tells a screen reader, `aria-valuenow` and `aria-valuetext` (the step,
/// the percentage, the amounts and the time left), changes as
/// [`Teller`] decides: at most every 30 s while the step stays the same, at
/// once for a new step. It is never announced: nothing here calls the
/// announcer.
///
/// Everything it draws is inside the bar, whose children are presentational,
/// so its focus ring surrounds what a sighted reader sees and a screen reader
/// hears the value text alone. A reading without a total is indeterminate
/// (WAI-ARIA `progressbar`): no `aria-valuenow` and no track, and the step,
/// or that the operation is working, where the percentage stands; never 0%,
/// which would say that nothing has been done (#422). The one timer that
/// tells a held value is cleared when the bar goes.
///
/// A bar that goes while it holds focus, as one does when its run ends,
/// hands focus to the element with the ID `then_focus` (a row's header), so
/// focus never falls to the page (WCAG 2.4.3).
#[component]
pub fn Progress(
    localizer: Arc<Localizer>,
    label: String,
    #[prop(into)] reading: Signal<crate::backend::Progress>,
    #[prop(optional, into)] then_focus: Option<String>,
) -> impl IntoView {
    let teller = StoredValue::new(Teller::default());
    let wake = StoredValue::new(Wake::<TimeoutHandle>::default());
    let told = RwSignal::new(None::<Told>);
    let bar = NodeRef::<leptos::html::Div>::new();
    on_cleanup(move || {
        wake.try_update_value(Wake::clear);
    });
    if let Some(id) = then_focus {
        hand_focus_when_gone(bar, id);
    }
    // The first reading is told as the bar is built, so it is never without
    // a value; the effect tells what follows, and asks again of the same.
    let first = reading.get_untracked();
    act(
        teller.try_update_value(|teller| teller.report(first, PageClock.now())),
        teller,
        wake,
        told,
    );
    Effect::new(move |_| {
        let progress = reading.get();
        act(
            teller.try_update_value(|teller| teller.report(progress, PageClock.now())),
            teller,
            wake,
            told,
        );
    });
    let percent =
        Memo::new(move |_| reading.with(|progress| percent(progress.current, progress.total)));
    let known = Memo::new(move |_| percent.get().is_some());
    let said = Memo::new(move |_| reading.with(progress_text));
    let value_localizer = Arc::clone(&localizer);
    let step_localizer = Arc::clone(&localizer);
    view! {
        <div
            node_ref=bar
            role="progressbar"
            tabindex="0"
            class="progress"
            aria-label=label
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow=move || {
                told.get().and_then(|told| told.measure).map(|measure| measure.percent.to_string())
            }
            aria-valuetext=move || {
                told.get()
                    .map(|told| value_text(&told, |id, args| text_with(&value_localizer, id, args)))
            }
        >
            {move || {
                said.get()
                    .above
                    .map(|id| {
                        view! { <span class="progress-step">{text(&step_localizer, id)}</span> }
                    })
            }}
            {move || {
                known
                    .get()
                    .then(|| {
                        view! {
                            <div class="progress-track">
                                // A style property, which the page's content security policy
                                // allows (§10.1), not a `style` attribute, which it refuses.
                                <div
                                    class="progress-fill"
                                    style:inline-size=move || {
                                        format!("{}%", percent.get().unwrap_or_default())
                                    }
                                ></div>
                            </div>
                        }
                    })
            }}
            <span>
                {move || match percent.get() {
                    Some(percent) => format!("{percent}%"),
                    None => text(&localizer, said.get().unknown),
                }}
            </span>
        </div>
    }
}

impl Clear for TimeoutHandle {
    fn clear(self) {
        TimeoutHandle::clear(&self);
    }
}

/// Does what a bar's [`Teller`] answered, `None` once the bar has gone:
/// tells the value now, clearing the timer that waited to, or waits on one
/// timer until the hold is over and asks again.
fn act(
    next: Option<Next>,
    teller: StoredValue<Teller>,
    wake: StoredValue<Wake<TimeoutHandle>>,
    told: RwSignal<Option<Told>>,
) {
    match next {
        Some(Next::Tell(value)) => {
            wake.try_update_value(Wake::clear);
            told.set(Some(value));
        }
        Some(Next::WaitUntil(due)) => {
            let ask = move || {
                act(
                    teller.try_update_value(|teller| teller.wake(PageClock.now())),
                    teller,
                    wake,
                    told,
                );
            };
            if let Ok(handle) = set_timeout_with_handle(ask, wait_for(due, PageClock.now())) {
                wake.try_update_value(|wake| wake.set(handle));
            }
        }
        Some(Next::Keep) | None => {}
    }
}

/// A toast: shown as plain text (§4.5 renders toasts through the announcer,
/// so the element itself is not a second live region), announced politely
/// once through the announcer by [`show_toast`], and gone after [`TOAST_MS`]
/// without any announcement of its going.
#[component]
pub fn Toast(text: RwSignal<Option<String>>) -> impl IntoView {
    view! { {move || { text.get().map(|message| view! { <p class="toast">{message}</p> }) }} }
}

/// Shows `message` as a toast for the lifetime [`crate::toast::arrival`]
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

/// The control that pauses a page's live updates (§4.5, "Live updates"; WCAG
/// 2.2.2): a toggle button, `aria-pressed`, whose name `label` stays the same
/// pressed or not, so a screen reader says the state and never a new name.
/// While it is pressed, the banner (the text of `banner`, in the element
/// with the id `banner_id`) shows beside it and describes it
/// (`aria-describedby`): it is not a live region, so it is read with the
/// button and never by itself.
///
/// Pressing it to pause runs `on_pause`, where the page holds what it shows
/// ([`crate::pause::Pausable`]), then says `paused_said` and holds the
/// announcer's events. Pressing it again runs `on_resume`, where the page
/// shows its live state again and words what changed meanwhile, which is
/// said before the events that waited. The button is built once and stays,
/// so focus never moves (2.4.3). Leaving the page ends the pause: nothing is
/// stored, so a reload starts unpaused.
#[component]
pub(crate) fn PauseUpdates(
    label: String,
    paused_said: String,
    #[prop(into)] paused: Signal<bool>,
    #[prop(into)] banner: Signal<String>,
    banner_id: &'static str,
    on_pause: Callback<()>,
    on_resume: Callback<(), String>,
) -> impl IntoView {
    on_cleanup(|| resume(None));
    view! {
        <div class="pause-updates">
            <button
                type="button"
                aria-pressed=move || if paused.get() { "true" } else { "false" }
                aria-describedby=move || paused.get().then_some(banner_id)
                on:click=move |_| {
                    if paused.get_untracked() {
                        resume(Some(on_resume.run(())));
                    } else {
                        on_pause.run(());
                        pause(&paused_said);
                    }
                }
            >
                {label}
            </button>
            {move || {
                paused
                    .get()
                    .then(|| {
                        view! {
                            <p id=banner_id class="pause-banner">
                                {move || banner.get()}
                            </p>
                        }
                    })
            }}
        </div>
    }
}

/// What reads a page of a folder of a snapshot's tree: given the folder's
/// path and the position the page starts at, the page, or the refusal as the
/// page says it.
pub(crate) type FolderLoader = Rc<dyn Fn(String, u64) -> FolderPage>;

/// A page of a folder on its way, or the refusal as the page says it.
pub(crate) type FolderPage = Pin<Box<dyn Future<Output = Result<FolderDto, Sentence>>>>;

/// The `id` of a row of the tree whose rows' ids start with `prefix`.
fn row_id(prefix: &str, row: tree::Row) -> String {
    match row {
        tree::Row::Entry(node) => format!("{prefix}-{node}"),
        tree::Row::More(folder) => format!("{prefix}-more-{folder}"),
    }
}

/// A snapshot's tree (§4.5, "Tree"): a WAI-ARIA tree, one stop for Tab with
/// a roving `tabindex`, each folder read through `loader` as it is first
/// opened, a page at a time. What a key does, which rows show and what an
/// answer changes are `tree.rs`'s, host-tested; this draws the rows, sends
/// what they ask for and moves focus where the tree says. Each row is named
/// by its entry's name and described by what it is: its kind, size and
/// modification time. A refused page is said in an alert under the tree;
/// `empty` is said in place of a tree when the top folder holds nothing.
/// `selected` follows what the tree selects ([`tree::Tree::selection`]), for
/// a pane beside it that acts on that entry.
#[component]
pub(crate) fn Tree(
    localizer: Arc<Localizer>,
    label: String,
    /// The start of each row's `id`, unique on the page.
    id: String,
    loader: FolderLoader,
    empty: String,
    #[prop(optional)] selected: Option<RwSignal<Option<tree::Selection>>>,
) -> impl IntoView {
    let rtl = localizer.direction() == Direction::Rtl;
    let loading = text(&localizer, msg::UI_LOADING);
    let localizer = StoredValue::new(localizer);
    let zone = StoredValue::new(l10n::viewer_zone());
    let prefix = StoredValue::new(id);
    let loader = StoredValue::new_local(loader);
    let mut initial = tree::Tree::new();
    let first = initial.start();
    let state = RwSignal::new(initial);
    if let Some(selected) = selected {
        let selection = Memo::new(move |_| state.with(tree::Tree::selection));
        Effect::new(move |_| selected.set(selection.get()));
    }
    // Puts focus on the row holding the tab stop once it is drawn.
    let focus_stop = move || {
        let Some(stop) = state.with_untracked(tree::Tree::tab_stop) else {
            return;
        };
        let id = row_id(&prefix.get_value(), stop);
        spawn_local(async move {
            tick().await;
            focus_element(&id);
        });
    };
    // Sends a request, and takes its answer if the tree is still there. An
    // answer that moves the tab stop from a row that went (the row that read
    // a page) moves focus with it, when focus was on that row.
    let fetch = move |request: tree::Request| {
        let future = loader.with_value(|load| load(request.path.clone(), request.from));
        spawn_local(async move {
            let answer = future.await;
            let Some(before) = state.try_with_untracked(tree::Tree::tab_stop) else {
                return;
            };
            state.update(|tree| tree.answered(&request, answer));
            let after = state.with_untracked(tree::Tree::tab_stop);
            if let (Some(before), Some(after)) = (before, after)
                && before != after
            {
                let (before, after) = (
                    row_id(&prefix.get_value(), before),
                    row_id(&prefix.get_value(), after),
                );
                tick().await;
                let focus_fell = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.active_element())
                    .is_none_or(|active| active.id() == before || active.tag_name() == "BODY");
                if focus_fell {
                    focus_element(&after);
                }
            }
        });
    };
    if let Some(first) = first {
        fetch(first);
    }
    // Each change walks the tree once: every row reads what it says from
    // `views`, and redraws only when that changes.
    let views = Memo::new(move |_| state.with(tree::Tree::views));
    let has_rows = Memo::new(move |_| views.with(|views| !views.rows.is_empty()));
    let starting = Memo::new(move |_| state.with(tree::Tree::starting));
    let is_empty = Memo::new(move |_| state.with(tree::Tree::is_empty));
    let failure = Memo::new(move |_| state.with(|tree| tree.failure().cloned()));
    let describe = move |kind: tree::State, entry: &EntryDto| {
        let localizer = localizer.get_value();
        let mut args = vec![
            ("kind", MsgArg::Text(kind.kind().to_owned())),
            ("size", MsgArg::Bytes(entry.size.unwrap_or_default())),
        ];
        let id = match &entry.modified {
            Some(raw) => {
                let when = instant(&localizer, raw, zone.get_value().as_ref());
                args.push(("modified", MsgArg::Text(when)));
                msg::UI_TREE_ENTRY_MODIFIED
            }
            None => msg::UI_TREE_ENTRY,
        };
        text_with(&localizer, id, &args)
    };
    // A row's level and its entry never change; what it says besides is its
    // `RowView`.
    let row_view = move |row: tree::Row| {
        let id = row_id(&prefix.get_value(), row);
        let about_id = format!("{id}-about");
        let level = state.with_untracked(|tree| tree.place(row).0);
        let indent = format!("{}rem", 0.25 + 1.5 * f64::from(tree::indent_steps(level)));
        let entry = match row {
            tree::Row::Entry(node) => {
                state.with_untracked(|tree| tree.node(node).map(|node| node.entry.clone()))
            }
            tree::Row::More(_) => None,
        };
        let says = Memo::new(move |_| views.with(|views| views.get(row)).unwrap_or_default());
        let entry_name = entry
            .as_ref()
            .map(|entry| entry.display.clone())
            .unwrap_or_default();
        let name = move || match row {
            tree::Row::Entry(_) => entry_name.clone(),
            tree::Row::More(_) => says.with(|says| {
                text_with(
                    &localizer.get_value(),
                    msg::UI_TREE_MORE,
                    &[
                        ("shown", MsgArg::Count(says.shown)),
                        ("total", MsgArg::Count(says.set_size)),
                    ],
                )
            }),
        };
        let stop = move || says.with(|says| says.stop);
        let described_by = entry.is_some().then(|| about_id.clone());
        view! {
            <li
                // tachys has no typed `aria-level`, and the macro reads any
                // `aria-` name as one, so it is set as a custom attribute,
                // first: after an unbraced value it would parse as a struct.
                {..custom_attribute("aria-level", level.to_string())}
                role="treeitem"
                id=id
                class="tree-row"
                style:padding-inline-start=indent
                aria-posinset=move || says.with(|says| says.position)
                aria-setsize=move || says.with(|says| says.set_size)
                aria-expanded=move || says.with(|says| says.expanded.map(|open| open.to_string()))
                aria-busy=move || says.with(|says| says.busy).then_some("true")
                aria-selected=move || stop().to_string()
                tabindex=move || if stop() { "0" } else { "-1" }
                aria-describedby=described_by
                on:click=move |_| {
                    if let Some(request) = state.try_update(|tree| tree.activate(row)).flatten() {
                        fetch(request);
                    }
                    focus_stop();
                }
            >
                // A name keeps its own direction: one written right to left
                // reads so in any page, and the text beside it can't
                // reorder it.
                <bdi>{name}</bdi>
                {entry
                    .map(|entry| {
                        let kind = Memo::new(move |_| says.with(|says| says.state));
                        // The row is named by its content, which leaves out
                        // what is hidden: the description is heard once,
                        // through `aria-describedby`, and the name is the
                        // visible name (2.5.3, Label in Name).
                        view! {
                            " "
                            <span id=about_id class="text-muted" aria-hidden="true">
                                {move || {
                                    kind.get()
                                        .map(|kind| describe(kind, &entry))
                                        .unwrap_or_default()
                                }}
                            </span>
                        }
                    })}
            </li>
        }
    };
    view! {
        {move || starting.get().then(|| view! { <p>{loading.clone()}</p> })}
        {move || is_empty.get().then(|| view! { <p>{empty.clone()}</p> })}
        {move || {
            has_rows
                .get()
                .then(|| {
                    view! {
                        <ul
                            role="tree"
                            class="tree"
                            aria-label=label.clone()
                            on:keydown=move |event: web_sys::KeyboardEvent| {
                                if event.ctrl_key() || event.alt_key() || event.meta_key() {
                                    return;
                                }
                                let key = event.key();
                                let now = event.time_stamp();
                                let Some(pressed) = state
                                    .try_update(|tree| tree.key(&key, rtl, now)) else {
                                    return;
                                };
                                if pressed.handled {
                                    event.prevent_default();
                                    focus_stop();
                                }
                                if let Some(request) = pressed.request {
                                    fetch(request);
                                }
                            }
                        >
                            <For
                                each=move || views.with(|views| views.rows.clone())
                                key=|row| *row
                                children=move |row| row_view(row)
                            />
                        </ul>
                    }
                })
        }}
        {move || {
            failure.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
        }}
    }
}
