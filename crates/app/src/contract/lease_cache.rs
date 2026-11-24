//! The `LeaseCache` contract (technical design §7.3, I1): a machine's
//! leases, epoch floors and each floor's proof read back as kept, across a
//! restart, each save replacing the last whole.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use domain::fleet::{KEY_BYTES, SIGNATURE_BYTES, Signature};
use domain::maintenance::{EpochFloor, FloorProof, HeldLease, HeldLeases, LeaseEpoch, Pause};
use domain::{RepositoryId, Timestamp};

use crate::ports::LeaseCache;

/// Opens a cache: a new, empty one, or the one before opened again on the
/// same place.
#[async_trait]
pub trait LeaseCacheHarness: Send + Sync {
    /// A cache that holds nothing yet.
    async fn empty(&self) -> Arc<dyn LeaseCache>;
    /// The cache [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn LeaseCache>;
}

pub async fn run(harness: &dyn LeaseCacheHarness) {
    an_empty_cache_holds_nothing(harness).await;
    i1_leases_and_floors_outlive_a_restart(harness).await;
    a_save_replaces_the_last_whole(harness).await;
    pauses_outlive_a_restart_and_a_save_of_the_leases(harness).await;
    i1_a_save_of_the_pauses_leaves_the_leases_as_kept(harness).await;
}

/// The value of `result`, after a failed assertion if it is an error.
fn ok<T, E: fmt::Debug>(result: Result<T, E>, what: &str) -> Option<T> {
    let error = result.as_ref().err();
    assert!(error.is_none(), "{what}: {error:?}");
    result.ok()
}

fn repository(digit: char) -> Option<RepositoryId> {
    ok(
        RepositoryId::parse(&digit.to_string().repeat(64)),
        "a repository ID",
    )
}

/// The epoch `number`; every epoch a test names is above 0.
fn epoch(number: u64) -> LeaseEpoch {
    LeaseEpoch::new(number).unwrap_or(LeaseEpoch::FIRST)
}

/// A lease on `photos` at epoch 2, a floor of it with its proof, and a
/// floor alone on `music`, to the millisecond and at the ends of each
/// number's range.
fn kept() -> Option<HeldLeases> {
    let floor = |recovery_number, generation, number| EpochFloor {
        recovery_number,
        generation,
        epoch: epoch(number),
    };
    let ends_at = ok(Timestamp::from_millisecond(1_800_000_000_123), "a time")?;
    let mut signature = [0; SIGNATURE_BYTES];
    signature[..KEY_BYTES].fill(7);
    let proof = FloorProof {
        expires_at: ok(Timestamp::from_second(1_800_604_800), "a time")?,
        signature: ok(Signature::from_bytes(signature), "a signature")?,
    };
    Some(HeldLeases::from_parts(
        [(
            repository('a')?,
            HeldLease {
                epoch: epoch(2),
                ends_at,
            },
        )],
        [
            (repository('a')?, floor(0, 1, 2)),
            (
                repository('b')?,
                floor(i64::MAX as u64, i64::MAX as u64, i64::MAX as u64),
            ),
        ],
        [(repository('a')?, proof)],
    ))
}

async fn an_empty_cache_holds_nothing(harness: &dyn LeaseCacheHarness) {
    let cache = harness.empty().await;
    assert_eq!(
        ok(cache.load().await, "the cache reads"),
        Some(HeldLeases::default())
    );
}

async fn i1_leases_and_floors_outlive_a_restart(harness: &dyn LeaseCacheHarness) {
    let cache = harness.empty().await;
    let kept = kept();
    assert!(kept.is_some(), "the leases to keep");
    let kept = kept.unwrap_or_default();
    ok(cache.save(&kept).await, "the cache keeps");
    let reopened = harness.reopened().await;
    assert_eq!(ok(reopened.load().await, "the cache reads"), Some(kept));
}

async fn a_save_replaces_the_last_whole(harness: &dyn LeaseCacheHarness) {
    let cache = harness.empty().await;
    ok(
        cache.save(&kept().unwrap_or_default()).await,
        "the cache keeps",
    );
    let fewer = HeldLeases::from_parts(
        [],
        repository('a').map(|repository| {
            (
                repository,
                EpochFloor {
                    recovery_number: 0,
                    generation: 1,
                    epoch: epoch(3),
                },
            )
        }),
        [],
    );
    ok(cache.save(&fewer).await, "the cache keeps");
    assert_eq!(ok(cache.load().await, "the cache reads"), Some(fewer));
}

/// A pause of `photos`, told, and one of `music`, not yet, to the
/// millisecond.
fn pauses() -> Option<Vec<(RepositoryId, Pause)>> {
    let at = |ms| ok(Timestamp::from_millisecond(ms), "a time");
    Some(vec![
        (
            repository('a')?,
            Pause {
                since: at(1_800_000_000_123)?,
                alerted: true,
            },
        ),
        (
            repository('b')?,
            Pause {
                since: at(1_800_000_000_456)?,
                alerted: false,
            },
        ),
    ])
}

async fn pauses_outlive_a_restart_and_a_save_of_the_leases(harness: &dyn LeaseCacheHarness) {
    let cache = harness.empty().await;
    let kept = kept().unwrap_or_default();
    let paused = kept.clone().with_pauses(pauses().unwrap_or_default());
    ok(cache.save(&kept).await, "the cache keeps");
    ok(
        cache.save_pauses(&paused).await,
        "the cache keeps the pauses",
    );
    let reopened = harness.reopened().await;
    assert_eq!(
        ok(reopened.load().await, "the cache reads"),
        Some(paused.clone())
    );
    // Taking grants saves the leases alone: the pauses stay.
    ok(
        reopened.save(&HeldLeases::default()).await,
        "the cache keeps",
    );
    assert_eq!(
        ok(reopened.load().await, "the cache reads"),
        Some(HeldLeases::default().with_pauses(pauses().unwrap_or_default()))
    );
}

async fn i1_a_save_of_the_pauses_leaves_the_leases_as_kept(harness: &dyn LeaseCacheHarness) {
    let cache = harness.empty().await;
    let kept = kept().unwrap_or_default();
    ok(cache.save(&kept).await, "the cache keeps");
    // A watch that read no lease saves its pauses: the lease taken
    // meanwhile stays.
    let watched = HeldLeases::default().with_pauses(pauses().unwrap_or_default());
    ok(
        cache.save_pauses(&watched).await,
        "the cache keeps the pauses",
    );
    assert_eq!(
        ok(cache.load().await, "the cache reads"),
        Some(kept.clone().with_pauses(pauses().unwrap_or_default()))
    );
    ok(
        cache.save_pauses(&HeldLeases::default()).await,
        "the cache keeps the pauses",
    );
    assert_eq!(ok(cache.load().await, "the cache reads"), Some(kept));
}
