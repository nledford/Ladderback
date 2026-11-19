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
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::alert::rules::AlertTone;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::{
    Alert, Banner, Button, Checkbox, ErrorSummary, ModalDialog, SummaryItem, TextField, text_field,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::{PageHeading, refusal, text_with};
use crate::session::{
    Answer, SignInOffNotice, Signed, Standing, about_the_password, asks_to_keep, lacking,
};
use crate::variants::Tone;
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

    /// The `id` of the form's Keep me signed in checkbox.
    fn keep(self) -> &'static str {
        match self {
            Self::Page => "sign-in-keep",
            Self::Dialog => "session-ended-keep",
        }
    }
}

/// The admin password's one field, **Keep me signed in on this device**
/// while the daemon offers it (`KeepOffered`), and the button that sends
/// them. The field is a password manager's (`current-password`) and takes
/// paste (A5). What was typed is cleared once the daemon has answered,
/// whatever it answered, so the page never holds the password longer than
/// the request (I4). `signed_in` runs with the new session, or none when
/// the daemon says sign-in is off.
#[component]
fn SignInForm(
    localizer: Arc<Localizer>,
    place: Place,
    signed_in: Callback<Option<Signed>>,
) -> impl IntoView {
    let value = RwSignal::new(String::new());
    let keep = RwSignal::new(false);
    let offered = use_context::<KeepOffered>().map(|KeepOffered(offered)| offered);
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
        let keep_signed_in = asks_to_keep(
            keep.get_untracked(),
            offered.is_some_and(|offered| offered.get_untracked()),
        );
        let localizer = Arc::clone(&submit_localizer);
        spawn_local(async move {
            let answer = ws::sign_in(password, keep_signed_in).await;
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
    let keep_label = text(&localizer, msg::UI_SIGN_IN_KEEP);
    let keep_hint = text(&localizer, msg::UI_SIGN_IN_KEEP_HINT);
    let keep_box = move || {
        offered.is_some_and(|offered| offered.get()).then(|| {
            view! {
                <Checkbox
                    id=place.keep()
                    label=keep_label.clone()
                    description=keep_hint.clone()
                    checked=keep
                />
            }
        })
    };
    let alert_localizer = Arc::clone(&localizer);
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
            <TextField
                id=field
                localizer=Arc::clone(&localizer)
                label=text(&localizer, msg::UI_SIGN_IN_PASSWORD_LABEL)
                description=text(&localizer, msg::UI_SIGN_IN_PASSWORD_HINT)
                error=error
                value=value
                kind=text_field::Kind::Password
                autocomplete="current-password"
                required=true
            />
            {keep_box}
            // The dialog's own `role="alert"`, in the dialog from the start so
            // a screen reader says the error alert the moment it is drawn in
            // it: the modal makes the announcer's regions inert (the Dialog
            // contract, §4.5).
            {(place == Place::Dialog)
                .then(|| {
                    view! {
                        <div role="alert">
                            {move || {
                                alert
                                    .get()
                                    .map(|sentence| {
                                        view! {
                                            <Alert
                                                localizer=Arc::clone(&alert_localizer)
                                                tone=AlertTone::Error
                                            >
                                                {sentence}
                                            </Alert>
                                        }
                                    })
                            }}
                        </div>
                    }
                })}
            <p class="actions">
                <Button
                    content=Content::Text(text(&localizer, msg::UI_SIGN_IN_SUBMIT).into())
                    appearance=Appearance::Filled
                    tone=Tone::Primary
                    kind=Kind::Submit
                    busy=busy
                />
            </p>
        </form>
    }
}

/// Whether the admin password has just been changed in Settings, which
/// ended every session, this page's too (decision 70): the sign-in page then
/// says so, until the page signs in again. `App` provides it.
#[derive(Clone, Copy)]
pub(crate) struct PasswordChanged(pub(crate) RwSignal<bool>);

/// Whether the sign-in form offers **Keep me signed in on this device**: it
/// doesn't under **Stay signed in for** Until I close the browser. Each
/// signed-out answer sets it (`ws::watch_session`); `App` provides it.
#[derive(Clone, Copy)]
pub(crate) struct KeepOffered(pub(crate) RwSignal<bool>);

/// The sign-in page, shown in place of the page asked for while sign-in is
/// on and the page has no session. Once signed in, the page asked for is
/// shown, at the address it was asked at. Shown because the admin password
/// was changed in Settings, it says so under its heading.
#[component]
pub(crate) fn SignInPage(localizer: Arc<Localizer>, standing: RwSignal<Standing>) -> impl IntoView {
    let changed = use_context::<PasswordChanged>().map(|PasswordChanged(changed)| changed);
    let signed_in = Callback::new(move |signed: Option<Signed>| {
        if let Some(changed) = changed {
            changed.set(false);
        }
        ws::signed_in(signed);
        standing.set(Standing::SignedIn);
    });
    let heading = text(&localizer, msg::UI_SIGN_IN_HEADING);
    // The page's state, not a failure: an information alert under the
    // heading, which the reader meets before the form.
    let said = changed
        .is_some_and(|changed| changed.get_untracked())
        .then(|| {
            let words = text(&localizer, msg::UI_ADMIN_PASSWORD_CHANGED);
            view! { <Alert localizer=Arc::clone(&localizer)>{words}</Alert> }
        });
    view! {
        <PageHeading>{heading}</PageHeading>
        {said}
        <SignInForm localizer=localizer place=Place::Page signed_in=signed_in />
    }
}

/// The dialog that asks to sign in again, shown while `ended` is `true`: a
/// request of the page was refused because its session ended. It opens over
/// the page, which stays as it was, so nothing typed there is lost, and the
/// requests that were refused wait for the sign-in, then go again. Escape
/// closes it, as any dialog (§4.5): the requests that waited then end with
/// their refusal, as do those still on their way and the page's own reads,
/// and the next request the reader asks for that is refused opens it again
/// (`session::Dismissals`). Focus returns where it was when it opened.
#[component]
pub(crate) fn SessionEnded(localizer: Arc<Localizer>, ended: RwSignal<bool>) -> impl IntoView {
    // Whether the dialog closes because the reader signed in, not because
    // the reader dismissed it.
    let signed = StoredValue::new(false);
    // Whether the form offers Keep me signed in may have changed in Settings
    // since the page last heard: a signed-out answer says, read without the
    // session, which this read must neither use nor bring back.
    spawn_local(ws::hear_keep_offered());
    // Signing in clears `ended`, which closes the dialog.
    let signed_in = Callback::new(move |session: Option<Signed>| {
        signed.set_value(true);
        ws::signed_in(session);
    });
    let closed = Callback::new(move |()| {
        if signed.try_get_value() == Some(false) {
            ws::gave_up();
        }
    });
    let body_text = text(&localizer, msg::UI_SESSION_ENDED_BODY);
    view! {
        <ModalDialog
            open=ended
            id="session-ended"
            title=text(&localizer, msg::UI_SESSION_ENDED_TITLE)
            described_by="session-ended-body"
            on_close=closed
            fallback="main".to_owned()
        >
            <p id="session-ended-body">{body_text}</p>
            <SignInForm localizer=Arc::clone(&localizer) place=Place::Dialog signed_in=signed_in />
        </ModalDialog>
    }
}

/// The warning shown two minutes before the session would end unused
/// (WCAG 2.2.1), while `warning` is `true`: the page's lease decides when
/// (`session.rs`). It is a [`ModalDialog`], named by its title and described
/// by what it says, so a screen reader reads both as focus moves in, onto
/// **Stay signed in**. That button and Escape both keep the session: either
/// says someone is there. Asking the daemon where the page stands is the use
/// that keeps it, and the warning closes once the answer is in, with focus
/// back where it was. Left alone, the warning gives way to the dialog asking
/// to sign in again when the session ends.
#[component]
pub(crate) fn IdleWarning(localizer: Arc<Localizer>, warning: RwSignal<bool>) -> impl IntoView {
    let stay = NodeRef::<leptos::html::Button>::new();
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
    // A warning shown anew says nothing of an earlier one's request.
    Effect::new(move |_| {
        if warning.get() {
            unanswered.set(false);
        }
    });
    let unreachable = text(&localizer, msg::UI_SESSION_UNREACHABLE);
    let alert_localizer = Arc::clone(&localizer);
    view! {
        // Escape keeps the session too; the warning closes once the daemon
        // has counted the use.
        <ModalDialog
            open=warning
            id="idle-warning"
            title=text(&localizer, msg::UI_IDLE_WARNING_TITLE)
            described_by="idle-warning-body"
            starts_on=stay
            on_cancel=Callback::new(move |()| keep())
            fallback="main".to_owned()
        >
            <p id="idle-warning-body">{text(&localizer, msg::UI_IDLE_WARNING_BODY)}</p>
            // The dialog's own `role="alert"`, there from the start, as the
            // sign-in dialog's is.
            <div role="alert">
                {move || {
                    unanswered
                        .get()
                        .then(|| {
                            let words = unreachable.clone();
                            view! {
                                <Alert localizer=Arc::clone(&alert_localizer) tone=AlertTone::Error>
                                    {words}
                                </Alert>
                            }
                        })
                }}
            </div>
            <p class="actions">
                <Button
                    content=Content::Text(text(&localizer, msg::UI_IDLE_WARNING_STAY).into())
                    appearance=Appearance::Filled
                    tone=Tone::Primary
                    node_ref=stay
                    busy=busy
                    on_press=Callback::new(move |()| keep())
                />
            </p>
        </ModalDialog>
    }
}

/// The header's Sign out control, shown while the page has a session. Once
/// the daemon has ended the session, the sign-in page takes the place of the
/// page, and focus moves to its heading, as on a route change (§4.5). A
/// sign-out that reaches no daemon sets `failed`, which [`SignOutFailure`]
/// shows, and is said assertively, as the press's answer; the page stays
/// signed in.
#[component]
pub(crate) fn SignOut(
    localizer: Arc<Localizer>,
    standing: RwSignal<Standing>,
    failed: RwSignal<bool>,
) -> impl IntoView {
    let busy = RwSignal::new(false);
    let unreachable = text(&localizer, msg::UI_SESSION_UNREACHABLE);
    let sign_out = move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        failed.set(false);
        let unreachable = unreachable.clone();
        spawn_local(async move {
            let answered = ws::sign_out().await;
            busy.try_set(false);
            failed.try_set(!answered);
            if answered {
                standing.set(Standing::SignedOut);
            } else {
                reply(Politeness::Assertive, &unreachable);
            }
        });
    };
    view! {
        <Button
            content=Content::IconAndText {
                glyph: LucideGlyph::LogOut.into(),
                label: text(&localizer, msg::UI_SIGN_OUT).into(),
            }
            appearance=Appearance::Ghost
            busy=busy
            on_press=Callback::new(sign_out)
        />
    }
}

/// Shows, at the top of the page's main content, that a sign-out reached no
/// daemon: an error alert, which [`SignOut`] says assertively, since it is
/// no live region. Not beside Sign out: a line in the header would grow it,
/// and one laid over the page would hide what is under it.
#[component]
pub(crate) fn SignOutFailure(localizer: Arc<Localizer>, failed: RwSignal<bool>) -> impl IntoView {
    let unreachable = text(&localizer, msg::UI_SESSION_UNREACHABLE);
    move || {
        failed.get().then(|| {
            let words = unreachable.clone();
            view! {
                <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Error>
                    {words}
                </Alert>
            }
        })
    }
}

/// The start page's notice that sign-in is off, an information banner:
/// anyone on this machine can use Ladderback, and how to turn sign-in on.
/// Shown only while it is off on loopback; on the network every page has a
/// warning of its own (`App`).
#[component]
pub(crate) fn SignInOff(localizer: Arc<Localizer>) -> impl IntoView {
    let notice = use_context::<RwSignal<SignInOffNotice>>();
    let words = text(&localizer, msg::UI_SIGN_IN_OFF);
    move || {
        (notice.map(|notice| notice.get()) == Some(SignInOffNotice::StartPage)).then(|| {
            let words = words.clone();
            view! { <Banner localizer=Arc::clone(&localizer)>{words}</Banner> }
        })
    }
}
