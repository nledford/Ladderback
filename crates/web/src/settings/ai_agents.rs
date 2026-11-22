//! The AI agents tab of Settings (security §10.1, decision 127): what AI
//! agents may do without an API token, the `[without-token]` table of
//! `config.toml`, a group per permission as a token's form has, saved with
//! the current admin password while sign-in is on. Which groups there are,
//! what each holds and the request a save sends are `api_tokens.rs`'s,
//! host-tested; this is the DOM glue, which the `@ui` scenarios of
//! `features/automation/ai_agents_settings.feature` run.

use std::sync::Arc;

use api::{ErrorCode, PermissionsDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use super::{Chosen, PermissionGroups};
use crate::announcer::Politeness;
use crate::api_tokens::{Form, WITHOUT_TOKEN_PATH, without_token_request};
use crate::components::ACTIONS;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Button, StepUp, StepUpField, Toast, ToastTone, Toasts, first_load, paced, point_at_refusal,
    refusal_alert, say_refusals,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::refused_sentence;
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Phase, Posted};

/// The `id` of the tab's heading, which names its section and takes focus
/// when its toast is dismissed.
const HEADING_ID: &str = "ai-agents-heading";

/// The `id` of the admin password's field, offered with sign-in on.
const PASSWORD_ID: &str = "ai-agents-password";

/// The `id` of the button that saves the permissions.
const SAVE_ID: &str = "ai-agents-save";

/// The AI agents' section: what its permissions limit, with sign-in off
/// that they limit only `ladderback mcp`, and, once the daemon has read
/// them (`read`, which Settings' tabs follow too), a group per permission
/// and the button that saves them, with the admin password's field before
/// it while sign-in is on (`on`). A read or a save the daemon refuses is an
/// alert; a wrong password is the field's error, which
/// [`point_at_refusal`] takes the reader to, since the field is on the page.
#[component]
pub(super) fn AiAgentsSection(
    localizer: Arc<Localizer>,
    on: bool,
    read: RwSignal<bool>,
) -> impl IntoView {
    let url = StoredValue::new(format!("{}{WITHOUT_TOKEN_PATH}", ws::origin()));
    let phase = RwSignal::new(Phase::Loading);
    let chosen = Chosen::new(&PermissionsDto::default());
    // A read or a save the daemon refused, or a save it didn't answer.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let read_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let answer = ws::get::<PermissionsDto>(&url.get_value()).await;
        // The reader may have left the page before the answer came.
        if phase.try_get_untracked().is_none() {
            return;
        }
        match answer {
            Some(Got::Read(permissions)) => {
                chosen.set(&permissions);
                read.set(true);
                phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                refused.set(Some(refused_sentence(
                    &read_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                phase.set(Phase::Ready);
            }
            None => phase.set(Phase::Unreachable),
        }
    });

    let password = RwSignal::new(String::new());
    let password_error = RwSignal::<Option<String>>::new(None);
    let busy = RwSignal::new(false);
    let toasts = Toasts::new();
    let save_localizer = Arc::clone(&localizer);
    let save = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        refused.set(None);
        reply(
            Politeness::Polite,
            &text(&save_localizer, msg::UI_AI_AGENTS_SAVING),
        );
        let localizer = Arc::clone(&save_localizer);
        let body = without_token_request(chosen.read(), password.get_untracked(), on);
        spawn_local(async move {
            let answer = ws::put(&url.get_value(), &body).await;
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            let kept = match answer {
                Some(Posted::Done(answer)) => serde_json::from_str::<PermissionsDto>(&answer).ok(),
                Some(Posted::Refused(ErrorCode::CurrentPasswordWrong, args, detail)) => {
                    // Cleared first, so a second wrong try is said again.
                    password_error.set(None);
                    tick().await;
                    let words = refused_sentence(
                        &localizer,
                        ErrorCode::CurrentPasswordWrong,
                        &args,
                        detail.as_deref(),
                    )
                    .plain();
                    password_error.try_set(Some(words.clone()));
                    point_at_refusal(PASSWORD_ID, &words);
                    return;
                }
                Some(Posted::Refused(code, args, detail)) => {
                    refused.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    return;
                }
                None => None,
            };
            match kept {
                Some(kept) => {
                    chosen.set(&kept);
                    password.set(String::new());
                    password_error.set(None);
                    toasts.show(
                        ToastTone::Success,
                        text(&localizer, msg::UI_AI_AGENTS_SAVED),
                    );
                }
                // No answer, or not the one the route gives.
                None => refused.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_SESSION_UNREACHABLE),
                    None,
                ))),
            }
        });
    });

    let heading = text(&localizer, msg::UI_AI_AGENTS_HEADING);
    let body = text(&localizer, msg::UI_AI_AGENTS_BODY);
    let sign_in_off = (!on).then(|| text(&localizer, msg::UI_AI_AGENTS_SIGN_IN_OFF));
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let saving = text(&localizer, msg::UI_AI_AGENTS_SAVING);
    let save_label = text(&localizer, msg::UI_AI_AGENTS_SAVE);
    let alert_localizer = Arc::clone(&localizer);
    let form_localizer = Arc::clone(&localizer);
    let (phase, pace) = paced(move || phase.get(), |phase| *phase == Phase::Loading);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            <p>{body}</p>
            {sign_in_off.map(|words| view! { <p>{words}</p> })}
            // Dismissed, a toast hands focus to the heading just before it.
            <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=HEADING_ID />
            {move || refused.get().map(|sentence| refusal_alert(&alert_localizer, sentence))}
            {move || match phase.get() {
                Phase::Loading => first_load(loading.clone(), Awaited::form(7), pace).into_any(),
                Phase::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
                Phase::Ready => {
                    read.get()
                        .then(|| {
                            let localizer = Arc::clone(&form_localizer);
                            let password_field = on
                                .then(|| {
                                    view! {
                                        <StepUpField
                                            id=PASSWORD_ID
                                            localizer=Arc::clone(&localizer)
                                            step_up=StepUp::Password
                                            value=password
                                            error=password_error
                                            on_page=true
                                        />
                                    }
                                });
                            view! {
                                <form
                                    novalidate
                                    on:submit=move |event| {
                                        event.prevent_default();
                                        save.run(());
                                    }
                                >
                                    <PermissionGroups
                                        localizer=Arc::clone(&localizer)
                                        form=Form::WithoutToken
                                        chosen=chosen
                                    />
                                    {password_field}
                                    <p class=ACTIONS>
                                        <Button
                                            content=Content::Text(save_label.clone().into())
                                            appearance=Appearance::Filled
                                            tone=Tone::Primary
                                            kind=Kind::Submit
                                            id=SAVE_ID
                                            busy=busy
                                        />
                                    </p>
                                </form>
                            }
                        })
                        .into_any()
                }
            }}
            {move || busy.get().then(|| view! { <p>{saving.clone()}</p> })}
        </section>
    }
}
