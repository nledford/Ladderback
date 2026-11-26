//! Settings' Agents section, on a hub (§16, decision 121, "The hub's
//! dialog"): the form that makes a pairing token, confirmed by a step-up
//! each time, and the dialog that shows the token once. The token's text is
//! held in one signal only while its dialog shows it: a cancel, a lapse or
//! leaving the dialog clears it from the page (I4). Leaving the dialog, by
//! Close or Escape, cancels a token no agent has used, and the section says
//! so. With sign-in off it warns that a hub behind a reverse proxy must turn
//! sign-in on (decision 120). It is DOM glue: `pairing.rs` decides for it.

use std::sync::Arc;
use std::time::Duration;

use api::{
    ErrorCode, HUB_PATH, HubDto, MakePairingTokenDto, PAIRING_TOKENS_PATH, PairingTokenDto,
    TokenCertificateDto,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::machines;
use crate::announcer::{Clock, Politeness};
use crate::components::ACTIONS;
use crate::components::alert::rules::AlertTone;
use crate::components::button::Content;
use crate::components::link::rules::{Destination, LinkPlacement, LinkTone};
use crate::components::text_field::{Access, Kind};
use crate::components::{
    Alert, Button, Fingerprint, Link, RadioGroup, ReviewDialog, StepUp, StepUpField, TextField,
    focus_element, holds_focus, point_at_refusal, refusal_alert, say_refusals,
};
use crate::live::{PageClock, reply};
use crate::pairing::{
    self, AddressWarning, CA, ClosedCancel, Countdown, PINNED, Scheme, asked, lapsed, lapses, life,
};
use crate::repositories::Sentence;
use crate::routes::refused_sentence;
use crate::session::Standing;
use crate::text;
use crate::ws::{self, Posted};

/// The section's heading, where focus goes when the dialog's way back is gone.
const HEADING_ID: &str = "agents-heading";
/// The field of the address agents will use.
const ADDRESS_ID: &str = "pairing-address";
/// The address's warning, which describes its field while it shows.
const WARNING_ID: &str = "pairing-address-warning";
/// The field of the token's life.
const LIFE_ID: &str = "pairing-life";

/// The certificate choice's group; `pairing.rs` holds its two values.
const CERTIFICATE_GROUP: &str = "pairing-certificate";

/// The step-up field, where focus goes when the step-up is refused.
const STEP_UP_ID: &str = "pairing-confirmation";
/// The button that makes a token, which opens the dialog: focus returns to
/// it once the dialog closes (§4.5, "Dialog").
const MAKE_ID: &str = "pairing-make";
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
/// How long the page waits for the hub's key: Settings is drawn without its
/// Machines tab once it has waited this long.
const READ_PATIENCE: Duration = Duration::from_secs(5);

/// Where the guide says a hub reached through a reverse proxy must turn
/// sign-in on: on `main`, since only a build that isn't a release has a hub
/// (§16, decision 122), and its release's tag predates the section.
const PROXY_GUIDE_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/guide/remote-access.md#a-hub-behind-a-reverse-proxy"
);

/// The hub's key as the page read it, which `App` provides, with the
/// standing it was read in: a key on a daemon serving as a hub, read with
/// sign-in off or in a session; `None` until the first read answers.
#[derive(Clone, Copy)]
pub(crate) struct HubKey(pub(crate) RwSignal<Option<(Standing, Option<HubDto>)>>);

/// Whether the daemon is a hub, once the page has read its key in the
/// standing it is in, for Settings to show its Machines tab; `None` until
/// then.
pub(crate) fn hub_known() -> impl Fn() -> Option<bool> + Copy + 'static {
    let key = use_context::<HubKey>().map(|HubKey(key)| key);
    let standing = use_context::<RwSignal<Standing>>();
    let known = Memo::new(move |_| {
        let Some(key) = key else {
            return Some(false);
        };
        let now = standing.map(|standing| standing.get());
        key.with(|read| {
            pairing::is_hub(read.as_ref().map(|(when, key)| (*when, key.is_some())), now)
        })
    });
    move || known.get()
}

/// The hub's key, the page's own read of it, or `None` on a daemon that
/// isn't a hub (it answers `route-unknown`), for any answer but the key,
/// and for none within `READ_PATIENCE`.
pub(crate) async fn read_hub() -> Option<HubDto> {
    ws::poll::<HubDto>(&format!("{}{HUB_PATH}", ws::origin()), READ_PATIENCE).await
}

/// What the dialog shows of a token made, its text apart.
#[derive(Clone)]
struct Shown {
    id: String,
    fingerprint: String,
    address: String,
    expires_at: String,
    /// The certificate the token's address shows: none for a plain
    /// `http://` address.
    certificate: Option<TokenCertificateDto>,
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
    /// The ID of the token Cancel this token is canceling, while its cancel
    /// is on its way: leaving the dialog meanwhile sends no second one, and
    /// this one's answer is what the page says.
    canceling: RwSignal<Option<String>>,
    /// What the section says once a closed dialog's token is canceled.
    page_said: RwSignal<Option<String>>,
    /// What the dialog says of Copy or of Cancel this token, for the token
    /// it shows only.
    said: RwSignal<Option<String>>,
    /// The ID of the last token the dialog showed: the section says its
    /// cancel's answer in full, and of an older token's that comes after
    /// it only that the older token still works.
    last_shown: StoredValue<Option<String>>,
    /// What the section says of an earlier token whose cancel failed once
    /// a later token was shown.
    earlier_said: RwSignal<Option<String>>,
}

impl Token {
    fn new() -> Self {
        Self {
            shown: RwSignal::new(None),
            secret: RwSignal::new(String::new()),
            ended: RwSignal::new(None),
            left: RwSignal::new(0),
            open: RwSignal::new(false),
            canceling: RwSignal::new(None),
            page_said: RwSignal::new(None),
            said: RwSignal::new(None),
            last_shown: StoredValue::new(None),
            earlier_said: RwSignal::new(None),
        }
    }

    /// Says what the cancel of the closed token `id` came to: in full for
    /// the last token the dialog showed; for an earlier one, only that it
    /// still works, so its answer is never read as the later token's. The
    /// section shows it, and the announcer says it (§4.5): the section is
    /// no live region of its own.
    fn say_closed(self, localizer: &Localizer, id: &str, answer: Option<&Posted>) {
        let Some(closed) = pairing::closed_cancel(self.last_shown(id), answer) else {
            return;
        };
        let (said, shown) = match closed {
            ClosedCancel::Last(said) => (said, self.page_said),
            ClosedCancel::Earlier(said) => (Some(said), self.earlier_said),
        };
        let said = said.map(|said| text(localizer, said));
        // A section gone with Settings meanwhile has nobody to tell.
        if shown.try_set(said.clone()).is_some() {
            return;
        }
        if let Some(said) = said {
            reply(Politeness::Polite, &said);
        }
    }

    /// Whether `id` is the last token the dialog showed.
    fn last_shown(self, id: &str) -> bool {
        self.last_shown
            .try_with_value(|last| pairing::same_token(last.as_deref(), Some(id)))
            .unwrap_or(false)
    }

    /// Whether Cancel this token, or leaving the dialog, cancels the token
    /// `id`.
    fn cancelable(self, id: &str) -> bool {
        let ended = self.ended.with_untracked(Option::is_some);
        self.canceling
            .with_untracked(|canceling| pairing::cancelable(ended, canceling.as_deref(), id))
    }

    /// Whether the open dialog shows the token `id`.
    fn shows(self, id: &str) -> bool {
        let open = self.open.get_untracked();
        self.shown.with_untracked(|shown| {
            pairing::shows(open, shown.as_ref().map(|shown| shown.id.as_str()), id)
        })
    }

    /// Shows `made` in the dialog and starts counting its life down.
    fn show(self, made: PairingTokenDto) {
        let countdown = Countdown::new(made.seconds_left, PageClock.now());
        let id = made.token_id.clone();
        self.last_shown.set_value(Some(id.clone()));
        self.secret.set(made.token);
        self.left.set(made.seconds_left);
        self.ended.set(None);
        self.said.set(None);
        self.shown.set(Some(Shown {
            id: made.token_id,
            fingerprint: made.fingerprint,
            address: made.address,
            expires_at: made.expires_at,
            certificate: made.certificate,
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
                // A section gone with Settings meanwhile has nothing to count.
                let counting = self.shown.try_with_untracked(|shown| {
                    pairing::counts_down(
                        shown.as_ref().map(|shown| shown.id.as_str()),
                        &id,
                        self.ended.with_untracked(Option::is_some),
                    )
                });
                if counting != Some(true) {
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
    /// Make a new token, which takes focus if the token's field or a button
    /// that went had it.
    fn end(self, how: Ended) {
        let had_focus = [TOKEN_ID, COPY_ID, CANCEL_ID].into_iter().any(|id| {
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

/// A request the hub didn't answer, or answered with what isn't a token.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_PAIRING_UNANSWERED), None)
}

/// The Agents section: what pairing is, the form that makes a token, and
/// the dialog that shows it. `on` is sign-in: the step-up asks for the
/// admin password then, and for the hub key's ID with sign-in off, which
/// anyone can read. So with sign-in off the section warns that a hub
/// reached through a reverse proxy, which makes a request from anywhere
/// look local, must turn sign-in on (decision 120, "Who may make a token,
/// approve or re-key").
#[component]
pub(crate) fn Agents(localizer: Arc<Localizer>, on: bool) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let hub = use_context::<HubKey>()
        .and_then(|HubKey(key)| key.get_untracked())
        .and_then(|(_, key)| key);
    // The hub's own certificate is offered while it serves one, first when
    // the page came in over it; a certificate authority's otherwise.
    let serves_own = hub
        .as_ref()
        .is_some_and(|hub| hub.own_certificate.is_some());
    let certificate = RwSignal::new(pairing::first_choice(
        hub.as_ref().map(|hub| hub.certificate),
        serves_own,
    ));
    let fingerprint = hub.map(|key| key.fingerprint).unwrap_or_default();
    let (step_up, proxy_warning) = if on {
        (StepUp::Password, None)
    } else {
        let step_up = StepUp::Fingerprint {
            label: t(msg::UI_PAIRING_KEY_FINGERPRINT_LABEL),
            hint: t(msg::UI_PAIRING_KEY_FINGERPRINT_HINT),
            fingerprint,
        };
        let warning = view! { <ProxyWarning localizer=Arc::clone(&localizer) /> };
        (step_up, Some(warning))
    };
    let address = RwSignal::new(ws::origin());
    // A plain address shows no certificate, so it is offered no choice of
    // one (#1343).
    let plain = Memo::new(move |_| Scheme::plain(&address.get()));
    let choice_t = t.clone();
    let life_typed = RwSignal::new(DEFAULT_LIFE.to_owned());
    let life_error = RwSignal::new(None::<String>);
    let confirmation = RwSignal::new(String::new());
    let step_up_error = RwSignal::new(None::<String>);
    let problem = RwSignal::new(None::<Sentence>);
    say_refusals(problem);
    let busy = RwSignal::new(false);
    let token = Token::new();

    let make_localizer = Arc::clone(&localizer);
    let make = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        problem.set(None);
        token.page_said.set(None);
        step_up_error.set(None);
        let Some(life_minutes) = life(&life_typed.get_untracked()) else {
            life_error.set(Some(text(&make_localizer, msg::UI_PAIRING_LIFE_INVALID)));
            focus_element(LIFE_ID);
            return;
        };
        life_error.set(None);
        let (asked_address, asked_certificate) =
            asked(&address.get_untracked(), certificate.get_untracked());
        let ask = MakePairingTokenDto {
            address: asked_address,
            certificate: asked_certificate,
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
                // Settings was left while the token was made: nobody saw it,
                // so no machine can be given it, and it is canceled.
                let made = match &answer {
                    Some(Posted::Done(body)) => serde_json::from_str::<PairingTokenDto>(body).ok(),
                    _ => None,
                };
                if let Some(made) = made {
                    ws::delete(&pairing::cancel_url(&ws::origin(), &made.token_id)).await;
                }
                return;
            }
            busy.set(false);
            match answer {
                Some(Posted::Done(body)) => match serde_json::from_str::<PairingTokenDto>(&body) {
                    Ok(made) => token.show(made),
                    Err(_) => problem.set(Some(unanswered(&localizer))),
                },
                Some(Posted::Refused(ErrorCode::StepUpFailed, ..)) => {
                    let words = text(&localizer, msg::UI_PAIRING_STEP_UP_FAILED);
                    step_up_error.set(Some(words.clone()));
                    point_at_refusal(STEP_UP_ID, &words);
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
    // A cancel Cancel this token sent, still on its way, answers for the
    // token: no second one goes out, which would find none.
    let close_localizer = Arc::clone(&localizer);
    Effect::new(move |was_open: Option<bool>| {
        let now = token.open.get();
        if pairing::left(was_open, now) {
            let live = token
                .shown
                .get_untracked()
                .filter(|live| token.cancelable(&live.id));
            token.secret.set(String::new());
            token.shown.set(None);
            token.ended.set(None);
            token.said.set(None);
            if let Some(live) = live {
                let localizer = Arc::clone(&close_localizer);
                spawn_local(async move {
                    let answer = ws::delete(&pairing::cancel_url(&ws::origin(), &live.id)).await;
                    token.say_closed(&localizer, &live.id, answer.as_ref());
                });
            }
        }
        now
    });

    let warning_localizer = Arc::clone(&localizer);
    let warning = Memo::new(move |_| AddressWarning::of(&address.get()));
    let problem_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID tabindex="-1">
                {t(msg::UI_AGENTS_HEADING)}
            </h2>
            <p>{t(msg::UI_AGENTS_INTRO)}</p>
            {proxy_warning}
            <p>{move || token.page_said.get()}</p>
            <p>{move || token.earlier_said.get()}</p>
            <machines::Machines localizer=Arc::clone(&localizer) step_up=step_up.clone() />
            <h3>{t(msg::UI_PAIRING_NEW_HEADING)}</h3>
            {move || problem.get().map(|sentence| refusal_alert(&problem_localizer, sentence))}
            <TextField
                id=ADDRESS_ID
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_PAIRING_ADDRESS_LABEL)
                description=t(msg::UI_PAIRING_ADDRESS_HINT)
                also_described_by=Signal::derive(move || {
                    warning.get().map(|_| WARNING_ID.to_owned())
                })
                value=address
            />
            {move || {
                warning
                    .get()
                    .map(|warning| {
                        let words = text(
                            &warning_localizer,
                            match warning {
                                AddressWarning::Loopback => msg::UI_PAIRING_WARNING_LOOPBACK,
                                AddressWarning::Plain => msg::UI_PAIRING_WARNING_PLAIN,
                            },
                        );
                        view! {
                            <div id=WARNING_ID>
                                <Alert
                                    localizer=Arc::clone(&warning_localizer)
                                    tone=AlertTone::Warning
                                >
                                    {words}
                                </Alert>
                            </div>
                        }
                    })
            }}
            {move || {
                if plain.get() {
                    let words = if serves_own {
                        msg::UI_PAIRING_CERTIFICATE_NONE
                    } else {
                        msg::UI_PAIRING_CERTIFICATE_NONE_UNSERVED
                    };
                    view! { <p>{choice_t(words)}</p> }.into_any()
                } else if serves_own {
                    view! {
                        <RadioGroup
                            name=CERTIFICATE_GROUP
                            legend=choice_t(msg::UI_PAIRING_CERTIFICATE_LEGEND)
                            description=choice_t(msg::UI_PAIRING_CERTIFICATE_HINT)
                            options=vec![
                                (PINNED, choice_t(msg::UI_PAIRING_CERTIFICATE_OWN)),
                                (CA, choice_t(msg::UI_PAIRING_CERTIFICATE_CA)),
                            ]
                            chosen=certificate
                        />
                    }
                        .into_any()
                } else {
                    view! { <p>{choice_t(msg::UI_PAIRING_CERTIFICATE)}</p> }.into_any()
                }
            }}
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
                on_page=true
            />
            <p class=ACTIONS>
                <Button
                    id=MAKE_ID
                    content=Content::Text(t(msg::UI_PAIRING_MAKE).into())
                    busy=busy
                    on_press=make
                />
            </p>
            <PairingDialog localizer=Arc::clone(&localizer) token=token />
        </section>
    }
}

/// The warning that a hub reached through a reverse proxy must turn
/// sign-in on, with the way to the guide's section that says why, in the
/// alert's own text color (§4.5, "Alert").
#[component]
fn ProxyWarning(localizer: Arc<Localizer>) -> impl IntoView {
    let words = text(&localizer, msg::UI_AGENTS_PROXY_SIGN_IN);
    let link = text(&localizer, msg::UI_AGENTS_PROXY_LINK);
    let link_localizer = Arc::clone(&localizer);
    view! {
        <Alert localizer=localizer tone=AlertTone::Warning>
            {words}
            " "
            {Destination::outside(PROXY_GUIDE_URL)
                .map(|to| {
                    view! {
                        <Link
                            to=to
                            localizer=link_localizer
                            tone=LinkTone::Inherit
                            placement=LinkPlacement::InText
                        >
                            {link}
                        </Link>
                    }
                })}
        </Alert>
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
    let fingerprint_name = t(msg::UI_PAIRING_FINGERPRINT);
    let copied = t(msg::UI_PAIRING_COPIED);
    let copy_refused = t(msg::UI_PAIRING_COPY_FAILED);
    let copy = Callback::new(move |()| {
        let secret = token.secret.get_untracked();
        if !secret.is_empty() {
            crate::components::copy_text(&secret, token.said, copied.clone(), copy_refused.clone());
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
        if !token.cancelable(&id) {
            return;
        }
        token.canceling.set(Some(id.clone()));
        let localizer = Arc::clone(&cancel_localizer);
        spawn_local(async move {
            let answer = ws::delete(&pairing::cancel_url(&ws::origin(), &id)).await;
            if token.canceling.try_get_untracked().is_none() {
                return;
            }
            if token
                .canceling
                .with_untracked(|canceling| pairing::cancel_on_its_way(canceling.as_deref(), &id))
            {
                token.canceling.set(None);
            }
            // Left while the cancel was on its way: the section says its
            // answer, as it does a cancel sent on leaving.
            if !token.shows(&id) {
                token.say_closed(&localizer, &id, answer.as_ref());
                return;
            }
            match answer {
                Some(Posted::Done(_)) => token.end(Ended::Canceled),
                Some(Posted::Refused(code, args, detail)) => {
                    token.said.set(Some(
                        refused_sentence(&localizer, code, &args, detail.as_deref()).plain(),
                    ));
                }
                None => token
                    .said
                    .set(Some(text(&localizer, msg::UI_PAIRING_UNANSWERED))),
            }
        });
    });
    // The zone the browser reads its clock in, asked once: the dialog writes
    // the token's lapse time in it, beside that zone's offset (§8.4).
    let viewer = l10n::viewer_zone();
    let status_viewer = viewer.clone();
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
            returns_to=MAKE_ID.to_owned()
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
                        let lapses_viewer = viewer.clone();
                        let expires_at = shown.expires_at.clone();
                        view! {
                            <dl>
                                <dt>{name.clone()}</dt>
                                <dd>
                                    <Fingerprint value=shown.fingerprint name=name />
                                </dd>
                                <dt>{text(&localizer, msg::UI_PAIRING_ADDRESS)}</dt>
                                <dd>{shown.address}</dd>
                                {shown
                                    .certificate
                                    .map(|certificate| {
                                        let said = text(
                                            &localizer,
                                            match certificate {
                                                TokenCertificateDto::Pinned => {
                                                    msg::UI_PAIRING_CERTIFICATE_PINNED
                                                }
                                                TokenCertificateDto::Ca => msg::UI_PAIRING_CERTIFICATE_CA,
                                            },
                                        );
                                        view! {
                                            <dt>
                                                {text(&localizer, msg::UI_PAIRING_CERTIFICATE_LABEL)}
                                            </dt>
                                            <dd>{said}</dd>
                                        }
                                    })}
                                <dt>{text(&localizer, msg::UI_PAIRING_LAPSES_LABEL)}</dt>
                                <dd>
                                    {move || lapses(
                                        &lapses_localizer,
                                        lapses_viewer.as_ref(),
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
                            <p class=ACTIONS>
                                <Button
                                    id=COPY_ID
                                    content=Content::Text(token_t(msg::UI_PAIRING_COPY).into())
                                    node_ref=copy_button
                                    on_press=copy
                                />
                                <Button
                                    id=CANCEL_ID
                                    content=Content::Text(token_t(msg::UI_PAIRING_CANCEL).into())
                                    busy=Signal::derive(move || {
                                        token
                                            .canceling
                                            .with(|canceling| {
                                                token
                                                    .shown
                                                    .with(|shown| {
                                                        pairing::canceling_shown(
                                                            canceling.as_deref(),
                                                            shown.as_ref().map(|shown| shown.id.as_str()),
                                                        )
                                                    })
                                            })
                                    })
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
                                lapsed(&status_localizer, status_viewer.as_ref(), &expires_at)
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
                            <p class=ACTIONS>
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
                {move || token.said.get()}
            </p>
            <p>{t(msg::UI_PAIRING_CLOSE_NOTE)}</p>
        </ReviewDialog>
    }
}
