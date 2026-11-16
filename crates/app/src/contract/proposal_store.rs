//! The `ProposalStore` contract (§9 `proposals`; decision 89): the
//! proposals, pending and ended, kept between the daemon's starts.

use std::sync::Arc;

use async_trait::async_trait;
use domain::Timestamp;
use domain::access::TOKEN_BYTES;
use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::planning::{ConfigRevision, ProposalId, ProposalStatus, Proposer, Rationale};

use crate::ports::{Added, ProposalStore, StoredProposal};

/// Opens a proposal store: a new, empty one, or the one before opened
/// again on the same place.
#[async_trait]
pub trait ProposalHarness: Send + Sync {
    /// A store that keeps nothing yet.
    async fn empty(&self) -> Arc<dyn ProposalStore>;
    /// The store [`empty`](Self::empty) last gave, opened again, as after a
    /// restart of the daemon.
    async fn reopened(&self) -> Arc<dyn ProposalStore>;
}

pub async fn run(harness: &dyn ProposalHarness) {
    a_new_store_keeps_no_proposal(harness).await;
    a_proposal_is_kept_as_it_was_given(harness).await;
    proposals_are_listed_newest_first(harness).await;
    a_proposer_has_no_more_than_the_limit_pending(harness).await;
    a_proposal_past_its_expiry_holds_no_place(harness).await;
    the_oldest_ended_proposals_beyond_the_limit_are_forgotten(harness).await;
    a_status_changes_only_from_the_status_named(harness).await;
    times_are_kept_to_the_millisecond(harness).await;
    proposals_outlive_the_daemon(harness).await;
    racing_status_changes_make_one_change(harness).await;
    racing_adds_take_the_last_place_once(harness).await;
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn bytes(seed: u8) -> [u8; TOKEN_BYTES] {
    std::array::from_fn(|index| seed.wrapping_add(u8::try_from(index).unwrap_or(0)))
}

/// The proposer with the token from `seed`.
fn token(seed: u8) -> Option<Proposer> {
    let label = ApiTokenLabel::new(format!("demo-agent-{seed}"));
    assert!(label.is_ok(), "the suite's own label is valid");
    Some(Proposer::Token {
        id: ApiTokenId::from_bytes(&bytes(seed)),
        label: label.ok()?,
    })
}

/// A pending proposal from `seed` by `proposer`, made at `seconds`.
fn proposal(seed: u8, proposer: Proposer, seconds: i64) -> StoredProposal {
    StoredProposal {
        id: ProposalId::from_bytes(&bytes(seed)),
        proposer,
        base: ConfigRevision::of(format!("base-{seed}").as_bytes()),
        changes: format!("changes-{seed}"),
        rationale: Rationale::parse(format!("Back up folder {seed} every night."))
            .unwrap_or_default(),
        status: ProposalStatus::Pending,
        created_at: at(seconds),
        expires_at: at(seconds + 48 * 3600),
    }
}

async fn add(store: &dyn ProposalStore, proposal: &StoredProposal) {
    let added = store.add(proposal, 20, 200).await;
    assert!(
        matches!(added, Ok(Added::Kept)),
        "the store keeps the proposal: {added:?}"
    );
}

async fn all(store: &dyn ProposalStore) -> Vec<StoredProposal> {
    let all = store.all().await;
    assert!(all.is_ok(), "the store answers: {all:?}");
    all.unwrap_or_default()
}

async fn ids(store: &dyn ProposalStore) -> Vec<ProposalId> {
    all(store).await.into_iter().map(|kept| kept.id).collect()
}

async fn get(store: &dyn ProposalStore, id: &ProposalId) -> Option<StoredProposal> {
    let found = store.get(id).await;
    assert!(found.is_ok(), "the store answers: {found:?}");
    found.ok().flatten()
}

async fn change(
    store: &dyn ProposalStore,
    id: &ProposalId,
    from: ProposalStatus,
    to: ProposalStatus,
) -> bool {
    let changed = store.change_status(id, from, to).await;
    assert!(changed.is_ok(), "the store answers: {changed:?}");
    changed.unwrap_or(false)
}

async fn a_new_store_keeps_no_proposal(harness: &dyn ProposalHarness) {
    assert_eq!(all(harness.empty().await.as_ref()).await, Vec::new());
}

async fn a_proposal_is_kept_as_it_was_given(harness: &dyn ProposalHarness) {
    let Some(proposer) = token(1) else {
        return;
    };
    let by_token = proposal(1, proposer, 0);
    let by_no_token = proposal(2, Proposer::NoToken, 10);
    let store = harness.empty().await;
    add(store.as_ref(), &by_token).await;
    add(store.as_ref(), &by_no_token).await;
    assert_eq!(get(store.as_ref(), &by_token.id).await, Some(by_token));
    assert_eq!(
        get(store.as_ref(), &by_no_token.id).await,
        Some(by_no_token)
    );
    let never = ProposalId::from_bytes(&bytes(3));
    assert_eq!(get(store.as_ref(), &never).await, None);
}

async fn proposals_are_listed_newest_first(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let (middle, late, early) = (
        proposal(1, Proposer::NoToken, 20),
        proposal(2, Proposer::NoToken, 30),
        proposal(3, Proposer::NoToken, 10),
    );
    for one in [&middle, &late, &early] {
        add(store.as_ref(), one).await;
    }
    assert_eq!(ids(store.as_ref()).await, [late.id, middle.id, early.id]);
}

async fn a_proposer_has_no_more_than_the_limit_pending(harness: &dyn ProposalHarness) {
    let (Some(first), Some(second)) = (token(1), token(2)) else {
        return;
    };
    let store = harness.empty().await;
    let kept = [proposal(1, first.clone(), 0), proposal(2, first.clone(), 1)];
    for one in &kept {
        assert_eq!(store.add(one, 2, 200).await, Ok(Added::Kept));
    }
    let one_more = proposal(3, first.clone(), 2);
    assert_eq!(store.add(&one_more, 2, 200).await, Ok(Added::Full));
    assert_eq!(get(store.as_ref(), &one_more.id).await, None);

    // Another token, and the clients with none, have places of their own.
    let others = [
        proposal(4, second, 3),
        proposal(5, Proposer::NoToken, 4),
        proposal(6, Proposer::NoToken, 5),
    ];
    for one in &others {
        assert_eq!(store.add(one, 2, 200).await, Ok(Added::Kept));
    }
    let seventh = proposal(7, Proposer::NoToken, 6);
    assert_eq!(store.add(&seventh, 2, 200).await, Ok(Added::Full));

    // An ended proposal frees its place.
    let [withdrawn, _] = &kept;
    assert!(
        change(
            store.as_ref(),
            &withdrawn.id,
            ProposalStatus::Pending,
            ProposalStatus::Withdrawn
        )
        .await
    );
    assert_eq!(store.add(&one_more, 2, 200).await, Ok(Added::Kept));
}

/// A pending proposal holds its proposer's place until its expiry, though
/// nothing has marked it expired: of a new proposal, only those still
/// pending at the moment it is made count.
async fn a_proposal_past_its_expiry_holds_no_place(harness: &dyn ProposalHarness) {
    let Some(proposer) = token(1) else {
        return;
    };
    let store = harness.empty().await;
    let first = proposal(1, proposer.clone(), 0);
    assert_eq!(store.add(&first, 1, 200).await, Ok(Added::Kept));

    let a_second_before = proposal(2, proposer.clone(), 48 * 3600 - 1);
    assert_eq!(store.add(&a_second_before, 1, 200).await, Ok(Added::Full));

    let at_its_expiry = proposal(3, proposer, 48 * 3600);
    assert_eq!(store.add(&at_its_expiry, 1, 200).await, Ok(Added::Kept));
    // The store changes no status on its own: the first is still kept as
    // pending, for a read to find it expired.
    let kept = get(store.as_ref(), &first.id).await;
    assert_eq!(kept.map(|one| one.status), Some(ProposalStatus::Pending));
}

async fn the_oldest_ended_proposals_beyond_the_limit_are_forgotten(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let made: Vec<_> = (1..=4)
        .map(|seed| proposal(seed, Proposer::NoToken, i64::from(seed)))
        .collect();
    for one in &made {
        assert_eq!(store.add(one, 20, 2).await, Ok(Added::Kept));
    }
    // Pending ones are never forgotten.
    assert_eq!(all(store.as_ref()).await.len(), 4);
    for one in made.iter().take(3) {
        assert!(
            change(
                store.as_ref(),
                &one.id,
                ProposalStatus::Pending,
                ProposalStatus::Expired
            )
            .await
        );
    }
    let fifth = proposal(5, Proposer::NoToken, 5);
    assert_eq!(store.add(&fifth, 20, 2).await, Ok(Added::Kept));
    let ids = ids(store.as_ref()).await;
    let expected: Vec<_> = std::iter::once(&fifth)
        .chain(made.iter().rev().take(3))
        .map(|one| one.id.clone())
        .collect();
    assert_eq!(ids, expected, "the oldest ended one is forgotten");
}

async fn a_status_changes_only_from_the_status_named(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let one = proposal(1, Proposer::NoToken, 0);
    add(store.as_ref(), &one).await;
    assert!(
        !change(
            store.as_ref(),
            &one.id,
            ProposalStatus::Applying,
            ProposalStatus::Applied
        )
        .await
    );
    assert_eq!(
        get(store.as_ref(), &one.id).await.map(|kept| kept.status),
        Some(ProposalStatus::Pending)
    );
    assert!(
        change(
            store.as_ref(),
            &one.id,
            ProposalStatus::Pending,
            ProposalStatus::Stale
        )
        .await
    );
    assert_eq!(
        get(store.as_ref(), &one.id).await.map(|kept| kept.status),
        Some(ProposalStatus::Stale)
    );
    let never = ProposalId::from_bytes(&bytes(9));
    assert!(
        !change(
            store.as_ref(),
            &never,
            ProposalStatus::Pending,
            ProposalStatus::Withdrawn
        )
        .await
    );
}

async fn times_are_kept_to_the_millisecond(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let mut one = proposal(1, Proposer::NoToken, 0);
    one.created_at = Timestamp::constant(1_800_000_000, 123_456_789);
    one.expires_at = Timestamp::constant(1_800_172_800, 987_654_321);
    add(store.as_ref(), &one).await;
    let kept = get(store.as_ref(), &one.id).await;
    assert_eq!(
        kept.map(|kept| (kept.created_at, kept.expires_at)),
        Some((
            Timestamp::constant(1_800_000_000, 123_000_000),
            Timestamp::constant(1_800_172_800, 987_000_000),
        ))
    );
}

async fn proposals_outlive_the_daemon(harness: &dyn ProposalHarness) {
    let Some(proposer) = token(1) else {
        return;
    };
    let one = proposal(1, proposer, 0);
    let store = harness.empty().await;
    add(store.as_ref(), &one).await;
    assert!(
        change(
            store.as_ref(),
            &one.id,
            ProposalStatus::Pending,
            ProposalStatus::Withdrawn
        )
        .await
    );
    drop(store);
    let again = harness.reopened().await;
    let mut withdrawn = one.clone();
    withdrawn.status = ProposalStatus::Withdrawn;
    assert_eq!(get(again.as_ref(), &one.id).await, Some(withdrawn));
}

async fn racing_status_changes_make_one_change(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let one = proposal(1, Proposer::NoToken, 0);
    add(store.as_ref(), &one).await;
    let tries = (0..8).map(|n| {
        let store = Arc::clone(&store);
        let id = one.id.clone();
        let to = if n % 2 == 0 {
            ProposalStatus::Withdrawn
        } else {
            ProposalStatus::Applying
        };
        async move { store.change_status(&id, ProposalStatus::Pending, to).await }
    });
    let made = futures::future::join_all(tries).await;
    assert_eq!(
        made.iter().filter(|made| matches!(made, Ok(true))).count(),
        1,
        "exactly one change is made: {made:?}"
    );
}

async fn racing_adds_take_the_last_place_once(harness: &dyn ProposalHarness) {
    let store = harness.empty().await;
    let adds = (1..=8).map(|seed| {
        let store = Arc::clone(&store);
        let one = proposal(seed, Proposer::NoToken, i64::from(seed));
        async move { store.add(&one, 1, 200).await }
    });
    let added = futures::future::join_all(adds).await;
    assert_eq!(
        added
            .iter()
            .filter(|added| matches!(added, Ok(Added::Kept)))
            .count(),
        1,
        "exactly one is kept: {added:?}"
    );
    assert_eq!(all(store.as_ref()).await.len(), 1);
}
