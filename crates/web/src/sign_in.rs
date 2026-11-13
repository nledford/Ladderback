//! The sign-in page, the warning shown before a session would end unused,
//! the dialog that asks to sign in again when a session ends while the page
//! is in use, the navigation's Sign out control, and the start page's notice
//! that sign-in is off (security §10.1, components §4.5). This is DOM glue:
//! what the daemon's answers mean, when to warn, and what the form lacks, is
//! decided in `session.rs` on the host; the `@ui` scenarios of
//! `features/security/sign_in.feature` run this in Chromium.

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::{ErrorSummary, FormField, SummaryItem, recover_focus, trap_focus};
use crate::live::{modal_gone, open_modal};
use crate::repositories::Sentence;
use crate::routes::{PageHeading, refusal, text_with};
use crate::session::{Answer, Signed, Standing, about_the_password, lacking};
use crate::{text, ws};

/// Where a sign-in form stands, which decides how it says a problem: the
/// page's form moves focus to an error summary (A6), and the dialog's says
/// it in an alert inside the dialog (the Dialog contract, §4.5).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Page,
    Dialog,
}

impl Place {
    /// The `id` of the form's one field.
    fn field(self) -> &'static str {
        match self {
            Self::Page => "sign-in-password",
            Self::Dialog => "session-ended-password",
        }
    }
}

/// The admin password's one field and the button that sends it. The field is
/// a password manager's (`current-password`) and takes paste (A5). What was
/// typed is cleared once the daemon has answered, whatever it answered, so
/// the page never holds the password longer than the request (I4).
/// `signed_in` runs with the new session, or none when the daemon says
/// sign-in is off.
#[component]
fn SignInForm(
    localizer: Arc<Localizer>,
    place: Place,
    signed_in: Callback<Option<Signed>>,
) -> impl IntoView {
    let value = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let summary = RwSignal::new(Vec::<SummaryItem>::new());
    let alert = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let field = place.field();
    // A problem: under the field when it is about what was typed, and in the
    // summary or the alert.
    let show = move |sentence: String, about_the_field: bool| {
        error.set(about_the_field.then(|| sentence.clone()));
        match place {
            Place::Page => summary.set(vec![SummaryItem {
                field: about_the_field.then_some(field),
                sentence: Sentence::new(&sentence, None),
            }]),
            Place::Dialog => alert.set(Some(sentence)),
        }
    };
    let submit_localizer = Arc::clone(&localizer);
    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        alert.set(None);
        let password = value.get_untracked();
        if let Some(id) = lacking(&password) {
            show(text(&submit_localizer, id), true);
            return;
        }
        busy.set(true);
        let localizer = Arc::clone(&submit_localizer);
        spawn_local(async move {
            let answer = ws::sign_in(password).await;
            // The form may have gone with its page while the daemon checked.
            if value.try_set(String::new()).is_some() {
                return;
            }
            busy.set(false);
            match answer {
                Answer::SignedIn(signed) => signed_in.run(Some(signed)),
                Answer::Off => signed_in.run(None),
                Answer::Refused(code, args) => {
                    show(
                        refusal(&localizer, code, &args, None),
                        about_the_password(code),
                    );
                }
                Answer::Unreadable => show(text(&localizer, msg::UI_UNREACHABLE_BODY), false),
            }
        });
    };
    let summary_localizer = Arc::clone(&localizer);
    view! {
        {move || {
            let items = summary.get();
            (!items.is_empty())
                .then(|| {
                    let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                    let heading = text_with(
                        &summary_localizer,
                        msg::UI_SIGN_IN_PROBLEMS,
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
            <FormField
                id=field
                label=text(&localizer, msg::UI_SIGN_IN_PASSWORD_LABEL)
                description=text(&localizer, msg::UI_SIGN_IN_PASSWORD_HINT)
                error=error
                value=value
                kind="password"
                autocomplete="current-password"
            />
            {(place == Place::Dialog).then(|| view! { <p role="alert">{move || alert.get()}</p> })}
            <p class="actions">
                <button type="submit" aria-disabled=move || busy.get().then_some("true")>
                    {text(&localizer, msg::UI_SIGN_IN_SUBMIT)}
                </button>
            </p>
        </form>
    }
}

/// The sign-in page, shown in place of the page asked for while sign-in is
/// on and the page has no session. Once signed in, the page asked for is
/// shown, at the address it was asked at.
#[component]
pub(crate) fn SignInPage(localizer: Arc<Localizer>, standing: RwSignal<Standing>) -> impl IntoView {
    let signed_in = Callback::new(move |signed: Option<Signed>| {
        ws::signed_in(signed);
        standing.set(Standing::SignedIn);
    });
    let heading = text(&localizer, msg::UI_SIGN_IN_HEADING);
    view! {
        <PageHeading>{heading}</PageHeading>
        <SignInForm localizer=localizer place=Place::Page signed_in=signed_in />
    }
}

/// The dialog that asks to sign in again, shown while `ended` is `true`: a
/// request of the page was refused because its session ended. It opens over
/// the page, which stays as it was, so nothing typed there is lost, and the
/// requests that were refused wait for the sign-in, then go again. Escape
/// closes it, as any dialog (§4.5): the requests that waited then end with
/// their refusal, and the next request refused opens it again. Focus returns
/// where it was when it opened.
#[component]
pub(crate) fn SessionEnded(localizer: Arc<Localizer>, ended: RwSignal<bool>) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    // Whether the announcer counts the dialog as an open modal.
    let counted = RwSignal::new(false);
    on_cleanup(move || modal_gone(counted));
    // Whether the dialog closes because the reader signed in, not because
    // the reader dismissed it.
    let signed = StoredValue::new(false);
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            open_modal(&dialog, counted);
        }
    });
    let signed_in = Callback::new(move |session: Option<Signed>| {
        signed.set_value(true);
        if let Some(dialog) = dialog.get_untracked() {
            dialog.close();
        }
        ws::signed_in(session);
    });
    view! {
        <dialog
            node_ref=dialog
            aria-labelledby="session-ended-title"
            aria-describedby="session-ended-body"
            on:close=move |_| {
                modal_gone(counted);
                if !signed.get_value() {
                    ws::gave_up();
                    ended.try_set(false);
                }
                recover_focus("main");
            }
            on:keydown=move |event: web_sys::KeyboardEvent| trap_focus(&dialog, &event)
        >
            <h2 id="session-ended-title">{text(&localizer, msg::UI_SESSION_ENDED_TITLE)}</h2>
            <p id="session-ended-body">{text(&localizer, msg::UI_SESSION_ENDED_BODY)}</p>
            <SignInForm localizer=Arc::clone(&localizer) place=Place::Dialog signed_in=signed_in />
        </dialog>
    }
}

/// The warning shown two minutes before the session would end unused
/// (WCAG 2.2.1), while `warning` is `true`: the page's lease decides when
/// (`session.rs`). It is a native modal `<dialog>`, named by its title and
/// described by what it says, so a screen reader reads both as focus moves
/// in, onto **Stay signed in**. That button and Escape both keep the session:
/// either says someone is there. Asking the daemon where the page stands is
/// the use that keeps it, and the warning closes once the answer is in, with
/// focus back where it was. Left alone, the warning gives way to the dialog
/// asking to sign in again when the session ends.
#[component]
pub(crate) fn IdleWarning(localizer: Arc<Localizer>, warning: RwSignal<bool>) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let stay = NodeRef::<leptos::html::Button>::new();
    let counted = RwSignal::new(false);
    on_cleanup(move || modal_gone(counted));
    // Whether a request to stay signed in is on its way, and whether the
    // last one found no daemon to answer it.
    let busy = RwSignal::new(false);
    let unanswered = RwSignal::new(false);
    let keep = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        unanswered.set(false);
        spawn_local(async move {
            let answered = ws::stay_signed_in().await;
            busy.try_set(false);
            unanswered.try_set(!answered);
        });
    };
    Effect::new(move |_| {
        let wanted = warning.get();
        let Some(element) = dialog.get() else {
            return;
        };
        if wanted {
            unanswered.set(false);
            open_modal(&element, counted);
            if let Some(stay) = stay.get_untracked() {
                let _ = stay.focus();
            }
        } else if element.open() {
            element.close();
        }
    });
    let unreachable = text(&localizer, msg::UI_SESSION_UNREACHABLE);
    view! {
        // On `cancel`, Escape keeps the session too; the warning closes once
        // the daemon has counted the use.
        <dialog
            node_ref=dialog
            aria-labelledby="idle-warning-title"
            aria-describedby="idle-warning-body"
            on:cancel=move |event: web_sys::Event| {
                event.prevent_default();
                keep();
            }
            on:close=move |_| {
                modal_gone(counted);
                recover_focus("main");
            }
            on:keydown=move |event: web_sys::KeyboardEvent| trap_focus(&dialog, &event)
        >
            <h2 id="idle-warning-title">{text(&localizer, msg::UI_IDLE_WARNING_TITLE)}</h2>
            <p id="idle-warning-body">{text(&localizer, msg::UI_IDLE_WARNING_BODY)}</p>
            <p role="alert">{move || unanswered.get().then(|| unreachable.clone())}</p>
            <p class="actions">
                <button
                    type="button"
                    node_ref=stay
                    aria-disabled=move || busy.get().then_some("true")
                    on:click=move |_| keep()
                >
                    {text(&localizer, msg::UI_IDLE_WARNING_STAY)}
                </button>
            </p>
        </dialog>
    }
}

/// The navigation's Sign out control, shown while the page has a session.
/// Once the daemon has ended the session, the sign-in page takes the
/// place of the page, and focus moves to its heading, as on a route change
/// (§4.5). A sign-out that reaches no daemon says so beside the control, and
/// the page stays signed in.
#[component]
pub(crate) fn SignOut(localizer: Arc<Localizer>, standing: RwSignal<Standing>) -> impl IntoView {
    let busy = RwSignal::new(false);
    let unanswered = RwSignal::new(false);
    let unreachable = text(&localizer, msg::UI_SESSION_UNREACHABLE);
    let sign_out = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        unanswered.set(false);
        spawn_local(async move {
            let answered = ws::sign_out().await;
            busy.try_set(false);
            unanswered.try_set(!answered);
            if answered {
                standing.set(Standing::SignedOut);
            }
        });
    };
    view! {
        <button
            type="button"
            class="sign-out"
            aria-disabled=move || busy.get().then_some("true")
            on:click=sign_out
        >
            {text(&localizer, msg::UI_SIGN_OUT)}
        </button>
        <p role="alert">{move || unanswered.get().then(|| unreachable.clone())}</p>
    }
}

/// The start page's notice that sign-in is off: anyone on this machine can
/// use Ladderback, and how to turn sign-in on. Shown only while it is off.
#[component]
pub(crate) fn SignInOff(localizer: Arc<Localizer>) -> impl IntoView {
    let standing = use_context::<RwSignal<Standing>>();
    let notice = text(&localizer, msg::UI_SIGN_IN_OFF);
    move || {
        (standing.map(|standing| standing.get()) == Some(Standing::Off))
            .then(|| view! { <p>{notice.clone()}</p> })
    }
}
