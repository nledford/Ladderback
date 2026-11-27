//! The hub's registry as the store reads it back (§16, decision 120): a row
//! this version couldn't have written is reported as corrupt, naming the
//! column, never read as an agent or a token that pairs or proves more than
//! it was given.

mod support;

use app::ports::{AgentRegistry, StoreError};
use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, Hostname, KEY_BYTES, PublicKey, SMALL_ORDER_KEYS,
    TokenRecord, TokenUse,
};
use domain::{AgentId, Uuid};
use sqlx::SqliteConnection;
use support::*;

const AGENT_ID: &str = "00000000-0000-0000-0000-000000000001";

fn good_key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; KEY_BYTES]).expect("a key a pairing accepts")
}

async fn insert_agent(raw: &mut SqliteConnection, counter: &str) {
    sqlx::query(
        "INSERT INTO machines (id, pubkey, hostname, hostname_key, status, status_at, counter, \
         last_seen) VALUES (?, ?, 'Laptop', 'laptop', 'approved', ?, ?, ?)",
    )
    .bind(AGENT_ID)
    .bind([9_u8; KEY_BYTES].as_slice())
    .bind(ms(10))
    .bind(counter)
    .bind(ms(20))
    .execute(raw)
    .await
    .expect("the agent row is written");
}

async fn insert_token(raw: &mut SqliteConnection) {
    sqlx::query(
        "INSERT INTO pairing_tokens (pairing_key, hub_key, kind, made_at, expires_at, state) \
         VALUES (?, ?, 1, ?, ?, 'unused')",
    )
    .bind([1_u8; KEY_BYTES].as_slice())
    .bind([2_u8; KEY_BYTES].as_slice())
    .bind(ms(0))
    .bind(ms(900))
    .execute(raw)
    .await
    .expect("the token row is written");
}

fn corrupt<T: std::fmt::Debug>(outcome: Result<T, StoreError>, what: &str) -> String {
    match outcome {
        Err(StoreError::Corrupt { detail }) => detail,
        other => panic!("{what}: expected a corrupt row, got {other:?}"),
    }
}

#[tokio::test]
async fn the_rows_the_cases_start_from_are_good() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    insert_agent(&mut raw, "18446744073709551615").await;
    insert_token(&mut raw).await;

    assert_eq!(
        store.agents().await,
        Ok(vec![AgentRecord {
            agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
            key: good_key(9),
            hostname: Hostname::parse("Laptop").expect("a hostname"),
            status: AgentStatus::Approved { at: at(10) },
            counter: u64::MAX,
            last_seen: at(20),
            zone: None,
            version: None,
        }])
    );
    assert_eq!(
        store.unused_tokens(at(1)).await,
        Ok(vec![TokenRecord {
            pairing_key: good_key(1),
            hub_key: good_key(2),
            kind: BindingKind::TlsExporter,
            made_at: at(0),
            expires_at: at(900),
            state: TokenUse::Unused,
        }])
    );
}

async fn write_counter(raw: &mut SqliteConnection, counter: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO machines (id, pubkey, hostname, hostname_key, status, status_at, counter, \
         last_seen) VALUES (?, ?, 'laptop', 'laptop', 'pending', 0, ?, 0)",
    )
    .bind(AGENT_ID)
    .bind([9_u8; KEY_BYTES].as_slice())
    .bind(counter)
    .execute(raw)
    .await
    .map(drop)
}

#[tokio::test]
async fn a_counter_the_column_cant_hold_is_refused_when_written() {
    let scratch = Scratch::new();
    let _store = scratch.open().await;
    let mut raw = scratch.raw().await;
    for counter in [
        "7",
        "1844674407370955161",
        "-0000000000000000001",
        "0000000000000000000x",
    ] {
        let written = write_counter(&mut raw, counter).await;
        assert!(written.is_err(), "counter {counter:?} was kept");
    }
    let written = write_counter(&mut raw, "00000000000000000000").await;
    assert!(written.is_ok(), "counter 0 was refused: {written:?}");
}

/// A key no pairing accepts, as an SQL literal: the first small-order key.
fn small_order_literal() -> String {
    let hex: String = SMALL_ORDER_KEYS[0]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("X'{hex}'")
}

/// Each damage done to a good row: its name, the column the report must
/// name, and the statement that does it.
fn agent_cases() -> Vec<(&'static str, &'static str, String)> {
    vec![
        (
            "an ID in upper case",
            "id",
            "UPDATE machines SET id = '00000000-0000-0000-0000-00000000000A'".to_owned(),
        ),
        (
            "an ID that isn't a UUID",
            "id",
            "UPDATE machines SET id = 'laptop'".to_owned(),
        ),
        (
            "a key no pairing accepts",
            "pubkey",
            format!("UPDATE machines SET pubkey = {}", small_order_literal()),
        ),
        (
            "an empty hostname",
            "hostname",
            "UPDATE machines SET hostname = ''".to_owned(),
        ),
        (
            "a counter past the largest a u64 holds",
            "counter",
            "UPDATE machines SET counter = '18446744073709551616'".to_owned(),
        ),
        (
            "a zone not shaped like a zone name",
            "zone",
            "UPDATE machines SET zone = '../etc/localtime'".to_owned(),
        ),
    ]
}

fn token_cases() -> Vec<(&'static str, &'static str, String)> {
    vec![
        (
            "a binding kind this version doesn't know",
            "kind",
            "UPDATE pairing_tokens SET kind = 7".to_owned(),
        ),
        (
            "a kind no byte holds",
            "kind",
            "UPDATE pairing_tokens SET kind = 256".to_owned(),
        ),
        (
            "a hub key no pairing accepts",
            "hub_key",
            format!(
                "UPDATE pairing_tokens SET hub_key = {}",
                small_order_literal()
            ),
        ),
    ]
}

async fn damage(raw: &mut SqliteConnection, statement: String, name: &str) {
    let done = sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(&mut *raw)
        .await
        .unwrap_or_else(|error| panic!("{name}: the damage is done: {error}"));
    assert_eq!(done.rows_affected(), 1, "{name}: one row is damaged");
}

async fn start_over(raw: &mut SqliteConnection) {
    for statement in ["DELETE FROM machines", "DELETE FROM pairing_tokens"] {
        sqlx::query(statement)
            .execute(&mut *raw)
            .await
            .expect("the registry is emptied");
    }
    insert_agent(raw, "00000000000000000007").await;
    insert_token(raw).await;
}

#[tokio::test]
async fn a_registry_row_this_version_didnt_write_is_reported_as_corrupt() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    for (name, column, statement) in agent_cases() {
        start_over(&mut raw).await;
        damage(&mut raw, statement, name).await;
        let report = corrupt(store.agents().await, name);
        assert!(
            report.contains(&format!("column {column}:")),
            "{name}: the report names the column {column}: {report}"
        );
    }
    for (name, column, statement) in token_cases() {
        start_over(&mut raw).await;
        damage(&mut raw, statement, name).await;
        let report = corrupt(store.unused_tokens(at(1)).await, name);
        assert!(
            report.contains(&format!("column {column}:")),
            "{name}: the report names the column {column}: {report}"
        );
    }
}
