use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::api_token::{ApiTokenId, ApiTokenLabel, Scope, Scopes};
use domain::access::{Password, PasswordFingerprint};

use super::*;
use crate::access::tests::{capture_the_log, logged};
use crate::fakes::{FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, SeededRandom};
use crate::ports::{AdminPassword, AdminPasswordError, ApiTokenStore};

const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ))
}

fn access_over(
    password: Arc<dyn AdminPassword>,
    tokens: Arc<dyn ApiTokenStore>,
    clock: &Arc<FakeClock>,
) -> Access {
    Access::new(
        password,
        Arc::new(SeededRandom::new(7)),
        clock.clone(),
        tokens,
        Arc::new(crate::fakes::InMemorySessionStore::new()),
    )
}

fn read() -> (ApiTokenLabel, Scopes) {
    (
        ApiTokenLabel::new("nightly-mcp".to_owned()).unwrap(),
        Scopes::new([Scope::Read]).unwrap(),
    )
}

async fn created(access: &Access) -> CreatedApiToken {
    let (label, scopes) = read();
    access.create_api_token(label, scopes).await.unwrap()
}

fn presented(created: &CreatedApiToken) -> PresentedApiToken {
    PresentedApiToken::parse(created.text.expose()).unwrap()
}

/// A store whose reads of one token meet before either answers, so two
/// checks reach their verdicts together.
struct Meeting {
    inner: InMemoryApiTokenStore,
    barrier: tokio::sync::Barrier,
}

#[async_trait]
impl ApiTokenStore for Meeting {
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError> {
        self.inner.add(token, at_most).await
    }
    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        self.barrier.wait().await;
        self.inner.get(id).await
    }
    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        self.inner.all().await
    }
    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError> {
        self.inner.revoke(id).await
    }
    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError> {
        self.inner.revoke_all_but(password).await
    }
    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError> {
        self.inner.record_use(id, at).await
    }
}

#[tokio::test]
async fn of_two_wrong_tokens_checked_at_once_only_one_counts_and_the_other_waits() {
    let clock = clock();
    let store = Arc::new(Meeting {
        inner: InMemoryApiTokenStore::new(),
        barrier: tokio::sync::Barrier::new(2),
    });
    let access = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        store,
        &clock,
    );
    let wrong = PresentedApiToken::parse(&format!("ladderback_aaaaaaaaaaaa_{}A", "A".repeat(42)));
    let (first, second) = tokio::join!(
        access.check_api_token(HOME, wrong.as_ref()),
        access.check_api_token(HOME, wrong.as_ref()),
    );
    let mut results = [first, second];
    results.sort_by_key(|result| format!("{result:?}"));
    assert_eq!(
        results,
        [
            Err(ApiTokenRefused::Invalid),
            Err(ApiTokenRefused::Wait { seconds: 1 })
        ]
    );
}

#[tokio::test]
async fn text_that_is_no_token_counts_as_a_failed_sign_in() {
    let clock = clock();
    let access = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(InMemoryApiTokenStore::new()),
        &clock,
    );
    let token = created(&access).await;
    assert_eq!(
        access.check_api_token(HOME, None).await,
        Err(ApiTokenRefused::Invalid)
    );
    assert_eq!(
        access.check_api_token(HOME, Some(&presented(&token))).await,
        Err(ApiTokenRefused::Wait { seconds: 1 })
    );
    clock.advance(domain::SignedDuration::from_secs(1));
    assert!(
        access
            .check_api_token(HOME, Some(&presented(&token)))
            .await
            .is_ok()
    );
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
    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        Err(AdminPasswordError::Unreadable(
            "Permission denied".to_owned(),
        ))
    }
}

#[tokio::test]
async fn a_password_that_cannot_be_read_accepts_no_token_and_counts_nothing() {
    let clock = clock();
    let store = Arc::new(InMemoryApiTokenStore::new());
    let readable = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        store.clone(),
        &clock,
    );
    let token = created(&readable).await;
    let access = access_over(Arc::new(Unreadable), store, &clock);
    for _ in 0..2 {
        assert!(matches!(
            access.check_api_token(HOME, Some(&presented(&token))).await,
            Err(ApiTokenRefused::Unavailable(text)) if text.contains("Permission denied")
        ));
    }
    assert!(!access.api_token_live(&token.token.id).await);
    let (label, scopes) = read();
    assert!(matches!(
        access.create_api_token(label, scopes).await,
        Err(ApiTokensRefused::Unavailable(text)) if text.contains("Permission denied")
    ));
    assert!(matches!(
        access.api_tokens().await,
        Err(ApiTokensRefused::Unavailable(_))
    ));
}

/// A store that can't be read or written.
struct Broken;

fn broken() -> StoreError {
    StoreError::Unavailable {
        detail: "disk I/O error".to_owned(),
    }
}

#[async_trait]
impl ApiTokenStore for Broken {
    async fn add(&self, _: &ApiToken, _: usize) -> Result<Added, StoreError> {
        Err(broken())
    }
    async fn get(&self, _: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        Err(broken())
    }
    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        Err(broken())
    }
    async fn revoke(&self, _: &ApiTokenId) -> Result<bool, StoreError> {
        Err(broken())
    }
    async fn revoke_all_but(&self, _: PasswordFingerprint) -> Result<(), StoreError> {
        Err(broken())
    }
    async fn record_use(&self, _: &ApiTokenId, _: Timestamp) -> Result<(), StoreError> {
        Err(broken())
    }
}

#[tokio::test]
async fn a_token_presented_during_the_wait_is_refused_unchecked() {
    let clock = clock();
    let access = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(Broken),
        &clock,
    );
    assert_eq!(
        access.check_api_token(HOME, None).await,
        Err(ApiTokenRefused::Invalid)
    );
    // Checked, it would meet the broken store; in the wait it is not looked
    // up at all.
    let token = PresentedApiToken::parse(&format!("ladderback_aaaaaaaaaaaa_{}A", "A".repeat(42)));
    assert_eq!(
        access.check_api_token(HOME, token.as_ref()).await,
        Err(ApiTokenRefused::Wait { seconds: 1 })
    );
}

#[tokio::test]
async fn a_store_that_fails_is_unavailable_and_counts_nothing() {
    let clock = clock();
    let access = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(Broken),
        &clock,
    );
    let wrong = PresentedApiToken::parse(&format!("ladderback_aaaaaaaaaaaa_{}A", "A".repeat(42)));
    for _ in 0..2 {
        assert!(matches!(
            access.check_api_token(HOME, wrong.as_ref()).await,
            Err(ApiTokenRefused::Unavailable(text)) if text.contains("disk I/O error")
        ));
    }
    let (label, scopes) = read();
    assert!(matches!(
        access.create_api_token(label, scopes).await,
        Err(ApiTokensRefused::Unavailable(text)) if text.contains("disk I/O error")
    ));
    assert!(matches!(
        access
            .revoke_api_token(&ApiTokenId::parse("aaaaaaaaaaaa").unwrap())
            .await,
        Err(ApiTokensRefused::Unavailable(_))
    ));
}

/// The in-memory store, but its uses can't be written while it is failing.
#[derive(Default)]
struct NoUses {
    kept: InMemoryApiTokenStore,
    working: std::sync::atomic::AtomicBool,
}

impl NoUses {
    fn working(&self, working: bool) {
        self.working
            .store(working, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl ApiTokenStore for NoUses {
    async fn add(&self, token: &ApiToken, at_most: usize) -> Result<Added, StoreError> {
        self.kept.add(token, at_most).await
    }
    async fn get(&self, id: &ApiTokenId) -> Result<Option<ApiToken>, StoreError> {
        self.kept.get(id).await
    }
    async fn all(&self) -> Result<Vec<ApiToken>, StoreError> {
        self.kept.all().await
    }
    async fn revoke(&self, id: &ApiTokenId) -> Result<bool, StoreError> {
        if self.working.load(std::sync::atomic::Ordering::SeqCst) {
            self.kept.revoke(id).await
        } else {
            Err(broken())
        }
    }
    async fn revoke_all_but(&self, password: PasswordFingerprint) -> Result<(), StoreError> {
        self.kept.revoke_all_but(password).await
    }
    async fn record_use(&self, id: &ApiTokenId, at: Timestamp) -> Result<(), StoreError> {
        if self.working.load(std::sync::atomic::Ordering::SeqCst) {
            self.kept.record_use(id, at).await
        } else {
            Err(broken())
        }
    }
}

/// An access over a store whose uses can't be written, holding one token;
/// the capture forgets the lines logged before.
async fn over_uses_that_cannot_be_written() -> (Arc<FakeClock>, Arc<NoUses>, Access, CreatedApiToken)
{
    let clock = clock();
    let store = Arc::new(NoUses::default());
    let access = access_over(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        store.clone(),
        &clock,
    );
    let token = created(&access).await;
    capture_the_log();
    (clock, store, access, token)
}

/// Checks `token` once its use is due to be written again.
async fn used_a_minute_later(access: &Access, clock: &FakeClock, token: &CreatedApiToken) {
    clock.advance(domain::access::api_token::LAST_USE_EVERY);
    access
        .check_api_token(HOME, Some(&presented(token)))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_use_that_cannot_be_written_still_lets_the_token_in() {
    let (_clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    let grant = access
        .check_api_token(HOME, Some(&presented(&token)))
        .await
        .unwrap();
    assert_eq!(grant.id, token.token.id);
    assert_eq!(grant.label.as_str(), "nightly-mcp");
}

#[tokio::test]
async fn the_first_use_the_store_cannot_write_is_logged_as_a_warning_with_its_code_alone() {
    let (clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    used_a_minute_later(&access, &clock, &token).await;
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        matches!(logged.first(), Some((log::Level::Warn, line)) if line.contains("store-unavailable")),
        "{logged:?}"
    );
    let shown = format!("{logged:?}");
    assert!(!shown.contains(token.text.expose()), "{shown}");
    assert!(!shown.contains("disk I/O error"), "{shown}");
}

#[tokio::test]
async fn uses_the_store_keeps_failing_to_write_are_logged_once() {
    let (clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged().len(), 1);
    used_a_minute_later(&access, &clock, &token).await;
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged(), Vec::new());
}

#[tokio::test]
async fn uses_that_fail_again_after_one_was_written_are_logged_again() {
    let (clock, store, access, token) = over_uses_that_cannot_be_written().await;
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged().len(), 1);
    store.working(true);
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged(), Vec::new());
    store.working(false);
    used_a_minute_later(&access, &clock, &token).await;
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(matches!(logged.first(), Some((log::Level::Warn, _))));
}

#[tokio::test]
async fn a_token_the_store_keeps_is_a_write_that_succeeded() {
    let (clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged().len(), 1);
    let other = created(&access).await;
    used_a_minute_later(&access, &clock, &other).await;
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[tokio::test]
async fn a_token_revoked_is_a_write_that_succeeded() {
    let (clock, store, access, token) = over_uses_that_cannot_be_written().await;
    store.working(true);
    let other = created(&access).await;
    store.working(false);
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged().len(), 1);
    store.working(true);
    assert!(access.revoke_api_token(&other.token.id).await.unwrap());
    store.working(false);
    used_a_minute_later(&access, &clock, &token).await;
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[tokio::test]
async fn listing_the_tokens_changes_nothing_so_is_no_write_that_succeeded() {
    let (clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged().len(), 1);
    assert_eq!(access.api_tokens().await.unwrap().len(), 1);
    used_a_minute_later(&access, &clock, &token).await;
    assert_eq!(logged(), Vec::new());
}

#[tokio::test]
async fn a_failure_the_caller_is_told_is_left_to_the_caller() {
    let (clock, _store, access, token) = over_uses_that_cannot_be_written().await;
    assert!(access.revoke_api_token(&token.token.id).await.is_err());
    assert_eq!(logged(), Vec::new(), "the caller says the revoke failed");
    used_a_minute_later(&access, &clock, &token).await;
    let logged = logged();
    assert_eq!(logged.len(), 1, "{logged:?}");
}

#[tokio::test]
async fn a_token_is_live_until_it_is_revoked_or_the_password_is_set_anew() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let access = access_over(kept.clone(), Arc::new(InMemoryApiTokenStore::new()), &clock);
    let revoked = created(&access).await;
    let reset = created(&access).await;
    assert!(access.api_token_live(&revoked.token.id).await);
    assert_eq!(access.revoke_api_token(&revoked.token.id).await, Ok(true));
    assert_eq!(access.revoke_api_token(&revoked.token.id).await, Ok(false));
    assert!(!access.api_token_live(&revoked.token.id).await);
    assert!(access.api_token_live(&reset.token.id).await);
    kept.set(&Password::parse("demo-admin-pass".to_owned()).unwrap())
        .await
        .unwrap();
    assert!(!access.api_token_live(&reset.token.id).await);
}

#[tokio::test]
async fn tokens_of_an_earlier_password_take_no_place_of_the_hundred() {
    let clock = clock();
    let kept = Arc::new(InMemoryAdminPassword::with("demo-admin-pass"));
    let access = access_over(kept.clone(), Arc::new(InMemoryApiTokenStore::new()), &clock);
    for _ in 0..MAX_API_TOKENS {
        created(&access).await;
    }
    kept.set(&Password::parse("demo-admin-pass".to_owned()).unwrap())
        .await
        .unwrap();
    let (label, scopes) = read();
    assert!(access.create_api_token(label, scopes).await.is_ok());
    assert_eq!(access.api_tokens().await.map(|tokens| tokens.len()), Ok(1));
}

#[test]
fn a_wait_is_said_in_whole_seconds_rounded_up() {
    use domain::SignedDuration;
    assert_eq!(seconds_left(SignedDuration::from_millis(1)), 1);
    assert_eq!(seconds_left(SignedDuration::from_millis(1000)), 1);
    assert_eq!(seconds_left(SignedDuration::from_millis(1001)), 2);
    assert_eq!(seconds_left(SignedDuration::ZERO), 0);
}
