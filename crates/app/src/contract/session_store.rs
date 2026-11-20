//! The `SessionStore` contract (technical design §10.1, §9 `sessions`;
//! decision 70): the admin's sessions, kept between the daemon's starts by
//! their tokens' digests.

use std::sync::Arc;

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::{KeptSession, PasswordFingerprint, SessionDigest, SessionLength};

use crate::ports::SessionStore;

/// Opens a session store: a new, empty one, or the one before opened again
/// on the same place.
#[async_trait]
pub trait SessionHarness: Send + Sync {
    /// A store that keeps nothing yet.
    async fn empty(&self) -> Arc<dyn SessionStore>;
    /// The store [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn SessionStore>;
}

pub async fn run(harness: &dyn SessionHarness) {
    a_new_store_keeps_no_session(harness).await;
    a_session_is_kept_as_it_was_given(harness).await;
    readable_sessions_are_never_forgotten_as_unreadable(harness).await;
    adding_a_session_forgets_the_ones_named(harness).await;
    removed_sessions_are_gone(harness).await;
    removing_all_forgets_every_session(harness).await;
    a_use_is_kept_and_never_brings_a_session_back(harness).await;
    times_are_kept_to_the_millisecond(harness).await;
    sessions_outlive_the_daemon(harness).await;
    a_new_store_keeps_no_session_length(harness).await;
    setting_the_length_says_whether_it_changed(harness).await;
    the_session_length_kept_last_is_kept_apart_from_the_sessions(harness).await;
    the_session_length_outlives_the_daemon(harness).await;
}

const PASSWORD: PasswordFingerprint = PasswordFingerprint::new(0x5eed_0000_0000_0001);

fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn digest(seed: u8) -> SessionDigest {
    SessionDigest::from_bytes(std::array::from_fn(|index| {
        seed.wrapping_add(u8::try_from(index).unwrap_or(0))
    }))
}

/// The session `seed`, started at `seconds` and not used since.
fn session(seed: u8, seconds: i64) -> KeptSession {
    KeptSession {
        digest: digest(seed),
        password: PASSWORD,
        started: at(seconds),
        last_used: at(seconds),
        keep_signed_in: false,
        secure: false,
    }
}

/// Every session kept, by digest, so the order the store gives them in
/// doesn't count.
async fn all(store: &dyn SessionStore) -> Vec<KeptSession> {
    let all = store.all().await;
    assert!(all.is_ok(), "the store answers: {all:?}");
    let mut all = all.unwrap_or_default();
    all.sort_by_key(|session| *session.digest.as_bytes());
    all
}

async fn add(store: &dyn SessionStore, session: &KeptSession, forget: &[SessionDigest]) {
    let added = store.add(session, forget).await;
    assert!(added.is_ok(), "the store keeps the session: {added:?}");
}

async fn a_new_store_keeps_no_session(harness: &dyn SessionHarness) {
    assert_eq!(all(harness.empty().await.as_ref()).await, Vec::new());
}

async fn a_session_is_kept_as_it_was_given(harness: &dyn SessionHarness) {
    let mut one = session(1, 0);
    one.last_used = at(5);
    one.password = PasswordFingerprint::new(u64::MAX);
    one.keep_signed_in = true;
    one.secure = true;
    let store = harness.empty().await;
    add(store.as_ref(), &one, &[]).await;
    add(store.as_ref(), &session(2, 3), &[]).await;
    assert_eq!(all(store.as_ref()).await, vec![one, session(2, 3)]);
}

async fn adding_a_session_forgets_the_ones_named(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    add(store.as_ref(), &session(1, 0), &[]).await;
    add(store.as_ref(), &session(2, 1), &[]).await;
    add(store.as_ref(), &session(3, 2), &[digest(1), digest(9)]).await;
    assert_eq!(
        all(store.as_ref()).await,
        vec![session(2, 1), session(3, 2)]
    );
}

async fn removed_sessions_are_gone(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    for seed in 1..=3 {
        add(store.as_ref(), &session(seed, 0), &[]).await;
    }
    let removed = store.remove(&[digest(1), digest(3), digest(9)]).await;
    assert_eq!(removed, Ok(true), "two of them were kept");
    let none_kept = store.remove(&[digest(1), digest(9)]).await;
    assert_eq!(none_kept, Ok(false), "none of them is kept");
    let none = store.remove(&[]).await;
    assert_eq!(none, Ok(false));
    assert_eq!(all(store.as_ref()).await, vec![session(2, 0)]);
}

async fn removing_all_forgets_every_session(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    for seed in 1..=3 {
        add(store.as_ref(), &session(seed, 0), &[]).await;
    }
    let removed = store.remove_all().await;
    assert_eq!(removed, Ok(true));
    assert_eq!(all(store.as_ref()).await, Vec::new());
    let none_kept = store.remove_all().await;
    assert_eq!(none_kept, Ok(false), "none is kept");
}

async fn a_use_is_kept_and_never_brings_a_session_back(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    let mut one = session(1, 0);
    add(store.as_ref(), &one, &[]).await;
    let used = store.record_use(&one.digest, at(90)).await;
    assert!(used.is_ok(), "{used:?}");
    one.last_used = at(90);
    assert_eq!(all(store.as_ref()).await, vec![one.clone()]);
    let removed = store.remove(&[one.digest]).await;
    assert!(removed.is_ok(), "{removed:?}");
    let late = store.record_use(&one.digest, at(120)).await;
    assert!(late.is_ok(), "{late:?}");
    assert_eq!(all(store.as_ref()).await, Vec::new());
}

async fn times_are_kept_to_the_millisecond(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    let mut one = session(1, 0);
    one.started = Timestamp::constant(1_800_000_000, 123_456_789);
    one.last_used = Timestamp::constant(1_800_000_030, 456_789_123);
    add(store.as_ref(), &one, &[]).await;
    one.started = Timestamp::constant(1_800_000_000, 123_000_000);
    one.last_used = Timestamp::constant(1_800_000_030, 456_000_000);
    assert_eq!(all(store.as_ref()).await, vec![one.clone()]);
    let used = store
        .record_use(&one.digest, Timestamp::constant(1_800_000_060, 987_654_321))
        .await;
    assert!(used.is_ok(), "{used:?}");
    one.last_used = Timestamp::constant(1_800_000_060, 987_000_000);
    assert_eq!(all(store.as_ref()).await, vec![one]);
}

async fn sessions_outlive_the_daemon(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    add(store.as_ref(), &session(1, 0), &[]).await;
    drop(store);
    let reopened = harness.reopened().await;
    assert_eq!(all(reopened.as_ref()).await, vec![session(1, 0)]);
}

async fn readable_sessions_are_never_forgotten_as_unreadable(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    let none = store.forget_unreadable().await;
    assert!(matches!(none, Ok(0)), "{none:?}");
    add(store.as_ref(), &session(1, 0), &[]).await;
    add(store.as_ref(), &session(2, 1), &[]).await;
    let forgot = store.forget_unreadable().await;
    assert!(matches!(forgot, Ok(0)), "{forgot:?}");
    assert_eq!(
        all(store.as_ref()).await,
        vec![session(1, 0), session(2, 1)]
    );
}

async fn length(store: &dyn SessionStore) -> Option<SessionLength> {
    let length = store.length().await;
    assert!(length.is_ok(), "the store answers: {length:?}");
    length.unwrap_or_default()
}

async fn set_length(store: &dyn SessionStore, length: SessionLength) {
    let set = store.set_length(length).await;
    assert!(set.is_ok(), "the store keeps the session length: {set:?}");
}

async fn a_new_store_keeps_no_session_length(harness: &dyn SessionHarness) {
    assert_eq!(length(harness.empty().await.as_ref()).await, None);
}

async fn setting_the_length_says_whether_it_changed(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    let first = store.set_length(SessionLength::SevenDays).await;
    assert_eq!(first, Ok(true), "none was kept");
    let same = store.set_length(SessionLength::SevenDays).await;
    assert_eq!(same, Ok(false), "the same one is kept already");
    let other = store.set_length(SessionLength::NinetyDays).await;
    assert_eq!(other, Ok(true));
    assert_eq!(
        length(store.as_ref()).await,
        Some(SessionLength::NinetyDays)
    );
}

async fn the_session_length_kept_last_is_kept_apart_from_the_sessions(
    harness: &dyn SessionHarness,
) {
    let store = harness.empty().await;
    for each in SessionLength::ALL {
        set_length(store.as_ref(), each).await;
        assert_eq!(length(store.as_ref()).await, Some(each));
    }
    add(store.as_ref(), &session(1, 0), &[]).await;
    let removed = store.remove_all().await;
    assert!(removed.is_ok(), "{removed:?}");
    assert_eq!(
        length(store.as_ref()).await,
        Some(SessionLength::UntilSignOut)
    );
    set_length(store.as_ref(), SessionLength::SevenDays).await;
    assert_eq!(all(store.as_ref()).await, Vec::new());
}

async fn the_session_length_outlives_the_daemon(harness: &dyn SessionHarness) {
    let store = harness.empty().await;
    set_length(store.as_ref(), SessionLength::NinetyDays).await;
    drop(store);
    let reopened = harness.reopened().await;
    assert_eq!(
        length(reopened.as_ref()).await,
        Some(SessionLength::NinetyDays)
    );
}
