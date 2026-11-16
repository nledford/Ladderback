use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::copy_text;
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
/// (3.3.8). The admin password is a `current-password` field, so a password
/// manager fills it in; a short ID is shown as text that can be selected,
/// with a button that copies it, and the field takes a paste. Nothing here
/// blocks a paste. Its error is an alert, since the field sits in a dialog
/// (the Dialog contract), and marks the input invalid; the caller sets it,
/// and clears it before setting it again so a second wrong try is heard.
/// `value` holds what is typed, so the caller reads and clears it.
#[component]
pub fn StepUpField(
    id: &'static str,
    localizer: Arc<Localizer>,
    step_up: StepUp,
    value: RwSignal<String>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let described_by = format!("{id}-description");
    let error_id = format!("{id}-error");
    let error_in_error = error_id.clone();
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
    let (label, kind, autocomplete, description, copy) = match step_up {
        StepUp::Password => (
            text(&localizer, msg::UI_STEP_UP_PASSWORD_LABEL),
            "password",
            "current-password",
            view! { <p id=described_by>{text(&localizer, msg::UI_STEP_UP_PASSWORD_HINT)}</p> }
                .into_any(),
            None,
        ),
        StepUp::ShortId { label, hint, short } => {
            let said = RwSignal::new(None::<String>);
            let copied = text(&localizer, msg::UI_STEP_UP_COPIED);
            let refused = text(&localizer, msg::UI_STEP_UP_COPY_FAILED);
            let shown = short.clone();
            let copy = view! {
                <p>
                    <button
                        type="button"
                        on:click=move |_| copy_text(&short, said, copied.clone(), refused.clone())
                    >
                        {text(&localizer, msg::UI_STEP_UP_COPY)}
                    </button>
                    " "
                    <span aria-live="polite" aria-atomic="true">
                        {move || said.get()}
                    </span>
                </p>
            };
            (
                label,
                "text",
                "off",
                view! { <p id=described_by>{hint} " " <code class="short-id">{shown}</code></p> }
                    .into_any(),
                Some(copy),
            )
        }
    };
    view! {
        <div class="field">
            <label for=id>{label}</label>
            {description}
            {copy}
            <input
                id=id
                type=kind
                autocomplete=autocomplete
                spellcheck="false"
                autocapitalize="off"
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                aria-describedby=description_ids
                aria-invalid=invalid
            />
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_in_error.clone() class="form-error" role="alert">
                                {text}
                            </p>
                        }
                    })
            }}
        </div>
    }
}
