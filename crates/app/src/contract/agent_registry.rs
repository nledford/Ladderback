//! The `AgentRegistry` contract (§7.1; §16, decision 120, "Pairing",
//! "Pending approval" and "The handshake"): the hub's agents and pairing
//! tokens, kept between its starts, and a pairing or a proof decided and
//! applied in one step.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, Hostname, KEY_BYTES, PENDING_LIMIT, PairRefusal,
    PairRequest, Pairing, ProofRefusal, ProofRequest, PublicKey, TokenRecord, TokenUse,
};
use domain::{AgentId, Timestamp, Uuid};

use crate::ports::{Added, AgentRegistry};

/// Opens a registry: a new, empty one, or the one before opened again on
/// the same place.
#[async_trait]
pub trait AgentRegistryHarness: Send + Sync {
    /// A registry that holds nothing yet.
    async fn empty(&self) -> Arc<dyn AgentRegistry>;
    /// The registry [`empty`](Self::empty) last gave, opened again, as
    /// after a restart of the hub.
    async fn reopened(&self) -> Arc<dyn AgentRegistry>;
}

pub async fn run(harness: &dyn AgentRegistryHarness) {
    nothing_is_registered_at_first(harness).await;
    a_token_is_kept_as_it_was_given_while_unused(harness).await;
    unused_tokens_are_listed_oldest_first(harness).await;
    no_more_than_the_limit_are_unused(harness).await;
    a_revoked_token_pairs_no_one(harness).await;
    a_new_hub_key_voids_the_unused_tokens(harness).await;
    no_token_is_voided_while_an_agent_is_registered(harness).await;
    a_pairing_keeps_a_pending_agent_and_spends_the_token(harness).await;
    a_lost_answer_is_answered_again(harness).await;
    a_taken_hostname_leaves_the_token_unused(harness).await;
    a_twenty_first_pending_agent_is_refused(harness).await;
    the_same_identity_keeps_its_record_and_the_greater_counter(harness).await;
    another_key_under_a_registered_id_is_refused(harness).await;
    records_outlive_the_hub(harness).await;
    racing_pairings_on_one_token_pair_one(harness).await;
    a_proof_keeps_the_greater_counter_and_when_it_was_seen(harness).await;
    a_proof_of_another_id_key_or_kind_changes_nothing(harness).await;
}

/// The value of `result`, after a failed assertion if it is an error.
fn ok<T, E: fmt::Debug>(result: Result<T, E>, what: &str) -> Option<T> {
    let error = result.as_ref().err();
    assert!(error.is_none(), "{what}: {error:?}");
    result.ok()
}

/// `seconds` after a fixed moment, to the millisecond as kept.
fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn key(byte: u8) -> Option<PublicKey> {
    ok(PublicKey::from_bytes([byte; KEY_BYTES]), "a test key")
}

fn agent_id(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn hostname(text: &str) -> Option<Hostname> {
    ok(Hostname::parse(text), "a test hostname")
}

/// The token whose pairing key is `key(byte)`, made at `made` and lapsing
/// 15 minutes later.
fn token(byte: u8, made: i64) -> Option<TokenRecord> {
    Some(TokenRecord {
        pairing_key: key(byte)?,
        hub_key: key(200)?,
        kind: BindingKind::TlsExporter,
        made_at: at(made),
        expires_at: at(made + 900),
        state: TokenUse::Unused,
    })
}

/// Agent `n` asking to pair with token `byte` under `name`, with key
/// `key(100 + n)`.
fn request(n: u8, byte: u8, name: &str) -> Option<PairRequest> {
    Some(PairRequest {
        pairing_key: key(byte)?,
        agent_id: agent_id(u128::from(n)),
        agent_key: key(n.wrapping_add(100))?,
        hostname: hostname(name)?,
        counter: u64::from(n),
        kind: BindingKind::TlsExporter,
        hub_key: key(200)?,
    })
}

async fn add(registry: &dyn AgentRegistry, token: &TokenRecord) {
    let added = registry.add_token(token, usize::MAX, token.made_at).await;
    assert_eq!(ok(added, "adding a token"), Some(Added::Kept));
}

async fn unused(registry: &dyn AgentRegistry, now: Timestamp) -> Vec<TokenRecord> {
    ok(registry.unused_tokens(now).await, "listing the tokens").unwrap_or_default()
}

async fn agents(registry: &dyn AgentRegistry) -> Vec<AgentRecord> {
    ok(registry.agents().await, "listing the agents").unwrap_or_default()
}

async fn pair(
    registry: &dyn AgentRegistry,
    request: &PairRequest,
    now: Timestamp,
) -> Option<Result<Pairing, PairRefusal>> {
    ok(registry.pair(request, now).await, "pairing")
}

async fn nothing_is_registered_at_first(harness: &dyn AgentRegistryHarness) {
    let registry = harness.empty().await;
    assert!(unused(registry.as_ref(), at(0)).await.is_empty());
    assert!(agents(registry.as_ref()).await.is_empty());
}

async fn a_token_is_kept_as_it_was_given_while_unused(harness: &dyn AgentRegistryHarness) {
    let Some(mut token) = token(1, 0) else {
        return;
    };
    token.made_at = Timestamp::constant(1_800_000_000, 123_000_000);
    token.kind = BindingKind::None;
    let registry = harness.empty().await;
    add(registry.as_ref(), &token).await;
    assert_eq!(unused(registry.as_ref(), at(899)).await, vec![token]);
    assert!(
        unused(registry.as_ref(), at(900)).await.is_empty(),
        "a token lapses at its expiry"
    );
}

async fn unused_tokens_are_listed_oldest_first(harness: &dyn AgentRegistryHarness) {
    let (Some(later), Some(earlier)) = (token(2, 5), token(1, 0)) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &later).await;
    add(registry.as_ref(), &earlier).await;
    assert_eq!(
        unused(registry.as_ref(), at(10)).await,
        vec![earlier, later]
    );
}

async fn no_more_than_the_limit_are_unused(harness: &dyn AgentRegistryHarness) {
    let (Some(first), Some(second), Some(third)) = (token(1, 0), token(2, 0), token(3, 0)) else {
        return;
    };
    let registry = harness.empty().await;
    for one in [&first, &second] {
        let added = registry.add_token(one, 2, at(0)).await;
        assert_eq!(ok(added, "adding a token"), Some(Added::Kept));
    }
    let full = registry.add_token(&third, 2, at(0)).await;
    assert_eq!(ok(full, "adding a third token"), Some(Added::Full));
    assert_eq!(unused(registry.as_ref(), at(1)).await.len(), 2);
    let lapsed = registry.add_token(&third, 2, at(900)).await;
    assert_eq!(
        ok(lapsed, "adding once the others lapsed"),
        Some(Added::Kept),
        "lapsed tokens don't count"
    );
}

async fn a_revoked_token_pairs_no_one(harness: &dyn AgentRegistryHarness) {
    let (Some(token), Some(request)) = (token(1, 0), request(1, 1, "laptop")) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &token).await;
    let revoked = registry.revoke_token(token.id(), at(1)).await;
    assert_eq!(ok(revoked, "revoking"), Some(true));
    let again = registry.revoke_token(token.id(), at(1)).await;
    assert_eq!(ok(again, "revoking again"), Some(false));
    assert!(unused(registry.as_ref(), at(1)).await.is_empty());
    assert_eq!(
        pair(registry.as_ref(), &request, at(2)).await,
        Some(Err(PairRefusal::TokenRevoked))
    );
    assert!(agents(registry.as_ref()).await.is_empty());
}

async fn a_new_hub_key_voids_the_unused_tokens(harness: &dyn AgentRegistryHarness) {
    let (Some(first), Some(second), Some(lapsed), Some(revoked)) =
        (token(1, 0), token(2, 0), token(3, -1000), token(4, 0))
    else {
        return;
    };
    let registry = harness.empty().await;
    for one in [&first, &second, &lapsed, &revoked] {
        add(registry.as_ref(), one).await;
    }
    let revoking = registry.revoke_token(revoked.id(), at(0)).await;
    assert_eq!(ok(revoking, "revoking"), Some(true));
    let voided = registry.void_tokens_for_new_key().await;
    assert_eq!(ok(voided, "voiding"), Some(true));
    assert!(unused(registry.as_ref(), at(1)).await.is_empty());
    for (n, byte) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
        let Some(request) = request(n, byte, &format!("machine-{n}")) else {
            return;
        };
        assert_eq!(
            pair(registry.as_ref(), &request, at(2)).await,
            Some(Err(PairRefusal::TokenRevoked)),
            "token {byte}: a voided token pairs no one, though the request names its hub key"
        );
    }
    assert!(agents(registry.as_ref()).await.is_empty());
    let reopened = harness.reopened().await;
    assert!(unused(reopened.as_ref(), at(3)).await.is_empty());
}

async fn no_token_is_voided_while_an_agent_is_registered(harness: &dyn AgentRegistryHarness) {
    let (Some(spent), Some(unspent), Some(request)) =
        (token(1, 0), token(2, 0), request(1, 1, "laptop"))
    else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &spent).await;
    add(registry.as_ref(), &unspent).await;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    let before = agents(registry.as_ref()).await;
    let voided = registry.void_tokens_for_new_key().await;
    assert_eq!(ok(voided, "voiding"), Some(false));
    assert_eq!(unused(registry.as_ref(), at(11)).await, vec![unspent]);
    assert_eq!(agents(registry.as_ref()).await, before);
    assert_eq!(
        pair(registry.as_ref(), &request, at(12)).await,
        Some(Ok(Pairing::Repeated)),
        "the spent token still answers its own pairing"
    );
}

async fn a_pairing_keeps_a_pending_agent_and_spends_the_token(harness: &dyn AgentRegistryHarness) {
    let (Some(token), Some(first), Some(second)) = (
        token(1, 0),
        request(1, 1, "laptop"),
        request(2, 1, "desktop"),
    ) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &token).await;
    let expected = AgentRecord {
        agent_id: first.agent_id,
        key: first.agent_key,
        hostname: first.hostname.clone(),
        status: AgentStatus::Pending { since: at(10) },
        counter: first.counter,
        last_seen: at(10),
    };
    assert_eq!(
        pair(registry.as_ref(), &first, at(10)).await,
        Some(Ok(Pairing::New(expected.clone())))
    );
    assert_eq!(agents(registry.as_ref()).await, vec![expected]);
    assert!(unused(registry.as_ref(), at(11)).await.is_empty());
    assert_eq!(
        pair(registry.as_ref(), &second, at(11)).await,
        Some(Err(PairRefusal::TokenUsed))
    );
}

async fn a_lost_answer_is_answered_again(harness: &dyn AgentRegistryHarness) {
    let (Some(token), Some(request)) = (token(1, 0), request(1, 1, "laptop")) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &token).await;
    let first = pair(registry.as_ref(), &request, at(10)).await;
    assert!(matches!(first, Some(Ok(Pairing::New(_)))), "{first:?}");
    let before = agents(registry.as_ref()).await;
    assert_eq!(
        pair(registry.as_ref(), &request, at(2000)).await,
        Some(Ok(Pairing::Repeated))
    );
    assert_eq!(agents(registry.as_ref()).await, before, "nothing changed");
}

async fn a_taken_hostname_leaves_the_token_unused(harness: &dyn AgentRegistryHarness) {
    let (Some(first_token), Some(second_token)) = (token(1, 0), token(2, 0)) else {
        return;
    };
    let (Some(holder), Some(taker), Some(renamed)) = (
        request(1, 1, "Laptop"),
        request(2, 2, "laptop"),
        request(2, 2, "desktop"),
    ) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &first_token).await;
    add(registry.as_ref(), &second_token).await;
    let paired = pair(registry.as_ref(), &holder, at(10)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    assert_eq!(
        pair(registry.as_ref(), &taker, at(20)).await,
        Some(Err(PairRefusal::HostnameTaken {
            holder: holder.hostname.clone(),
            last_seen: at(10),
        }))
    );
    assert_eq!(unused(registry.as_ref(), at(21)).await, vec![second_token]);
    let paired = pair(registry.as_ref(), &renamed, at(30)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    assert_eq!(agents(registry.as_ref()).await.len(), 2);
}

async fn a_twenty_first_pending_agent_is_refused(harness: &dyn AgentRegistryHarness) {
    let registry = harness.empty().await;
    let Ok(waiting) = u8::try_from(PENDING_LIMIT) else {
        return;
    };
    for n in 1..=waiting {
        let (Some(token), Some(request)) = (token(n, 0), request(n, n, &format!("machine-{n}")))
        else {
            return;
        };
        add(registry.as_ref(), &token).await;
        let paired = pair(registry.as_ref(), &request, at(10)).await;
        assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    }
    let next = waiting + 1;
    let (Some(token), Some(request)) = (token(next, 0), request(next, next, "one-more")) else {
        return;
    };
    add(registry.as_ref(), &token).await;
    assert_eq!(
        pair(registry.as_ref(), &request, at(20)).await,
        Some(Err(PairRefusal::PendingLimit))
    );
    assert_eq!(unused(registry.as_ref(), at(21)).await, vec![token]);
}

async fn the_same_identity_keeps_its_record_and_the_greater_counter(
    harness: &dyn AgentRegistryHarness,
) {
    let (Some(first_token), Some(second_token), Some(third_token)) =
        (token(1, 0), token(2, 0), token(3, 0))
    else {
        return;
    };
    let Some(mut request) = request(1, 1, "laptop") else {
        return;
    };
    let registry = harness.empty().await;
    for one in [&first_token, &second_token, &third_token] {
        add(registry.as_ref(), one).await;
    }
    request.counter = u64::MAX - 1;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    request.pairing_key = second_token.pairing_key;
    request.counter = 3;
    let again = pair(registry.as_ref(), &request, at(20)).await;
    let Some(Ok(Pairing::Again(record))) = again else {
        assert!(again.is_none(), "paired again: {again:?}");
        return;
    };
    assert_eq!(
        record.counter,
        u64::MAX - 1,
        "a lower counter is never kept"
    );
    assert_eq!(record.status, AgentStatus::Pending { since: at(10) });
    assert_eq!(record.last_seen, at(20));
    assert_eq!(agents(registry.as_ref()).await, vec![record]);
    request.pairing_key = third_token.pairing_key;
    request.counter = u64::MAX;
    let highest = pair(registry.as_ref(), &request, at(30)).await;
    assert!(
        matches!(highest, Some(Ok(Pairing::Again(_)))),
        "{highest:?}"
    );
    let kept = agents(registry.as_ref()).await;
    assert_eq!(
        kept.iter().map(|agent| agent.counter).collect::<Vec<_>>(),
        vec![u64::MAX],
        "every counter a u64 holds is kept"
    );
}

async fn another_key_under_a_registered_id_is_refused(harness: &dyn AgentRegistryHarness) {
    let (Some(first_token), Some(second_token), Some(request), Some(other_key)) =
        (token(1, 0), token(2, 0), request(1, 1, "laptop"), key(50))
    else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &first_token).await;
    add(registry.as_ref(), &second_token).await;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    let before = agents(registry.as_ref()).await;
    let taking_over = PairRequest {
        pairing_key: second_token.pairing_key,
        agent_key: other_key,
        hostname: hostname("desktop").unwrap_or(request.hostname.clone()),
        ..request
    };
    assert_eq!(
        pair(registry.as_ref(), &taking_over, at(20)).await,
        Some(Err(PairRefusal::AgentKeyMismatch))
    );
    assert_eq!(agents(registry.as_ref()).await, before);
    assert_eq!(unused(registry.as_ref(), at(21)).await, vec![second_token]);
}

/// Agent `n`'s proof with `counter`, under the key `request(n, ..)` pairs
/// with and the binding kind of every test token.
fn proof(n: u8, counter: u64) -> Option<ProofRequest> {
    Some(ProofRequest {
        agent_id: agent_id(u128::from(n)),
        agent_key: key(n.wrapping_add(100))?,
        kind: BindingKind::TlsExporter,
        counter,
    })
}

async fn a_proof_keeps_the_greater_counter_and_when_it_was_seen(
    harness: &dyn AgentRegistryHarness,
) {
    let (Some(spent), Some(request), Some(lower), Some(highest)) = (
        token(1, 0),
        request(1, 1, "laptop"),
        proof(1, 0),
        proof(1, u64::MAX),
    ) else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &spent).await;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    let Some(Ok(Pairing::New(record))) = paired else {
        assert!(paired.is_none(), "paired: {paired:?}");
        return;
    };
    assert_eq!(
        ok(registry.agent(request.agent_id).await, "reading the agent"),
        Some(Some(record.clone()))
    );
    assert_eq!(
        ok(registry.agent(agent_id(9)).await, "reading no agent"),
        Some(None)
    );
    let seen = AgentRecord {
        last_seen: at(20),
        ..record
    };
    assert_eq!(
        ok(registry.take_proof(&lower, at(20)).await, "proving"),
        Some(Ok(seen.clone())),
        "a lower counter is never kept"
    );
    let reopened = harness.reopened().await;
    assert_eq!(agents(reopened.as_ref()).await, vec![seen.clone()]);
    let raised = AgentRecord {
        counter: u64::MAX,
        last_seen: at(30),
        ..seen
    };
    assert_eq!(
        ok(reopened.take_proof(&highest, at(30)).await, "proving"),
        Some(Ok(raised.clone()))
    );
    assert_eq!(agents(reopened.as_ref()).await, vec![raised]);
}

async fn a_proof_of_another_id_key_or_kind_changes_nothing(harness: &dyn AgentRegistryHarness) {
    let (Some(spent), Some(request), Some(proof), Some(other_key)) =
        (token(1, 0), request(1, 1, "laptop"), proof(1, 5), key(50))
    else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &spent).await;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    let before = agents(registry.as_ref()).await;
    for refused in [
        ProofRequest {
            agent_id: agent_id(9),
            ..proof
        },
        ProofRequest {
            agent_key: other_key,
            ..proof
        },
        ProofRequest {
            kind: BindingKind::None,
            ..proof
        },
    ] {
        assert_eq!(
            ok(registry.take_proof(&refused, at(20)).await, "proving"),
            Some(Err(ProofRefusal::ProofRefused)),
            "{refused:?}"
        );
    }
    assert_eq!(agents(registry.as_ref()).await, before);
}

async fn records_outlive_the_hub(harness: &dyn AgentRegistryHarness) {
    let (Some(spent), Some(unspent), Some(request)) =
        (token(1, 0), token(2, 0), request(1, 1, "laptop"))
    else {
        return;
    };
    let registry = harness.empty().await;
    add(registry.as_ref(), &spent).await;
    add(registry.as_ref(), &unspent).await;
    let paired = pair(registry.as_ref(), &request, at(10)).await;
    let before = agents(registry.as_ref()).await;
    assert!(matches!(paired, Some(Ok(Pairing::New(_)))), "{paired:?}");
    let reopened = harness.reopened().await;
    assert_eq!(agents(reopened.as_ref()).await, before);
    assert_eq!(unused(reopened.as_ref(), at(11)).await, vec![unspent]);
    assert_eq!(
        pair(reopened.as_ref(), &request, at(12)).await,
        Some(Ok(Pairing::Repeated)),
        "the spent token remembers whom it paired"
    );
}

async fn racing_pairings_on_one_token_pair_one(harness: &dyn AgentRegistryHarness) {
    let Some(token) = token(1, 0) else {
        return;
    };
    let racers: Vec<PairRequest> = (1..9)
        .filter_map(|n| request(n, 1, &format!("machine-{n}")))
        .collect();
    let registry = harness.empty().await;
    add(registry.as_ref(), &token).await;
    let pairings = racers.iter().map(|one| registry.pair(one, at(10)));
    let results = futures::future::join_all(pairings).await;
    let paired = results
        .iter()
        .filter(|result| matches!(result, Ok(Ok(Pairing::New(_)))))
        .count();
    let used = results
        .iter()
        .filter(|result| matches!(result, Ok(Err(PairRefusal::TokenUsed))))
        .count();
    assert_eq!((paired, used), (1, racers.len() - 1), "{results:?}");
    assert_eq!(agents(registry.as_ref()).await.len(), 1);
}
