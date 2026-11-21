//! The text field (components §4.5): a native `<input>` or `<textarea>`
//! with its label, description and error, drawn from the shared vocabulary's
//! tokens. [`rules`] decides what each kind asks of the browser, what a
//! screen reader meets and when a password's show button stands beside it
//! and hides it again, host-tested; this module is the glue that draws it.

use std::sync::Arc;

use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::Button;
use super::button::Content;
use crate::text;
use crate::variants::Size;

pub(crate) mod rules;
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests;

pub use rules::{Access, Kind};

/// A text field (§4.5): its label above it, in the Label weight; its
/// description under the label; the note that says, in words, that it is
/// read only or unavailable; its error, while it has one, just above the
/// field; then the field, in a 1 px edge with the button's small corner, as
/// tall as a [`Button`] of its `size` (Small 1.75em, Medium 2em, the
/// default, Large 2.5em, XLarge 3em), so the two line up in a row of
/// actions, and as wide as its line up to 32rem (40rem and in monospace for
/// a `token`'s whole text). The description, the note and the error
/// describe it (`aria-describedby`); its error marks it invalid
/// (`aria-invalid`), edged thicker in the Error tone beside the words.
/// `required` shows "(required)" after the label, which a screen reader
/// skips, and says it once with `aria-required`, so the field's name stays
/// its label. `icon` stands at its start, inside its edge, decoration a
/// screen reader skips.
///
/// `id` is the input's (an error summary's link points at it), a constant
/// or one built for a field of a list, and `value`
/// holds what is typed, so the caller reads and clears it. `autocomplete`
/// is the input's token (1.3.5): `off` unless the field has a known
/// purpose, such as `current-password`, which a password manager then fills
/// in (3.3.8).
///
/// Its `kind` ([`Kind`]) is one line (the default); many lines, a
/// `<textarea>` where Enter starts a line, whose focus brings the whole of
/// it into view (2.4.11: a browser scrolls only far enough to show its
/// caret); a password, with a toggle button beside it, "Show password",
/// pressed while the password is shown, which keeps its value, its caret and
/// focus and never changes `autocomplete`, and which hides it again when the
/// page sets what the field holds, as it clears its form or dialog; or a
/// number, text that asks for the keyboard of digits. `access` ([`Access`])
/// makes it read only (`readonly`, still in the Tab order, its edge dashed)
/// or unavailable (`disabled`, out of the Tab order, as HTML makes it, with
/// no show button beside a password, which would stay there).
///
/// `description_code` ends the description on a value set as data, in
/// monospace, such as the short ID a step-up asks to be typed back; and
/// `children` stand between the description and the field, such as the
/// button that copies that value. `also_described_by` names an element of
/// the caller's, such as a warning under the field, that describes it too
/// while the signal names it, after the field's own parts.
///
/// What the caller owes WCAG 2.2 AA, since the field does none of it:
///
/// - **Announcing an error (4.1.3).** The field is no live region: the page
///   says the error through the announcer, or moves focus to an
///   [`ErrorSummary`](super::ErrorSummary) on a failed submit. In a dialog,
///   `alert` makes the error an alert instead.
/// - **An error that says what is wrong and how to fix it** (3.3.1, 3.3.3),
///   set only when the form is checked, never while typing.
/// - **A description that says why** a field is read only or unavailable,
///   where the note's words alone don't.
#[component]
pub fn TextField(
    #[prop(into)] id: String,
    localizer: Arc<Localizer>,
    label: String,
    #[prop(optional, into)] description: Option<String>,
    #[prop(optional, into)] description_code: Option<String>,
    #[prop(optional, into)] error: Signal<Option<String>>,
    #[prop(optional, into)] also_described_by: Signal<Option<String>>,
    #[prop(optional)] alert: bool,
    value: RwSignal<String>,
    #[prop(optional)] kind: Kind,
    #[prop(default = "off")] autocomplete: &'static str,
    #[prop(default = rules::default_size())] size: Size,
    #[prop(optional)] access: Access,
    #[prop(optional)] required: bool,
    #[prop(optional)] icon: Option<LucideGlyph>,
    #[prop(optional)] token: bool,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    let marker = rules::required_marker(required).map(|marker| {
        view! {
            <span class="text-field-required" aria-hidden="true">
                {text(&localizer, marker)}
            </span>
        }
    });
    let has_description = description.is_some() || description_code.is_some();
    let described_by = {
        let id = id.clone();
        move || {
            also_described_by.with(|also| {
                rules::described_by(
                    &id,
                    has_description,
                    access,
                    error.with(Option::is_some),
                    also.as_deref(),
                )
            })
        }
    };
    let invalid = move || rules::invalid(error.with(Option::is_some));
    let shown = RwSignal::new(false);
    // What the reader last typed: a value the page sets instead, clearing
    // its form once it is sent, hides a shown password again.
    let typed = StoredValue::new(value.get_untracked());
    if kind == Kind::Password {
        Effect::new(move |_| {
            let hides =
                value.with(|value| typed.with_value(|typed| rules::hides_again(typed, value)));
            if hides && shown.get_untracked() {
                shown.set(false);
            }
        });
    }
    let input_ref = NodeRef::<leptos::html::Input>::new();
    let input_class = rules::input_class(kind, icon.is_some());
    let control = match kind.rows() {
        Some(rows) => view! {
            <textarea
                id=id.clone()
                class=input_class
                rows=rows
                autocomplete=autocomplete
                spellcheck=kind.spellcheck()
                readonly=access == Access::ReadOnly
                disabled=access == Access::Disabled
                aria-required=rules::aria_required(required)
                aria-describedby=described_by
                aria-invalid=invalid
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                on:focus=move |event| {
                    let options = web_sys::ScrollIntoViewOptions::new();
                    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    event_target::<web_sys::Element>(&event)
                        .scroll_into_view_with_scroll_into_view_options(&options);
                }
            ></textarea>
        }
        .into_any(),
        None => view! {
            <input
                id=id.clone()
                node_ref=input_ref
                class=input_class
                type=move || kind.input_type(shown.get())
                inputmode=kind.input_mode()
                autocomplete=autocomplete
                spellcheck=kind.spellcheck()
                autocapitalize=kind.autocapitalize()
                readonly=access == Access::ReadOnly
                disabled=access == Access::Disabled
                aria-required=rules::aria_required(required)
                aria-describedby=described_by
                aria-invalid=invalid
                prop:value=move || value.get()
                on:input=move |event| {
                    let entered = event_target_value(&event);
                    if kind == Kind::Password {
                        typed.update_value(|typed| typed.clone_from(&entered));
                    }
                    value.set(entered);
                }
            />
        }
        .into_any(),
    };
    let toggle = rules::shows_toggle(kind, access).then(|| {
        let name = text(&localizer, msg::UI_TEXT_FIELD_SHOW_PASSWORD);
        let show = Callback::new(move |()| {
            let Some(input) = input_ref.get_untracked() else {
                return;
            };
            // The type changes here, at once, so the caret put back below
            // stays: a browser may move it when the type changes.
            let (start, end) = (input.selection_start(), input.selection_end());
            let now = !shown.get_untracked();
            input.set_type(kind.input_type(now));
            if let (Ok(Some(start)), Ok(Some(end))) = (start, end) {
                let _ = input.set_selection_range(start, end);
            }
            shown.set(now);
        });
        view! {
            <Button
                content=Content::IconOnly {
                    glyph: Signal::derive(move || rules::show_glyph(shown.get())),
                    label: name.into(),
                }
                size=size
                pressed=shown
                on_press=show
                attr:aria-describedby=rules::label_id(&id)
            />
        }
    });
    let description = has_description.then(|| {
        let code = description_code.map(|code| {
            view! {
                " "
                <code>{code}</code>
            }
        });
        view! { <p id=rules::description_id(&id)>{description}{code}</p> }
    });
    let note = access.note().map(|note| {
        view! {
            <p id=rules::note_id(&id) class="text-field-note">
                {text(&localizer, note)}
            </p>
        }
    });
    let label_id = rules::label_id(&id);
    let error_id = rules::error_id(&id);
    view! {
        <div class=rules::FIELD>
            <label id=label_id for=id>
                {label}
                {marker}
            </label>
            {description}
            {note}
            {children.map(|children| children())}
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_id.clone() class="form-error" role=rules::error_role(alert)>
                                {text}
                            </p>
                        }
                    })
            }}
            <div class=rules::ROW>
                <span class=rules::box_class(
                    size,
                    icon.is_some(),
                    token,
                )>
                    {icon
                        .map(|glyph| {
                            view! {
                                <span class=rules::ICON>
                                    <super::Icon glyph=glyph />
                                </span>
                            }
                        })} {control}
                </span>
                {toggle}
            </div>
        </div>
    }
    // Erased, so a page holding fields doesn't carry each one's whole view
    // type in its own: without it, the wasm32 build of a page overflows
    // rustc's query depth.
    .into_any()
}
