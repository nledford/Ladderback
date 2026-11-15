//! Steps for the untagged scenarios of `features/security/sign_in.feature`
//! and `features/security/admin_password.feature`: the sign-in use case and
//! the one that sets the admin password, over an in-memory admin password,
//! seeded randomness and the world's fake clock.

use std::assert_matches;
use std::net::IpAddr;
use std::sync::Arc;

use app::access::{
    Access, LiveSession, PasswordChangeRefused, PasswordSet, SetPasswordRefused, SignInRefused,
    SignedIn, set_admin_password,
};
use app::fakes::{
    InMemoryAdminPassword, InMemoryApiTokenStore, InMemorySessionStore, SeededRandom,
};
use app::ports::{AdminPassword, AdminPasswordError, SessionStore};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::access::{CookieLife, Password, SessionLength, Token};

use super::Agent;

/// What the sign-in scenarios keep between steps.
#[derive(Default)]
pub struct SignInWorld {
    access: Option<Access>,
    /// Where the admin password is kept, for `set-password` to replace it.
    kept: Option<Arc<InMemoryAdminPassword>>,
    /// Where the sessions and the API tokens are kept, which a restart
    /// opens again.
    stores: Option<(Arc<InMemorySessionStore>, Arc<InMemoryApiTokenStore>)>,
    /// How many times Ladderback has started, which seeds its randomness.
    starts: u64,
    /// What the last sign-in came to.
    last: Option<Result<SignedIn, SignInRefused>>,
    /// The sessions started so far, first first.
    sessions: Vec<SignedIn>,
    /// What setting the admin password in Settings with sign-in off came to.
    set: Option<Result<PasswordSet, SetPasswordRefused>>,
    /// What the last change of the admin password came to.
    changed: Option<Result<PasswordSet, PasswordChangeRefused>>,
    /// What the session's last use came to.
    used: Option<LiveSession>,
    /// What the last read the page made on its own came to.
    read: Option<LiveSession>,
}

impl std::fmt::Debug for SignInWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInWorld")
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

impl SignInWorld {
    pub(crate) fn access(&self) -> &Access {
        self.access.as_ref().expect("an admin password is set")
    }

    fn session(&self) -> &Token {
        &self
            .sessions
            .first()
            .expect("the admin has signed in")
            .token
    }
}

fn address(text: &str) -> IpAddr {
    text.parse().expect("an IP address")
}

async fn sign_in(agent: &mut Agent, password: &str, from: &str, keep_signed_in: bool) {
    let password = Password::parse(password.to_owned()).expect("a password");
    let result = agent
        .sign_in
        .access()
        .sign_in(address(from), &password, keep_signed_in, false)
        .await;
    if let Ok(signed_in) = &result {
        agent.sign_in.sessions.push(SignedIn {
            token: signed_in.token.clone(),
            csrf: signed_in.csrf.clone(),
            ends_in: signed_in.ends_in,
            idle_limit: signed_in.idle_limit,
            cookie: signed_in.cookie,
        });
    }
    agent.sign_in.last = Some(result);
}

#[given(expr = "the admin password is {string}")]
fn admin_password(agent: &mut Agent, password: String) {
    let kept = Arc::new(InMemoryAdminPassword::with(&password));
    let stores = (
        Arc::new(InMemorySessionStore::new()),
        Arc::new(InMemoryApiTokenStore::new()),
    );
    agent.sign_in.starts = 1;
    agent.sign_in.access = Some(Access::new(
        kept.clone(),
        Arc::new(SeededRandom::new(1)),
        agent.clock.clone(),
        stores.1.clone(),
        stores.0.clone(),
    ));
    agent.sign_in.kept = Some(kept);
    agent.sign_in.stores = Some(stores);
}

/// Ladderback stops and starts again on the same admin password and store,
/// with randomness of its own, and brings back the sessions it kept.
#[when("Ladderback is restarted")]
async fn restarted(agent: &mut Agent) {
    let kept = agent
        .sign_in
        .kept
        .clone()
        .expect("an admin password is set");
    let (sessions, tokens) = agent.sign_in.stores.clone().expect("a store");
    agent.sign_in.starts += 1;
    let access = Access::new(
        kept,
        Arc::new(SeededRandom::new(1_000 * agent.sign_in.starts)),
        agent.clock.clone(),
        tokens,
        sessions,
    );
    access
        .restore()
        .await
        .expect("the sessions are brought back");
    agent.sign_in.access = Some(access);
}

/// The session store, opened here when no step has opened one yet.
fn session_store(agent: &mut Agent) -> Arc<InMemorySessionStore> {
    let stores = agent.sign_in.stores.get_or_insert_with(|| {
        (
            Arc::new(InMemorySessionStore::new()),
            Arc::new(InMemoryApiTokenStore::new()),
        )
    });
    stores.0.clone()
}

/// `ladderback set-password`, which calls the use case Settings calls.
#[when(expr = "the admin password is set to {string}")]
async fn set_password(agent: &mut Agent, password: String) {
    let kept = agent
        .sign_in
        .kept
        .clone()
        .expect("an admin password is set");
    let sessions = session_store(agent);
    let set = set_admin_password(kept.as_ref(), sessions.as_ref(), password)
        .await
        .expect("the password is kept");
    assert_eq!(set, PasswordSet::SessionsForgotten);
}

#[given("no admin password is set")]
fn no_admin_password(agent: &mut Agent) {
    agent.sign_in.kept = Some(Arc::new(InMemoryAdminPassword::default()));
}

#[when(expr = "the admin password {string} is set in Settings")]
async fn set_in_settings(agent: &mut Agent, password: String) {
    let kept = agent.sign_in.kept.clone().expect("where it is kept");
    let sessions = session_store(agent);
    agent.sign_in.set = Some(set_admin_password(kept.as_ref(), sessions.as_ref(), password).await);
}

#[then("Ladderback keeps no session for after a restart")]
async fn keeps_no_session(agent: &mut Agent) {
    let kept = session_store(agent).all().await.expect("the store reads");
    assert_eq!(kept, Vec::new());
}

#[when(expr = "the admin changes the admin password from {string} to {string} from {word}")]
async fn changes(agent: &mut Agent, current: String, new: String, from: String) {
    let current = Password::parse(current).expect("a password");
    let changed = agent
        .sign_in
        .access()
        .change_password(address(&from), &current, new)
        .await;
    agent.sign_in.changed = Some(changed);
}

#[then(expr = "the admin password is {string}")]
async fn kept_is(agent: &mut Agent, password: String) {
    let kept = agent.sign_in.kept.clone().expect("where it is kept");
    let password = Password::parse(password).expect("a password");
    assert_eq!(kept.verify(&password).await, Ok(true));
}

#[then("no admin password is kept")]
fn none_kept(agent: &mut Agent) {
    let kept = agent.sign_in.kept.clone().expect("where it is kept");
    assert_eq!(kept.fingerprint().err(), Some(AdminPasswordError::NotSet));
}

#[then("the password is refused as shorter than 8 characters")]
fn refused_too_short(agent: &mut Agent) {
    let refused = match (&agent.sign_in.set, &agent.sign_in.changed) {
        (Some(set), None) => set.clone().err(),
        (None, Some(changed)) => match changed {
            Err(PasswordChangeRefused::New(refused)) => Some(refused.clone()),
            other => panic!("not refused for the new password: {other:?}"),
        },
        _ => panic!("no admin password was set or changed"),
    };
    assert_eq!(refused, Some(SetPasswordRefused::TooShort));
}

#[then("the change is refused as a wrong current password")]
fn change_refused_wrong(agent: &mut Agent) {
    assert_eq!(
        agent.sign_in.changed,
        Some(Err(PasswordChangeRefused::Current(
            SignInRefused::WrongPassword
        )))
    );
}

#[then(expr = "the change is refused, to be tried again in {int} second(s)")]
fn change_refused_wait(agent: &mut Agent, seconds: u64) {
    assert_eq!(
        agent.sign_in.changed,
        Some(Err(PasswordChangeRefused::Current(SignInRefused::Wait {
            seconds
        })))
    );
}

#[given(regex = r"^the admin has signed in(?: again)? from (\S+)$")]
async fn signed_in(agent: &mut Agent, from: String) {
    sign_in(agent, "demo-admin-pass", &from, false).await;
    assert_matches!(agent.sign_in.last, Some(Ok(_)), "{:?}", agent.sign_in.last);
}

#[given(regex = r"^the admin has signed in from (\S+), keeping signed in on this device$")]
async fn signed_in_kept(agent: &mut Agent, from: String) {
    sign_in(agent, "demo-admin-pass", &from, true).await;
    assert_matches!(agent.sign_in.last, Some(Ok(_)), "{:?}", agent.sign_in.last);
}

#[when(expr = "the admin signs in with {string} from {word}")]
async fn signs_in(agent: &mut Agent, password: String, from: String) {
    sign_in(agent, &password, &from, false).await;
}

#[when(
    expr = "the admin signs in with {string} from {word} {int} times, each once the wait has passed"
)]
async fn signs_in_times(agent: &mut Agent, password: String, from: String, times: u32) {
    for _ in 0..times {
        sign_in(agent, &password, &from, false).await;
        // A sign-in during the wait says what is left of it and counts
        // nothing: let that pass, and try again.
        if let Some(Err(SignInRefused::Wait { seconds })) = &agent.sign_in.last {
            let seconds = i64::try_from(*seconds).expect("a wait in range");
            agent.clock.advance(SignedDuration::from_secs(seconds));
            sign_in(agent, &password, &from, false).await;
        }
        refused_wrong(agent);
    }
}

#[when(expr = "{int} other addresses each sign in with a wrong password once")]
async fn many_others(agent: &mut Agent, count: u32) {
    let password = Password::parse("not-the-password".to_owned()).expect("a password");
    for index in 0..count {
        let address = IpAddr::V4(std::net::Ipv4Addr::from(0x0a00_0000 + index));
        let refused = agent
            .sign_in
            .access()
            .sign_in(address, &password, false, false)
            .await;
        assert_eq!(refused.err(), Some(SignInRefused::WrongPassword));
    }
}

#[when("the admin signs out")]
async fn signs_out(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    agent
        .sign_in
        .access()
        .sign_out(&token)
        .await
        .expect("the store forgets the session");
}

#[when("the admin signs out everywhere")]
async fn signs_out_everywhere(agent: &mut Agent) {
    agent
        .sign_in
        .access()
        .sign_out_everywhere()
        .await
        .expect("the store forgets every session");
}

#[then("the second session is not live")]
fn second_not_live(agent: &mut Agent) {
    let second = agent.sign_in.sessions.get(1).expect("a second session");
    assert!(agent.sign_in.access().peek(&second.token).is_none());
}

#[given("the session is used")]
#[when("the session is used")]
async fn used(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    agent.sign_in.used = agent.sign_in.access().session(&token).await;
}

#[when(regex = r"^the session is used every (hour|day) for (\d+) days$")]
async fn used_every(agent: &mut Agent, every: String, days: i64) {
    let token = agent.sign_in.session().clone();
    let (step, uses) = match every.as_str() {
        "hour" => (SignedDuration::from_hours(1), days * 24),
        _ => (SignedDuration::from_hours(24), days),
    };
    for _ in 0..uses {
        agent.clock.advance(step);
        agent.sign_in.used = agent.sign_in.access().session(&token).await;
    }
}

#[when("the page reads on its own")]
fn reads_on_its_own(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    agent.sign_in.read = agent.sign_in.access().peek(&token);
    assert!(agent.sign_in.read.is_some(), "the session is live");
}

#[then("the read didn't renew the session's cookie")]
fn read_didnt_renew(agent: &mut Agent) {
    let read = agent.sign_in.read.as_ref().expect("the page read");
    assert_eq!(read.renewed_cookie(), None);
}

#[then(regex = r"^the (?:last )?use renewed the session's cookie to last (\d+) days$")]
fn use_renewed(agent: &mut Agent, days: i64) {
    assert_renewed(agent, SignedDuration::from_hours(days * 24));
}

#[then(regex = r"^the use renewed the session's cookie to last (\d+) days and (\d+) hours$")]
fn use_renewed_hours(agent: &mut Agent, days: i64, hours: i64) {
    assert_renewed(agent, SignedDuration::from_hours(days * 24 + hours));
}

fn assert_renewed(agent: &Agent, lasts: SignedDuration) {
    let used = agent.sign_in.used.as_ref().expect("the session was used");
    assert_eq!(used.renewed_cookie(), Some(CookieLife::Lasts(lasts)));
}

#[then("the use didn't renew the session's cookie")]
fn use_didnt_renew(agent: &mut Agent) {
    let used = agent.sign_in.used.as_ref().expect("the session was used");
    assert_eq!(used.renewed_cookie(), None);
}

/// The last sign-in, which started a session.
fn last_signed_in(agent: &Agent) -> &SignedIn {
    match &agent.sign_in.last {
        Some(Ok(signed_in)) => signed_in,
        other => panic!("the admin signed in: {other:?}"),
    }
}

#[then(expr = "the sign-in's cookie lasts {int} days")]
fn sign_in_cookie_lasts(agent: &mut Agent, days: i64) {
    assert_eq!(
        last_signed_in(agent).cookie,
        CookieLife::Lasts(SignedDuration::from_hours(days * 24))
    );
}

#[then("the sign-in's cookie lasts until the browser closes")]
fn sign_in_cookie_until_browser_closes(agent: &mut Agent) {
    assert_eq!(last_signed_in(agent).cookie, CookieLife::UntilBrowserCloses);
}

#[then("the session is said to have no idle limit")]
fn said_no_idle_limit(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    let live = agent.sign_in.access().peek(&token);
    assert_eq!(live.expect("a live session").idle_limit(), None);
}

#[then(regex = r"^the session is said to end after (\d+) hours unused$")]
fn said_idle_limit(agent: &mut Agent, hours: i64) {
    let token = agent.sign_in.session().clone();
    let live = agent.sign_in.access().peek(&token);
    assert_eq!(
        live.expect("a live session").idle_limit(),
        Some(SignedDuration::from_hours(hours))
    );
}

#[then("the sign-in form doesn't offer Keep me signed in on this device")]
fn keep_not_offered(agent: &mut Agent) {
    assert!(!agent.sign_in.access().offers_keep_signed_in());
}

/// The session length a choice of **Stay signed in for** names, as Settings
/// shows it.
fn session_length(choice: &str) -> SessionLength {
    match choice {
        "Until I close the browser" => SessionLength::UntilBrowserCloses,
        "7 days" => SessionLength::SevenDays,
        "30 days" => SessionLength::ThirtyDays,
        "90 days" => SessionLength::NinetyDays,
        "1 year" => SessionLength::OneYear,
        "Until I sign out" => SessionLength::UntilSignOut,
        other => panic!("no choice of Stay signed in for is {other:?}"),
    }
}

#[given(expr = "Stay signed in for is {string}")]
#[when(expr = "Stay signed in for is set to {string}")]
async fn stay_signed_in_for(agent: &mut Agent, choice: String) {
    agent
        .sign_in
        .access()
        .set_session_length(session_length(&choice))
        .await
        .expect("the store keeps the session length");
}

#[when("the page reads on its own every hour for 12 hours")]
fn read_on_its_own_hourly(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    for hour in 1..12 {
        agent.clock.advance(SignedDuration::from_hours(1));
        assert!(
            agent.sign_in.access().peek(&token).is_some(),
            "live at hour {hour}"
        );
    }
    agent.clock.advance(SignedDuration::from_hours(1));
}

#[when(regex = r"^(\d+) hours and (\d+) minutes pass$")]
fn hours_and_minutes_pass(agent: &mut Agent, hours: i64, minutes: i64) {
    agent
        .clock
        .advance(SignedDuration::from_hours(hours) + SignedDuration::from_mins(minutes));
}

#[when(regex = r"^(\d+) days, (\d+) hours and (\d+) minutes pass$")]
fn days_hours_and_minutes_pass(agent: &mut Agent, days: i64, hours: i64, minutes: i64) {
    agent.clock.advance(
        SignedDuration::from_hours(days * 24 + hours) + SignedDuration::from_mins(minutes),
    );
}

#[when(regex = r"^(\d+) (day|days|hour|hours|minute|minutes|second|seconds) pass(?:es)?$")]
fn time_passes(agent: &mut Agent, count: i64, unit: String) {
    let by = match unit.trim_end_matches('s') {
        "day" => SignedDuration::from_hours(count * 24),
        "hour" => SignedDuration::from_hours(count),
        "minute" => SignedDuration::from_mins(count),
        _ => SignedDuration::from_secs(count),
    };
    agent.clock.advance(by);
}

#[then("the admin is signed in")]
fn is_signed_in(agent: &mut Agent) {
    assert_matches!(agent.sign_in.last, Some(Ok(_)), "{:?}", agent.sign_in.last);
}

#[then("the sign-in is refused as a wrong password")]
fn refused_wrong(agent: &mut Agent) {
    assert_eq!(
        agent
            .sign_in
            .last
            .as_ref()
            .and_then(|last| last.as_ref().err()),
        Some(&SignInRefused::WrongPassword)
    );
}

#[then(expr = "the sign-in is refused, to be tried again in {int} second(s)")]
fn refused_wait(agent: &mut Agent, seconds: u64) {
    assert_eq!(
        agent
            .sign_in
            .last
            .as_ref()
            .and_then(|last| last.as_ref().err()),
        Some(&SignInRefused::Wait { seconds })
    );
}

/// Looks at the session as a check does, which is no use of it.
#[then("the session is live")]
fn is_live(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    assert!(agent.sign_in.access().peek(&token).is_some());
}

#[then("the session is not live")]
fn is_not_live(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    assert!(agent.sign_in.access().peek(&token).is_none());
}

#[then("no session is live")]
fn none_live(agent: &mut Agent) {
    assert!(agent.sign_in.sessions.is_empty());
}

#[then("the session accepts its own CSRF token")]
async fn accepts_own(agent: &mut Agent) {
    let first = agent.sign_in.sessions.first().expect("a session");
    let session = agent
        .sign_in
        .access()
        .session(&first.token)
        .await
        .expect("live");
    assert!(session.accepts(&first.csrf));
}

#[then("the first session refuses the second session's CSRF token")]
async fn refuses_other(agent: &mut Agent) {
    let [first, second] = agent.sign_in.sessions.as_slice() else {
        panic!("two sessions, not {}", agent.sign_in.sessions.len());
    };
    let session = agent
        .sign_in
        .access()
        .session(&first.token)
        .await
        .expect("live");
    assert!(!session.accepts(&second.csrf));
}

#[then("the session refuses the CSRF token it had before the restart")]
async fn refuses_old_csrf(agent: &mut Agent) {
    let first = agent.sign_in.sessions.first().expect("a session");
    let session = agent
        .sign_in
        .access()
        .session(&first.token)
        .await
        .expect("live");
    assert!(!session.accepts(&first.csrf));
}

/// The page reads the session's CSRF token anew after a refusal, and its
/// next request carries that one.
#[then("the CSRF token the page reads from the session now is accepted")]
async fn accepts_read_csrf(agent: &mut Agent) {
    let first = agent.sign_in.sessions.first().expect("a session");
    let access = agent.sign_in.access();
    let read = access.session(&first.token).await.expect("live");
    let again = access.session(&first.token).await.expect("live");
    assert!(again.accepts(read.csrf()));
}

#[then(expr = "Ladderback remembers at most {int} addresses")]
fn remembers_at_most(agent: &mut Agent, most: usize) {
    assert!(agent.sign_in.access().remembered_addresses() <= most);
}
