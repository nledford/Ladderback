//! The admin password's section of Settings (security §10.1, decision 70):
//! with sign-in off it sets the admin password, which waits for a restart to
//! turn sign-in on; with sign-in on it changes it, given the current one,
//! which ends every session, this page's too, so the page asks to sign in
//! with the new one. What the form lacks, the request it sends, which field
//! a refusal is about, the section's words and the steps the page takes on
//! an answer are `admin_password.rs`'s, host-tested; this is the DOM glue,
//! which the `@ui` scenarios of `features/security/admin_password.feature`
//! run in Chromium.
//!
//! The passwords live only in the form's inputs and leave them in the
//! request: a set or changed password clears them, and none is ever shown
//! (I4).

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::admin_password::{
    ADMIN_PASSWORD_PATH, Answer, Field, HEADING_ID, PASSWORD_MIN_CHARS, PasswordForm, Step, body,
    button, error_of, problems_of, said_once_set, steps,
};
use crate::announcer::Politeness;
use crate::components::{ErrorSummary, FormField, SummaryItem};
use crate::live::reply;
use crate::routes::{refused_sentence, text_with};
use crate::session::Standing;
use crate::sign_in::PasswordChanged;
use crate::text;
use crate::ws::{self, Posted};

/// What a person has typed, one signal per field.
#[derive(Clone, Copy)]
struct Inputs {
    current: RwSignal<String>,
    new: RwSignal<String>,
    again: RwSignal<String>,
}

impl Inputs {
    fn new() -> Self {
        Self {
            current: RwSignal::new(String::new()),
            new: RwSignal::new(String::new()),
            again: RwSignal::new(String::new()),
        }
    }

    fn read(self, on: bool) -> PasswordForm {
        PasswordForm::typed(
            on,
            self.current.get_untracked(),
            self.new.get_untracked(),
            self.again.get_untracked(),
        )
    }

    fn clear(self) {
        self.current.set(String::new());
        self.new.set(String::new());
        self.again.set(String::new());
    }
}

/// What is wrong with the form: the summary, and the error each field
/// shows, set together so they never disagree.
#[derive(Clone, Copy)]
struct Problems {
    summary: RwSignal<Vec<SummaryItem>>,
    current: RwSignal<Option<String>>,
    new: RwSignal<Option<String>>,
    again: RwSignal<Option<String>>,
}

impl Problems {
    fn new() -> Self {
        Self {
            summary: RwSignal::new(Vec::new()),
            current: RwSignal::new(None),
            new: RwSignal::new(None),
            again: RwSignal::new(None),
        }
    }

    fn show(self, items: Vec<SummaryItem>) {
        self.current.set(error_of(&items, Field::Current));
        self.new.set(error_of(&items, Field::New));
        self.again.set(error_of(&items, Field::Again));
        self.summary.set(items);
    }
}

/// The admin password's section, offered with sign-in off (`on` false) or
/// in a session (`on` true). With sign-in off, a password set is said in
/// `waiting`, Settings' word that a restart turns sign-in on, and the
/// section's word that none is set gives way to it. With sign-in on, a
/// changed password forgets the page's session, which the daemon has ended,
/// and the sign-in page is shown, saying why, even when the reader has left
/// Settings before the answer came.
#[component]
pub(super) fn AdminPassword(
    localizer: Arc<Localizer>,
    on: bool,
    waiting: RwSignal<Option<MessageId>>,
) -> impl IntoView {
    let standing = use_context::<RwSignal<Standing>>();
    let changed = use_context::<PasswordChanged>();
    let inputs = Inputs::new();
    let problems = Problems::new();
    let busy = RwSignal::new(false);
    let submit_localizer = Arc::clone(&localizer);
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let form = inputs.read(on);
        if let Some(items) = problems_of(&form, |id, args| text_with(&submit_localizer, id, args)) {
            problems.show(items);
            return;
        }
        busy.set(true);
        reply(
            Politeness::Polite,
            &text(&submit_localizer, msg::UI_ADMIN_PASSWORD_SAVING),
        );
        let localizer = Arc::clone(&submit_localizer);
        let url = format!("{}{ADMIN_PASSWORD_PATH}", ws::origin());
        spawn_local(async move {
            let answer = match ws::post(&url, &form.request()).await {
                Some(Posted::Done(_)) => Answer::Done,
                Some(Posted::Refused(code, args, detail)) => Answer::Refused(
                    code,
                    refused_sentence(&localizer, code, &args, detail.as_deref()),
                ),
                None => Answer::Unreachable,
            };
            // The reader may have left the page before the answer came.
            let shown = busy.try_set(false).is_none();
            for step in steps(on, answer, shown, |id| text(&localizer, id)) {
                match step {
                    Step::SignInWithTheNew => {
                        // The daemon ended every session, this page's too.
                        ws::forget_session();
                        if let Some(PasswordChanged(changed)) = changed {
                            changed.try_set(true);
                        }
                        if let Some(standing) = standing {
                            standing.try_set(Standing::SignedOut);
                        }
                    }
                    Step::Clear => {
                        problems.show(Vec::new());
                        inputs.clear();
                    }
                    Step::SayWaiting => {
                        let said = said_once_set(ws::session().await.as_ref());
                        if waiting.try_set(Some(said)).is_none() {
                            reply(Politeness::Polite, &text(&localizer, said));
                        }
                    }
                    Step::Show(items) => problems.show(items),
                }
            }
        });
    };

    let heading = text(&localizer, msg::UI_ADMIN_PASSWORD_HEADING);
    let body_localizer = Arc::clone(&localizer);
    let about =
        move || body(on, waiting.get()).map(|id| view! { <p>{text(&body_localizer, id)}</p> });
    let summary_localizer = Arc::clone(&localizer);
    let field_text = |id| text(&localizer, id);
    let current = on.then(|| {
        view! {
            <FormField
                id=Field::Current.id()
                label=field_text(msg::UI_ADMIN_PASSWORD_CURRENT_LABEL)
                description=field_text(msg::UI_ADMIN_PASSWORD_CURRENT_HINT)
                error=problems.current
                value=inputs.current
                kind="password"
                autocomplete="current-password"
            />
        }
    });
    let new_hint = text_with(
        &localizer,
        msg::UI_ADMIN_PASSWORD_NEW_HINT,
        &[("min", MsgArg::Count(PASSWORD_MIN_CHARS))],
    );
    let button_text = field_text(button(on));
    let saving = field_text(msg::UI_ADMIN_PASSWORD_SAVING);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {heading}
            </h2>
            {about}
            {move || {
                let items = problems.summary.get();
                (!items.is_empty())
                    .then(|| {
                        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                        let heading = text_with(
                            &summary_localizer,
                            msg::UI_ADMIN_PASSWORD_PROBLEMS,
                            &[("count", MsgArg::Count(count))],
                        );
                        view! { <ErrorSummary heading=heading items=items advice=None /> }
                    })
            }}
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    submit();
                }
            >
                {current}
                <FormField
                    id=Field::New.id()
                    label=field_text(msg::UI_ADMIN_PASSWORD_NEW_LABEL)
                    description=new_hint
                    error=problems.new
                    value=inputs.new
                    kind="password"
                    autocomplete="new-password"
                />
                <FormField
                    id=Field::Again.id()
                    label=field_text(msg::UI_ADMIN_PASSWORD_AGAIN_LABEL)
                    description=field_text(msg::UI_ADMIN_PASSWORD_AGAIN_HINT)
                    error=problems.again
                    value=inputs.again
                    kind="password"
                    autocomplete="new-password"
                />
                <p class="actions">
                    <button type="submit">{button_text}</button>
                </p>
            </form>
            {move || busy.get().then(|| view! { <p>{saving.clone()}</p> })}
        </section>
    }
}
