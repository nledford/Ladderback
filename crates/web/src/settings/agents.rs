//! Settings' Agents section, on a hub (§16, decision 121, "The hub's
//! dialog"): the form that makes a pairing token, confirmed by a step-up
//! each time, and the dialog that shows the token once. The token's text is
//! held in one signal only while its dialog shows it: a cancel, a lapse or
//! leaving the dialog clears it from the page (I4). Leaving the dialog, by
//! Close or Escape, cancels a token no agent has used, and the section says
//! so. `pairing.rs` decides for it.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use api::{
    ErrorCode, HUB_PATH, HubDto, MakePairingTokenDto, MsgArg, PAIRING_TOKENS_PATH, PairingTokenDto,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Clock;
use crate::components::alert::rules::AlertTone;
use crate::components::button::Content;
use crate::components::text_field::{Access, Kind};
use crate::components::{
    Alert, Button, Fingerprint, ReviewDialog, StepUp, StepUpField, TextField, focus_element,
    holds_focus, refusal_alert, say_refusals,
};
use crate::live::PageClock;
use crate::pairing::{AddressWarning, Countdown, life, minutes};
use crate::repositories::Sentence;
use crate::routes::{instant, refused_sentence, text_with};
use crate::snapshots::viewer_offset;
use crate::text;
use crate::ws::{self, Got, Posted};

/// The section's heading, where focus goes when the dialog's way back is gone.
const HEADING_ID: &str = "agents-heading";
/// The field of the address agents will use.
const ADDRESS_ID: &str = "pairing-address";
/// The field of the token's life.
const LIFE_ID: &str = "pairing-life";
/// The step-up field, where focus returns once the dialog closes.
const STEP_UP_ID: &str = "pairing-confirmation";
/// What the dialog says first, its description.
const BODY_ID: &str = "pairing-dialog-body";
/// The read-only field that holds the token's text, which a keyboard can
/// select and copy where the page can't.
const TOKEN_ID: &str = "pairing-token-text";
/// The dialog's buttons for a live token.
const COPY_ID: &str = "pairing-copy";
const CANCEL_ID: &str = "pairing-cancel";
/// The button that closes the dialog once its token is canceled or lapsed.
const MAKE_NEW_ID: &str = "pairing-make-new";
/// How often the dialog reads the page's clock to count the token down.
const TICK: Duration = Duration::from_millis(250);
/// The life a token is made with unless another is typed (decision 121).
const DEFAULT_LIFE: &str = "15";

/// The hub's key as the page read it, which `App` provides: `Some` on a
/// daemon serving as a hub, read with sign-in off or in a session.
#[derive(Clone, Copy)]
pub(crate) struct HubKey(pub(crate) RwSignal<Option<HubDto>>);

/// Whether the page has read a hub's key, for Settings to show its Agents
/// tab; it changes when the key is read or a session ends.
pub(crate) fn hub_known() -> impl Fn() -> bool + Copy + 'static {
    let key = use_context::<HubKey>().map(|HubKey(key)| key);
    let known = Memo::new(move |_| key.is_some_and(|key| key.with(Option::is_some)));
    move || known.get()
}

/// The hub's key, or `None` on a daemon that isn't a hub (it answers
/// `route-unknown`), and for any answer but the key.
pub(crate) async fn read_hub() -> Option<HubDto> {
    match ws::get::<HubDto>(&format!("{}{HUB_PATH}", ws::origin())).await? {
        Got::Read(hub) => Some(hub),
        Got::Refused(..) => None,
    }
}

/// What the dialog shows of a token made, its text apart.
#[derive(Clone)]
struct Shown {
    id: String,
    fingerprint: String,
    address: String,
    expires_at: String,
}

/// How a token the dialog showed ended while it was open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ended {
    Canceled,
    Lapsed,
}

/// The signals of a token the dialog shows, so its parts share them.
#[derive(Clone, Copy)]
struct Token {
    shown: RwSignal<Option<Shown>>,
    /// The token's text, only while the dialog shows it live (I4).
    secret: RwSignal<String>,
    ended: RwSignal<Option<Ended>>,
    /// The whole seconds left, counted on the page's monotonic clock.
    left: RwSignal<u64>,
    open: RwSignal<bool>,
}

impl Token {
    fn new() -> Self {
        Self {
            shown: RwSignal::new(None),
            secret: RwSignal::new(String::new()),
            ended: RwSignal::new(None),
            left: RwSignal::new(0),
            open: RwSignal::new(false),
        }
    }

    /// Shows `made` in the dialog and starts counting its life down.
    fn show(self, made: PairingTokenDto) {
        let countdown = Countdown::new(made.seconds_left, PageClock.now());
        let id = made.token_id.clone();
        self.secret.set(made.token);
        self.left.set(made.seconds_left);
        self.ended.set(None);
        self.shown.set(Some(Shown {
            id: made.token_id,
            fingerprint: made.fingerprint,
            address: made.address,
            expires_at: made.expires_at,
        }));
        self.open.set(true);
        self.count_down(id, countdown);
    }

    /// Counts the token `id` down until it lapses, it ends otherwise, or
    /// the dialog shows another.
    fn count_down(self, id: String, countdown: Countdown) {
        spawn_local(async move {
            loop {
                gloo_timers::future::sleep(TICK).await;
                let still = self
                    .shown
                    .try_with_untracked(|shown| shown.as_ref().is_some_and(|shown| shown.id == id));
                if still != Some(true) || self.ended.get_untracked().is_some() {
                    return;
                }
                let left = countdown.left(PageClock.now());
                self.left.set(left);
                if left == 0 {
                    self.end(Ended::Lapsed);
                    return;
                }
            }
        });
    }

    /// Ends the live token: its text and buttons give way to a status and
    /// Make a new token, which takes focus if a button that went had it.
    fn end(self, how: Ended) {
        let had_focus = [COPY_ID, CANCEL_ID].into_iter().any(|id| {
            web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(id))
                .is_some_and(|button| holds_focus(&button))
        });
        self.secret.set(String::new());
        self.ended.set(Some(how));
        if had_focus {
            request_animation_frame(|| {
                focus_element(MAKE_NEW_ID);
            });
        }
    }
}

/// The address a token's path cancels it at.
fn cancel_url(id: &str) -> String {
    format!("{}{PAIRING_TOKENS_PATH}/{id}", ws::origin())
}

/// A request the hub didn't answer, or answered with what isn't a token.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_PAIRING_UNANSWERED), None)
}

/// The Agents section: what pairing is, the form that makes a token, and
/// the dialog that shows it. `on` is sign-in: the step-up asks for the
/// admin password then, and for the hub key's ID with sign-in off.
#[component]
pub(crate) fn Agents(localizer: Arc<Localizer>, on: bool) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let key_id = use_context::<HubKey>()
        .and_then(|HubKey(key)| key.get_untracked())
        .map(|key| key.key_id)
        .unwrap_or_default();
    let step_up = if on {
        StepUp::Password
    } else {
        StepUp::ShortId {
            label: t(msg::UI_PAIRING_KEY_ID_LABEL),
            hint: t(msg::UI_PAIRING_KEY_ID_HINT),
            short: key_id,
        }
    };
    let address = RwSignal::new(ws::origin());
    let life_typed = RwSignal::new(DEFAULT_LIFE.to_owned());
    let life_error = RwSignal::new(None::<String>);
    let confirmation = RwSignal::new(String::new());
    let step_up_error = RwSignal::new(None::<String>);
    let problem = RwSignal::new(None::<Sentence>);
    say_refusals(problem);
    let busy = RwSignal::new(false);
    // What the section says once a closed dialog's token is canceled.
    let page_said = RwSignal::new(None::<String>);
    let token = Token::new();

    let make_localizer = Arc::clone(&localizer);
    let make = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        problem.set(None);
        page_said.set(None);
        step_up_error.set(None);
        let Some(life_minutes) = life(&life_typed.get_untracked()) else {
            life_error.set(Some(text(&make_localizer, msg::UI_PAIRING_LIFE_INVALID)));
            focus_element(LIFE_ID);
            return;
        };
        life_error.set(None);
        let ask = MakePairingTokenDto {
            address: address.get_untracked().trim().to_owned(),
            life_minutes: Some(life_minutes),
            confirmation: confirmation.get_untracked(),
        };
        // The step-up is asked for again each time (decision 121).
        confirmation.set(String::new());
        busy.set(true);
        let localizer = Arc::clone(&make_localizer);
        spawn_local(async move {
            let answer = ws::post(&format!("{}{PAIRING_TOKENS_PATH}", ws::origin()), &ask).await;
            if busy.try_get_untracked().is_none() {
                return;
            }
            busy.set(false);
            match answer {
                Some(Posted::Done(body)) => match serde_json::from_str::<PairingTokenDto>(&body) {
                    Ok(made) => token.show(made),
                    Err(_) => problem.set(Some(unanswered(&localizer))),
                },
                Some(Posted::Refused(ErrorCode::StepUpFailed, ..)) => {
                    step_up_error.set(Some(text(&localizer, msg::UI_PAIRING_STEP_UP_FAILED)));
                    focus_element(STEP_UP_ID);
                }
                Some(Posted::Refused(code, args, detail)) => problem.set(Some(refused_sentence(
                    &localizer,
                    code,
                    &args,
                    detail.as_deref(),
                ))),
                None => problem.set(Some(unanswered(&localizer))),
            }
        });
    });

    // Leaving the dialog clears the token from the page, and cancels one
    // still live: no agent can use a token nobody copied (decision 121).
    // A cancel the hub refuses or doesn't answer is said, since the dialog
    // promised it; one the hub no longer holds was used or lapsed already.
    let closed_words = t(msg::UI_PAIRING_CLOSED_CANCELED);
    let not_canceled_words = t(msg::UI_PAIRING_CLOSED_NOT_CANCELED);
    Effect::new(move |was_open: Option<bool>| {
        let now = token.open.get();
        if was_open == Some(true) && !now {
            let live = token
                .shown
                .get_untracked()
                .filter(|_| token.ended.get_untracked().is_none());
            token.secret.set(String::new());
            token.shown.set(None);
            token.ended.set(None);
            if let Some(live) = live {
                let canceled = closed_words.clone();
                let not_canceled = not_canceled_words.clone();
                spawn_local(async move {
                    let said = match ws::delete(&cancel_url(&live.id)).await {
                        Some(Posted::Done(_)) => Some(canceled),
                        Some(Posted::Refused(ErrorCode::PairingTokenUnknown, ..)) => None,
                        Some(Posted::Refused(..)) | None => Some(not_canceled),
                    };
                    page_said.try_set(said);
                });
            }
        }
        now
    });

    let warning_localizer = Arc::clone(&localizer);
    let problem_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {t(msg::UI_AGENTS_HEADING)}
            </h2>
            <p>{t(msg::UI_AGENTS_INTRO)}</p>
            <p role="status">{move || page_said.get()}</p>
            <h3>{t(msg::UI_PAIRING_NEW_HEADING)}</h3>
            {move || problem.get().map(|sentence| refusal_alert(&problem_localizer, sentence))}
            <TextField
                id=ADDRESS_ID
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_PAIRING_ADDRESS_LABEL)
                description=t(msg::UI_PAIRING_ADDRESS_HINT)
                value=address
            />
            {move || {
                AddressWarning::of(&address.get())
                    .map(|warning| {
                        let words = text(
                            &warning_localizer,
                            match warning {
                                AddressWarning::Loopback => msg::UI_PAIRING_WARNING_LOOPBACK,
                                AddressWarning::Plain => msg::UI_PAIRING_WARNING_PLAIN,
                            },
                        );
                        view! {
                            <Alert localizer=Arc::clone(&warning_localizer) tone=AlertTone::Warning>
                                {words}
                            </Alert>
                        }
                    })
            }}
            <p>{t(msg::UI_PAIRING_CERTIFICATE)}</p>
            <TextField
                id=LIFE_ID
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_PAIRING_LIFE_LABEL)
                description=t(msg::UI_PAIRING_LIFE_HINT)
                error=life_error
                value=life_typed
            />
            <StepUpField
                id=STEP_UP_ID
                localizer=Arc::clone(&localizer)
                step_up=step_up
                value=confirmation
                error=step_up_error
            />
            <p class="actions">
                <Button
                    content=Content::Text(t(msg::UI_PAIRING_MAKE).into())
                    busy=busy
                    on_press=make
                />
            </p>
            <PairingDialog localizer=Arc::clone(&localizer) token=token />
        </section>
    }
}

/// The dialog that shows a new pairing token, the only time it is shown:
/// the hub key's fingerprint in full, the address, the certificate and when
/// it lapses, counted down, then the token, which a printout leaves out. It
/// starts on Copy, so a stray Enter copies rather than throwing the token
/// away, and says beside Close that closing cancels it.
#[component]
fn PairingDialog(localizer: Arc<Localizer>, token: Token) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let copy_button = NodeRef::<leptos::html::Button>::new();
    let said = RwSignal::new(None::<String>);
    let canceling = RwSignal::new(false);
    let fingerprint_name = t(msg::UI_PAIRING_FINGERPRINT);
    let copied = t(msg::UI_PAIRING_COPIED);
    let copy_refused = t(msg::UI_PAIRING_COPY_FAILED);
    let copy = Callback::new(move |()| {
        let secret = token.secret.get_untracked();
        if !secret.is_empty() {
            crate::components::copy_text(&secret, said, copied.clone(), copy_refused.clone());
        }
    });
    let cancel_localizer = Arc::clone(&localizer);
    let cancel = Callback::new(move |()| {
        let Some(id) = token
            .shown
            .with_untracked(|shown| shown.as_ref().map(|shown| shown.id.clone()))
        else {
            return;
        };
        if canceling.get_untracked() || token.ended.get_untracked().is_some() {
            return;
        }
        canceling.set(true);
        let localizer = Arc::clone(&cancel_localizer);
        spawn_local(async move {
            let answer = ws::delete(&cancel_url(&id)).await;
            if canceling.try_get_untracked().is_none() {
                return;
            }
            canceling.set(false);
            match answer {
                Some(Posted::Done(_)) => token.end(Ended::Canceled),
                Some(Posted::Refused(code, args, detail)) => {
                    said.set(Some(
                        refused_sentence(&localizer, code, &args, detail.as_deref()).plain(),
                    ));
                }
                None => said.set(Some(text(&localizer, msg::UI_PAIRING_UNANSWERED))),
            }
        });
    });
    let shown_localizer = Arc::clone(&localizer);
    let status_localizer = Arc::clone(&localizer);
    let token_t = t.clone();
    let token_localizer = Arc::clone(&localizer);
    let new_t = t.clone();
    view! {
        <ReviewDialog
            open=token.open
            title=t(msg::UI_PAIRING_DIALOG_TITLE)
            dismiss_label=t(msg::UI_REVIEW_CLOSE)
            confirm=None
            returns_to=STEP_UP_ID.to_owned()
            described_by=BODY_ID.to_owned()
            fallback=HEADING_ID.to_owned()
            starts_on=copy_button
        >
            <p id=BODY_ID>{t(msg::UI_PAIRING_DIALOG_BODY)}</p>
            {move || {
                let localizer = Arc::clone(&shown_localizer);
                let name = fingerprint_name.clone();
                token
                    .shown
                    .get()
                    .map(|shown| {
                        let lapses_localizer = Arc::clone(&localizer);
                        let expires_at = shown.expires_at.clone();
                        view! {
                            <dl>
                                <dt>{name.clone()}</dt>
                                <dd>
                                    <Fingerprint value=shown.fingerprint name=name />
                                </dd>
                                <dt>{text(&localizer, msg::UI_PAIRING_ADDRESS)}</dt>
                                <dd>{shown.address}</dd>
                                <dt>{text(&localizer, msg::UI_PAIRING_CERTIFICATE_LABEL)}</dt>
                                <dd>{text(&localizer, msg::UI_PAIRING_CERTIFICATE_CA)}</dd>
                                <dt>{text(&localizer, msg::UI_PAIRING_LAPSES_LABEL)}</dt>
                                <dd>
                                    {move || lapses(
                                        &lapses_localizer,
                                        &expires_at,
                                        token.left.get(),
                                    )}
                                </dd>
                            </dl>
                        }
                    })
            }}
            {move || {
                (token.ended.get().is_none())
                    .then(|| {
                        view! {
                            <div class="print:hidden">
                                <TextField
                                    id=TOKEN_ID
                                    localizer=Arc::clone(&token_localizer)
                                    label=token_t(msg::UI_PAIRING_TOKEN)
                                    value=token.secret
                                    kind=Kind::Code
                                    access=Access::ReadOnly
                                    token=true
                                />
                            </div>
                            <p class="actions">
                                <Button
                                    id=COPY_ID
                                    content=Content::Text(token_t(msg::UI_PAIRING_COPY).into())
                                    node_ref=copy_button
                                    on_press=copy
                                />
                                <Button
                                    id=CANCEL_ID
                                    content=Content::Text(token_t(msg::UI_PAIRING_CANCEL).into())
                                    busy=canceling
                                    on_press=cancel
                                />
                            </p>
                        }
                    })
            }}
            <p role="status">
                {move || {
                    let ended = token.ended.get()?;
                    Some(
                        match ended {
                            Ended::Canceled => text(&status_localizer, msg::UI_PAIRING_CANCELED),
                            Ended::Lapsed => {
                                let expires_at = token
                                    .shown
                                    .with(|shown| {
                                        shown.as_ref().map(|shown| shown.expires_at.clone())
                                    })?;
                                text_with(
                                    &status_localizer,
                                    msg::UI_PAIRING_LAPSED,
                                    &[
                                        (
                                            "when",
                                            MsgArg::Text(instant(&status_localizer, &expires_at, None)),
                                        ),
                                    ],
                                )
                            }
                        },
                    )
                }}
            </p>
            {move || {
                token
                    .ended
                    .get()
                    .map(|_| {
                        view! {
                            <p class="actions">
                                <Button
                                    id=MAKE_NEW_ID
                                    content=Content::Text(new_t(msg::UI_PAIRING_MAKE_NEW).into())
                                    on_press=Callback::new(move |()| token.open.set(false))
                                />
                            </p>
                        }
                    })
            }}
            <p aria-live="polite" aria-atomic="true">
                {move || said.get()}
            </p>
            <p>{t(msg::UI_PAIRING_CLOSE_NOTE)}</p>
        </ReviewDialog>
    }
}

/// When the token lapses, in this computer's zone with its offset named,
/// and in how many minutes, rounded up.
fn lapses(localizer: &Localizer, expires_at: &str, left: u64) -> String {
    let offset = jiff::Timestamp::from_str(expires_at)
        .map(viewer_offset)
        .map(|offset| api::ClockOffset::new(offset).to_string())
        .unwrap_or_default();
    text_with(
        localizer,
        msg::UI_PAIRING_LAPSES,
        &[
            ("when", MsgArg::Text(instant(localizer, expires_at, None))),
            ("offset", MsgArg::Text(offset)),
            ("minutes", MsgArg::Count(minutes(left))),
        ],
    )
}
