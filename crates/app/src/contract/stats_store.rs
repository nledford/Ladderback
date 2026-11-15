//! The `StatsStore` contract (technical design §9 `repo_stats`, §16 decision
//! 104): the readings of repositories' stats, kept a year.

use std::sync::Arc;

use async_trait::async_trait;
use domain::maintenance::{RepositoryStats, StatsReading};
use domain::{RepositoryAlias, RepositoryId, SignedDuration, Timestamp};

use crate::ports::StatsStore;

/// Opens a stats store: a new, empty one, or the one before opened again on
/// the same place.
#[async_trait]
pub trait StatsHarness: Send + Sync {
    /// A store that keeps nothing yet.
    async fn empty(&self) -> Arc<dyn StatsStore>;
    /// The store [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn StatsStore>;
}

pub async fn run(harness: &dyn StatsHarness) {
    let Some(names) = Names::parsed() else {
        return;
    };
    a_new_store_keeps_no_reading(harness, &names).await;
    a_reading_is_kept_as_it_was_given(harness, &names).await;
    the_latest_is_the_one_taken_last(harness, &names).await;
    readings_taken_at_one_instant_are_in_the_order_kept(harness, &names).await;
    each_repository_keeps_its_own(harness, &names).await;
    a_reading_drops_those_taken_more_than_a_year_before_it(harness, &names).await;
    an_older_reading_drops_nothing_newer(harness, &names).await;
    the_largest_figures_are_kept_whole(harness, &names).await;
    a_figure_past_the_largest_is_refused(harness, &names).await;
    readings_outlive_the_daemon(harness, &names).await;
}

/// The suite's own repositories, parsed once: they are all valid.
struct Names {
    demo: RepositoryAlias,
    other: RepositoryAlias,
    repository: RepositoryId,
}

impl Names {
    fn parsed() -> Option<Self> {
        let demo = RepositoryAlias::parse("demo");
        let other = RepositoryAlias::parse("other");
        let repository = RepositoryId::parse(&format!("{:064x}", 7));
        assert!(
            demo.is_ok() && other.is_ok() && repository.is_ok(),
            "the suite's own names are valid: {demo:?}, {other:?}, {repository:?}"
        );
        Some(Self {
            demo: demo.ok()?,
            other: other.ok()?,
            repository: repository.ok()?,
        })
    }

    fn reading(
        &self,
        alias: &RepositoryAlias,
        taken_at: Timestamp,
        snapshots: u64,
    ) -> StatsReading {
        StatsReading {
            alias: alias.clone(),
            repository: Some(self.repository.clone()),
            taken_at,
            stats: RepositoryStats {
                snapshots,
                total_size: 3_000,
                stored_size: 1_200,
                raw_size: 2_400,
            },
        }
    }
}

const BASE: Timestamp = Timestamp::constant(1_800_000_000, 0);

fn at_ms(ms: i64) -> Timestamp {
    let at = BASE.checked_add(SignedDuration::from_millis(ms));
    assert!(at.is_ok(), "the suite's own instants are valid: {at:?}");
    at.unwrap_or(BASE)
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

async fn keep(store: &dyn StatsStore, reading: &StatsReading) {
    let kept = store.record(reading).await;
    assert!(kept.is_ok(), "the store keeps the reading: {kept:?}");
}

async fn latest(store: &dyn StatsStore, alias: &RepositoryAlias) -> Option<StatsReading> {
    let latest = store.latest(alias).await;
    assert!(latest.is_ok(), "the store answers: {latest:?}");
    latest.ok().flatten()
}

async fn history(store: &dyn StatsStore, alias: &RepositoryAlias) -> Vec<StatsReading> {
    let history = store.history(alias).await;
    assert!(history.is_ok(), "the store answers: {history:?}");
    history.unwrap_or_default()
}

async fn a_new_store_keeps_no_reading(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    assert_eq!(latest(store.as_ref(), &n.demo).await, None);
    assert_eq!(history(store.as_ref(), &n.demo).await, Vec::new());
}

async fn a_reading_is_kept_as_it_was_given(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let mut kept = n.reading(&n.demo, at_ms(123), 3);
    kept.repository = None;
    keep(store.as_ref(), &kept).await;
    assert_eq!(latest(store.as_ref(), &n.demo).await, Some(kept.clone()));
    assert_eq!(history(store.as_ref(), &n.demo).await, vec![kept]);
}

async fn the_latest_is_the_one_taken_last(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let later = n.reading(&n.demo, at_ms(2_000), 2);
    let earlier = n.reading(&n.demo, at_ms(1_000), 1);
    keep(store.as_ref(), &later).await;
    keep(store.as_ref(), &earlier).await;
    assert_eq!(
        latest(store.as_ref(), &n.demo).await,
        Some(later.clone()),
        "taken last, though kept first"
    );
    assert_eq!(history(store.as_ref(), &n.demo).await, vec![earlier, later]);
}

async fn readings_taken_at_one_instant_are_in_the_order_kept(
    harness: &dyn StatsHarness,
    n: &Names,
) {
    let store = harness.empty().await;
    let first = n.reading(&n.demo, at_ms(1_000), 1);
    let second = n.reading(&n.demo, at_ms(1_000), 2);
    keep(store.as_ref(), &first).await;
    keep(store.as_ref(), &second).await;
    assert_eq!(latest(store.as_ref(), &n.demo).await, Some(second.clone()));
    assert_eq!(history(store.as_ref(), &n.demo).await, vec![first, second]);
}

async fn each_repository_keeps_its_own(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let demo = n.reading(&n.demo, at_ms(1_000), 1);
    let other = n.reading(&n.other, at_ms(2_000), 2);
    keep(store.as_ref(), &demo).await;
    keep(store.as_ref(), &other).await;
    assert_eq!(latest(store.as_ref(), &n.demo).await, Some(demo.clone()));
    assert_eq!(history(store.as_ref(), &n.other).await, vec![other]);
    // A year past another repository's reading drops nothing of this one.
    let much_later = n.reading(&n.other, at_ms(1_000 + 400 * DAY_MS), 3);
    keep(store.as_ref(), &much_later).await;
    assert_eq!(history(store.as_ref(), &n.demo).await, vec![demo]);
}

async fn a_reading_drops_those_taken_more_than_a_year_before_it(
    harness: &dyn StatsHarness,
    n: &Names,
) {
    let store = harness.empty().await;
    let newest = at_ms(400 * DAY_MS);
    let too_old = n.reading(&n.demo, at_ms(400 * DAY_MS - 365 * DAY_MS - 1), 1);
    let a_year = n.reading(&n.demo, at_ms(400 * DAY_MS - 365 * DAY_MS), 2);
    let recent = n.reading(&n.demo, at_ms(400 * DAY_MS - 364 * DAY_MS), 3);
    for kept in [&too_old, &a_year, &recent] {
        keep(store.as_ref(), kept).await;
    }
    let new = n.reading(&n.demo, newest, 4);
    keep(store.as_ref(), &new).await;
    assert_eq!(
        history(store.as_ref(), &n.demo).await,
        vec![a_year, recent, new],
        "the reading a millisecond past the year is gone"
    );
}

async fn an_older_reading_drops_nothing_newer(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let newer = n.reading(&n.demo, at_ms(400 * DAY_MS), 2);
    keep(store.as_ref(), &newer).await;
    let older = n.reading(&n.demo, at_ms(0), 1);
    keep(store.as_ref(), &older).await;
    assert_eq!(history(store.as_ref(), &n.demo).await, vec![older, newer]);
}

async fn the_largest_figures_are_kept_whole(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let large = i64::MAX.unsigned_abs();
    let kept = StatsReading {
        stats: RepositoryStats {
            snapshots: large,
            total_size: large,
            stored_size: large,
            raw_size: large,
        },
        ..n.reading(&n.demo, at_ms(0), 0)
    };
    keep(store.as_ref(), &kept).await;
    assert_eq!(latest(store.as_ref(), &n.demo).await, Some(kept));
}

async fn a_figure_past_the_largest_is_refused(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let past = i64::MAX.unsigned_abs() + 1;
    for stats in [
        RepositoryStats {
            snapshots: past,
            ..RepositoryStats::default()
        },
        RepositoryStats {
            total_size: past,
            ..RepositoryStats::default()
        },
        RepositoryStats {
            stored_size: past,
            ..RepositoryStats::default()
        },
        RepositoryStats {
            raw_size: past,
            ..RepositoryStats::default()
        },
    ] {
        let refused = StatsReading {
            stats,
            ..n.reading(&n.demo, at_ms(0), 0)
        };
        assert!(
            store.record(&refused).await.is_err(),
            "{stats:?} is refused, never wrapped"
        );
    }
    assert_eq!(latest(store.as_ref(), &n.demo).await, None);
}

async fn readings_outlive_the_daemon(harness: &dyn StatsHarness, n: &Names) {
    let store = harness.empty().await;
    let kept = n.reading(&n.demo, at_ms(5), 3);
    keep(store.as_ref(), &kept).await;
    drop(store);
    let reopened = harness.reopened().await;
    assert_eq!(latest(reopened.as_ref(), &n.demo).await, Some(kept));
}
