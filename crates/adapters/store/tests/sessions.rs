//! The admin's sessions as the store keeps them (technical design §9
//! `sessions`, §10.1): a row that can't be read back is reported, and is
//! forgotten on its own, never with the sessions that can be read. The bad
//! rows are inserted with a connection of our own, the way damage or another
//! program would put them there.

mod support;

use app::ports::{SessionStore, StoreError};
use domain::access::{KeptSession, PasswordFingerprint, SessionDigest};
use support::*;

/// Rows no build of Ladderback writes: a digest of 3 bytes, a start that is
/// no time. A password or a time that is text the schema itself refuses
/// (STRICT, decision 165).
const UNREADABLE: [&str; 2] = [
    "INSERT INTO sessions (digest, password, started_at, last_used_at) \
     VALUES (x'010203', 1, 0, 0)",
    "INSERT INTO sessions (digest, password, started_at, last_used_at) \
     VALUES (x'0909090909090909090909090909090909090909090909090909090909090909', \
     1, 9223372036854775807, 0)",
];

#[tokio::test]
async fn only_the_sessions_that_cant_be_read_are_forgotten() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let good = KeptSession {
        digest: SessionDigest::from_bytes([7; 32]),
        password: PasswordFingerprint::new(u64::MAX),
        started: at(0),
        last_used: at(5),
        keep_signed_in: true,
        secure: true,
    };
    store
        .add(&good, &[])
        .await
        .expect("the store keeps a session");
    let mut raw = scratch.raw().await;
    for row in UNREADABLE {
        sqlx::query(row)
            .execute(&mut raw)
            .await
            .expect("the bad row is inserted");
    }
    let read = store.all().await;
    assert!(
        matches!(&read, Err(StoreError::Corrupt { detail }) if detail.contains("session")),
        "a row that can't be read is reported, never read as a default: {read:?}"
    );
    let forgot = store.forget_unreadable().await;
    assert!(matches!(forgot, Ok(2)), "{forgot:?}");
    assert_eq!(store.all().await.expect("the rest reads back"), vec![good]);
    let again = store.forget_unreadable().await;
    assert!(matches!(again, Ok(0)), "{again:?}");
}
