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

/// Machine `n`'s connector to `hub`, with the key files `identity` holds.
fn connecting(hub: &Hub, identity: Arc<InMemoryIdentity>) -> ConnectToHub {
    ConnectToHub::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(Timestamp::from_second(NOW).unwrap())),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        "0.5.0-agent".to_owned(),
        None,
    )
}

/// Machine `n` connects to `hub` and sends `entries` on one connection.
async fn reports(hub: &Hub, identity: Arc<InMemoryIdentity>, n: u128, entries: &[OutboxEntry]) {
    let (_, mut channel) = connecting(hub, identity)
        .connect_channel(agent_id(n), hostname(n), None, None)
        .await
        .unwrap();
    for entry in entries {
        channel.send_report(entry).await.unwrap();
    }
}

/// The hub's `Welcome` tells a machine to start a new epoch when the hub
/// took the place the machine's `Hello` says its outbox gives next, of
/// that epoch: the machine numbers its reports again from an older place,
/// as one restored from an older copy of its database does. A `Hello`
/// that says nothing of its outbox, as 0.6.0's, a place past the last the
/// hub took, or an epoch the hub took nothing of, is welcomed as before
/// (decision 177).
#[tokio::test]
async fn a_machine_whose_next_place_the_hub_took_is_told_to_start_a_new_epoch() {
    let hub = hub();
    crate::test_log::capture_the_log();
    let identity = paired(&hub, 1).await;
    approve(&hub, 1).await;
    let taken: Vec<_> = (1..=3)
        .map(|seq| entry(seq, u128::from(seq), Happening::Succeeded, 1_800_000_000))
        .collect();
    reports(&hub, identity.clone(), 1, &taken).await;
    let epoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
    let other = OutboxEpoch::from_uuid(Uuid::from_u128(0xe2));
    let at = |epoch, next| {
        Some(domain::fleet::OutboxPosition {
            epoch,
            next: ReportSeq::new(next).unwrap(),
        })
    };
    for (outbox, told) in [
        (None, false),
        (at(epoch, 4), false),
        (at(epoch, 9), false),
        (at(other, 1), false),
        (at(epoch, 3), true),
        (at(epoch, 1), true),
    ] {
        let (connected, _) = connecting(&hub, identity.clone())
            .connect_channel(agent_id(1), hostname(1), None, outbox)
            .await
            .unwrap();
        assert_eq!(connected.new_epoch, told, "{outbox:?}");
    }
    assert_eq!(
        hub.inbox.taken_through(agent_id(1), epoch).await.unwrap(),
        ReportSeq::new(3),
        "telling it changes nothing the hub took"
    );
    let told: Vec<String> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(_, line)| line.contains("new epoch"))
        .map(|(level, line)| {
            assert_eq!(level, log::Level::Warn, "{line}");
            line
        })
        .collect();
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(
        told.iter()
            .all(|line| line.contains(&agent_id(1).as_uuid().to_string())),
        "{told:?}"
    );
}

/// The hub's reports of machine 1's epoch `0xe1` at places 1 to 3, each
/// of its own operation.
async fn three_taken(hub: &Hub, identity: Arc<InMemoryIdentity>) -> Vec<OutboxEntry> {
    let taken: Vec<_> = (1..=3)
        .map(|seq| entry(seq, u128::from(seq), Happening::Succeeded, 1_800_000_000))
        .collect();
    reports(hub, identity, 1, &taken).await;
    taken
}

/// Where an outbox of epoch `0xe1` stands past [`three_taken`]'s places.
fn past_the_three() -> Option<domain::fleet::OutboxPosition> {
    Some(domain::fleet::OutboxPosition {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        next: ReportSeq::new(9).unwrap(),
    })
}

/// A machine whose `Hello` states its outbox, past the last place the hub
/// took, and which sends at a taken place a report other than the one the
/// hub took there, as one restored from an older copy of its database
/// that gave more places since than the hub took after the copy: the hub
/// keeps nothing, acknowledges nothing, ends the connection, and tells the
/// machine at its next `Hello` to start a new epoch. The report taken,
/// sent again, is acknowledged as before (#1387, decision 177).
#[tokio::test]
async fn a_report_unlike_the_one_taken_at_its_place_ends_the_connection_and_starts_a_new_epoch() {
    let hub = hub();
    let identity = paired(&hub, 1).await;
    approve(&hub, 1).await;
    let taken = three_taken(&hub, identity.clone()).await;
    let (connected, mut channel) = connecting(&hub, identity.clone())
        .connect_channel(agent_id(1), hostname(1), None, past_the_three())
        .await
        .unwrap();
    assert!(!connected.new_epoch);
    channel.send_report(&taken[0]).await.unwrap();
    assert_eq!(
        channel.receive_ack().await.unwrap().seq,
        ReportSeq::FIRST,
        "the report taken, sent again"
    );
    let restored = entry(2, 0x1387, Happening::Succeeded, 1_800_000_000);
    let _ = channel.send_report(&restored).await;
    assert!(
        channel.receive_ack().await.is_err(),
        "a report the hub didn't take is never acknowledged"
    );
    assert_eq!(hub.inbox.reports(agent_id(1)).await.unwrap(), taken);
    let epoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
    assert!(hub.inbox.diverged(agent_id(1), epoch).await.unwrap());
    let (connected, _) = connecting(&hub, identity)
        .connect_channel(agent_id(1), hostname(1), None, past_the_three())
        .await
        .unwrap();
    assert!(connected.new_epoch, "the next Hello starts a new epoch");
}

/// A machine whose `Hello` states nothing of its outbox, as 0.6.0's, can't
/// start a new epoch: a report of its other than the one the hub took at
/// its place is acknowledged as taken and kept nowhere, as before, and the
/// hub warns once while it runs, naming the agent and the place only (I4;
/// #1387, decision 177).
#[tokio::test]
async fn a_machine_that_states_no_outbox_has_a_report_unlike_the_one_taken_acknowledged() {
    let hub = hub();
    let identity = paired(&hub, 1).await;
    approve(&hub, 1).await;
    let taken = three_taken(&hub, identity.clone()).await;
    crate::test_log::capture_the_log();
    let (_, mut channel) = connecting(&hub, identity.clone())
        .connect_channel(agent_id(1), hostname(1), None, None)
        .await
        .unwrap();
    for (seq, n) in [(2, 0x1387), (3, 0x1388)] {
        let restored = entry(seq, n, Happening::Succeeded, 1_800_000_000);
        channel.send_report(&restored).await.unwrap();
        assert_eq!(channel.receive_ack().await.unwrap().seq, restored.seq);
    }
    assert_eq!(hub.inbox.reports(agent_id(1)).await.unwrap(), taken);
    let warned: Vec<(log::Level, String)> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .collect();
    assert_eq!(
        warned,
        [(
            log::Level::Warn,
            format!(
                "agent {} sent a report at place 2 of its outbox other than the one the hub \
                 took there: its database may have been restored from an older copy, and its \
                 version can't start a new epoch; the hub takes such reports as held already, \
                 keeps none of them, and doesn't log the next ones",
                agent_id(1).as_uuid()
            )
        )]
    );
}

/// A machine that pairs is told to the browsers as waiting, once: the same
/// pairing again tells nothing new. Its approval is told as well.
#[tokio::test]
async fn a_machine_that_pairs_and_is_approved_is_told() {
    let hub = hub();
    paired(&hub, 1).await;
    assert_eq!(
        hub.told.taken(),
        [UiEvent::MachinePending {
            machine: agent_id(1).as_uuid().to_string(),
            hostname: "laptop-1".to_owned(),
            at: now(),
        }]
    );
    approve(&hub, 1).await;
    assert_eq!(
        hub.told.taken(),
        [UiEvent::MachineApproved {
            machine: agent_id(1).as_uuid().to_string(),
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
            entry(4, 0x0c, Happening::Canceled, 260),
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
            UiEvent::MachineReported {
                machine: agent,
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
                MachineOutcomeDto::Canceled
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
            [UiEvent::MachineRemoved {
                machine: agent_id(n).as_uuid().to_string(),
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

fn repository(byte: u8) -> domain::RepositoryId {
    domain::RepositoryId::parse(&format!("{byte:02x}").repeat(32)).unwrap()
}

fn backing_up(n: u128, to: u8) -> domain::maintenance::RunningBackup {
    domain::maintenance::RunningBackup {
        op_id: OperationId::from_uuid(Uuid::from_u128(n)),
        repository: Some(repository(to)),
    }
}

/// Machines 1 and 2, paired and approved, each with a live connection.
async fn two_online() -> Hub {
    let hub = hub();
    for n in [1, 2] {
        paired(&hub, n).await;
        approve(&hub, n).await;
        hub.pairing
            .went_live(agent_id(n), n as u64, Box::new(|| {}))
            .await;
    }
    hub
}

/// What the hub answers machine `n`'s heartbeat that names `running`.
fn beat(
    hub: &Hub,
    n: u128,
    running: Option<&[domain::maintenance::RunningBackup]>,
) -> super::Answered {
    hub.pairing.take_heartbeat(
        agent_id(n),
        &crate::pairing::wire::heartbeat_payload(running),
    )
}

fn holding(held: Option<&[domain::RepositoryId]>) -> super::Answered {
    Ok(Some((
        domain::fleet::heartbeat_ack_kind(),
        crate::pairing::wire::heartbeat_ack_payload(held),
    )))
}

/// The hub answers a machine's heartbeat with the prunes it holds for it:
/// none it can tell while another machine's heartbeat hasn't said what it
/// runs, then the repository another machine is backing up to; and it
/// refuses a payload that isn't a heartbeat (decisions 159 and 166).
#[tokio::test]
async fn a_heartbeat_is_answered_with_the_prunes_the_hub_holds_for_its_machine() {
    let hub = two_online().await;
    assert_eq!(
        beat(&hub, 2, Some(&[backing_up(7, 3)])),
        holding(None),
        "machine 1 hasn't said what it runs"
    );
    assert_eq!(beat(&hub, 1, Some(&[])), holding(Some(&[repository(3)])));
    assert_eq!(
        beat(&hub, 2, Some(&[backing_up(7, 3)])),
        holding(Some(&[])),
        "a machine's own backups hold none of its prunes"
    );
    assert_eq!(beat(&hub, 2, Some(&[])), holding(Some(&[])));
    assert_eq!(
        beat(&hub, 1, Some(&[])),
        holding(Some(&[])),
        "the backup finished"
    );
    assert_eq!(beat(&hub, 2, None), holding(Some(&[])));
    assert_eq!(
        beat(&hub, 1, Some(&[])),
        holding(None),
        "machine 2 no longer says what it runs"
    );
    assert_eq!(
        hub.pairing.take_heartbeat(agent_id(1), b"not a heartbeat"),
        Err(super::MessageRefused::Unexpected)
    );
}

/// A machine that went offline holds no prune, and one that connects again
/// holds every prune until its heartbeat says what it runs. A heartbeat
/// taken as the connection's first message, before it went live, counts.
#[tokio::test]
async fn a_machine_offline_holds_no_prune_and_one_back_holds_until_it_says() {
    let hub = two_online().await;
    hub.pairing.heard_running(agent_id(1), Some(Vec::new()));
    hub.pairing
        .heard_running(agent_id(2), Some(vec![backing_up(7, 3)]));
    assert_eq!(
        hub.pairing.prunes_held_for(agent_id(1)),
        Some(vec![repository(3)])
    );

    hub.pairing.went_away(agent_id(2), 2).await;
    assert_eq!(hub.pairing.prunes_held_for(agent_id(1)), Some(Vec::new()));

    hub.pairing.went_live(agent_id(2), 3, Box::new(|| {})).await;
    assert_eq!(
        hub.pairing.prunes_held_for(agent_id(1)),
        None,
        "back online, it hasn't said what it runs"
    );

    hub.pairing.went_away(agent_id(2), 3).await;
    hub.pairing
        .heard_running(agent_id(2), Some(vec![backing_up(8, 4)]));
    hub.pairing.went_live(agent_id(2), 4, Box::new(|| {})).await;
    assert_eq!(
        hub.pairing.prunes_held_for(agent_id(1)),
        Some(vec![repository(4)])
    );
}

/// Counts the times what closes a connection was called.
fn closer(closed: &Arc<Mutex<u32>>) -> super::CloseConnection {
    let closed = closed.clone();
    Box::new(move || *closed.lock().unwrap() += 1)
}

/// A machine is online from its connection going live until that one
/// goes away; a newer connection replaces it and closes the older, whose
/// end then leaves the machine online. The browsers hear each change once.
#[tokio::test]
async fn a_machine_is_online_while_the_hub_holds_its_one_live_connection() {
    let hub = hub();
    paired(&hub, 1).await;
    approve(&hub, 1).await;
    hub.told.taken();
    let agent = agent_id(1);
    let older = Arc::new(Mutex::new(0));
    let newer = Arc::new(Mutex::new(0));
    assert!(!hub.pairing.is_online(agent));

    hub.pairing.went_live(agent, 1, closer(&older)).await;
    assert!(hub.pairing.is_online(agent));
    assert!(hub.pairing.machines().await.unwrap()[0].online);
    let online = UiEvent::MachineOnline {
        machine: agent.as_uuid().to_string(),
        at: now(),
    };
    assert_eq!(hub.told.taken(), [online]);

    hub.pairing.went_live(agent, 2, closer(&newer)).await;
    assert_eq!(*older.lock().unwrap(), 1, "the older connection is closed");
    hub.pairing.went_away(agent, 1).await;
    assert!(hub.pairing.is_online(agent), "the newer one still holds");
    assert_eq!(hub.told.taken(), []);

    hub.pairing.went_away(agent, 2).await;
    assert!(!hub.pairing.is_online(agent));
    assert!(!hub.pairing.machines().await.unwrap()[0].online);
    assert_eq!(
        *newer.lock().unwrap(),
        0,
        "a connection that ended isn't closed again"
    );
    assert_eq!(
        hub.told.taken(),
        [UiEvent::MachineOffline {
            machine: agent.as_uuid().to_string(),
            at: now(),
        }]
    );
    hub.pairing.went_away(agent, 2).await;
    assert_eq!(hub.told.taken(), [], "an end heard twice is told once");
}

/// A machine a person removes is let go of at once: the hub closes the
/// connection it holds, so the machine sends nothing more on one it proved
/// before its removal (decision 154; §16, decision 159).
#[tokio::test]
async fn a_removed_machines_live_connection_is_closed() {
    let hub = hub();
    paired(&hub, 1).await;
    approve(&hub, 1).await;
    let agent = agent_id(1);
    let closed = Arc::new(Mutex::new(0));
    hub.pairing.went_live(agent, 1, closer(&closed)).await;
    let key = hub.registry.agent(agent).await.unwrap().unwrap().key;
    let removed = hub.pairing.remove_asked(removal(1, key, false)).await;
    assert!(matches!(removed, Ok(Ok(_))), "{removed:?}");
    assert_eq!(*closed.lock().unwrap(), 1, "its connection is closed");
    assert!(!hub.pairing.is_online(agent));
}

/// A machine removed while its connection made its handshake is let go
/// of as that connection goes live: the hub closes it at once, and the
/// machine is never shown online (decision 154; §16, decision 159).
#[tokio::test]
async fn a_machine_removed_during_its_handshake_is_let_go_of_as_it_goes_live() {
    let hub = hub();
    paired(&hub, 1).await;
    approve(&hub, 1).await;
    let agent = agent_id(1);
    let key = hub.registry.agent(agent).await.unwrap().unwrap().key;
    let removed = hub.pairing.remove_asked(removal(1, key, false)).await;
    assert!(matches!(removed, Ok(Ok(_))), "{removed:?}");
    hub.told.taken();
    let closed = Arc::new(Mutex::new(0));
    hub.pairing.went_live(agent, 1, closer(&closed)).await;
    assert_eq!(*closed.lock().unwrap(), 1, "its connection is closed");
    assert!(!hub.pairing.is_online(agent));
    assert_eq!(hub.told.taken(), [], "the browsers hear nothing");
}

/// When a machine's live connection ends, the hub keeps that time as when
/// it last heard from the machine, which the dashboard shows as when it
/// last connected; an older connection's end keeps nothing (§16, decision
/// 159).
#[tokio::test]
async fn the_end_of_a_live_connection_is_kept_as_when_the_machine_was_last_heard_from() {
    let hub = hub();
    paired(&hub, 1).await;
    approve(&hub, 1).await;
    let agent = agent_id(1);
    let earlier = Timestamp::from_second(NOW - 3600).unwrap();
    let last_seen = async || hub.registry.agent(agent).await.unwrap().unwrap().last_seen;
    hub.pairing
        .went_live(agent, 1, closer(&Arc::default()))
        .await;
    hub.registry.record_seen(agent, earlier).await.unwrap();
    hub.pairing.went_away(agent, 2).await;
    assert_eq!(last_seen().await, earlier, "not its live connection");
    hub.pairing.went_away(agent, 1).await;
    assert_eq!(last_seen().await, Timestamp::from_second(NOW).unwrap());
}

/// A machine's progress reads as the API tells it: its step and unit by
/// their names, and its time as the API writes times. A time past what a
/// timestamp holds reads as none.
#[test]
fn a_progress_reads_as_the_api_tells_it() {
    use api::{MachineProgressDto, OperationStep, ProgressUnit};
    use domain::fleet::{ReportedProgress, WireName};

    use crate::pairing::{progress_dto, time_of};

    let progress = ReportedProgress {
        op_id: op(1),
        step: Some(WireName::parse("backing-up").unwrap()),
        unit: WireName::parse("bytes").unwrap(),
        current: 5,
        total: Some(9),
        at: EpochSeconds::from_seconds(1_800_000_000),
    };
    assert_eq!(
        progress_dto(&progress),
        MachineProgressDto {
            operation: op(1).as_uuid().to_string(),
            step: Some(OperationStep::BackingUp),
            unit: ProgressUnit::Bytes,
            current: 5,
            total: Some(9),
            at: "2027-01-15T08:00:00Z".to_owned(),
        }
    );
    assert_eq!(time_of(EpochSeconds::from_seconds(u64::MAX)), None);
}

/// The hub's keys show their public halves in a log, never the seed.
#[tokio::test]
async fn the_hubs_keys_show_their_public_halves_and_no_seed() {
    let hub = hub();
    let keys = hub.pairing.keys().await.unwrap();
    assert_eq!(
        format!("{keys:?}"),
        format!(
            "HubKeys {{ key: {:?}, recovery_key: {:?}, .. }}",
            keys.key(),
            keys.recovery_key()
        )
    );
}

/// What the page asks for shows in a log without the step-up's
/// confirmation (I4).
#[test]
fn an_ask_shows_in_a_log_without_its_confirmation() {
    use std::net::{IpAddr, Ipv4Addr};

    use domain::access::step_up::StepUpConfirmation;
    use domain::fleet::PublicKey;

    use crate::pairing::{ApprovalAsk, RecoveryAsk, TokenAsk};

    let confirmation = || Some(StepUpConfirmation::parse("canary-step-up".to_owned()).unwrap());
    let peer = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let address = HubAddress::parse("https://hub.example:9443").unwrap();
    let token = TokenAsk {
        address: address.clone(),
        tls: TokenTls::Ca,
        life: None,
        peer,
        beyond_loopback: false,
        confirmation: confirmation(),
    };
    assert_eq!(
        format!("{token:?}"),
        format!(
            "TokenAsk {{ address: {address:?}, tls: Ca, life: None, beyond_loopback: false, .. }}"
        )
    );
    let key = PublicKey::from_bytes([0x22; 32]).unwrap();
    let approval = ApprovalAsk {
        agent_id: agent_id(1),
        agent_key: key,
        peer,
        beyond_loopback: true,
        confirmation: confirmation(),
    };
    assert_eq!(
        format!("{approval:?}"),
        format!(
            "ApprovalAsk {{ agent_id: {:?}, agent_key: {key:?}, peer: 127.0.0.1, \
             beyond_loopback: true, .. }}",
            agent_id(1)
        )
    );
    let recovery = RecoveryAsk {
        peer,
        beyond_loopback: false,
        confirmation: confirmation(),
    };
    assert_eq!(
        format!("{recovery:?}"),
        "RecoveryAsk { peer: 127.0.0.1, beyond_loopback: false, .. }"
    );
}
