//! Steps for the untagged scenarios of `features/security/sign_in.feature`:
//! the sign-in use case over an in-memory admin password, seeded randomness
//! and the world's fake clock.

use std::net::IpAddr;
use std::sync::Arc;

use app::access::{Access, SignInRefused, SignedIn};
use app::fakes::{InMemoryAdminPassword, SeededRandom};
use app::ports::AdminPassword;
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::access::{Password, Token};

use super::Agent;

/// What the sign-in scenarios keep between steps.
#[derive(Default)]
pub struct SignInWorld {
    access: Option<Access>,
    /// Where the admin password is kept, for `set-password` to replace it.
    kept: Option<Arc<InMemoryAdminPassword>>,
    /// What the last sign-in came to.
    last: Option<Result<SignedIn, SignInRefused>>,
    /// The sessions started so far, first first.
    sessions: Vec<SignedIn>,
}

impl std::fmt::Debug for SignInWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignInWorld")
            .field("last", &self.last)
            .finish_non_exhaustive()
    }
}

impl SignInWorld {
    fn access(&self) -> &Access {
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

async fn sign_in(agent: &mut Agent, password: &str, from: &str) {
    let password = Password::parse(password.to_owned()).expect("a password");
    let result = agent
        .sign_in
        .access()
        .sign_in(address(from), &password)
        .await;
    if let Ok(signed_in) = &result {
        agent.sign_in.sessions.push(SignedIn {
            token: signed_in.token.clone(),
            csrf: signed_in.csrf.clone(),
            ends_at: signed_in.ends_at,
        });
    }
    agent.sign_in.last = Some(result);
}

#[given(expr = "the admin password is {string}")]
fn admin_password(agent: &mut Agent, password: String) {
    let kept = Arc::new(InMemoryAdminPassword::with(&password));
    agent.sign_in.access = Some(Access::new(
        kept.clone(),
        Arc::new(SeededRandom::new(1)),
        agent.clock.clone(),
    ));
    agent.sign_in.kept = Some(kept);
}

#[when(expr = "the admin password is set to {string}")]
async fn set_password(agent: &mut Agent, password: String) {
    let password = Password::parse(password).expect("a password");
    let kept = agent
        .sign_in
        .kept
        .as_ref()
        .expect("an admin password is set");
    kept.set(&password).await.expect("the password is kept");
}

#[given(regex = r"^the admin has signed in(?: again)? from (\S+)$")]
async fn signed_in(agent: &mut Agent, from: String) {
    sign_in(agent, "demo-admin-pass", &from).await;
    assert!(
        matches!(agent.sign_in.last, Some(Ok(_))),
        "{:?}",
        agent.sign_in.last
    );
}

#[when(expr = "the admin signs in with {string} from {word}")]
async fn signs_in(agent: &mut Agent, password: String, from: String) {
    sign_in(agent, &password, &from).await;
}

#[when(
    expr = "the admin signs in with {string} from {word} {int} times, each once the wait has passed"
)]
async fn signs_in_times(agent: &mut Agent, password: String, from: String, times: u32) {
    for _ in 0..times {
        sign_in(agent, &password, &from).await;
        // A sign-in during the wait says what is left of it and counts
        // nothing: let that pass, and try again.
        if let Some(Err(SignInRefused::Wait { seconds })) = &agent.sign_in.last {
            let seconds = i64::try_from(*seconds).expect("a wait in range");
            agent.clock.advance(SignedDuration::from_secs(seconds));
            sign_in(agent, &password, &from).await;
        }
        refused_wrong(agent);
    }
}

#[when(expr = "{int} other addresses each sign in with a wrong password once")]
async fn many_others(agent: &mut Agent, count: u32) {
    let password = Password::parse("not-the-password".to_owned()).expect("a password");
    for index in 0..count {
        let address = IpAddr::V4(std::net::Ipv4Addr::from(0x0a00_0000 + index));
        let refused = agent.sign_in.access().sign_in(address, &password).await;
        assert_eq!(refused.err(), Some(SignInRefused::WrongPassword));
    }
}

#[when("the admin signs out")]
fn signs_out(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    agent.sign_in.access().sign_out(&token);
}

#[when("the session is used")]
fn used(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    let _ = agent.sign_in.access().session(&token);
}

#[when("the session is used every hour for 30 days")]
fn used_hourly(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    for _ in 0..(30 * 24) {
        agent.clock.advance(SignedDuration::from_hours(1));
        let _ = agent.sign_in.access().session(&token);
    }
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

#[when(regex = r"^(\d+) (hour|hours|minute|minutes|second|seconds) pass(?:es)?$")]
fn time_passes(agent: &mut Agent, count: i64, unit: String) {
    let by = match unit.trim_end_matches('s') {
        "hour" => SignedDuration::from_hours(count),
        "minute" => SignedDuration::from_mins(count),
        _ => SignedDuration::from_secs(count),
    };
    agent.clock.advance(by);
}

#[then("the admin is signed in")]
fn is_signed_in(agent: &mut Agent) {
    assert!(
        matches!(agent.sign_in.last, Some(Ok(_))),
        "{:?}",
        agent.sign_in.last
    );
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

#[then(expr = "the sign-in is refused, to be tried again in {int} seconds")]
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

#[then("the session is live")]
fn is_live(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    assert!(agent.sign_in.access().session(&token).is_some());
}

#[then("the session is not live")]
fn is_not_live(agent: &mut Agent) {
    let token = agent.sign_in.session().clone();
    assert!(agent.sign_in.access().session(&token).is_none());
}

#[then("no session is live")]
fn none_live(agent: &mut Agent) {
    assert!(agent.sign_in.sessions.is_empty());
}

#[then("the session accepts its own CSRF token")]
fn accepts_own(agent: &mut Agent) {
    let first = agent.sign_in.sessions.first().expect("a session");
    let session = agent.sign_in.access().session(&first.token).expect("live");
    assert!(session.accepts(&first.csrf));
}

#[then("the first session refuses the second session's CSRF token")]
fn refuses_other(agent: &mut Agent) {
    let [first, second] = agent.sign_in.sessions.as_slice() else {
        panic!("two sessions, not {}", agent.sign_in.sessions.len());
    };
    let session = agent.sign_in.access().session(&first.token).expect("live");
    assert!(!session.accepts(&second.csrf));
}

#[then(expr = "Ladderback remembers at most {int} addresses")]
fn remembers_at_most(agent: &mut Agent, most: usize) {
    assert!(agent.sign_in.access().remembered_addresses() <= most);
}
