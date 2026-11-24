//! The DOM glue around the live pages' connections (technical design §4.4):
//! the fetch of the initial history and the events WebSocket, and their
//! reconnects. Every decision — what the state is, how an event changes it,
//! when to reconnect — lives in `backend.rs`, host-tested; this module only
//! touches the browser's types, so a host run of mutants skips it and the
//! browser journeys (`@ui`) cover it instead, the `live.rs` precedent.

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use crate::backend::{
    OperationsState, REREAD_PATIENCE, Rereads, asks_for_a_reread, ended, events_url,
    reconnect_backoff, succeeded,
};
use crate::maintenance::{self, Awaited, Beat, Kind, Pace, Watched};
use crate::repositories::Sentence;
use crate::session::{
    Answer, Asked, Dismissals, Lease, Resend, Signed, Stand, Told, keep_signed_in_offered,
    signed_out_all_the_same, wait_ms,
};
use crate::visit::Visit;
use api::{
    ErrorCode, MsgArg, OperationDetailDto, OperationsPageDto, SessionDto, SignInDto,
    SignInStateDto, UiEvent,
};
use futures::channel::oneshot;
use futures::{FutureExt, Sink, StreamExt};
use gloo_net::http::{Request, RequestBuilder};
use gloo_net::websocket::futures::WebSocket;
use gloo_net::websocket::{Message, State};
use leptos::prelude::{GetUntracked, RwSignal, Set, Update};
#[cfg(feature = "hub")]
use leptos::prelude::{StoredValue, UpdateValue};
use leptos::task::spawn_local;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The route that says where the page stands with sign-in, signs in and
/// signs out (security §10.1).
const SESSION_PATH: &str = "/api/v1/session";

/// The route that signs out everywhere: every browser's session ends.
const SESSIONS_PATH: &str = "/api/v1/sessions";

/// The header a changed request carries its session's CSRF token in.
const CSRF_HEADER: &str = "x-csrf-token";

/// Where a live page's reading of the daemon stands: still loading its first
/// answer, ready (the table is the history), or unable to reach the daemon at
/// all. The page's heading follows this, so the reader is told before any
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Loading,
    Ready,
    Unreachable,
}

/// The body `POST /api/v1/operations` answers with: the started operation's
/// ID, unenveloped.
#[derive(Deserialize)]
struct StartedOperation {
    operation: String,
}

/// The operation a start's answer names, for the settle-poll that follows it.
pub(crate) fn started_operation(answer: &str) -> Option<String> {
    serde_json::from_str::<StartedOperation>(answer)
        .ok()
        .map(|started| started.operation)
}

/// A typed refusal, as the daemon writes it: the code, the arguments its
/// message takes, and sometimes the engine's own words. The page renders these
/// through the catalog (§8.4), so no refusal is ever shown as a status code.
#[derive(Deserialize)]
struct Refusal {
    code: ErrorCode,
    #[serde(default)]
    args: std::collections::BTreeMap<String, MsgArg>,
    #[serde(default)]
    detail: Option<String>,
}

/// What a POST came back with: `Done` (with the raw answer, for whoever wants
/// its body), a typed refusal to render (its code, arguments and the engine's
/// words, if it gave any), or `None` — the daemon never answered, which the
/// page's phase says its own way.
pub(crate) enum Posted {
    Done(String),
    Refused(ErrorCode, Vec<(String, MsgArg)>, Option<String>),
}

/// The page's own origin, where the daemon that served the page listens.
pub(crate) fn origin() -> String {
    web_sys::window().map_or_default(|window| window.location().origin().unwrap_or_default())
}

/// What the page's requests need of its session (security §10.1): the CSRF
/// token its changed requests carry, its lease and the timer that looks at
/// it again, the signals that show the warning before it ends unused and the
/// dialog asking to sign in again, the requests waiting for that sign-in,
/// whether the reader signed out, after which the sign-in page asks, and
/// whether the sign-in form offers **Keep me signed in on this device**, as
/// the daemon's last signed-out answer said.
#[derive(Default)]
struct Session {
    csrf: Option<String>,
    lease: Option<Lease>,
    timer: Option<gloo_timers::callback::Timeout>,
    warning: Option<RwSignal<bool>>,
    ended: Option<RwSignal<bool>>,
    told: Option<RwSignal<Option<Told>>>,
    keep_offered: Option<RwSignal<bool>>,
    waiting: Vec<oneshot::Sender<bool>>,
    signed_out: bool,
    dismissals: Dismissals,
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::default());
}

/// The page's clock, in milliseconds since the epoch. It is the wall clock,
/// which goes on while the computer sleeps, so a timer that fires late still
/// sees how much time has passed.
pub(crate) fn now_ms() -> i64 {
    // `as` saturates: no clock is past what an `i64` counts.
    js_sys::Date::now() as i64
}

/// The page shows the warning that its session is about to end unused while
/// `warning` is `true`, and the dialog asking to sign in again while `ended`
/// is; the session's lease and a request refused for want of a session set
/// them. The sign-in form offers **Keep me signed in on this device** while
/// `keep_offered` is `true`, which each signed-out answer sets.
pub(crate) fn watch_session(
    warning: RwSignal<bool>,
    ended: RwSignal<bool>,
    told: RwSignal<Option<Told>>,
    keep_offered: RwSignal<bool>,
) {
    SESSION.with_borrow_mut(|session| {
        session.warning = Some(warning);
        session.ended = Some(ended);
        session.told = Some(told);
        session.keep_offered = Some(keep_offered);
    });
}

/// The daemon told the page what it runs and the update it found, as it
/// does once the page may see the pages: the header shows them. An answer
/// that doesn't tell them leaves what the page last heard.
fn told(told: Option<Told>) {
    if let Some(told) = told
        && let Some(shown) = SESSION.with_borrow(|session| session.told)
    {
        shown.try_set(Some(told));
    }
}

/// The daemon said where the page stands: a signed-out answer says whether
/// the sign-in form offers **Keep me signed in on this device**
/// (`keep_signed_in_offered` decides); any other leaves it as it was.
fn heard(read: &SessionDto) {
    if let Some(offered) = keep_signed_in_offered(read)
        && let Some(keep_offered) = SESSION.with_borrow(|session| session.keep_offered)
    {
        keep_offered.try_set(offered);
    }
}

/// The page has a live session (none with sign-in off). Its changed requests
/// carry its CSRF token, the page watches its lease, the requests that waited
/// for it are sent again, as they were, and the dialog closes.
pub(crate) fn signed_in(signed: Option<Signed>) {
    told(signed.as_ref().and_then(|signed| signed.told.clone()));
    let (waiting, ended) = SESSION.with_borrow_mut(|session| {
        session.csrf = signed.as_ref().map(|signed| signed.csrf.clone());
        session.lease = signed.map(|signed| signed.lease);
        session.signed_out = false;
        session.dismissals = session.dismissals.signed_in();
        (std::mem::take(&mut session.waiting), session.ended)
    });
    look_again();
    // Outside the borrow: what the signal or a request wakes may send again.
    for request in waiting {
        let _ = request.send(true);
    }
    if let Some(ended) = ended {
        ended.try_set(false);
    }
}

/// A request sent at `sent` used the session.
fn used(sent: i64) {
    let watched = SESSION.with_borrow_mut(|session| {
        session.lease = session.lease.map(|lease| lease.used(sent));
        session.lease.is_some()
    });
    if watched {
        look_again();
    }
}

/// Looks at where the session's lease stands now: says nothing until the
/// moment it changes, warns that the session is about to end unused, or,
/// once it has ended, forgets it and asks to sign in again. The decisions are
/// `Lease::at`'s, host-tested.
fn look_again() {
    let now = now_ms();
    let (stand, warning, ended) = SESSION.with_borrow_mut(|session| {
        // Dropping the timer cancels it.
        session.timer = None;
        let stand = session.lease.map(|lease| lease.at(now));
        if stand == Some(Stand::Ended) {
            session.lease = None;
        }
        (stand, session.warning, session.ended)
    });
    let warn = |on: bool| {
        if let Some(warning) = warning {
            warning.try_set(on);
        }
    };
    let until = match stand {
        None => return warn(false),
        Some(Stand::Quiet { until }) => {
            warn(false);
            until
        }
        Some(Stand::Warning { until }) => {
            warn(true);
            until
        }
        Some(Stand::Ended) => {
            warn(false);
            if let Some(ended) = ended {
                ended.try_set(true);
            }
            return;
        }
    };
    let timer = gloo_timers::callback::Timeout::new(wait_ms(now, until), look_again);
    SESSION.with_borrow_mut(|session| session.timer = Some(timer));
}

/// The reader closed the dialog without signing in: the requests that waited
/// for it end with the refusal they had, which their pages say, and so do
/// those still on their way and the page's own reads after it
/// (`Dismissals` decides).
pub(crate) fn gave_up() {
    let waiting = SESSION.with_borrow_mut(|session| {
        session.dismissals = session.dismissals.one_more();
        std::mem::take(&mut session.waiting)
    });
    for request in waiting {
        let _ = request.send(false);
    }
}

/// A request `asked` so and sent while the reader stood at `sent` was
/// refused for want of a session: asks the reader to sign in again, and
/// waits. `true` is a sign-in, after which the request is sent again;
/// `false` is the reader closing the dialog, a page that shows none, a
/// reader who signed out, or a request the dialog's closing ended, as one
/// on its way then is (`Dismissals` decides): the sign-in page asks then,
/// and a request still on its way when they did ends with its refusal.
async fn session_ended(sent: Dismissals, asked: Asked) -> bool {
    let (sender, signed_in) = oneshot::channel();
    let (ended, warning) = SESSION.with_borrow_mut(|session| {
        session.lease = None;
        session.timer = None;
        let ended = session.ended.filter(|ended| {
            let open = ended.try_get_untracked().unwrap_or(false);
            !session.signed_out && session.dismissals.waits_for_sign_in(sent, asked, open)
        });
        (
            ended.inspect(|_| session.waiting.push(sender)),
            session.warning,
        )
    });
    if let Some(warning) = warning {
        warning.try_set(false);
    }
    let Some(ended) = ended else {
        return false;
    };
    ended.try_set(true);
    signed_in.await.unwrap_or(false)
}

/// Sends the request `make` builds, with the CSRF token and the abort signal
/// it is given, and reads the answer's status and text. A refusal for want
/// of a session waits for the reader to sign in again, then builds and sends
/// the request anew, with the new session's token; a refusal of its CSRF
/// token, as after signing in again in another tab, reads the session's
/// current token and sends it once more. The daemon refused either before
/// any route ran, so the request did nothing the first time
/// (`Resend::after` decides). With `patience`, a send that takes longer is
/// aborted, and that is `None`, as no answer is.
async fn send(
    patience: Option<Duration>,
    asked: Asked,
    make: impl Fn(Option<&str>, Option<&web_sys::AbortSignal>) -> Option<Request>,
) -> Option<(u16, String)> {
    let mut token_read = false;
    loop {
        let controller = match patience {
            Some(_) => Some(web_sys::AbortController::new().ok()?),
            None => None,
        };
        let signal = controller.as_ref().map(web_sys::AbortController::signal);
        // Dropping the timer cancels it, so it ends with the send: an answer
        // in time is never aborted.
        let _deadline = patience.zip(controller).map(|(patience, controller)| {
            let millis = u32::try_from(patience.as_millis()).unwrap_or(u32::MAX);
            gloo_timers::callback::Timeout::new(millis, move || controller.abort())
        });
        let (csrf, standing) =
            SESSION.with_borrow(|session| (session.csrf.clone(), session.dismissals));
        let sent = now_ms();
        let response = make(csrf.as_deref(), signal.as_ref())?.send().await.ok()?;
        let status = response.status();
        let text = response.text().await.ok()?;
        match Resend::after(status, &text, token_read) {
            Resend::OnceSignedIn => {
                if session_ended(standing, asked).await {
                    continue;
                }
            }
            Resend::WithFreshToken => {
                token_read = true;
                // Reading the session is a use, which gives the page the
                // session's current token and lease; a session that has ended
                // meanwhile is refused as such when the request is sent again.
                if session().await.is_some() {
                    continue;
                }
            }
            Resend::No if asked == Asked::ByPerson => used(sent),
            Resend::No => {}
        }
        return Some((status, text));
    }
}

/// Reads `url` as typed JSON, `asked` by a person or by the page on its own,
/// which marks the request so. `None` is anything but the answer: the daemon
/// not answering, a refusal, or a body that isn't what was asked for — the
/// caller's phase decides what that means; with `patience`, an answer that
/// takes longer is aborted, and that is `None` too.
async fn read_as<T: DeserializeOwned>(
    url: &str,
    asked: Asked,
    patience: Option<Duration>,
) -> Option<T> {
    match got_as(url, asked, patience).await? {
        Got::Read(read) => Some(read),
        Got::Refused(..) => None,
    }
}

/// What a GET came back with: the typed answer, or a typed refusal, which
/// the page renders as it renders a refused POST.
pub(crate) enum Got<T> {
    Read(T),
    Refused(ErrorCode, Vec<(String, MsgArg)>, Option<String>),
}

/// Reads `url` as typed JSON like `read_as`, but tells a refusal from a daemon
/// that did not answer: `None` is the daemon not answering, or an answer that
/// is neither the JSON asked for nor a typed refusal.
async fn got_as<T: DeserializeOwned>(
    url: &str,
    asked: Asked,
    patience: Option<Duration>,
) -> Option<Got<T>> {
    let (status, text) = send(patience, asked, |_, signal| {
        let builder = Request::get(url).abort_signal(signal);
        let builder = match asked {
            Asked::OnItsOwn => builder.header(api::BACKGROUND_HEADER, "1"),
            Asked::ByPerson => builder,
        };
        builder.build().ok()
    })
    .await?;
    if succeeded(status) {
        return serde_json::from_str::<T>(&text).ok().map(Got::Read);
    }
    let refusal = serde_json::from_str::<Refusal>(&text).ok()?;
    Some(Got::Refused(
        refusal.code,
        refusal.args.into_iter().collect::<Vec<_>>(),
        refusal.detail,
    ))
}

/// Reads `url` as typed JSON, as a person's request, telling a refusal from a
/// daemon that did not answer as `got_as` does.
pub(crate) async fn get<T: DeserializeOwned>(url: &str) -> Option<Got<T>> {
    got_as(url, Asked::ByPerson, None).await
}

/// Reads `url` as typed JSON, as the page's own read (a poll), which keeps
/// no session alive; `None` is anything but the answer, as for `read_as`,
/// an answer that takes longer than `patience` included.
pub(crate) async fn poll<T: DeserializeOwned>(url: &str, patience: Duration) -> Option<T> {
    read_as(url, Asked::OnItsOwn, Some(patience)).await
}

/// Whether the daemon answers, within `patience`: the page's own read of the
/// session route, whose answer counts signed in or not, as from a daemon
/// just started with sign-in newly on, but only as Ladderback's own
/// (`restart::is_ladderback`), never a proxy's for a daemon that is down.
pub(crate) async fn answers(patience: Duration) -> bool {
    let url = format!("{}{SESSION_PATH}", origin());
    send(Some(patience), Asked::OnItsOwn, |_, signal| {
        Request::get(&url)
            .header(api::BACKGROUND_HEADER, "1")
            .abort_signal(signal)
            .build()
            .ok()
    })
    .await
    .is_some_and(|(status, text)| crate::restart::is_ladderback(status, &text))
}

/// Posts `body` as JSON to `url` (the JSON-body rule applies to every
/// mutating method, §4.2), with the session's CSRF token when sign-in is on,
/// and sorts the answer.
pub(crate) async fn post<S: Serialize>(url: &str, body: &S) -> Option<Posted> {
    change(Request::post, url, body).await
}

/// Puts `body` as JSON at `url`, as [`post`] posts it.
pub(crate) async fn put<S: Serialize>(url: &str, body: &S) -> Option<Posted> {
    change(Request::put, url, body).await
}

/// Sends `body` as JSON to `url` with the method `start` begins, with the
/// session's CSRF token when sign-in is on, and sorts the answer.
async fn change<S: Serialize>(
    start: fn(&str) -> RequestBuilder,
    url: &str,
    body: &S,
) -> Option<Posted> {
    let (status, text) = send(None, Asked::ByPerson, |csrf, _| {
        let builder = start(url);
        let builder = match csrf {
            Some(csrf) => builder.header(CSRF_HEADER, csrf),
            None => builder,
        };
        builder.json(body).ok()
    })
    .await?;
    sorted(status, &text)
}

/// Sends `DELETE url`, a JSON request with no body, with the session's CSRF
/// token when sign-in is on, and sorts the answer as [`post`] does.
pub(crate) async fn delete(url: &str) -> Option<Posted> {
    let (status, text) = send(None, Asked::ByPerson, |csrf, _| {
        let builder = Request::delete(url).header("content-type", "application/json");
        let builder = match csrf {
            Some(csrf) => builder.header(CSRF_HEADER, csrf),
            None => builder,
        };
        builder.build().ok()
    })
    .await?;
    sorted(status, &text)
}

/// A changed request's answer: done, with its text, or a typed refusal.
fn sorted(status: u16, text: &str) -> Option<Posted> {
    if succeeded(status) {
        return Some(Posted::Done(text.to_owned()));
    }
    let refusal = serde_json::from_str::<Refusal>(text).ok()?;
    Some(Posted::Refused(
        refusal.code,
        refusal.args.into_iter().collect::<Vec<_>>(),
        refusal.detail,
    ))
}

/// Where the page stands with sign-in: `GET /api/v1/session`, which uses a
/// live session. A live session's CSRF token and lease are the page's from
/// then on; a signed-out answer says whether the sign-in form offers Keep me
/// signed in. `None` is no answer the page could read.
pub(crate) async fn session() -> Option<SessionDto> {
    let sent = now_ms();
    let response = Request::get(&format!("{}{SESSION_PATH}", origin()))
        .send()
        .await
        .ok()?;
    if response.status() != 200 {
        return None;
    }
    let read = response.json::<SessionDto>().await.ok()?;
    heard(&read);
    match Signed::of(&read, sent) {
        Some(signed) => signed_in(Some(signed)),
        None => told(Told::of(&read)),
    }
    Some(read)
}

/// Whether the sign-in form offers Keep me signed in, as a page with no
/// session hears it: `GET /api/v1/session` without the session's cookie.
/// The dialog asking to sign in again asks so once the page's clock has
/// ended the session, a moment before the daemon's does (`Lease`): with the
/// cookie, that read would use the session, keeping it another 12 hours,
/// and give the page its lease again, closing the dialog, so a page left
/// open would never let its session end unused (security §10.1). Any
/// answer but a signed-out one says nothing of the form.
pub(crate) async fn hear_keep_offered() {
    let Ok(response) = Request::get(&format!("{}{SESSION_PATH}", origin()))
        .credentials(web_sys::RequestCredentials::Omit)
        .send()
        .await
    else {
        return;
    };
    if response.status() != 200 {
        return;
    }
    if let Ok(read) = response.json::<SessionDto>().await {
        heard(&read);
    }
}

/// The reader chose to stay signed in: asking where the page stands is a
/// use, which keeps the session another 12 hours unused, up to its lifetime,
/// and the warning closes. A session that ended meanwhile asks to sign in
/// again. `false` is no answer: the warning stays, and says so.
pub(crate) async fn stay_signed_in() -> bool {
    let Some(read) = session().await else {
        return false;
    };
    if read.sign_in == SignInStateDto::SignedOut {
        SESSION.with_borrow_mut(|session| session.lease = None);
        look_again();
        if let Some(ended) = SESSION.with_borrow(|session| session.ended) {
            ended.try_set(true);
        }
    }
    true
}

/// Signs in with `password`, kept on this device past the idle limit when
/// `keep_signed_in`: `POST /api/v1/session`. A session that starts gives its
/// CSRF token to every changed request from then on.
pub(crate) async fn sign_in(password: String, keep_signed_in: bool) -> Answer {
    let body = SignInDto {
        password,
        keep_signed_in,
    };
    let Ok(request) = Request::post(&format!("{}{SESSION_PATH}", origin())).json(&body) else {
        return Answer::Unreadable;
    };
    let sent = now_ms();
    let Ok(response) = request.send().await else {
        return Answer::Unreadable;
    };
    let status = response.status();
    let Ok(text) = response.text().await else {
        return Answer::Unreadable;
    };
    Answer::of(status, &text, sent)
}

/// Signs out: `DELETE /api/v1/session`, a changed request, so it says its
/// body is JSON (§4.2) and carries the session's CSRF token, read afresh
/// once if the session has another (`send`). Once the daemon has ended the
/// session, the page forgets its token and its lease: when it is done, and
/// when the database could not forget the session, which has ended all the
/// same. `false` is no answer, or another refusal: the session may still be
/// live.
pub(crate) async fn sign_out() -> bool {
    match end_sessions(SESSION_PATH).await {
        Some(Posted::Done(_)) => true,
        Some(Posted::Refused(code, ..)) if signed_out_all_the_same(code) => {
            forget_session();
            true
        }
        Some(Posted::Refused(..)) | None => false,
    }
}

/// Signs out everywhere: `DELETE /api/v1/sessions`, which ends every
/// browser's session, this page's included, sent as [`sign_out`] sends its
/// request. Done, the page forgets its session; a refusal is the daemon's to
/// say, and `None` is no answer.
pub(crate) async fn sign_out_everywhere() -> Option<Posted> {
    end_sessions(SESSIONS_PATH).await
}

/// Sends `DELETE` to `path`, which ends this page's session and perhaps
/// others. Done, the page forgets its session, and the signed-out answer
/// says whether the sign-in form offers Keep me signed in.
async fn end_sessions(path: &str) -> Option<Posted> {
    let posted = delete(&format!("{}{path}", origin())).await?;
    if let Posted::Done(text) = &posted {
        if let Ok(read) = serde_json::from_str::<SessionDto>(text) {
            heard(&read);
        }
        forget_session();
    }
    Some(posted)
}

/// Forgets the page's session, which the daemon has ended: its CSRF token
/// and its lease. Signing out ends it, and so does a new admin password set
/// in Settings, which ends every session. No dialog asks to sign in again:
/// the caller shows the sign-in page.
pub(crate) fn forget_session() {
    SESSION.with_borrow_mut(|session| {
        session.csrf = None;
        session.lease = None;
        session.signed_out = true;
    });
    look_again();
}

/// Reads the operations history from `base` (the page's own origin) into
/// `state`, then opens the events stream and applies each event to `state`.
/// When the stream drops, it waits `backend::reconnect_backoff` and starts
/// over with the fetch: the history is the source of truth a reconnect
/// returns to, never a guess at what was missed. Each stream that opens
/// reads the history once more (`stream`). A fetch that gets no answer at
/// any point of the loop leaves the page in `Phase::Unreachable`; one the
/// daemon refuses, such as a page's own read after the reader closed Sign in
/// again (§4.5), leaves it ready with the refusal in `refused`, in the words
/// `say` gives it, until a read gets the history. Either way the loop keeps
/// trying with the same backoff, so a daemon that comes back is a page that
/// heals. All of it, the re-reads too, lasts as long as `visit`: once the
/// reader has left the page, the loop ends at its next await, and the
/// stream it holds closes (#300).
pub(crate) fn watch(
    base: &str,
    state: RwSignal<OperationsState>,
    phase: RwSignal<Phase>,
    refused: RwSignal<Option<Sentence>>,
    say: impl Fn(ErrorCode, &[(String, MsgArg)], Option<&str>) -> Sentence + 'static,
    visit: &Visit,
) {
    let base = base.to_owned();
    let lasting = visit.clone();
    spawn_local(
        visit
            .clone()
            .during(async move {
                let mut attempt = 0u32;
                // The first answer builds the state; every answer after a disconnect
                // merges into it (§4.4): a status an event advanced is never handed
                // back to the store's lagging write, and a live row the store has
                // not written yet stays.
                let mut first = true;
                // Shared by every round, so a reconnect never runs a second re-read
                // beside one still out.
                let rereads = Rc::new(Cell::new(Rereads::default()));
                // The first read is the page opening; every later one, a reconnect's
                // or a retry's, is the page's own.
                let mut asked = Asked::ByPerson;
                loop {
                    let url = format!("{base}/api/v1/operations");
                    let read = got_as::<OperationsPageDto>(&url, asked, None).await;
                    asked = Asked::OnItsOwn;
                    match read {
                        Some(Got::Read(page)) => {
                            attempt = 0;
                            refused.set(None);
                            phase.set(Phase::Ready);
                            if first {
                                first = false;
                                state
                                    .set(OperationsState::from_history(page.operations, page.next));
                            } else {
                                state.update(|state| state.merge(page));
                            }
                            stream(&base, &state, &rereads, &lasting).await;
                            // The stream ended: a closed socket is a failure like any
                            // other, and the next round waits for it.
                            attempt += 1;
                            sleep(reconnect_backoff(attempt)).await;
                        }
                        Some(Got::Refused(code, args, detail)) => {
                            attempt += 1;
                            refused.set(Some(say(code, &args, detail.as_deref())));
                            phase.set(Phase::Ready);
                            sleep(reconnect_backoff(attempt)).await;
                        }
                        None => {
                            attempt += 1;
                            phase.set(Phase::Unreachable);
                            sleep(reconnect_backoff(attempt)).await;
                        }
                    }
                }
            })
            .map(|_| ()),
    );
}

/// Applies the events stream to `state` until it ends. A `UiEvent` the state
/// holds no operation for is ignored here too: the reconnect's re-fetch is
/// what brings the whole story, never a half-invented row. The end of a run
/// is read again from the history, which has what the run produced.
///
/// The page hears nothing told before its stream opened, such as the start
/// of a run its first read had queued (#305), so the history is read again
/// once the stream is open, as an end's re-read is, one at a time. The
/// events are applied meanwhile; neither `apply` nor `merge` takes a row
/// back for the order the two arrive in.
async fn stream(
    base: &str,
    state: &RwSignal<OperationsState>,
    rereads: &Rc<Cell<Rereads>>,
    visit: &Visit,
) {
    let Ok(mut socket) = WebSocket::open(&events_url(base)) else {
        return;
    };
    // Ready once the socket has opened or failed: gloo wakes it on either.
    let _ = poll_fn(|cx| Pin::new(&mut socket).poll_ready(cx)).await;
    if !matches!(socket.state(), State::Open) {
        return;
    }
    reread(base, *state, Rc::clone(rereads), visit);
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Text(text) = message
            && let Ok(event) = serde_json::from_str::<UiEvent>(&text)
        {
            state.update(|state| state.apply(&event));
            if asks_for_a_reread(&event) {
                reread(base, *state, Rc::clone(rereads), visit);
            }
        }
    }
}

/// Listens to the events stream of `base` (the page's own origin) while
/// `visit` lasts, for a page that reads its own state rather than the
/// operations history (#569). `opened` runs each time a stream opens, the
/// first time and after each reconnect, so the page reads again what it may
/// have missed while no stream was open; `heard` gets each event in turn.
/// A stream that drops, or never opens, is tried again after
/// `backend::reconnect_backoff`, as `watch`'s is.
#[cfg(feature = "hub")]
pub(crate) fn listen(
    base: &str,
    visit: &Visit,
    opened: impl Fn() + 'static,
    heard: impl Fn(UiEvent) + 'static,
) {
    let url = events_url(base);
    spawn_local(
        visit
            .clone()
            .during(async move {
                let mut attempt = 0u32;
                loop {
                    if let Some(mut socket) = opened_stream(&url).await {
                        attempt = 0;
                        opened();
                        while let Some(Ok(message)) = socket.next().await {
                            if let Message::Text(text) = message
                                && let Ok(event) = serde_json::from_str::<UiEvent>(&text)
                            {
                                heard(event);
                            }
                        }
                    }
                    attempt = attempt.saturating_add(1);
                    sleep(reconnect_backoff(attempt)).await;
                }
            })
            .map(|_| ()),
    );
}

/// The events stream at `url`, once it has opened; `None` when it failed to.
#[cfg(feature = "hub")]
async fn opened_stream(url: &str) -> Option<WebSocket> {
    let mut socket = WebSocket::open(url).ok()?;
    // Ready once the socket has opened or failed: gloo wakes it on either.
    let _ = poll_fn(|cx| Pin::new(&mut socket).poll_ready(cx)).await;
    matches!(socket.state(), State::Open).then_some(socket)
}

/// Reads `url` into a hub page through `land` (#569), as `backend::Rereads`
/// paces the operations' re-reads: one read out at a time, every answer
/// landing, one more after it when an event or a stream's opening asked
/// meanwhile, and a few tries again, later each time, when a read gets no
/// answer in `patience`. `land` gets each read's answer, or `None`, and says
/// whether the page still waits for its first answer: until it has one, the
/// page asks again every `ask_again`, as it tells its reader. A read lasts no
/// longer than the `visit`.
#[cfg(feature = "hub")]
pub(crate) fn reread_hub<T: DeserializeOwned + 'static>(
    url: String,
    rereads: StoredValue<Rereads>,
    visit: &Visit,
    (patience, ask_again): (Duration, Duration),
    land: impl Fn(Option<T>) -> bool + 'static,
) {
    if rereads.try_update_value(Rereads::ask) != Some(true) {
        return;
    }
    spawn_local(
        visit
            .clone()
            .during(async move {
                loop {
                    let read = poll::<T>(&url, patience).await;
                    let answered = read.is_some();
                    let waiting = land(read);
                    match rereads.try_update_value(|now| now.finish(answered)) {
                        Some(Some(wait)) if wait.is_zero() => {}
                        Some(Some(wait)) => sleep(wait).await,
                        Some(None) if waiting => {
                            sleep(ask_again).await;
                            if rereads.try_update_value(Rereads::ask) != Some(true) {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            })
            .map(|_| ()),
    );
}

/// Reads the history into `state` again after a run's end or a stream's
/// opening, as `backend::Rereads` paces it: one read out at a time, one more
/// after it when another end or opening arrived meanwhile, and a few tries
/// again, later each time, when a read gets no history in
/// `REREAD_PATIENCE`. `rereads` is the one `watch` keeps across its
/// reconnects, so no number of reconnects has more than two reads waiting.
/// A read lasts no longer than the `visit`.
fn reread(base: &str, state: RwSignal<OperationsState>, rereads: Rc<Cell<Rereads>>, visit: &Visit) {
    if !turn(&rereads, Rereads::ask) {
        return;
    }
    let url = format!("{base}/api/v1/operations");
    spawn_local(
        visit
            .clone()
            .during(async move {
                loop {
                    let read =
                        read_as::<OperationsPageDto>(&url, Asked::OnItsOwn, Some(REREAD_PATIENCE));
                    let page = read.await;
                    let answered = page.is_some();
                    if let Some(page) = page {
                        state.update(|state| state.merge(page));
                    }
                    match turn(&rereads, |now| now.finish(answered)) {
                        None => break,
                        Some(wait) if wait.is_zero() => {}
                        Some(wait) => sleep(wait).await,
                    }
                }
            })
            .map(|_| ()),
    );
}

/// Turns the shared `rereads` by `step` and says what the step answered.
fn turn<T>(rereads: &Cell<Rereads>, step: impl FnOnce(&mut Rereads) -> T) -> T {
    let mut now = rereads.get();
    let answer = step(&mut now);
    rereads.set(now);
    answer
}

/// After a start or a cancel, polls the history into `state` until
/// `operation` has ended, as the page's own reads: the start or the cancel
/// was the person's use of the session. A start announces before the first
/// event can arrive, and the page takes no news from a cancel's `UiEvent`,
/// so the page does not guess — it reads the store until the store says the
/// run settled.
/// `false` is the budget running out with no end in sight: the caller says
/// so, rather than leave the reader watching a row that stopped moving.
pub(crate) async fn settle(base: &str, state: RwSignal<OperationsState>, operation: &str) -> bool {
    let url = format!("{base}/api/v1/operations");
    for _ in 0..(SETTLE_FOR_MS / POLL_EVERY_MS) {
        if let Some(page) = read_as::<OperationsPageDto>(&url, Asked::OnItsOwn, None).await {
            let settled = page
                .operations
                .iter()
                .any(|summary| summary.id == operation && ended(&summary.status));
            state.update(|state| state.merge(page));
            if settled {
                return true;
            }
        }
        sleep(POLL_EVERY).await;
    }
    false
}

/// Reads the operation of a maintenance run of `kind` until it has ended, as
/// [`await_operation`] does.
pub(crate) async fn await_end(
    base: &str,
    operation: &str,
    kind: Kind,
    alive: impl Fn() -> bool,
) -> Watched {
    Watched::of(kind, await_operation(base, operation, alive).await)
}

/// Reads `operation` until it has ended, as the page's own reads. The pace
/// of the reads and the patience with a silent daemon are
/// `maintenance::Pace`'s; `alive` says whether the page that waits is still
/// there.
pub(crate) async fn await_operation(
    base: &str,
    operation: &str,
    alive: impl Fn() -> bool,
) -> Awaited {
    let url = format!("{base}{}", maintenance::operation_path(operation));
    let mut pace = Pace::default();
    while alive() {
        let answer = read_as::<OperationDetailDto>(&url, Asked::OnItsOwn, None).await;
        match pace.after(answer.as_ref()) {
            Beat::Again(delay) => sleep(delay).await,
            Beat::Ended(_) => {
                if let Some(detail) = answer {
                    return Awaited::Ended(Box::new(detail));
                }
            }
            Beat::GaveUp => return Awaited::GaveUp,
        }
    }
    Awaited::Gone
}

/// How long a settle-poll keeps reading, and how often.
const SETTLE_FOR_MS: u32 = 120_000;
const POLL_EVERY_MS: u32 = 250;
const POLL_EVERY: Duration = Duration::from_millis(POLL_EVERY_MS as u64);

/// Waits `for_`, in the browser's own time.
async fn sleep(for_: Duration) {
    gloo_timers::future::sleep(for_).await;
}
