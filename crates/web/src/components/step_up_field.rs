use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::tick;

use super::button::Content;
use super::text_field::Kind;
use super::{Button, TextField, focus_element, holds_focus, write_clipboard};
use crate::announcer::Politeness;
use crate::live::reply;
use crate::text;

// What the field's error goes through once a try is answered, decided on
// the host.
mod rules;

use rules::Change;

/// What a step-up confirmation asks for (§10.4): the admin password with
/// sign-in on, and with sign-in off the short ID of what is confirmed, or
/// the hub key's fingerprint, shown beside the field, which `label` and
/// `hint` name ("Snapshot ID", "Hub key fingerprint").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepUp {
    Password,
    ShortId {
        label: String,
        hint: String,
        short: String,
    },
    /// A key's fingerprint as every screen shows it (decision 121, "The
    /// fingerprint"): code text in the description, copied with "Copy the
    /// fingerprint".
    Fingerprint {
        label: String,
        hint: String,
        fingerprint: String,
    },
}

/// The step-up field (§4.5, §10.4): the one field a restore, a download or
/// an approval is confirmed with, which is no test of memory or transcription
/// (3.3.8). A [`TextField`]: the admin password is its password kind, a
/// `current-password` field, so a password manager fills it in; a short ID
/// is shown in its description as text that can be selected, with a button
/// that copies it, and the field takes a paste. Nothing here blocks a
/// paste. The field marks the input invalid when `error` holds a sentence;
/// the caller hands each answer to a try to [`show_answer`], which sets it
/// for a wrong confirmation and clears it for any other answer. `value`
/// holds what is typed, so the caller reads and clears it.
///
/// Where the field stands decides how it speaks, since the announcer waits
/// while a modal is open and works everywhere else:
///
/// - **In a dialog** (the default), its error is an alert inside the dialog
///   (the Dialog contract), and a short ID's "Copied." is a polite region
///   beside the button that copies it.
/// - **On a page** (`on_page`), it is no live region: the error is the
///   field's plain error, linked to it, and "Copied." stays on screen as
///   plain text while the announcer says it as a polite reply. A caller that
///   sets the error follows it with [`point_at_refusal`], which takes the
///   reader to the field, whose description reads the error, or says it
///   through the announcer when focus is on the field already.
#[component]
pub fn StepUpField(
    id: &'static str,
    localizer: Arc<Localizer>,
    step_up: StepUp,
    value: RwSignal<String>,
    error: RwSignal<Option<String>>,
    #[prop(optional)] on_page: bool,
) -> impl IntoView {
    let (label, hint, short, copy, copy_failed) = match step_up {
        StepUp::Password => {
            return view! {
                <TextField
                    id=id
                    localizer=Arc::clone(&localizer)
                    label=text(&localizer, msg::UI_STEP_UP_PASSWORD_LABEL)
                    description=text(&localizer, msg::UI_STEP_UP_PASSWORD_HINT)
                    error=error
                    alert=!on_page
                    value=value
                    kind=Kind::Password
                    autocomplete="current-password"
                />
            }
            .into_any();
        }
        StepUp::ShortId { label, hint, short } => (
            label,
            hint,
            short,
            msg::UI_STEP_UP_COPY,
            msg::UI_STEP_UP_COPY_FAILED,
        ),
        StepUp::Fingerprint {
            label,
            hint,
            fingerprint,
        } => (
            label,
            hint,
            fingerprint,
            msg::UI_STEP_UP_COPY_FINGERPRINT,
            msg::UI_STEP_UP_COPY_FINGERPRINT_FAILED,
        ),
    };
    let said = RwSignal::new(None::<String>);
    let copied = text(&localizer, msg::UI_STEP_UP_COPIED);
    let refused = text(&localizer, copy_failed);
    let copy_label = text(&localizer, copy);
    let shown = short.clone();
    view! {
        <TextField
            id=id
            localizer=localizer
            label=label
            description=hint
            description_code=shown
            error=error
            alert=!on_page
            value=value
            kind=Kind::Code
        >
            <p>
                <Button
                    content=Content::Text(copy_label.into())
                    on_press=Callback::new(move |()| {
                        said.set(None);
                        let (copied, refused) = (copied.clone(), refused.clone());
                        write_clipboard(
                            &short,
                            move |done| {
                                let words = if done { &copied } else { &refused };
                                said.try_set(Some(words.clone()));
                                if on_page {
                                    reply(Politeness::Polite, words);
                                }
                            },
                        );
                    })
                />
                " "
                <span
                    aria-live=(!on_page).then_some("polite")
                    aria-atomic=(!on_page).then_some("true")
                >
                    {move || said.get()}
                </span>
            </p>
        </TextField>
    }
    .into_any()
}

/// Shows on the step-up field whose error is `error` what the daemon's
/// answer to a try leaves there (§4.5, "Step-up field"; [`rules::changes`]):
/// `wrong`, the words of a refusal of what was typed in the field, cleared
/// first so a second wrong try is heard again, or, for any other answer, no
/// error, so the field never keeps a wrong confirmation's error beside a
/// refusal for another reason. A caller on a page follows a wrong one with
/// [`point_at_refusal`].
pub(crate) async fn show_answer(error: RwSignal<Option<String>>, wrong: Option<String>) {
    for change in rules::changes(wrong) {
        match change {
            Change::Clear => {
                error.try_set(None);
            }
            Change::Draw => tick().await,
            Change::Show(words) => {
                error.try_set(Some(words));
            }
        }
    }
}

/// Tells the reader that the step-up field `id`, on a page, was refused with
/// `words`, which the caller has set as its error (§4.5, "Step-up field"):
/// focus moves to the field, whose description holds the error, so it is read
/// once, with the field. When focus is on the field already, as when Enter
/// sent the form from it, nothing is read again, so the announcer says the
/// error as an assertive reply. A field in a dialog needs neither: its error
/// is an alert.
pub(crate) fn point_at_refusal(id: &str, words: &str) {
    let focused = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .is_some_and(|field| holds_focus(&field));
    if focused {
        reply(Politeness::Assertive, words);
    } else {
        focus_element(id);
    }
}
