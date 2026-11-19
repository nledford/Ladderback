//! The "Signing in" section of Settings (security §10.1, decision 70):
//! **Stay signed in for**, how long every session lasts after its sign-in,
//! and **Sign out everywhere**, which ends every browser's session after
//! asking. The choices, their words, the request a save sends and the steps
//! an answer calls for are `sign_in_settings.rs`'s, host-tested; this is the
//! DOM glue, which the `@ui` scenarios of
//! `features/security/session_length.feature` run in Chromium.

use std::sync::Arc;

use api::SignInSettingsDto;
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::alert::rules::AlertTone;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::first_load;
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Alert, Button, Confirmation, RadioGroup, ReviewDialog, Working, dialog_refusal_alert,
    refusal_alert, say_refusals,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::refused_sentence;
use crate::session::Standing;
use crate::sign_in_settings::{
    Answer, Change, HEADING_ID, LENGTH_GROUP, SETTINGS_PATH, Step, length_value, options, request,
    steps,
};
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Phase, Posted};

/// The `id` of the Sign out everywhere button, where focus returns from its
/// dialog.
const EVERYWHERE_ID: &str = "sign-out-everywhere";

/// The `id` of what the Sign out everywhere dialog says, which describes it.
const EVERYWHERE_BODY_ID: &str = "sign-out-everywhere-body";

/// The section, offered with sign-in off (`on` false), where it says only
/// that its choices apply once sign-in is on, and in a session (`on` true).
#[component]
pub(super) fn SignInSection(localizer: Arc<Localizer>, on: bool) -> impl IntoView {
    let heading = text(&localizer, msg::UI_SIGNING_IN_HEADING);
    let content = if on {
        view! { <Choices localizer=localizer /> }.into_any()
    } else {
        view! { <p>{text(&localizer, msg::UI_SIGNING_IN_OFF_BODY)}</p> }.into_any()
    };
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            {content}
        </section>
    }
}

/// The section's controls in a session: Stay signed in for, read from the
/// daemon and saved only by its button, so arrowing through the choices
/// never saves a shorter one on the way, which would end sessions; then
/// Sign out everywhere and the dialog that asks before it.
#[component]
fn Choices(localizer: Arc<Localizer>) -> impl IntoView {
    let base = StoredValue::new(ws::origin());
    let phase = RwSignal::new(Phase::Loading);
    // A read the daemon refused, said in the form's place as it said it.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    // The choice made, by its radio button's value; empty until read.
    let chosen = RwSignal::<&'static str>::new("");
    let load_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let url = format!("{}{SETTINGS_PATH}", base.get_value());
        let read = ws::get::<SignInSettingsDto>(&url).await;
        // The reader may have left the page before the answer came.
        if phase.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                chosen.set(length_value(read.session_length));
                phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                refused.set(Some(refused_sentence(
                    &load_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                phase.set(Phase::Ready);
            }
            None => phase.set(Phase::Unreachable),
        }
    });

    // Saving: whether the request is out, what it came to.
    let busy = RwSignal::new(false);
    let saved = RwSignal::new(false);
    let save_error = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(save_error);
    let save_localizer = Arc::clone(&localizer);
    let save = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let Some(request) = request(chosen.get_untracked()) else {
            return;
        };
        busy.set(true);
        saved.set(false);
        save_error.set(None);
        reply(
            Politeness::Polite,
            &text(&save_localizer, msg::UI_SESSION_LENGTH_SAVING),
        );
        let localizer = Arc::clone(&save_localizer);
        let url = format!("{}{SETTINGS_PATH}", base.get_value());
        spawn_local(async move {
            let answer = answer_of(&localizer, ws::put(&url, &request).await);
            // The reader may have left the page before the answer came.
            let shown = busy.try_set(false).is_none();
            for step in steps(Change::Save, answer, shown, |id| text(&localizer, id)) {
                match step {
                    Step::SaySaved => {
                        saved.set(true);
                        reply(
                            Politeness::Polite,
                            &text(&localizer, msg::UI_SESSION_LENGTH_SAVED),
                        );
                    }
                    Step::Show(sentence) => save_error.set(Some(sentence)),
                    Step::SignIn => {}
                }
            }
        });
    });

    // Sign out everywhere: whether its dialog is open, whether the request is
    // out, and a refusal said inside the dialog.
    let standing = use_context::<RwSignal<Standing>>();
    let everywhere_open = RwSignal::new(false);
    let everywhere_busy = RwSignal::new(false);
    let everywhere_error = RwSignal::<Option<Sentence>>::new(None);
    let everywhere_localizer = Arc::clone(&localizer);
    let sign_out_everywhere = Callback::new(move |()| {
        if everywhere_busy.get_untracked() {
            return;
        }
        everywhere_busy.set(true);
        everywhere_error.set(None);
        let localizer = Arc::clone(&everywhere_localizer);
        spawn_local(async move {
            let answer = answer_of(&localizer, ws::sign_out_everywhere().await);
            // The reader may have left the page before the answer came.
            let shown = everywhere_busy.try_set(false).is_none();
            for step in steps(Change::SignOutEverywhere, answer, shown, |id| {
                text(&localizer, id)
            }) {
                match step {
                    // `ws` has forgotten the page's session: the sign-in page
                    // takes its place.
                    Step::SignIn => {
                        if let Some(standing) = standing {
                            standing.try_set(Standing::SignedOut);
                        }
                    }
                    Step::Show(sentence) => {
                        everywhere_error.try_set(Some(sentence));
                    }
                    Step::SaySaved => {}
                }
            }
        });
    });

    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let form_localizer = Arc::clone(&localizer);
    let saving = text(&localizer, msg::UI_SESSION_LENGTH_SAVING);
    let saved_text = text(&localizer, msg::UI_SESSION_LENGTH_SAVED);
    let saved_localizer = Arc::clone(&localizer);
    let save_error_localizer = Arc::clone(&localizer);
    let everywhere_error_localizer = Arc::clone(&localizer);
    let form = move || match phase.get() {
        Phase::Loading => first_load(loading.clone(), Awaited::form(1)).into_any(),
        Phase::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
        Phase::Ready => match refused.get() {
            // A read refused, shown in the form's place.
            Some(sentence) => refusal_alert(&form_localizer, sentence).into_any(),
            None => {
                let options = options(|id| text(&form_localizer, id));
                view! {
                    <form
                        novalidate
                        on:submit=move |event| {
                            event.prevent_default();
                            save.run(());
                        }
                    >
                        <RadioGroup
                            name=LENGTH_GROUP
                            legend=text(&form_localizer, msg::UI_SESSION_LENGTH_LEGEND)
                            description=text(&form_localizer, msg::UI_SESSION_LENGTH_HINT)
                            options=options
                            chosen=chosen
                        />
                        <p class="actions">
                            <Button
                                content=Content::Text(
                                    text(&form_localizer, msg::UI_SESSION_LENGTH_SAVE).into(),
                                )
                                appearance=Appearance::Filled
                                tone=Tone::Primary
                                kind=Kind::Submit
                                busy=busy
                            />
                        </p>
                    </form>
                }
                .into_any()
            }
        },
    };
    let confirm = Confirmation {
        label: text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE_CONFIRM),
        tone: Tone::Error,
        action: sign_out_everywhere,
    };
    let working = Working {
        active: everywhere_busy.into(),
        status: text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE_WORKING),
    };
    view! {
        {form}
        {move || busy.get().then(|| view! { <p>{saving.clone()}</p> })}
        {move || {
            saved
                .get()
                .then(|| {
                    let words = saved_text.clone();
                    view! {
                        <Alert localizer=Arc::clone(&saved_localizer) tone=AlertTone::Success>
                            {words}
                        </Alert>
                    }
                })
        }}
        {move || {
            save_error.get().map(|sentence| refusal_alert(&save_error_localizer, sentence))
        }}
        <p>{text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE_BODY)}</p>
        <p class="actions">
            <Button
                content=Content::Text(text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE).into())
                tone=Tone::Error
                id=EVERYWHERE_ID
                on_press=Callback::new(move |()| {
                    everywhere_error.set(None);
                    everywhere_open.set(true);
                })
            />
        </p>
        <ReviewDialog
            open=everywhere_open
            title=text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE_TITLE)
            dismiss_label=text(&localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(confirm)
            returns_to=EVERYWHERE_ID.to_owned()
            described_by=EVERYWHERE_BODY_ID.to_owned()
            working=working
            fallback=HEADING_ID.to_owned()
        >
            <p id=EVERYWHERE_BODY_ID>
                {text(&localizer, msg::UI_SIGN_OUT_EVERYWHERE_CONFIRM_BODY)}
            </p>
            {move || {
                everywhere_error
                    .get()
                    .map(|sentence| dialog_refusal_alert(&everywhere_error_localizer, sentence))
            }}
        </ReviewDialog>
    }
}

/// The [`Answer`] a change's reply makes, a refusal said in `localizer`'s
/// words.
fn answer_of(localizer: &Localizer, posted: Option<Posted>) -> Answer {
    match posted {
        Some(Posted::Done(_)) => Answer::Done,
        Some(Posted::Refused(code, args, detail)) => {
            Answer::Refused(refused_sentence(localizer, code, &args, detail.as_deref()))
        }
        None => Answer::Unreachable,
    }
}
