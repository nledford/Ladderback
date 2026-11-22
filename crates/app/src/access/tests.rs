use std::assert_matches;
use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use domain::SignedDuration;
use domain::access::TOKEN_BYTES;
use domain::execution::next_run::RandomDraw;
use futures::FutureExt;

use super::*;
use crate::fakes::{FakeClock, InMemoryAdminPassword, InMemorySessionStore, SeededRandom};
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
    over(
        clock,
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(InMemorySessionStore::new()),
    )
}

/// An `Access` over the admin password `kept` and the session store
/// `sessions`: a second one over the same two is the daemon restarted.
fn over(
    clock: &Arc<FakeClock>,
    kept: Arc<dyn AdminPassword>,
    sessions: Arc<dyn SessionStore>,
) -> Access {
    Access::new(
        kept,
        Arc::new(SeededRandom::new(7)),
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        sessions,
    )
}

fn signed_in(access: &Access) -> SignedIn {
    access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap()
}

fn live(access: &Access, token: &Token) -> Option<LiveSession> {
    access.session(token).now_or_never().unwrap()
}

fn sign_out(access: &Access, token: &Token) -> Result<(), StoreError> {
    access.sign_out(token).now_or_never().unwrap()
}

#[test]
fn a_wait_with_part_of_a_second_left_is_said_in_whole_seconds_rounded_up() {
    let clock = clock();
    let access = access(&clock);
    let wrong = password("not-the-password");
    assert_eq!(
        access
            .sign_in(HOME, &wrong, false, false)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::WrongPassword
    );
    clock.advance(SignedDuration::from_millis(1));
    assert_eq!(
        access
            .sign_in(HOME, &wrong, false, false)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::Wait { seconds: 1 }
    );
    clock.advance(SignedDuration::from_millis(999));
    access
        .sign_in(HOME, &wrong, false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    clock.advance(SignedDuration::from_millis(1));
    assert_eq!(
        access
            .sign_in(HOME, &wrong, false, false)
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
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(!signed_in.token.matches(&signed_in.csrf));
    let session = live(&access, &signed_in.token).unwrap();
    assert!(session.accepts(&signed_in.csrf));
    assert_eq!(signed_in.ends_in, Some(THIRTY_DAYS));
}

#[test]
fn a_live_session_says_how_long_it_has_left_however_it_is_used() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    let hour = SignedDuration::from_hours(1);
    clock.advance(hour);
    let session = live(&access, &signed_in.token).unwrap();
    assert_eq!(session.ends_in(), Some(THIRTY_DAYS - hour));
}

#[test]
fn peeking_at_a_session_shows_it_without_using_it() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    let hour = SignedDuration::from_hours(1);
    clock.advance(hour);
    let peeked = access.peek(&signed_in.token).unwrap();
    assert!(peeked.accepts(&signed_in.csrf));
    assert_eq!(peeked.ends_in(), Some(THIRTY_DAYS - hour));
    clock.advance(domain::access::SESSION_IDLE - hour);
    assert!(
        access.peek(&signed_in.token).is_none(),
        "a peek an hour in kept the session past 12 hours unused"
    );
    assert!(live(&access, &signed_in.token).is_none());
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
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        Arc::new(InMemorySessionStore::new()),
    );
    let refused = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert_matches!(
        &refused,
        SignInRefused::Unavailable(text) if text.contains("Permission denied")
    );
}

#[test]
fn no_randomness_starts_no_session_and_keeps_the_wait() {
    let clock = clock();
    let access = Access::new(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(NoRandomness),
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        Arc::new(InMemorySessionStore::new()),
    );
    let refused = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert_eq!(refused, SignInRefused::Unavailable("no entropy".to_owned()));
    assert_eq!(access.remembered_addresses(), 1);
}

#[test]
fn each_address_that_failed_is_remembered_on_its_own() {
    let clock = clock();
    let access = access(&clock);
    assert_eq!(access.remembered_addresses(), 0);
    let wrong = password("not-the-password");
    for address in [HOME, IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11))] {
        access
            .sign_in(address, &wrong, false, false)
            .now_or_never()
            .unwrap()
            .unwrap_err();
    }
    assert_eq!(access.remembered_addresses(), 2);
}

#[test]
fn a_right_step_up_password_starts_the_addresss_waits_over_and_a_wrong_one_counts() {
    let clock = clock();
    let access = access(&clock);
    let wrong = password("not-the-password");
    let step_up = |attempt: &Password| access.step_up(HOME, attempt).now_or_never().unwrap();
    assert_eq!(step_up(&wrong), Err(PasswordRefused::WrongPassword));
    assert_eq!(access.remembered_addresses(), 1);
    assert_eq!(
        step_up(&password("demo-admin-pass")),
        Err(PasswordRefused::Wait { seconds: 1 })
    );
    clock.advance(SignedDuration::from_secs(1));
    assert_eq!(step_up(&password("demo-admin-pass")), Ok(StepUpConfirmed));
    assert_eq!(access.remembered_addresses(), 0);
    access
        .sign_in(HOME, &wrong, false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    clock.advance(SignedDuration::from_millis(1));
    assert_eq!(
        access
            .sign_in(HOME, &wrong, false, false)
            .now_or_never()
            .unwrap()
            .unwrap_err(),
        SignInRefused::Wait { seconds: 1 }
    );
}

#[test]
fn signing_out_ends_the_session_it_names() {
    let clock = clock();
    let access = access(&clock);
    let first = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    let second = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    sign_out(&access, &first.token).unwrap();
    assert!(live(&access, &first.token).is_none());
    assert!(live(&access, &second.token).is_some());
}

#[test]
fn a_new_admin_password_ends_the_sessions_started_before_it() {
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let access = over(
        &clock(),
        kept.clone(),
        Arc::new(InMemorySessionStore::new()),
    );
    let before = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    kept.set(&password("another-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(live(&access, &before.token).is_none());
    let after = access
        .sign_in(HOME, &password("another-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap();
    assert!(live(&access, &after.token).is_some());
}

#[test]
fn with_sign_in_off_an_admin_password_set_since_the_start_waits_for_the_next_one() {
    let kept = InMemoryAdminPassword::default();
    assert_eq!(PasswordWaiting::of(&kept), None, "nothing is kept yet");
    kept.set(&password("demo-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    assert_eq!(PasswordWaiting::of(&kept), Some(PasswordWaiting::Set));
}

#[test]
fn with_sign_in_off_an_admin_password_file_that_cant_be_read_is_said_so() {
    assert_eq!(
        PasswordWaiting::of(&Unreadable),
        Some(PasswordWaiting::Unreadable)
    );
}

/// The daemon started again over the admin password `kept` and the session
/// store `sessions`, its sessions restored. Its randomness differs from the
/// first daemon's, as the system's does.
fn restarted(
    clock: &Arc<FakeClock>,
    kept: Arc<dyn AdminPassword>,
    sessions: Arc<dyn SessionStore>,
) -> Access {
    let access = Access::new(
        kept,
        Arc::new(SeededRandom::new(1_000)),
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        sessions,
    );
    access.restore().now_or_never().unwrap().unwrap();
    access
}

#[test]
fn a_session_outlives_a_restart_with_a_new_csrf_token() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let before = signed_in(&over(&clock, kept.clone(), store.clone()));
    let after = restarted(&clock, kept, store);
    let session = live(&after, &before.token).expect("the session outlived the restart");
    assert!(!session.accepts(&before.csrf));
    assert!(!session.csrf().matches(&before.token));
    assert_eq!(session.ends_in(), Some(THIRTY_DAYS));
}

#[test]
fn the_store_never_holds_a_sessions_token_or_its_csrf_token() {
    let clock = clock();
    let store = Arc::new(InMemorySessionStore::new());
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let session = signed_in(&over(&clock, kept, store.clone()));
    let all = store.all().now_or_never().unwrap().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        all.first().map(|kept| kept.digest),
        Some(session.token.digest())
    );
    let shown = format!("{all:?}");
    assert!(!shown.contains(&session.token.to_text()), "{shown}");
    assert!(!shown.contains(&session.csrf.to_text()), "{shown}");
}

#[test]
fn a_session_signed_out_before_a_restart_stays_ended() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let first = over(&clock, kept.clone(), store.clone());
    let session = signed_in(&first);
    sign_out(&first, &session.token).unwrap();
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &session.token).is_none());
}

#[test]
fn a_sessions_last_use_outlives_a_restart() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let first = over(&clock, kept.clone(), store.clone());
    let session = signed_in(&first);
    clock.advance(SignedDuration::from_hours(11));
    assert!(live(&first, &session.token).is_some());
    let after = restarted(&clock, kept, store);
    clock.advance(SignedDuration::from_hours(11));
    assert!(live(&after, &session.token).is_some());
}

#[test]
fn a_session_that_ended_is_forgotten_by_the_store_too() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    clock.advance(domain::access::SESSION_IDLE);
    assert!(live(&access, &session.token).is_none());
    assert_eq!(store.all().now_or_never().unwrap().unwrap(), Vec::new());
}

#[test]
fn a_restart_forgets_the_sessions_that_ended_while_it_was_stopped() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let session = signed_in(&over(&clock, kept.clone(), store.clone()));
    clock.advance(domain::access::SESSION_IDLE);
    let after = restarted(&clock, kept, store.clone());
    assert!(live(&after, &session.token).is_none());
    assert_eq!(store.all().now_or_never().unwrap().unwrap(), Vec::new());
}

#[test]
fn a_new_admin_password_set_while_stopped_ends_the_sessions_from_before() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let session = signed_in(&over(&clock, kept.clone(), store.clone()));
    kept.set(&password("another-admin-pass"))
        .now_or_never()
        .unwrap()
        .unwrap();
    let after = restarted(&clock, kept, store.clone());
    assert!(live(&after, &session.token).is_none());
    assert_eq!(store.all().now_or_never().unwrap().unwrap(), Vec::new());
}

/// The in-memory session store, which answers every call with an error while
/// it is failing, and which holds `unreadable` sessions it can't read
/// besides the ones it keeps.
#[derive(Default)]
struct Flaky {
    kept: InMemorySessionStore,
    failing: std::sync::atomic::AtomicBool,
    unreadable: std::sync::atomic::AtomicUsize,
}

impl Flaky {
    fn failing(&self, failing: bool) {
        self.failing
            .store(failing, std::sync::atomic::Ordering::SeqCst);
    }

    fn answer(&self) -> Result<(), StoreError> {
        if self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(StoreError::Unavailable {
                detail: "disk I/O error".to_owned(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStore for Flaky {
    async fn add(
        &self,
        session: &domain::access::KeptSession,
        forget: &[domain::access::SessionDigest],
    ) -> Result<(), StoreError> {
        self.answer()?;
        self.kept.add(session, forget).await
    }

    async fn all(&self) -> Result<Vec<domain::access::KeptSession>, StoreError> {
        self.answer()?;
        self.kept.all().await
    }

    async fn forget_unreadable(&self) -> Result<usize, StoreError> {
        self.answer()?;
        Ok(self.unreadable.swap(0, std::sync::atomic::Ordering::SeqCst))
    }

    async fn remove(&self, digests: &[domain::access::SessionDigest]) -> Result<bool, StoreError> {
        self.answer()?;
        self.kept.remove(digests).await
    }

    async fn remove_all(&self) -> Result<bool, StoreError> {
        self.answer()?;
        self.kept.remove_all().await
    }

    async fn record_use(
        &self,
        digest: &domain::access::SessionDigest,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.answer()?;
        self.kept.record_use(digest, at).await
    }

    async fn length(&self) -> Result<Option<SessionLength>, StoreError> {
        self.answer()?;
        self.kept.length().await
    }

    async fn set_length(&self, length: SessionLength) -> Result<bool, StoreError> {
        self.answer()?;
        self.kept.set_length(length).await
    }
}

pub(super) use crate::test_log::capture_the_log;

/// What `access` has logged on this thread since this was last asked.
pub(super) fn logged() -> Vec<(log::Level, String)> {
    crate::test_log::logged_by(module_path!().trim_end_matches("::tests"))
}

/// Uses the session of `token` once the store is due to keep its last use
/// again, so the use writes to the store; whether it was live.
fn used_a_minute_later(access: &Access, clock: &FakeClock, token: &Token) -> bool {
    clock.advance(domain::access::SESSION_USE_KEPT_EVERY);
    live(access, token).is_some()
}

/// An access over a store that keeps a session of its own, then starts
/// failing; the capture forgets the lines logged before.
fn over_a_store_that_starts_failing() -> (Arc<FakeClock>, Arc<Flaky>, Access, SignedIn) {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    capture_the_log();
    store.failing(true);
    (clock, store, access, session)
}

#[test]
fn the_first_write_the_store_fails_without_saying_so_is_logged_as_a_warning_with_its_code() {
    let (clock, _store, access, session) = over_a_store_that_starts_failing();
    assert!(used_a_minute_later(&access, &clock, &session.token));
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_matches!(
        logged.first(),
        Some((log::Level::Warn, line)) if line.contains("store-unavailable")
    );
}

#[test]
fn a_store_that_keeps_failing_is_logged_once() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    let other = signed_in(&access);
    capture_the_log();
    store.failing(true);
    assert!(used_a_minute_later(&access, &clock, &session.token));
    assert_eq!(logged().len(), 1);
    assert!(used_a_minute_later(&access, &clock, &session.token));
    // The sign-out's failure is the caller's to say; the forgetting it
    // leaves for the next write fails there again, and is not logged again.
    assert!(sign_out(&access, &other.token).is_err());
    assert!(used_a_minute_later(&access, &clock, &session.token));
    assert_eq!(logged(), Vec::new());
}

#[test]
fn a_store_that_fails_again_after_a_write_succeeded_is_logged_again() {
    let (clock, store, access, session) = over_a_store_that_starts_failing();
    assert!(used_a_minute_later(&access, &clock, &session.token));
    assert_eq!(logged().len(), 1);
    store.failing(false);
    assert!(used_a_minute_later(&access, &clock, &session.token));
    assert_eq!(logged(), Vec::new());
    store.failing(true);
    assert!(used_a_minute_later(&access, &clock, &session.token));
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_matches!(logged.first(), Some((log::Level::Warn, _)));
}

#[test]
fn a_forgetting_the_store_fails_is_logged_and_a_sign_out_it_refuses_is_left_to_the_caller() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    let other = signed_in(&access);
    capture_the_log();
    store.failing(true);
    assert!(sign_out(&access, &other.token).is_err());
    assert_eq!(logged(), Vec::new(), "the caller says the sign-out failed");
    // No last use is due yet, so the use writes only the forgetting the
    // sign-out left for later.
    assert!(live(&access, &session.token).is_some());
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_matches!(
        logged.first(),
        Some((log::Level::Warn, line)) if line.contains("store-unavailable")
    );
}

#[test]
fn a_session_the_store_keeps_is_a_write_that_succeeded() {
    let (clock, store, access, session) = over_a_store_that_starts_failing();
    assert!(used_a_minute_later(&access, &clock, &session.token));
    assert_eq!(logged().len(), 1);
    store.failing(false);
    let other = signed_in(&access);
    store.failing(true);
    assert!(sign_out(&access, &other.token).is_err());
    assert!(live(&access, &session.token).is_some());
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

/// Signs in while the store fails: refused, and the session the sign-in
/// started, which the store never kept, is left for the next write to
/// forget.
fn refused_sign_in(access: &Access) {
    assert_matches!(
        access
            .sign_in(HOME, &password("demo-admin-pass"), false, false)
            .now_or_never()
            .unwrap(),
        Err(SignInRefused::Store(StoreError::Unavailable { .. }))
    );
}

#[test]
fn only_a_forgetting_that_forgets_a_session_kept_has_the_next_failure_logged() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    let other = signed_in(&access);
    capture_the_log();
    store.failing(true);
    refused_sign_in(&access);
    // No last use is due yet, so each use writes only the forgetting left
    // for later.
    assert!(live(&access, &session.token).is_some());
    assert_eq!(logged().len(), 1);
    store.failing(false);
    // The refused sign-in's session was never kept: forgetting it changes
    // nothing, as a full disk can let it.
    assert!(live(&access, &session.token).is_some());
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &session.token).is_some());
    assert_eq!(
        logged(),
        Vec::new(),
        "a write that changed nothing doesn't show the store writes again"
    );
    store.failing(false);
    sign_out(&access, &other.token).unwrap();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &session.token).is_some());
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[test]
fn only_a_sign_out_everywhere_that_forgets_a_session_kept_has_the_next_failure_logged() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let ended = signed_in(&access);
    sign_out(&access, &ended.token).unwrap();
    capture_the_log();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    assert_eq!(logged().len(), 1);
    store.failing(false);
    // The store keeps no session: forgetting every one changes nothing.
    sign_out_everywhere(&access).unwrap();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    assert_eq!(
        logged(),
        Vec::new(),
        "a write that changed nothing doesn't show the store writes again"
    );
    store.failing(false);
    let session = signed_in(&access);
    store.failing(true);
    assert!(sign_out(&access, &session.token).is_err());
    assert!(live(&access, &ended.token).is_none());
    assert_eq!(logged().len(), 1);
    store.failing(false);
    sign_out_everywhere(&access).unwrap();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[test]
fn only_a_session_length_that_changes_has_the_next_failure_logged() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    set_length(&access, SessionLength::SevenDays).unwrap();
    let ended = signed_in(&access);
    sign_out(&access, &ended.token).unwrap();
    capture_the_log();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    assert_eq!(logged().len(), 1);
    store.failing(false);
    // The store keeps this length already: keeping it again changes
    // nothing.
    set_length(&access, SessionLength::SevenDays).unwrap();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    assert_eq!(
        logged(),
        Vec::new(),
        "a write that changed nothing doesn't show the store writes again"
    );
    store.failing(false);
    set_length(&access, SessionLength::NinetyDays).unwrap();
    store.failing(true);
    refused_sign_in(&access);
    assert!(live(&access, &ended.token).is_none());
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[test]
fn the_failed_write_logged_holds_no_token_csrf_token_or_the_stores_own_words() {
    let (clock, _store, access, session) = over_a_store_that_starts_failing();
    assert!(used_a_minute_later(&access, &clock, &session.token));
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    let shown = format!("{logged:?}");
    assert!(!shown.contains(&session.token.to_text()), "{shown}");
    assert!(!shown.contains(&session.csrf.to_text()), "{shown}");
    assert!(!shown.contains("disk I/O error"), "{shown}");
}

#[test]
fn a_session_the_store_cant_keep_never_starts_says_the_store_and_counts_no_guess() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    access
        .sign_in(HOME, &password("wrong-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    clock.advance(SignedDuration::from_secs(1));
    store.failing(true);
    let refused = access
        .sign_in(HOME, &password("demo-admin-pass"), false, false)
        .now_or_never()
        .unwrap()
        .unwrap_err();
    assert_eq!(
        refused,
        SignInRefused::Store(StoreError::Unavailable {
            detail: "disk I/O error".to_owned()
        })
    );
    assert!(access.lock().sessions.is_empty());
    assert_eq!(access.remembered_addresses(), 0);
    store.failing(false);
    assert!(
        access
            .sign_in(HOME, &password("demo-admin-pass"), false, false)
            .now_or_never()
            .unwrap()
            .is_ok(),
        "the right password waits for nothing"
    );
}

#[test]
fn a_sign_out_the_store_cant_keep_ends_the_session_says_so_and_is_kept_at_the_next_write() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept.clone(), store.clone());
    let session = signed_in(&access);
    let other = signed_in(&access);
    store.failing(true);
    assert_matches!(
        sign_out(&access, &session.token),
        Err(StoreError::Unavailable { .. })
    );
    assert!(live(&access, &session.token).is_none());
    store.failing(false);
    assert!(live(&access, &other.token).is_some());
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &session.token).is_none());
    assert!(live(&after, &other.token).is_some());
}

fn sign_out_everywhere(access: &Access) -> Result<(), StoreError> {
    access.sign_out_everywhere().now_or_never().unwrap()
}

#[test]
fn signing_out_everywhere_ends_every_session_and_the_store_forgets_them_all() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let access = over(&clock, kept.clone(), store.clone());
    let first = signed_in(&access);
    let second = signed_in(&access);
    sign_out_everywhere(&access).unwrap();
    assert!(live(&access, &first.token).is_none());
    assert!(live(&access, &second.token).is_none());
    assert_eq!(kept_sessions(store.as_ref()), 0);
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &first.token).is_none());
    assert!(live(&after, &second.token).is_none());
}

#[test]
fn a_sign_out_everywhere_the_store_cant_keep_ends_every_session_says_so_and_is_kept_at_the_next_write()
 {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept.clone(), store.clone());
    let first = signed_in(&access);
    let second = signed_in(&access);
    store.failing(true);
    assert_matches!(
        sign_out_everywhere(&access),
        Err(StoreError::Unavailable { .. })
    );
    assert!(live(&access, &first.token).is_none());
    assert!(live(&access, &second.token).is_none());
    store.failing(false);
    let third = signed_in(&access);
    assert_eq!(kept_sessions(&store.kept), 1, "only the new session");
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &first.token).is_none());
    assert!(live(&after, &second.token).is_none());
    assert!(live(&after, &third.token).is_some());
}

#[test]
fn the_sessions_the_store_cant_read_are_forgotten_and_counted_and_the_rest_restored() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let first = over(&clock, kept.clone(), store.clone());
    let session = signed_in(&first);
    let other = signed_in(&first);
    store
        .unreadable
        .store(2, std::sync::atomic::Ordering::SeqCst);
    let access = Access::new(
        kept,
        Arc::new(SeededRandom::new(1_000)),
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        store.clone(),
    );
    assert_eq!(
        access.restore().now_or_never().unwrap().unwrap(),
        Restored {
            sessions: 2,
            unreadable: 2
        }
    );
    assert!(live(&access, &session.token).is_some());
    assert!(live(&access, &other.token).is_some());
}

#[test]
fn a_store_that_cant_be_read_restores_no_session_and_keeps_them() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let session = signed_in(&over(&clock, kept.clone(), store.clone()));
    store.failing(true);
    let access = over(&clock, kept, store.clone());
    assert_matches!(
        access.restore().now_or_never().unwrap(),
        Err(RestoreFailed::Store(StoreError::Unavailable { .. }))
    );
    assert!(live(&access, &session.token).is_none());
    store.failing(false);
    assert_eq!(store.all().now_or_never().unwrap().unwrap().len(), 1);
}

#[test]
fn sessions_kept_while_the_admin_password_cant_be_read_are_kept_not_restored() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    signed_in(&over(&clock, kept, store.clone()));
    let access = over(&clock, Arc::new(Unreadable), store.clone());
    assert_matches!(
        access.restore().now_or_never().unwrap(),
        Err(RestoreFailed::Password(AdminPasswordError::Unreadable(_)))
    );
    assert!(access.lock().sessions.is_empty());
    assert_eq!(store.all().now_or_never().unwrap().unwrap().len(), 1);
}

#[test]
fn sessions_kept_without_randomness_to_draw_csrf_tokens_are_not_restored() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let session = signed_in(&over(&clock, kept.clone(), store.clone()));
    let access = Access::new(
        kept,
        Arc::new(NoRandomness),
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        store,
    );
    assert_matches!(
        access.restore().now_or_never().unwrap(),
        Err(RestoreFailed::Random(_))
    );
    assert!(live(&access, &session.token).is_none());
}

/// The session length by default, written out: one read back from the code
/// would follow any change to it.
const THIRTY_DAYS: SignedDuration = SignedDuration::from_hours(30 * 24);

fn set_length(access: &Access, length: SessionLength) -> Result<(), StoreError> {
    access
        .set_session_length(length)
        .now_or_never()
        .expect("the in-memory store answers at once")
}

/// Uses the session of `token` every 11 hours for `days` days, so its idle
/// limit never ends it; whether it was live at each use.
fn used_every_eleven_hours(access: &Access, clock: &FakeClock, token: &Token, days: i64) -> bool {
    let end = clock.now() + SignedDuration::from_hours(days * 24);
    let mut live_throughout = true;
    while clock.now() + SignedDuration::from_hours(11) < end {
        clock.advance(SignedDuration::from_hours(11));
        live_throughout &= live(access, token).is_some();
    }
    clock.advance(end.duration_since(clock.now()));
    live_throughout && live(access, token).is_some()
}

#[test]
fn the_session_length_is_thirty_days_until_set_then_the_one_set_also_after_a_restart() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let before = over(&clock, kept.clone(), store.clone());
    assert_eq!(before.session_length(), SessionLength::ThirtyDays);
    set_length(&before, SessionLength::SevenDays).unwrap();
    assert_eq!(before.session_length(), SessionLength::SevenDays);
    let session = signed_in(&before);
    assert_eq!(session.ends_in, Some(SignedDuration::from_hours(7 * 24)));
    let after = restarted(&clock, kept, store);
    assert_eq!(after.session_length(), SessionLength::SevenDays);
    assert!(!used_every_eleven_hours(&after, &clock, &session.token, 7));
}

#[test]
fn shortening_the_session_length_ends_the_older_sessions_in_the_store_too_at_once() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let access = over(&clock, kept, store.clone());
    let older = signed_in(&access);
    assert!(used_every_eleven_hours(&access, &clock, &older.token, 2));
    let younger = signed_in(&access);
    assert!(used_every_eleven_hours(&access, &clock, &younger.token, 6));
    set_length(&access, SessionLength::SevenDays).unwrap();
    let kept: Vec<_> = store
        .all()
        .now_or_never()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|session| session.digest)
        .collect();
    assert_eq!(kept, vec![younger.token.digest()]);
    assert!(live(&access, &older.token).is_none());
    assert!(live(&access, &younger.token).is_some());
}

#[test]
fn lengthening_the_session_length_extends_the_sessions_already_open() {
    let clock = clock();
    let access = access(&clock);
    set_length(&access, SessionLength::SevenDays).unwrap();
    let session = signed_in(&access);
    assert!(used_every_eleven_hours(&access, &clock, &session.token, 6));
    set_length(&access, SessionLength::ThirtyDays).unwrap();
    assert!(used_every_eleven_hours(&access, &clock, &session.token, 2));
}

#[test]
fn a_session_with_no_end_after_its_sign_in_says_so() {
    for length in [
        SessionLength::UntilBrowserCloses,
        SessionLength::UntilSignOut,
    ] {
        let clock = clock();
        let access = access(&clock);
        set_length(&access, length).unwrap();
        let session = signed_in(&access);
        assert_eq!(session.ends_in, None, "{length:?}");
        let found = live(&access, &session.token).expect("a live session");
        assert_eq!(found.ends_in(), None, "{length:?}");
        let peeked = access.peek(&session.token).expect("a live session");
        assert_eq!(peeked.ends_in(), None, "{length:?}");
    }
}

#[test]
fn a_session_length_the_store_cant_keep_changes_nothing() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let access = over(&clock, kept, store.clone());
    let session = signed_in(&access);
    clock.advance(SignedDuration::from_hours(11));
    store.failing(true);
    assert_matches!(
        set_length(&access, SessionLength::UntilBrowserCloses),
        Err(StoreError::Unavailable { .. })
    );
    store.failing(false);
    assert_eq!(access.session_length(), SessionLength::ThirtyDays);
    assert_eq!(store.kept.length().now_or_never().unwrap().unwrap(), None);
    let found = live(&access, &session.token).expect("the session goes on");
    assert_eq!(
        found.ends_in(),
        Some(THIRTY_DAYS - SignedDuration::from_hours(11))
    );
}

#[test]
fn a_session_length_the_store_cant_read_restores_no_session() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let before = over(&clock, kept.clone(), store.clone());
    set_length(&before, SessionLength::SevenDays).unwrap();
    let session = signed_in(&before);
    let access = over(&clock, kept, store.clone());
    store.failing(true);
    assert_matches!(
        access.restore().now_or_never().unwrap(),
        Err(RestoreFailed::Store(_))
    );
    store.failing(false);
    assert!(live(&access, &session.token).is_none());
}

fn kept_sessions(store: &dyn SessionStore) -> usize {
    store.all().now_or_never().unwrap().unwrap().len()
}

#[test]
fn a_changed_admin_password_has_the_store_forget_every_session_it_keeps() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let access = over(&clock, kept.clone(), store.clone());
    let first = signed_in(&access);
    let second = signed_in(&access);
    assert_eq!(kept_sessions(store.as_ref()), 2);
    let changed = access
        .change_password(
            HOME,
            &password("demo-admin-pass"),
            "another-admin-pass".to_owned(),
        )
        .now_or_never()
        .unwrap();
    assert_eq!(changed, Ok(PasswordSet::SessionsForgotten));
    assert_eq!(kept_sessions(store.as_ref()), 0);
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &first.token).is_none());
    assert!(live(&after, &second.token).is_none());
}

#[test]
fn a_new_admin_password_has_the_store_forget_every_session_and_a_refused_one_none() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    signed_in(&over(&clock, kept.clone(), store.clone()));
    let set = |typed: &str| {
        set_admin_password(kept.as_ref(), store.as_ref(), typed.to_owned())
            .now_or_never()
            .unwrap()
    };
    assert_eq!(set("short"), Err(SetPasswordRefused::TooShort));
    assert_eq!(kept_sessions(store.as_ref()), 1, "a refused password");
    assert_eq!(
        set("another-admin-pass"),
        Ok(PasswordSet::SessionsForgotten)
    );
    assert_eq!(kept_sessions(store.as_ref()), 0);
}

#[test]
fn a_new_admin_password_the_store_cant_forget_the_sessions_of_is_kept_and_they_end_all_the_same() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Flaky::default());
    let session = signed_in(&over(&clock, kept.clone(), store.clone()));
    store.failing(true);
    let set = set_admin_password(
        kept.as_ref(),
        store.as_ref(),
        "another-admin-pass".to_owned(),
    )
    .now_or_never()
    .unwrap();
    assert_matches!(
        set,
        Ok(PasswordSet::SessionsLeft(StoreError::Unavailable { detail })) if detail == "disk I/O error"
    );
    assert!(
        kept.verify(&password("another-admin-pass"))
            .now_or_never()
            .unwrap()
            .unwrap()
    );
    store.failing(false);
    assert_eq!(kept_sessions(&store.kept), 1);
    let after = restarted(&clock, kept, store.clone());
    assert!(live(&after, &session.token).is_none());
    assert_eq!(kept_sessions(&store.kept), 0);
}

fn kept_signed_in(access: &Access) -> SignedIn {
    access
        .sign_in(HOME, &password("demo-admin-pass"), true, false)
        .now_or_never()
        .unwrap()
        .unwrap()
}

#[test]
fn a_session_signed_in_with_keep_me_signed_in_says_it_has_no_idle_limit_and_outlives_a_restart() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(InMemorySessionStore::new());
    let before = over(&clock, kept.clone(), store.clone());
    let signed_in_kept = kept_signed_in(&before);
    assert_eq!(signed_in_kept.idle_limit, None);
    assert_eq!(
        signed_in_kept.cookie,
        CookieLife::Lasts(THIRTY_DAYS),
        "a sign-in's cookie lasts as long as the session"
    );
    let unkept = signed_in(&before);
    assert_eq!(unkept.idle_limit, Some(SignedDuration::from_hours(12)));
    let after = restarted(&clock, kept, store);
    clock.advance(SignedDuration::from_hours(13));
    let session = live(&after, &signed_in_kept.token).expect("no idle limit after the restart");
    assert_eq!(session.idle_limit(), None);
    assert!(live(&after, &unkept.token).is_none());
}

#[test]
fn until_the_browser_closes_offers_no_keep_me_signed_in_and_its_cookie_lasts_until_the_browser_closes()
 {
    let clock = clock();
    let access = access(&clock);
    assert!(access.offers_keep_signed_in());
    set_length(&access, SessionLength::UntilBrowserCloses).unwrap();
    assert!(!access.offers_keep_signed_in());
    let signed_in = kept_signed_in(&access);
    assert_eq!(signed_in.idle_limit, Some(SignedDuration::from_hours(12)));
    assert_eq!(signed_in.cookie, CookieLife::UntilBrowserCloses);
}

#[test]
fn a_use_renews_the_cookie_once_a_day_and_a_peek_never_does() {
    let clock = clock();
    let access = access(&clock);
    let signed_in = kept_signed_in(&access);
    let day = SignedDuration::from_hours(24);
    clock.advance(day);
    let peeked = access.peek(&signed_in.token).unwrap();
    assert_eq!(peeked.renewed_cookie(), None);
    let used = live(&access, &signed_in.token).unwrap();
    assert_eq!(
        used.renewed_cookie(),
        Some(CookieLife::Lasts(THIRTY_DAYS - day))
    );
    clock.advance(SignedDuration::from_hours(1));
    let again = live(&access, &signed_in.token).unwrap();
    assert_eq!(again.renewed_cookie(), None);
}

#[test]
fn a_sessions_cookie_stays_secure_as_its_sign_in_set_it_also_after_a_restart() {
    for secure in [true, false] {
        let clock = clock();
        let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
        let store = Arc::new(InMemorySessionStore::new());
        let first = over(&clock, kept.clone(), store.clone());
        let session = first
            .sign_in(HOME, &password("demo-admin-pass"), false, secure)
            .now_or_never()
            .unwrap()
            .unwrap();
        let secure_of = |live: Option<LiveSession>| live.map(|live| live.secure());
        assert_eq!(secure_of(live(&first, &session.token)), Some(secure));
        assert_eq!(secure_of(first.peek(&session.token)), Some(secure));
        let after = restarted(&clock, kept, store);
        assert_eq!(secure_of(live(&after, &session.token)), Some(secure));
    }
}

/// The in-memory session store, with one call, by its name, that fails, and
/// one held unanswered until let go; it notes the sessions each `add` was
/// told to forget.
#[derive(Default)]
struct Scripted {
    kept: InMemorySessionStore,
    failing: Mutex<Option<&'static str>>,
    held: Mutex<Option<&'static str>>,
    told_to_forget: Mutex<Vec<Vec<domain::access::SessionDigest>>>,
}

impl Scripted {
    fn fail(&self, call: Option<&'static str>) {
        *self.failing.lock().unwrap() = call;
    }

    fn hold(&self, call: Option<&'static str>) {
        *self.held.lock().unwrap() = call;
    }

    async fn answer(&self, call: &'static str) -> Result<(), StoreError> {
        futures::future::poll_fn(|_| {
            if *self.held.lock().unwrap() == Some(call) {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        })
        .await;
        if *self.failing.lock().unwrap() == Some(call) {
            return Err(StoreError::Unavailable {
                detail: "disk I/O error".to_owned(),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStore for Scripted {
    async fn add(
        &self,
        session: &domain::access::KeptSession,
        forget: &[domain::access::SessionDigest],
    ) -> Result<(), StoreError> {
        self.answer("add").await?;
        self.told_to_forget.lock().unwrap().push(forget.to_vec());
        self.kept.add(session, forget).await
    }

    async fn all(&self) -> Result<Vec<domain::access::KeptSession>, StoreError> {
        self.answer("all").await?;
        self.kept.all().await
    }

    async fn forget_unreadable(&self) -> Result<usize, StoreError> {
        self.answer("forget_unreadable").await?;
        self.kept.forget_unreadable().await
    }

    async fn remove(&self, digests: &[domain::access::SessionDigest]) -> Result<bool, StoreError> {
        self.answer("remove").await?;
        self.kept.remove(digests).await
    }

    async fn remove_all(&self) -> Result<bool, StoreError> {
        self.answer("remove_all").await?;
        self.kept.remove_all().await
    }

    async fn record_use(
        &self,
        digest: &domain::access::SessionDigest,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.answer("record_use").await?;
        self.kept.record_use(digest, at).await
    }

    async fn length(&self) -> Result<Option<SessionLength>, StoreError> {
        self.answer("length").await?;
        self.kept.length().await
    }

    async fn set_length(&self, length: SessionLength) -> Result<bool, StoreError> {
        self.answer("set_length").await?;
        self.kept.set_length(length).await
    }
}

fn kept_digests(store: &dyn SessionStore) -> Vec<domain::access::SessionDigest> {
    let mut digests: Vec<_> = store
        .all()
        .now_or_never()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|session| session.digest)
        .collect();
    digests.sort_by_key(|digest| *digest.as_bytes());
    digests
}

fn sorted(mut digests: Vec<domain::access::SessionDigest>) -> Vec<domain::access::SessionDigest> {
    digests.sort_by_key(|digest| *digest.as_bytes());
    digests
}

#[test]
fn a_write_dropped_before_it_returns_leaves_the_sessions_it_forgets_to_the_next_write() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Scripted::default());
    let access = over(&clock, kept.clone(), store.clone());
    let first = signed_in(&access);
    let second = signed_in(&access);
    store.hold(Some("remove"));
    assert!(
        access.sign_out(&first.token).now_or_never().is_none(),
        "the sign-out waits on the store, and is dropped"
    );
    store.hold(None);
    let third = signed_in(&access);
    assert_eq!(
        kept_digests(&store.kept),
        sorted(vec![second.token.digest(), third.token.digest()])
    );
    store.hold(Some("remove_all"));
    assert!(
        access.sign_out_everywhere().now_or_never().is_none(),
        "Sign out everywhere waits on the store, and is dropped"
    );
    store.hold(None);
    let fourth = signed_in(&access);
    assert_eq!(kept_digests(&store.kept), vec![fourth.token.digest()]);
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &second.token).is_none());
    assert!(live(&after, &third.token).is_none());
    assert!(live(&after, &fourth.token).is_some());
}

#[test]
fn a_write_that_returns_tells_the_store_of_each_forgotten_session_once() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Scripted::default());
    let access = over(&clock, kept, store.clone());
    let first = signed_in(&access);
    store.hold(Some("remove"));
    assert!(
        access.sign_out(&first.token).now_or_never().is_none(),
        "the sign-out waits on the store, and is dropped"
    );
    store.hold(None);
    signed_in(&access);
    signed_in(&access);
    assert_eq!(
        *store.told_to_forget.lock().unwrap(),
        vec![vec![], vec![first.token.digest()], vec![]]
    );
}

#[test]
fn sign_out_everywhere_waits_for_a_sign_in_under_way_and_the_store_keeps_neither_session() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Scripted::default());
    let access = over(&clock, kept.clone(), store.clone());
    let first = signed_in(&access);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let attempt = password("demo-admin-pass");
    store.hold(Some("add"));
    let mut signing_in = Box::pin(access.sign_in(HOME, &attempt, false, false));
    assert!(
        signing_in.as_mut().poll(&mut cx).is_pending(),
        "the sign-in waits on the store"
    );
    let mut everywhere = Box::pin(access.sign_out_everywhere());
    let ended_at_once = everywhere.as_mut().poll(&mut cx).is_ready();
    store.hold(None);
    let second = signing_in.now_or_never().unwrap().unwrap();
    if !ended_at_once {
        everywhere.now_or_never().unwrap().unwrap();
    }
    assert_eq!(kept_digests(&store.kept), Vec::new());
    let after = restarted(&clock, kept, store);
    assert!(live(&after, &first.token).is_none());
    assert!(live(&after, &second.token).is_none());
}

/// The session length a daemon restarted over `store` measures sessions by
/// once its restore has failed.
fn length_after_a_failed_restore(
    clock: &Arc<FakeClock>,
    kept: Arc<dyn AdminPassword>,
    random: Arc<dyn Random>,
    store: Arc<dyn SessionStore>,
) -> SessionLength {
    let access = Access::new(
        kept,
        random,
        clock.clone(),
        Arc::new(crate::fakes::InMemoryApiTokenStore::new()),
        store,
    );
    assert!(access.restore().now_or_never().unwrap().is_err());
    access.session_length()
}

#[test]
fn a_failed_restore_measures_sessions_by_the_length_kept_or_the_shortest_when_it_cant_be_read() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let store = Arc::new(Scripted::default());
    let before = over(&clock, kept.clone(), store.clone());
    set_length(&before, SessionLength::SevenDays).unwrap();
    signed_in(&before);
    let seeded = || Arc::new(SeededRandom::new(1_000));
    for call in ["forget_unreadable", "all"] {
        store.fail(Some(call));
        assert_eq!(
            length_after_a_failed_restore(&clock, kept.clone(), seeded(), store.clone()),
            SessionLength::SevenDays,
            "{call}"
        );
    }
    store.fail(None);
    assert_eq!(
        length_after_a_failed_restore(&clock, Arc::new(Unreadable), seeded(), store.clone()),
        SessionLength::SevenDays,
        "the admin password"
    );
    assert_eq!(
        length_after_a_failed_restore(&clock, kept.clone(), Arc::new(NoRandomness), store.clone()),
        SessionLength::SevenDays,
        "randomness"
    );
    store.fail(Some("length"));
    assert_eq!(
        length_after_a_failed_restore(&clock, kept, seeded(), store),
        SessionLength::UntilBrowserCloses,
        "the session length"
    );
}
