//! The `ApiTokenStore` contract (technical design §10.1, §9 `api_tokens`;
//! decision 93): the API tokens, kept between the daemon's starts.

use std::sync::Arc;

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::api_token::{
    ApiToken, ApiTokenId, ApiTokenLabel, ApiTokenSecret, Scope, Scopes,
};
use domain::access::{PasswordFingerprint, TOKEN_BYTES};

use crate::ports::{Added, ApiTokenStore};

/// Opens an API token store: a new, empty one, or the one before opened
/// again on the same place.
#[async_trait]
pub trait ApiTokenHarness: Send + Sync {
    /// A store that keeps nothing yet.
    async fn empty(&self) -> Arc<dyn ApiTokenStore>;
    /// The store [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn ApiTokenStore>;
}

pub async fn run(harness: &dyn ApiTokenHarness) {
    a_new_store_keeps_no_token(harness).await;
    a_token_is_kept_as_it_was_given(harness).await;
    tokens_are_listed_oldest_first(harness).await;
    no_more_than_the_limit_are_kept(harness).await;
    a_revoked_token_is_gone(harness).await;
    tokens_of_another_password_are_revoked(harness).await;
    a_use_is_kept_and_never_brings_a_token_back(harness).await;
    times_are_kept_to_the_millisecond(harness).await;
    tokens_outlive_the_daemon(harness).await;
    racing_adds_take_the_last_place_once(harness).await;
}

const PASSWORD: PasswordFingerprint = PasswordFingerprint::new(0x5eed_0000_0000_0001);

fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn bytes(seed: u8) -> [u8; TOKEN_BYTES] {
    std::array::from_fn(|index| seed.wrapping_add(u8::try_from(index).unwrap_or(0)))
}

/// A token from `seed`, created at `seconds`, under `password`.
fn token(seed: u8, seconds: i64, password: PasswordFingerprint) -> Option<ApiToken> {
    let label = ApiTokenLabel::new(format!("demo-token-{seed}"));
    let scopes = Scopes::new([Scope::Read, Scope::Preview]);
    assert!(
        label.is_ok() && scopes.is_ok(),
        "the suite's own tokens are valid"
    );
    Some(ApiToken::issue(
        ApiTokenId::from_bytes(&bytes(seed)),
        label.ok()?,
        scopes.ok()?,
        &ApiTokenSecret::from_bytes(bytes(seed.wrapping_add(128))),
        password,
        at(seconds),
    ))
}

/// What a kept token is, to compare: everything it holds.
type Kept = (
    String,
    String,
    Vec<Scope>,
    [u8; 32],
    u64,
    Timestamp,
    Option<Timestamp>,
);

fn kept(token: &ApiToken) -> Kept {
    (
        token.id.as_str().to_owned(),
        token.label.as_str().to_owned(),
        token.scopes.iter().collect(),
        *token.digest.as_bytes(),
        token.password.value(),
        token.created_at,
        token.last_used_at,
    )
}

async fn add(store: &dyn ApiTokenStore, token: &ApiToken) {
    let added = store.add(token, 100).await;
    assert!(
        matches!(added, Ok(Added::Kept)),
        "the store keeps the token: {added:?}"
    );
}

async fn all(store: &dyn ApiTokenStore) -> Vec<Kept> {
    let all = store.all().await;
    assert!(all.is_ok(), "the store answers: {all:?}");
    all.unwrap_or_default().iter().map(kept).collect()
}

async fn get(store: &dyn ApiTokenStore, id: &ApiTokenId) -> Option<Kept> {
    let found = store.get(id).await;
    assert!(found.is_ok(), "the store answers: {found:?}");
    found.ok().flatten().as_ref().map(kept)
}

async fn a_new_store_keeps_no_token(harness: &dyn ApiTokenHarness) {
    assert_eq!(all(harness.empty().await.as_ref()).await, Vec::new());
}

async fn a_token_is_kept_as_it_was_given(harness: &dyn ApiTokenHarness) {
    let Some(mut one) = token(1, 0, PASSWORD) else {
        return;
    };
    one.last_used_at = Some(at(5));
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    assert_eq!(get(store.as_ref(), &one.id).await, Some(kept(&one)));
    let Some(other) = token(2, 0, PASSWORD) else {
        return;
    };
    assert_eq!(get(store.as_ref(), &other.id).await, None);
}

async fn tokens_are_listed_oldest_first(harness: &dyn ApiTokenHarness) {
    let (Some(late), Some(early), Some(middle)) = (
        token(1, 30, PASSWORD),
        token(2, 10, PASSWORD),
        token(3, 20, PASSWORD),
    ) else {
        return;
    };
    let store = harness.empty().await;
    for one in [&late, &early, &middle] {
        add(store.as_ref(), one).await;
    }
    assert_eq!(
        all(store.as_ref()).await,
        vec![kept(&early), kept(&middle), kept(&late)]
    );
}

async fn no_more_than_the_limit_are_kept(harness: &dyn ApiTokenHarness) {
    let (Some(first), Some(second), Some(third)) = (
        token(1, 0, PASSWORD),
        token(2, 1, PASSWORD),
        token(3, 2, PASSWORD),
    ) else {
        return;
    };
    let store = harness.empty().await;
    for one in [&first, &second] {
        let added = store.add(one, 2).await;
        assert!(matches!(added, Ok(Added::Kept)), "{added:?}");
    }
    let refused = store.add(&third, 2).await;
    assert!(matches!(refused, Ok(Added::Full)), "{refused:?}");
    assert_eq!(all(store.as_ref()).await, vec![kept(&first), kept(&second)]);
}

async fn a_revoked_token_is_gone(harness: &dyn ApiTokenHarness) {
    let (Some(one), Some(other)) = (token(1, 0, PASSWORD), token(2, 1, PASSWORD)) else {
        return;
    };
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    add(store.as_ref(), &other).await;
    let revoked = store.revoke(&one.id).await;
    assert!(matches!(revoked, Ok(true)), "{revoked:?}");
    let again = store.revoke(&one.id).await;
    assert!(matches!(again, Ok(false)), "{again:?}");
    assert_eq!(get(store.as_ref(), &one.id).await, None);
    assert_eq!(all(store.as_ref()).await, vec![kept(&other)]);
}

async fn tokens_of_another_password_are_revoked(harness: &dyn ApiTokenHarness) {
    let newer = PasswordFingerprint::new(u64::MAX);
    let (Some(old), Some(new)) = (token(1, 0, PASSWORD), token(2, 1, newer)) else {
        return;
    };
    let store = harness.empty().await;
    add(store.as_ref(), &old).await;
    add(store.as_ref(), &new).await;
    let revoked = store.revoke_all_but(newer).await;
    assert!(revoked.is_ok(), "{revoked:?}");
    assert_eq!(all(store.as_ref()).await, vec![kept(&new)]);
}

async fn a_use_is_kept_and_never_brings_a_token_back(harness: &dyn ApiTokenHarness) {
    let Some(mut one) = token(1, 0, PASSWORD) else {
        return;
    };
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    let used = store.record_use(&one.id, at(90)).await;
    assert!(used.is_ok(), "{used:?}");
    one.last_used_at = Some(at(90));
    assert_eq!(get(store.as_ref(), &one.id).await, Some(kept(&one)));
    let revoked = store.revoke(&one.id).await;
    assert!(matches!(revoked, Ok(true)), "{revoked:?}");
    let late = store.record_use(&one.id, at(120)).await;
    assert!(late.is_ok(), "{late:?}");
    assert_eq!(all(store.as_ref()).await, Vec::new());
}

async fn times_are_kept_to_the_millisecond(harness: &dyn ApiTokenHarness) {
    let Some(mut one) = token(1, 0, PASSWORD) else {
        return;
    };
    one.created_at = Timestamp::constant(1_800_000_000, 123_456_789);
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    let used = store
        .record_use(&one.id, Timestamp::constant(1_800_000_060, 987_654_321))
        .await;
    assert!(used.is_ok(), "{used:?}");
    one.created_at = Timestamp::constant(1_800_000_000, 123_000_000);
    one.last_used_at = Some(Timestamp::constant(1_800_000_060, 987_000_000));
    assert_eq!(get(store.as_ref(), &one.id).await, Some(kept(&one)));
}

async fn tokens_outlive_the_daemon(harness: &dyn ApiTokenHarness) {
    let Some(one) = token(1, 0, PASSWORD) else {
        return;
    };
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    drop(store);
    let reopened = harness.reopened().await;
    assert_eq!(all(reopened.as_ref()).await, vec![kept(&one)]);
}

async fn racing_adds_take_the_last_place_once(harness: &dyn ApiTokenHarness) {
    let Some(first) = token(1, 0, PASSWORD) else {
        return;
    };
    let store = harness.empty().await;
    add(store.as_ref(), &first).await;
    let racers: Vec<ApiToken> = (2..10)
        .filter_map(|seed| token(seed, 1, PASSWORD))
        .collect();
    let adds = racers.iter().map(|one| store.add(one, 2));
    let results = futures::future::join_all(adds).await;
    let kept_now = results
        .iter()
        .filter(|added| matches!(added, Ok(Added::Kept)))
        .count();
    assert_eq!(kept_now, 1, "{results:?}");
    assert_eq!(all(store.as_ref()).await.len(), 2);
}
