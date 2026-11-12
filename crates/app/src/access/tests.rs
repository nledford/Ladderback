use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use domain::SignedDuration;
use domain::access::TOKEN_BYTES;
use domain::execution::next_run::RandomDraw;
use futures::FutureExt;

use super::*;
use crate::fakes::{FakeClock, InMemoryAdminPassword, SeededRandom};
use crate::ports::{AdminPasswordError, RandomUnavailable};

const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

fn password(text: &str) -> Password {
    Password::parse(text.to_owned()).unwrap()
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

fn access(clock: &Arc<FakeClock>) -> Access {
    Access::new(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(SeededRandom::new(7)),
        clock.clone(),
    )
}

#[test]
fn a_wait_with_part_of_a_second_left_is_said_in_whole_seconds_rounded_up() {
    let clock = clock();
    let access = access(&clock);
    let wrong = password("not-the-password");
    assert_eq!(
        access
            .sign_in(HOME, &wrong)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::WrongPassword
    );
    clock.advance(SignedDuration::from_millis(1));
    assert_eq!(
        access
            .sign_in(HOME, &wrong)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::Wait { seconds: 1 }
    );
    clock.advance(SignedDuration::from_millis(999));
    access
        .sign_in(HOME, &wrong)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    clock.advance(SignedDuration::from_millis(1));
    assert_eq!(
        access
            .sign_in(HOME, &wrong)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::Wait { seconds: 2 }
    );
}

#[test]
fn a_sign_in_draws_a_session_token_and_a_csrf_token_that_differ() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(!signed_in.token.matches(&signed_in.csrf));
    let session = access.session(&signed_in.token).unwrap();
    assert!(session.accepts(&signed_in.csrf));
    assert_eq!(
        signed_in.ends_at,
        clock.now() + domain::access::SESSION_LIFETIME
    );
}

#[test]
fn a_live_session_says_how_long_it_has_left_however_it_is_used() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    let hour = SignedDuration::from_hours(1);
    clock.advance(hour);
    let session = access.session(&signed_in.token).unwrap();
    assert_eq!(session.ends_in(), domain::access::SESSION_LIFETIME - hour);
}

#[test]
fn peeking_at_a_session_shows_it_without_using_it() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    let hour = SignedDuration::from_hours(1);
    clock.advance(hour);
    let peeked = access.peek(&signed_in.token).unwrap();
    assert!(peeked.accepts(&signed_in.csrf));
    assert_eq!(peeked.ends_in(), domain::access::SESSION_LIFETIME - hour);
    clock.advance(domain::access::SESSION_IDLE - hour);
    assert!(
        access.peek(&signed_in.token).is_none(),
        "a peek an hour in kept the session past 12 hours unused"
    );
    assert!(access.session(&signed_in.token).is_none());
}

struct Unreadable;

#[async_trait]
impl AdminPassword for Unreadable {
    async fn verify(&self, _: &Password) -> Result<bool, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "Permission denied".to_owned(),
        ))
    }

    async fn set(&self, _: &Password) -> Result<(), AdminPasswordError> {
        Err(AdminPasswordError::Unwritable(
            "Permission denied".to_owned(),
        ))
    }

    fn fingerprint(&self) -> Result<domain::access::PasswordFingerprint, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "Permission denied".to_owned(),
        ))
    }
}

struct NoRandomness;

impl Random for NoRandomness {
    fn draw(&self) -> RandomDraw {
        panic!("signing in draws no jitter")
    }

    fn token(&self) -> Result<[u8; TOKEN_BYTES], RandomUnavailable> {
        Err(RandomUnavailable("no entropy".to_owned()))
    }
}

#[test]
fn a_password_that_cannot_be_checked_starts_no_session() {
    let access = Access::new(
        Arc::new(Unreadable),
        Arc::new(SeededRandom::new(7)),
        clock(),
    );
    let refused = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(&refused, SignInRefused::Unavailable(text) if text.contains("Permission denied")),
        "{refused:?}"
    );
}

#[test]
fn no_randomness_starts_no_session_and_keeps_the_wait() {
    let clock = clock();
    let access = Access::new(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(NoRandomness),
        clock.clone(),
    );
    let refused = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert_eq!(refused, SignInRefused::Unavailable("no entropy".to_owned()));
    assert_eq!(access.remembered_addresses(), 1);
}

#[test]
fn signing_out_ends_the_session_it_names() {
    let clock = clock();
    let access = access(&clock);
    let first = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    let second = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    access.sign_out(&first.token);
    assert!(access.session(&first.token).is_none());
    assert!(access.session(&second.token).is_some());
}

#[test]
fn a_new_admin_password_ends_the_sessions_started_before_it() {
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let access = Access::new(kept.clone(), Arc::new(SeededRandom::new(7)), clock());
    let before = access
        .sign_in(HOME, &password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    kept.set(&password("another-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(access.session(&before.token).is_none());
    let after = access
        .sign_in(HOME, &password("another-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(access.session(&after.token).is_some());
}
