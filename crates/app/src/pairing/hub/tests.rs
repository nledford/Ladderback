//! What the hub tells its browsers of its machines, and what its dashboard
//! reads of them (#569).

use std::sync::{Arc, Mutex};

use api::{MachineOutcomeDto, UiEvent};
use async_trait::async_trait;
use domain::fleet::{
    AgentReport, EpochSeconds, Fingerprint, Happening, Hostname, HubAddress, OPERATIONS_SHOWN,
    OutboxEpoch, Pairing, ReportSeq, ReportedOperation, ReportedOutcome, TokenTls,
};
use domain::{AgentId, OperationId, Timestamp, Uuid};
use futures::StreamExt as _;
use futures::stream::BoxStream;

use crate::fakes::{
    FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, InMemoryReportInbox,
    LoopbackHubConnector, SeededRandom,
};
use crate::pairing::{
    AgentPairing, ConnectToHub, HubPairing, PairWithHub, RemovalAsk, TokenAskRefused,
};
use crate::ports::{AgentRegistry as _, EventBus, OutboxEntry, ReportInbox as _};

/// The events the hub told, in order.
#[derive(Default)]
struct Told(Mutex<Vec<UiEvent>>);

#[async_trait]
impl EventBus for Told {
    async fn publish(&self, event: UiEvent) {
        self.0.lock().unwrap().push(event);
    }

    async fn subscribe(&self) -> BoxStream<'static, UiEvent> {
        futures::stream::empty().boxed()
    }
}

impl Told {
    fn taken(&self) -> Vec<UiEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct Hub {
    registry: Arc<InMemoryAgentRegistry>,
    inbox: Arc<InMemoryReportInbox>,
    told: Arc<Told>,
    pairing: Arc<HubPairing>,
}

const NOW: i64 = 1_800_000_000;

fn hub() -> Hub {
    let registry = Arc::new(InMemoryAgentRegistry::new());
    let inbox = Arc::new(InMemoryReportInbox::default());
    let told = Arc::new(Told::default());
    let pairing = HubPairing::new(
        registry.clone(),
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(1)),
        Arc::new(FakeClock::new(Timestamp::from_second(NOW).unwrap())),
        "0.5.0-hub".to_owned(),
    )
    .with_inbox(inbox.clone())
    .with_events(told.clone());
    Hub {
        registry,
        inbox,
        told,
        pairing: Arc::new(pairing),
    }
}

fn agent_id(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn hostname(n: u128) -> Hostname {
    Hostname::parse(&format!("laptop-{n}")).unwrap()
}

fn now() -> String {
    Timestamp::from_second(NOW).unwrap().to_string()
}

/// Machine `n`'s key files, paired with `hub`.
async fn paired(hub: &Hub, n: u128) -> Arc<InMemoryIdentity> {
    let keys = hub.pairing.keys().await.unwrap();
    let address = HubAddress::parse("https://hub.example:9443").unwrap();
    let tls = TokenTls::Pinned(Fingerprint::from_digest([7; 32]));
    let token = hub
        .pairing
        .make_token(&keys, address, tls, None)
        .await
        .unwrap();
    let identity = Arc::new(InMemoryIdentity::default());
    let pairing = AgentPairing::new(
        identity.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(9_000 + u64::try_from(n).unwrap())),
        Arc::new(crate::fakes::InMemoryLeaseCache::default()),
    );
    PairWithHub::new(
        pairing,
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    )
    .pair(token, agent_id(n), hostname(n), None)
    .await
    .unwrap();
    identity
}

async fn approve(hub: &Hub, n: u128) {
    let waiting = hub.registry.agent(agent_id(n)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
}

fn op(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

fn entry(seq: u64, n: u128, happening: Happening, at: u64) -> OutboxEntry {
    OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(seq).unwrap(),
        report: AgentReport {
            op_id: op(n),
            happening,
            at: EpochSeconds::from_seconds(at),
        },
    }
}

/// Machine `n` connects to `hub` and sends `entries` on one connection.
async fn reports(hub: &Hub, identity: Arc<InMemoryIdentity>, n: u128, entries: &[OutboxEntry]) {
    let connecting = ConnectToHub::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(Timestamp::from_second(NOW).unwrap())),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        "0.5.0-agent".to_owned(),
        None,
    );
    let (_, mut channel) = connecting
        .connect_channel(agent_id(n), hostname(n), None)
        .await
        .unwrap();
    for entry in entries {
        channel.send_report(entry).await.unwrap();
    }
}

/// A machine that pairs is told to the browsers as waiting, once: the same
/// pairing again tells nothing new. Its approval is told as well.
#[tokio::test]
async fn a_machine_that_pairs_and_is_approved_is_told() {
    let hub = hub();
    paired(&hub, 1).await;
    assert_eq!(
        hub.told.taken(),
        [UiEvent::AgentPending {
            agent: agent_id(1).as_uuid().to_string(),
            hostname: "laptop-1".to_owned(),
            at: now(),
        }]
    );
    approve(&hub, 1).await;
    assert_eq!(
        hub.told.taken(),
        [UiEvent::AgentApproved {
            agent: agent_id(1).as_uuid().to_string(),
            hostname: "laptop-1".to_owned(),
            at: now(),
        }]
    );
}

/// Only the pairing that makes a machine wait is told: not the same pairing
/// repeated, nor an approved machine that pairs again.
#[tokio::test]
async fn only_a_pairing_that_makes_a_machine_wait_is_told() {
    let hub = hub();
    paired(&hub, 1).await;
    let at = Timestamp::from_second(NOW).unwrap();
    let pending = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    assert!(super::fleet::waiting(&Pairing::Again(pending.clone()), at).is_some());
    assert_eq!(super::fleet::waiting(&Pairing::Repeated(pending), at), None);
    approve(&hub, 1).await;
    let approved = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    assert_eq!(super::fleet::waiting(&Pairing::Again(approved), at), None);
}

/// Each report the hub takes is told, with how the operation stands; one
/// it took before is not told again.
#[tokio::test]
async fn each_report_taken_is_told_once() {
    let hub = hub();
    let identity = paired(&hub, 1).await;
    approve(&hub, 1).await;
    hub.told.taken();
    let code = domain::fleet::WireName::parse("repository-unreachable").unwrap();
    reports(
        &hub,
        identity.clone(),
        1,
        &[
            entry(1, 0x0a, Happening::Started, 100),
            entry(2, 0x0a, Happening::Succeeded, 160),
            entry(3, 0x0b, Happening::Failed { code }, 200),
            entry(4, 0x0c, Happening::Cancelled, 260),
        ],
    )
    .await;
    reports(
        &hub,
        identity,
        1,
        &[entry(3, 0x0b, Happening::Started, 200)],
    )
    .await;
    let told: Vec<_> = hub
        .told
        .taken()
        .into_iter()
        .filter_map(|event| match event {
            UiEvent::AgentReported {
                agent,
                operation,
                outcome,
            } => Some((agent, operation, outcome)),
            _ => None,
        })
        .collect();
    let agent = agent_id(1).as_uuid().to_string();
    assert_eq!(
        told,
        [
            (
                agent.clone(),
                op(0x0a).as_uuid().to_string(),
                MachineOutcomeDto::Running
            ),
            (
                agent.clone(),
                op(0x0a).as_uuid().to_string(),
                MachineOutcomeDto::Succeeded
            ),
            (
                agent.clone(),
                op(0x0b).as_uuid().to_string(),
                MachineOutcomeDto::Failed
            ),
            (
                agent,
                op(0x0c).as_uuid().to_string(),
                MachineOutcomeDto::Cancelled
            ),
        ]
    );
}

/// The dashboard shows only approved machines, oldest first, each with its
/// latest operations and how many of its reports the hub let go of.
#[tokio::test]
async fn the_dashboard_shows_approved_machines_with_their_latest_operations() {
    let hub = hub();
    let first = paired(&hub, 1).await;
    paired(&hub, 2).await;
    approve(&hub, 1).await;
    reports(
        &hub,
        first,
        1,
        &[
            entry(1, 0x0a, Happening::Started, 100),
            entry(2, 0x0a, Happening::Succeeded, 160),
            entry(3, 0x0b, Happening::Started, 200),
        ],
    )
    .await;
    let machines = hub.pairing.machines().await.unwrap();
    assert_eq!(machines.len(), 1, "the waiting machine isn't shown");
    let machine = machines.first().unwrap();
    assert_eq!(machine.record.agent_id, agent_id(1));
    assert_eq!(machine.reports_dropped, 0);
    assert_eq!(
        machine.operations,
        [
            ReportedOperation {
                op_id: op(0x0b),
                started_at: Some(EpochSeconds::from_seconds(200)),
                ended_at: None,
                outcome: ReportedOutcome::Running,
            },
            ReportedOperation {
                op_id: op(0x0a),
                started_at: Some(EpochSeconds::from_seconds(100)),
                ended_at: Some(EpochSeconds::from_seconds(160)),
                outcome: ReportedOutcome::Succeeded,
            },
        ]
    );
    assert!(machine.operations.len() <= OPERATIONS_SHOWN);
    assert_eq!(
        hub.inbox
            .latest(agent_id(1), 1)
            .await
            .unwrap()
            .reports
            .len(),
        1
    );
}

/// The dashboard reads enough of a machine's reports to show each of the
/// operations it shows from its start to its end.
#[tokio::test]
async fn the_dashboard_shows_each_operation_it_shows_whole() {
    let hub = hub();
    let identity = paired(&hub, 1).await;
    approve(&hub, 1).await;
    let entries: Vec<_> = (0..u64::try_from(OPERATIONS_SHOWN).unwrap())
        .flat_map(|n| {
            let op = 0x100 + u128::from(n);
            [
                entry(2 * n + 1, op, Happening::Started, 100 * n),
                entry(2 * n + 2, op, Happening::Succeeded, 100 * n + 50),
            ]
        })
        .collect();
    reports(&hub, identity, 1, &entries).await;
    let machines = hub.pairing.machines().await.unwrap();
    let operations = &machines.first().unwrap().operations;
    assert_eq!(operations.len(), OPERATIONS_SHOWN);
    assert!(
        operations
            .iter()
            .all(|operation| operation.started_at.is_some() && operation.ended_at.is_some()),
        "{operations:?}"
    );
}

fn removal(n: u128, key: domain::fleet::PublicKey, beyond_loopback: bool) -> RemovalAsk {
    RemovalAsk {
        agent_id: agent_id(n),
        agent_key: key,
        beyond_loopback,
    }
}

/// A machine removed, waiting or approved, is told to the browsers once,
/// so another page drops it at once; a refused removal tells nothing.
#[tokio::test]
async fn a_removed_machine_is_told_and_a_refused_removal_is_not() {
    let hub = hub();
    paired(&hub, 1).await;
    paired(&hub, 2).await;
    approve(&hub, 2).await;
    hub.told.taken();
    for n in [1, 2] {
        let key = hub.registry.agent(agent_id(n)).await.unwrap().unwrap().key;
        let removed = hub.pairing.remove_asked(removal(n, key, false)).await;
        assert!(matches!(removed, Ok(Ok(_))), "{removed:?}");
        assert_eq!(
            hub.told.taken(),
            [UiEvent::AgentRemoved {
                agent: agent_id(n).as_uuid().to_string(),
                hostname: format!("laptop-{n}"),
                at: now(),
            }]
        );
        let again = hub.pairing.remove_asked(removal(n, key, false)).await;
        assert!(
            matches!(again, Ok(Err(domain::fleet::RemovalRefused::Unknown))),
            "{again:?}"
        );
        assert_eq!(hub.told.taken(), []);
    }
}

/// Beyond loopback with sign-in off, no removal is taken, as no approval
/// is: the machine stays and nothing is told.
#[tokio::test]
async fn a_removal_beyond_loopback_needs_sign_in() {
    let hub = hub();
    paired(&hub, 1).await;
    hub.told.taken();
    let key = hub.registry.agent(agent_id(1)).await.unwrap().unwrap().key;
    let refused = hub.pairing.remove_asked(removal(1, key, true)).await;
    assert!(
        matches!(refused, Err(TokenAskRefused::NeedsSignIn)),
        "{refused:?}"
    );
    assert!(hub.registry.agent(agent_id(1)).await.unwrap().is_some());
    assert_eq!(hub.told.taken(), []);
}

/// Machine `n`, waiting for approval since 7 days and an hour before the
/// hub's clock: it has lapsed, though nothing has listed the agents since.
fn lapsed(n: u128) -> domain::fleet::AgentRecord {
    let since = Timestamp::from_second(NOW - 7 * 24 * 3_600 - 3_600).unwrap();
    domain::fleet::AgentRecord {
        agent_id: agent_id(n),
        key: domain::fleet::PublicKey::from_bytes([u8::try_from(n).unwrap(); 32]).unwrap(),
        hostname: hostname(n),
        status: domain::fleet::AgentStatus::Pending { since },
        counter: 1,
        last_seen: since,
        zone: None,
    }
}

/// An approval drops the lapsed machines before it decides, so a machine
/// that waited past 7 days unlisted is not approved, and is gone.
#[tokio::test]
async fn a_machine_that_lapsed_unlisted_is_not_approved() {
    let hub = hub();
    let laptop = lapsed(1);
    hub.registry.hold(laptop.clone());
    assert_eq!(
        hub.pairing.approve(&laptop).await.unwrap(),
        Err(domain::fleet::ApprovalRefused::Unknown)
    );
    assert_eq!(hub.registry.agent(agent_id(1)).await.unwrap(), None);
}

/// A pairing drops the lapsed machines before it counts the waiting ones,
/// so 20 that lapsed unlisted leave room for a new machine.
#[tokio::test]
async fn machines_that_lapsed_unlisted_leave_room_for_a_new_one() {
    let hub = hub();
    // The hub makes its key only while no agent is registered.
    hub.pairing.keys().await.unwrap();
    for n in 1..=20 {
        hub.registry.hold(lapsed(n));
    }
    paired(&hub, 21).await;
    let ids: Vec<_> = hub
        .registry
        .agents()
        .await
        .unwrap()
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    assert_eq!(ids, [agent_id(21)]);
}
