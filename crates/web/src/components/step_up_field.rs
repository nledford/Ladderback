use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::button::Content;
use super::text_field::Kind;
use super::{Button, TextField, copy_text};
use crate::text;

/// What a step-up confirmation asks for (§10.4): the admin password with
/// sign-in on, and with sign-in off the short ID of what is confirmed, shown
/// beside the field, which `label` and `hint` name ("Snapshot ID").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepUp {
    Password,
    ShortId {
        label: String,
        hint: String,
        short: String,
    },
}

/// The step-up field (§4.5, §10.4): the one field a restore, a download or
/// an approval is confirmed with, which is no test of memory or transcription
/// (3.3.8). A [`TextField`]: the admin password is its password kind, a
/// `current-password` field, so a password manager fills it in; a short ID
/// is shown in its description as text that can be selected, with a button
/// that copies it, and the field takes a paste. Nothing here blocks a
/// paste. Its error is an alert, since the field sits in a dialog (the
/// Dialog contract), and marks the input invalid; the caller sets it, and
/// clears it before setting it again so a second wrong try is heard.
/// `value` holds what is typed, so the caller reads and clears it.
#[component]
pub fn StepUpField(
    id: &'static str,
    localizer: Arc<Localizer>,
    step_up: StepUp,
    value: RwSignal<String>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    match step_up {
        StepUp::Password => view! {
            <TextField
                id=id
                localizer=Arc::clone(&localizer)
                label=text(&localizer, msg::UI_STEP_UP_PASSWORD_LABEL)
                description=text(&localizer, msg::UI_STEP_UP_PASSWORD_HINT)
                error=error
                alert=true
                value=value
                kind=Kind::Password
                autocomplete="current-password"
            />
        }
        .into_any(),
        StepUp::ShortId { label, hint, short } => {
            let said = RwSignal::new(None::<String>);
            let copied = text(&localizer, msg::UI_STEP_UP_COPIED);
            let refused = text(&localizer, msg::UI_STEP_UP_COPY_FAILED);
            let copy_label = text(&localizer, msg::UI_STEP_UP_COPY);
            let shown = short.clone();
            view! {
                <TextField
                    id=id
                    localizer=localizer
                    label=label
                    description=hint
                    description_code=shown
                    error=error
                    alert=true
                    value=value
                    kind=Kind::Code
                >
                    <p>
                        <Button
                            content=Content::Text(copy_label.into())
                            on_press=Callback::new(move |()| {
                                copy_text(&short, said, copied.clone(), refused.clone());
                            })
                        />
                        " "
                        <span aria-live="polite" aria-atomic="true">
                            {move || said.get()}
                        </span>
                    </p>
                </TextField>
            }
            .into_any()
        }
    }
}
