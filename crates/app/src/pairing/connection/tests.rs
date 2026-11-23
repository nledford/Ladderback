use std::sync::Arc;

use api::sync::{AgentFrame, HubFrame};
use async_trait::async_trait;
use domain::fleet::{
    AgentStatus, Approval, Binding, EpochSeconds, Fingerprint, HubAddress, HubEnvelope, HubRoute,
    KEY_BYTES, MachineFingerprint, MachineIdentifier, MachineZone, Nonce, PayloadDigest, Protocols,
    PublicKey, SeedFile, SignedMessage, SigningSeed, TokenTls, Welcome, WelcomeRefused, WireName,
    hello_kind,
};
use domain::{SignedDuration, Timestamp, Uuid};

use super::*;
use crate::fakes::{
    FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, LOOPBACK_EXPORTER,
    LoopbackHubConnector, SeededRandom,
};
use crate::pairing::wire::{
    AgentMessage, HubMessage, agent_message, hub_envelope_frame, hub_message, read_hello,
    read_welcome, welcome_payload,
};
use crate::pairing::{
    AgentPairing, HelloAnswer, HubPairing, MessageRefused, PairWithHub, ProofAnswer,
};
use crate::ports::{AgentRegistry, HubLink};

/// One hub on fakes: its registry, its key files, its clock.
struct Hub {
    registry: Arc<InMemoryAgentRegistry>,
    identity: Arc<InMemoryIdentity>,
    clock: Arc<FakeClock>,
    pairing: Arc<HubPairing>,
}

fn hub(seed: u64) -> Hub {
    hub_with(seed, |pairing| pairing)
}

/// One hub on fakes, its pairing as `shaped` makes it.
fn hub_with(seed: u64, shaped: impl FnOnce(HubPairing) -> HubPairing) -> Hub {
    let registry = Arc::new(InMemoryAgentRegistry::new());
    let identity = Arc::new(InMemoryIdentity::default());
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let pairing = Arc::new(shaped(HubPairing::new(
        registry.clone(),
        identity.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(seed)),
        clock.clone(),
        "0.5.0-hub".to_owned(),
    )));
    Hub {
        registry,
        identity,
        clock,
        pairing,
    }
}

fn agent_id(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn hostname(n: u128) -> Hostname {
    Hostname::parse(&format!("laptop-{n}")).unwrap()
}

/// The key files of machine `n`, paired with `hub` through a token for the
/// hub's own TLS.
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
    );
    let connector = LoopbackHubConnector::to(hub.pairing.clone());
    PairWithHub::new(pairing, Arc::new(connector))
        .pair(token, agent_id(n), hostname(n), None)
        .await
        .unwrap();
    identity
}

/// The key files of a machine that holds `like`'s pin and an agent key of
/// its own, and never paired.
async fn with_pin_of(like: &InMemoryIdentity) -> Arc<InMemoryIdentity> {
    let identity = Arc::new(InMemoryIdentity::default());
    identity
        .write_pin(&like.pin().await.unwrap().unwrap())
        .await
        .unwrap();
    identity
        .keep_seed(SeedFile::AgentKey, SigningSeed::from_bytes([5; KEY_BYTES]))
        .await
        .unwrap();
    identity
}

fn connecting(identity: Arc<InMemoryIdentity>, connector: Arc<dyn HubConnector>) -> ConnectToHub {
    connecting_on(identity, connector, None)
}

/// Connecting from the computer whose identifier is `machine`.
fn connecting_on(
    identity: Arc<InMemoryIdentity>,
    connector: Arc<dyn HubConnector>,
    machine: Option<MachineIdentifier>,
) -> ConnectToHub {
    ConnectToHub::new(
        identity,
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        connector,
        "0.5.0-agent".to_owned(),
        machine,
    )
}

/// The identifier of computer `n`.
fn identifier(n: u128) -> MachineIdentifier {
    MachineIdentifier::parse(&format!("{n:032x}")).unwrap()
}

/// The key files of a copy of `of`'s data folder: its pin and its agent
/// key.
async fn copied(of: &InMemoryIdentity) -> Arc<InMemoryIdentity> {
    let copy = Arc::new(InMemoryIdentity::default());
    copy.write_pin(&of.pin().await.unwrap().unwrap())
        .await
        .unwrap();
    copy.keep_seed(
        SeedFile::AgentKey,
        of.seed(SeedFile::AgentKey).await.unwrap().unwrap(),
    )
    .await
    .unwrap();
    copy
}

fn agent_key(seed: &SigningSeed) -> PublicKey {
    FakeSigning.public_key(seed).unwrap()
}

/// A paired agent proves itself and is told the hub holds it pending; the
/// hub keeps the counter of this connection and when it saw the agent
/// (decision 120, "The handshake").
#[tokio::test]
async fn a_paired_agent_is_welcomed_pending_and_its_counter_kept() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let before = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.clock.advance(SignedDuration::from_secs(60));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = connecting(identity.clone(), connector.clone())
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    let seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    assert_eq!(
        connected,
        Connected {
            approval: Approval::Pending,
            pin: identity.pin().await.unwrap().unwrap(),
            agent_key: agent_key(&seed),
        }
    );
    let after = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    assert_eq!(after.counter, before.counter + 1);
    assert_eq!(after.last_seen, hub.clock.now());
    assert_eq!(after.status, before.status);
    let kinds: Vec<_> = connector
        .sent()
        .iter()
        .map(
            |text| match agent_message(&AgentFrame::from_text(text).unwrap()) {
                Ok(AgentMessage::Connect(_)) => "connect",
                Ok(AgentMessage::Proof(_)) => "proof",
                Ok(AgentMessage::Signed(envelope)) if envelope.kind == hello_kind() => "hello",
                other => panic!("{other:?}"),
            },
        )
        .collect();
    assert_eq!(kinds, ["connect", "proof", "hello"]);
}

/// An agent a person approved is told so.
#[tokio::test]
async fn an_approved_agent_is_welcomed_approved() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = connecting(identity, connector)
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    assert_eq!(connected.approval, Approval::Approved);
    assert!(matches!(
        hub.registry
            .agent(agent_id(1))
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentStatus::Approved { .. }
    ));
}

/// Past the cap of reports the hub keeps of one agent, it lets go of the
/// oldest and warns once for that agent while it runs, with the count let
/// go of so far and nothing a report says (I4); the ones after are counted
/// and not logged (§7.2).
#[tokio::test]
async fn reports_past_the_cap_are_let_go_of_counted_and_warned_of_once() {
    use crate::ports::{Outbox as _, OutboxEntry, ReportInbox as _};
    use crate::sync::{SyncWithHub, Synced};
    use domain::fleet::{AgentReport, Happening, MAX_REPORTS_HELD, OutboxEpoch, ReportSeq};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let operation = Uuid::from_u128(0x0b);
    let report = AgentReport {
        op_id: domain::OperationId::from_uuid(operation),
        happening: Happening::Started,
        at: EpochSeconds::from_seconds(1_800_000_000),
    };
    let earlier = OutboxEpoch::from_uuid(Uuid::from_u128(0xe0));
    for place in 1..=MAX_REPORTS_HELD {
        let entry = OutboxEntry {
            epoch: earlier,
            seq: ReportSeq::new(place).unwrap(),
            report: report.clone(),
        };
        inbox
            .ingest(agent_id(1), &entry, hub.clock.now())
            .await
            .unwrap();
    }
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(0xe1),
    )));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity, connector),
        outbox.clone(),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    );
    crate::test_log::capture_the_log();
    for round in [2, 1] {
        for _ in 0..round {
            outbox.append(&report).await.unwrap();
        }
        assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: round }));
    }
    let next = OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(4).unwrap(),
        report: report.clone(),
    };
    assert_eq!(
        inbox
            .ingest(agent_id(1), &next, hub.clock.now())
            .await
            .unwrap()
            .dropped,
        Some(4),
        "the count goes on from the 3 let go of"
    );
    let warned: Vec<String> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect();
    assert_eq!(
        warned,
        [format!(
            "agent {} has more reports than the 10000 the hub keeps of one machine: its \
             oldest 1 were let go of; the hub counts the next ones without logging each",
            agent_id(1).as_uuid()
        )]
    );
    assert!(!warned.concat().contains(&operation.to_string()));
}

/// The hub's reports, on which every new report lets go of one before it.
#[derive(Default)]
struct FullInbox {
    inner: crate::fakes::InMemoryReportInbox,
    let_go: std::sync::atomic::AtomicU64,
}

#[async_trait]
impl crate::ports::ReportInbox for FullInbox {
    async fn ingest(
        &self,
        agent: domain::AgentId,
        entry: &crate::ports::OutboxEntry,
        received_at: Timestamp,
    ) -> Result<crate::ports::Ingested, crate::ports::StoreError> {
        use std::sync::atomic::Ordering;
        let mut taken = self.inner.ingest(agent, entry, received_at).await?;
        if taken.ingest == domain::fleet::Ingest::New {
            taken.dropped = Some(self.let_go.fetch_add(1, Ordering::SeqCst) + 1);
        }
        Ok(taken)
    }

    async fn reports(
        &self,
        agent: domain::AgentId,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        self.inner.reports(agent).await
    }

    async fn latest(
        &self,
        agent: domain::AgentId,
        limit: usize,
    ) -> Result<crate::ports::LatestReports, crate::ports::StoreError> {
        self.inner.latest(agent, limit).await
    }
}

/// The hub warns that it let go of an agent's reports in the step that let
/// go of them, with no wait between taking the report and the warning: a
/// step cut short after the report was taken, as the transport's idle
/// bound cuts one, can't leave the agent marked as warned with no warning
/// logged (§7.2).
#[tokio::test]
async fn a_step_cut_short_after_letting_go_of_a_report_loses_no_warning() {
    use crate::ports::OutboxEntry;
    use domain::fleet::{AgentReport, Happening, OutboxEpoch, ReportSeq};

    let inbox = Arc::new(FullInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let connecting = connecting(
        identity,
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    );
    let entry = |place| OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(place).unwrap(),
        report: AgentReport {
            op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0b)),
            happening: Happening::Started,
            at: EpochSeconds::from_seconds(1_800_000_000),
        },
    };
    crate::test_log::capture_the_log();
    let (_, mut cut) = connecting
        .connect_channel(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    // Cut short at the step's first wait after the ingest, if it has one.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        cut.send_report(&entry(1)),
    )
    .await;
    let (_, mut next) = connecting
        .connect_channel(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    next.send_report(&entry(2)).await.unwrap();
    next.receive_ack().await.unwrap();
    let warned: Vec<String> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect();
    assert_eq!(
        warned,
        [format!(
            "agent {} has more reports than the 10000 the hub keeps of one machine: its \
             oldest 1 were let go of; the hub counts the next ones without logging each",
            agent_id(1).as_uuid()
        )]
    );
}

/// An agent restored from an older database numbers its new reports again
/// from places of its epoch the hub took before, further behind than a
/// resend: the hub acknowledges them as taken, keeps none, and warns once
/// for that agent while it runs, naming nothing a report says (I4; #1387).
#[tokio::test]
async fn reports_far_behind_the_last_taken_are_warned_of_once() {
    use crate::ports::{Outbox as _, OutboxEntry, ReportInbox as _};
    use crate::sync::{SyncWithHub, Synced};
    use domain::fleet::{AgentReport, Happening, OutboxEpoch, ReportSeq, SEND_WINDOW};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let operation = Uuid::from_u128(0x0c);
    let report = AgentReport {
        op_id: domain::OperationId::from_uuid(operation),
        happening: Happening::Succeeded,
        at: EpochSeconds::from_seconds(1_800_000_000),
    };
    let epoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
    let taken_before = u64::try_from(SEND_WINDOW).unwrap() + 8;
    for place in 1..=taken_before {
        let entry = OutboxEntry {
            epoch,
            seq: ReportSeq::new(place).unwrap(),
            report: report.clone(),
        };
        inbox
            .ingest(agent_id(1), &entry, hub.clock.now())
            .await
            .unwrap();
    }
    // The restored outbox: the same epoch, numbering from its first place.
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(epoch));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity, connector),
        outbox.clone(),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    );
    crate::test_log::capture_the_log();
    for round in [1, 2] {
        for _ in 0..round {
            outbox.append(&report).await.unwrap();
        }
        assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: round }));
    }
    let held = inbox.reports(agent_id(1)).await.unwrap();
    assert_eq!(u64::try_from(held.len()).unwrap(), taken_before);
    let warned: Vec<String> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect();
    assert_eq!(
        warned,
        [format!(
            "agent {} sent a report at place 1 of its outbox, far behind the last the hub \
             took of it: its database may have been restored from an older copy; the hub \
             takes such reports as held already, keeps none of them, and doesn't log the \
             next ones",
            agent_id(1).as_uuid()
        )]
    );
    assert!(!warned.concat().contains(&operation.to_string()));
}

/// An outbox the agent's database can't read.
struct UnreadableOutbox;

#[async_trait]
impl crate::ports::Outbox for UnreadableOutbox {
    async fn append(
        &self,
        _: &domain::fleet::AgentReport,
    ) -> Result<crate::ports::Appended, crate::ports::StoreError> {
        Err(unreadable())
    }

    async fn unacknowledged(
        &self,
        _: Option<domain::fleet::ReportSeq>,
        _: usize,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        Err(unreadable())
    }

    async fn acknowledge(
        &self,
        _: domain::fleet::OutboxEpoch,
        _: domain::fleet::ReportSeq,
    ) -> Result<(), crate::ports::StoreError> {
        Err(unreadable())
    }
}

fn unreadable() -> crate::ports::StoreError {
    crate::ports::StoreError::Unavailable {
        detail: "disk I/O error".to_owned(),
    }
}

/// An outbox of one epoch whose reports lie at the places given, as a full
/// one's lie once it gave up its starts, and which can't record the first
/// `Ack` it is given.
struct GappedOutbox {
    epoch: domain::fleet::OutboxEpoch,
    held: std::sync::Mutex<Vec<crate::ports::OutboxEntry>>,
    recorded_an_ack: std::sync::atomic::AtomicBool,
}

impl GappedOutbox {
    fn at(places: impl IntoIterator<Item = u64>) -> Self {
        let epoch = domain::fleet::OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
        let held = places
            .into_iter()
            .map(|place| crate::ports::OutboxEntry {
                epoch,
                seq: domain::fleet::ReportSeq::new(place).unwrap(),
                report: domain::fleet::AgentReport {
                    op_id: domain::OperationId::from_uuid(Uuid::from_u128(u128::from(place))),
                    happening: domain::fleet::Happening::Succeeded,
                    at: EpochSeconds::from_seconds(1_800_000_000),
                },
            })
            .collect();
        Self {
            epoch,
            held: std::sync::Mutex::new(held),
            recorded_an_ack: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl crate::ports::Outbox for GappedOutbox {
    async fn append(
        &self,
        _: &domain::fleet::AgentReport,
    ) -> Result<crate::ports::Appended, crate::ports::StoreError> {
        Err(unreadable())
    }

    async fn unacknowledged(
        &self,
        after: Option<domain::fleet::ReportSeq>,
        limit: usize,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        let held = self.held.lock().unwrap();
        Ok(held
            .iter()
            .filter(|entry| after.is_none_or(|after| entry.seq > after))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn acknowledge(
        &self,
        epoch: domain::fleet::OutboxEpoch,
        through: domain::fleet::ReportSeq,
    ) -> Result<(), crate::ports::StoreError> {
        use std::sync::atomic::Ordering;
        if !self.recorded_an_ack.swap(true, Ordering::SeqCst) {
            return Err(unreadable());
        }
        if epoch == self.epoch {
            self.held
                .lock()
                .unwrap()
                .retain(|entry| entry.seq > through);
        }
        Ok(())
    }
}

/// A send window spans at most [`SEND_WINDOW`] places, however far apart
/// the outbox's reports lie: the hub tells a report sent again from an
/// older database's by how far behind its last it lies. So a window sent
/// again, its acknowledgements not recorded, is taken as resent, and the
/// hub warns of no older database (§7.2).
///
/// [`SEND_WINDOW`]: domain::fleet::SEND_WINDOW
#[tokio::test]
async fn a_window_sent_again_across_the_outboxs_gaps_is_taken_as_resent() {
    use crate::ports::ReportInbox as _;
    use crate::sync::{SyncWithHub, Synced};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let places: Vec<u64> = (1..=32).map(|n| 2 * n).collect();
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(GappedOutbox::at(places.clone())),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    );
    crate::test_log::capture_the_log();
    assert!(
        sync.sync_once().await.is_err(),
        "the first acknowledgement is recorded"
    );
    assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: 32 }));
    let taken: Vec<u64> = inbox
        .reports(agent_id(1))
        .await
        .unwrap()
        .iter()
        .map(|entry| entry.seq.get())
        .collect();
    assert_eq!(taken, places);
    let warned: Vec<String> = crate::test_log::logged_by("app::pairing::hub")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect();
    assert_eq!(warned, Vec::<String>::new());
}

/// Runs the agent's sync loop through `waits` of its waits on `clock`, and
/// returns the warnings it logged. The loop runs on a thread of its own, so
/// a loop that never waits fails the test within a second rather than
/// holding it up forever.
async fn warned_by_the_sync_loop(
    sync: crate::sync::SyncWithHub,
    clock: &FakeClock,
    waits: usize,
) -> Vec<String> {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let (logged, logs) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let lines = runtime.block_on(async {
            crate::test_log::capture_the_log();
            tokio::select! {
                () = sync.run() => unreachable!("the sync loop runs until it is stopped"),
                _ = stopped => {}
            }
            crate::test_log::logged_by("app::sync")
        });
        let _ = logged.send(lines);
    });
    for _ in 0..waits {
        let mut slept = false;
        for _ in 0..1_000 {
            if let Some(next) = clock.until_next_sleeper() {
                clock.advance(next);
                slept = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(slept, "the sync loop stopped waiting");
    }
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), logs)
        .await
        .expect("the sync loop stops when told")
        .unwrap()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect()
}

/// The agent's sync loop logs a streak of failures once, at its first: a hub
/// it can't reach, and an outbox it can't read, aren't logged at every retry.
#[tokio::test]
async fn the_sync_loop_logs_a_streak_of_failures_once() {
    use crate::ports::Outbox as _;
    use crate::sync::SyncWithHub;
    use domain::fleet::{AgentReport, Happening, OutboxEpoch};

    let hub = hub_with(1, |pairing| pairing);
    let identity = paired(&hub, 1).await;
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(0xe1),
    )));
    outbox
        .append(&AgentReport {
            op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0c)),
            happening: Happening::Started,
            at: EpochSeconds::from_seconds(1_800_000_000),
        })
        .await
        .unwrap();
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    connector.set_down(true);
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_030).unwrap(),
    ));
    let sync = SyncWithHub::new(
        connecting(identity.clone(), connector.clone()),
        outbox,
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    );
    let warned = warned_by_the_sync_loop(sync, &clock, 3).await;
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].starts_with("the agent's reports didn't reach its hub"));

    let sync = SyncWithHub::new(
        connecting(identity, connector),
        Arc::new(UnreadableOutbox),
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    );
    let warned = warned_by_the_sync_loop(sync, &clock, 3).await;
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].starts_with("the outbox couldn't be read"));
}

/// A channel shows its sequence numbers in a log and leaves out its keys.
#[tokio::test]
async fn a_channel_shows_its_sequence_numbers_and_no_key() {
    let hub = hub_with(1, |pairing| pairing);
    let identity = paired(&hub, 1).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let (_, channel) = connecting(identity, connector)
        .connect_channel(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    let next = domain::fleet::FIRST_SEQUENCE + 1;
    assert_eq!(
        format!("{channel:?}"),
        format!("AgentChannel {{ next_sent: {next}, next_received: {next}, .. }}")
    );
}

/// The hub logs an approved agent's first connection since its approval
/// once, even when the approval lands while the hub checks that
/// connection's proof, after it read the agent and before it kept the
/// proof (#1342).
#[tokio::test]
async fn an_approval_during_a_proof_logs_the_first_connection_once() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    hub.clock.advance(SignedDuration::from_secs(30));
    hub.registry.approves_before_next_proof(hub.clock.now());
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    crate::test_log::capture_the_log();
    for _ in 0..2 {
        hub.clock.advance(SignedDuration::from_secs(30));
        let connected = connecting(identity.clone(), connector.clone())
            .connect(agent_id(1), hostname(1), None)
            .await
            .unwrap();
        assert_eq!(connected.approval, Approval::Approved);
    }
    let logged = crate::test_log::logged_by("app::pairing");
    let first: Vec<_> = logged
        .iter()
        .filter(|(_, line)| line.contains("its first connection since it was approved"))
        .collect();
    assert_eq!(first.len(), 1, "{logged:?}");
}

/// A machine with no pin, or no agent key, never connects.
#[tokio::test]
async fn a_machine_never_paired_connects_to_nothing() {
    let hub = hub(1);
    let paired_one = paired(&hub, 1).await;
    let pin_only = Arc::new(InMemoryIdentity::default());
    pin_only
        .write_pin(&paired_one.pin().await.unwrap().unwrap())
        .await
        .unwrap();
    for identity in [Arc::new(InMemoryIdentity::default()), pin_only] {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let failure = connecting(identity, connector.clone())
            .connect(agent_id(2), hostname(2), None)
            .await;
        assert_eq!(failure, Err(ConnectFailure::NotPaired));
        assert!(connector.routes().is_empty());
    }
}

/// A hub with another key than the pinned one gets the agent's `Connect`
/// and nothing after it.
#[tokio::test]
async fn another_hub_gets_no_proof() {
    let pinned = hub(1);
    let identity = paired(&pinned, 1).await;
    let other = hub(2);
    let connector = Arc::new(LoopbackHubConnector::to(other.pairing.clone()));
    let failure = connecting(identity, connector.clone())
        .connect(agent_id(1), hostname(1), None)
        .await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch))
    );
    assert_eq!(connector.sent().len(), 1);
}

/// An agent the hub never registered, and another key under a registered
/// agent's ID, get the same signed refusal, and the hub keeps its record
/// as it was.
#[tokio::test]
async fn an_unknown_agent_and_another_key_get_one_signed_refusal() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let before = hub.registry.agents().await.unwrap();
    for id in [agent_id(9), agent_id(1)] {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let failure = connecting(stranger.clone(), connector)
            .connect(id, hostname(9), None)
            .await;
        let Err(ConnectFailure::Refused(refused)) = failure else {
            panic!("{failure:?}");
        };
        assert_eq!(refused.code.as_str(), "proof-refused");
        assert_eq!(refused.holder, None);
    }
    assert_eq!(hub.registry.agents().await.unwrap(), before);
}

/// The `Hello` carries the fingerprint of the computer the agent runs on,
/// under its ID, and never the identifier itself (decision 150).
#[tokio::test]
async fn a_hello_carries_this_computers_fingerprint_under_the_agents_id() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    connecting_on(identity, connector.clone(), Some(identifier(1)))
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    let told: Vec<_> = connector
        .sent()
        .iter()
        .filter_map(
            |text| match agent_message(&AgentFrame::from_text(text).unwrap()) {
                Ok(AgentMessage::Signed(envelope)) if envelope.kind == hello_kind() => {
                    Some(read_hello(&envelope.payload).unwrap().machine)
                }
                _ => None,
            },
        )
        .collect();
    assert_eq!(
        told,
        [Some(MachineFingerprint::of(agent_id(1), &identifier(1)))]
    );
    assert!(
        connector
            .sent()
            .iter()
            .all(|text| !text.contains(identifier(1).as_str())),
        "the identifier stays on the computer"
    );
}

/// A second computer under one agent's ID, as a copied data folder makes,
/// is refused with a signed `identity-conflict`, keeps neither its reports
/// nor its zone on the hub, and is flagged and warned of once; the first
/// computer is taken as before (I1, I6; decision 150).
#[tokio::test]
async fn a_second_computer_under_one_id_is_refused_and_the_first_kept() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let copy = copied(&identity).await;
    let connect = |identity: &Arc<InMemoryIdentity>, n: u128, zone: Option<MachineZone>| {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let identity = identity.clone();
        async move {
            connecting_on(identity, connector, Some(identifier(n)))
                .connect(agent_id(1), hostname(1), zone)
                .await
        }
    };
    let first = connect(&identity, 1, None).await.unwrap();
    assert_eq!(first.approval, Approval::Approved);
    crate::test_log::capture_the_log();
    let berlin = MachineZone::parse("Europe/Berlin").ok();
    for _ in 0..2 {
        hub.clock.advance(SignedDuration::from_secs(60));
        let failure = connect(&copy, 2, berlin.clone()).await;
        let Err(ConnectFailure::Refused(refused)) = failure else {
            panic!("{failure:?}");
        };
        assert_eq!(refused.code.as_str(), "identity-conflict");
        assert_eq!(refused.holder, None);
    }
    let held = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    assert_eq!(held.zone, None, "the copy's zone isn't kept");
    let listed = hub.pairing.agents().await.unwrap();
    assert_eq!(listed[0].second_machine_at, Some(hub.clock.now()));
    let again = connect(&identity, 1, None).await.unwrap();
    assert_eq!(again.approval, Approval::Approved, "the first is taken");
    let logged = crate::test_log::logged_by("app::pairing");
    let warned: Vec<_> = logged
        .iter()
        .filter(|(level, line)| *level == log::Level::Warn && line.contains("second computer"))
        .collect();
    assert_eq!(warned.len(), 1, "{logged:?}");
    assert!(
        warned[0].1.contains(&agent_id(1).as_uuid().to_string()),
        "{warned:?}"
    );
    assert!(
        warned[0]
            .1
            .contains(&format!("pairing-token --forget-computer {}", hostname(1))),
        "the line names the way back: {warned:?}"
    );
}

/// The way back for a machine refused as a second computer: a person at the
/// hub forgets the computer it took under the ID, and the next one to
/// connect is taken in its place; the one forgotten is then the second
/// (decision 150).
#[tokio::test]
async fn a_forgotten_computer_lets_the_next_one_be_taken_in_its_place() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let copy = copied(&identity).await;
    let connect = |identity: &Arc<InMemoryIdentity>, n: u128| {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let identity = identity.clone();
        async move {
            connecting_on(identity, connector, Some(identifier(n)))
                .connect(agent_id(1), hostname(1), None)
                .await
        }
    };
    connect(&identity, 1).await.unwrap();
    assert!(connect(&copy, 2).await.is_err());
    crate::test_log::capture_the_log();
    let held = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.forget_machine(&held).await.unwrap();
    let listed = hub.pairing.agents().await.unwrap();
    assert_eq!(listed[0].second_machine_at, None);
    let taken = connect(&copy, 2).await.unwrap();
    assert_eq!(taken.approval, Approval::Approved);
    let failure = connect(&identity, 1).await;
    let Err(ConnectFailure::Refused(refused)) = failure else {
        panic!("{failure:?}");
    };
    assert_eq!(refused.code.as_str(), "identity-conflict");
    let logged = crate::test_log::logged_by("app::pairing");
    assert!(
        logged.iter().any(|(level, line)| *level == log::Level::Info
            && line.contains("forgot the computer")
            && line.contains(&agent_id(1).as_uuid().to_string())),
        "{logged:?}"
    );
}

/// A computer whose identifier can't be read tells no fingerprint, and is
/// taken: nothing can be told of it (decision 144).
#[tokio::test]
async fn a_computer_that_tells_no_fingerprint_is_taken() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let first = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    connecting_on(identity.clone(), first, Some(identifier(1)))
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    let untold = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = connecting_on(copied(&identity).await, untold, None)
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    assert_eq!(connected.approval, Approval::Pending);
    let listed = hub.pairing.agents().await.unwrap();
    assert_eq!(listed[0].second_machine_at, None);
}

/// What the hub signs on one connection, to forge its answers with.
struct Forge {
    seed: SigningSeed,
    hub_key: PublicKey,
    agent_id: AgentId,
    agent_nonce: Nonce,
    hub_nonce: Nonce,
}

impl Forge {
    /// `envelope` signed as the hub signs on this connection.
    fn sign(&self, mut envelope: HubEnvelope) -> HubFrame {
        let signed = SignedMessage::HubMessage {
            hub_key: self.hub_key,
            agent_id: self.agent_id,
            agent_nonce: self.agent_nonce,
            hub_nonce: self.hub_nonce,
            binding: Binding::TlsExporter(LOOPBACK_EXPORTER),
            sequence: envelope.seq,
            expires_at: envelope.expires_at,
            kind: &envelope.kind,
            payload: PayloadDigest::of(&envelope.payload),
        };
        envelope.signature = FakeSigning.sign(&self.seed, &signed.to_bytes()).unwrap();
        hub_envelope_frame(&envelope)
    }

    /// The `Welcome` in `frame`, changed by `change` and signed again.
    fn rewelcome(&self, frame: HubFrame, change: impl FnOnce(&mut Welcome)) -> HubFrame {
        let Ok(HubMessage::Signed(mut envelope)) = hub_message(&frame) else {
            panic!("{frame:?}");
        };
        let mut welcome = read_welcome(&envelope.payload).unwrap();
        change(&mut welcome);
        envelope.payload = welcome_payload(&welcome);
        self.sign(envelope)
    }
}

type Rewrite = fn(&Forge, HubFrame) -> HubFrame;

/// A connector to `hub` whose every answer after the `Challenge` is
/// rewritten, as a proxy or a hub's stolen key would.
struct Rewriting {
    hub: Arc<HubPairing>,
    hub_identity: Arc<InMemoryIdentity>,
    rewrite: Rewrite,
}

struct RewritingLink {
    inner: Box<dyn HubLink>,
    hub_identity: Arc<InMemoryIdentity>,
    rewrite: Rewrite,
    connect: Option<(AgentId, Nonce)>,
    challenge: Option<(PublicKey, Nonce)>,
}

#[async_trait]
impl HubConnector for Rewriting {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let inner = LoopbackHubConnector::to(self.hub.clone())
            .connect(route)
            .await?;
        Ok(Box::new(RewritingLink {
            inner,
            hub_identity: self.hub_identity.clone(),
            rewrite: self.rewrite,
            connect: None,
            challenge: None,
        }))
    }
}

#[async_trait]
impl HubLink for RewritingLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.inner.exporter()
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        if let Ok(AgentMessage::Connect(connect)) = agent_message(frame) {
            self.connect = Some((connect.agent_id, connect.agent_nonce));
        }
        self.inner.send(frame).await
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        let frame = self.inner.receive().await?;
        if let Ok(HubMessage::Challenge(challenge)) = hub_message(&frame) {
            self.challenge = Some((challenge.hub_key, challenge.hub_nonce));
            return Ok(frame);
        }
        let (agent_id, agent_nonce) = self.connect.unwrap();
        let (hub_key, hub_nonce) = self.challenge.unwrap();
        let forge = Forge {
            seed: self
                .hub_identity
                .seed(SeedFile::HubKey)
                .await
                .unwrap()
                .unwrap(),
            hub_key,
            agent_id,
            agent_nonce,
            hub_nonce,
        };
        Ok((self.rewrite)(&forge, frame))
    }
}

async fn connect_rewritten(
    hub: &Hub,
    identity: Arc<InMemoryIdentity>,
    id: AgentId,
    rewrite: Rewrite,
) -> Result<Connected, ConnectFailure> {
    let connector = Rewriting {
        hub: hub.pairing.clone(),
        hub_identity: hub.identity.clone(),
        rewrite,
    };
    connecting(identity, Arc::new(connector))
        .connect(id, hostname(1), None)
        .await
}

/// An answer that isn't signed on this connection, or is signed out of
/// sequence, is a dropped connection; a signed message of another kind is
/// out of turn (decision 120, "Signed messages after the handshake").
#[tokio::test]
async fn an_unsigned_or_out_of_sequence_answer_is_a_dropped_connection() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let unsigned_refusal: Rewrite = |_, frame| match frame {
        HubFrame::Refused { code, holder, .. } => HubFrame::Refused {
            code,
            holder,
            signature: "A".repeat(86),
        },
        other => other,
    };
    let failure = connect_rewritten(&hub, stranger, agent_id(9), unsigned_refusal).await;
    assert_eq!(failure, Err(ConnectFailure::Unsigned));
    let altered: Rewrite = |_, frame| match frame {
        HubFrame::Signed {
            kind,
            seq,
            expires_at,
            signature,
            ..
        } => HubFrame::Signed {
            kind,
            seq,
            expires_at,
            payload: "e30".to_owned(),
            signature,
        },
        other => other,
    };
    let second: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        forge.sign(HubEnvelope { seq: 2, ..envelope })
    };
    let later: Rewrite = |_, frame| match frame {
        HubFrame::Signed {
            kind,
            seq,
            expires_at,
            payload,
            signature,
        } => HubFrame::Signed {
            kind,
            seq,
            expires_at: expires_at + 1,
            payload,
            signature,
        },
        other => other,
    };
    for rewrite in [altered, second, later] {
        let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), rewrite).await;
        assert_eq!(failure, Err(ConnectFailure::Unsigned));
    }
    let another_kind: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        forge.sign(HubEnvelope {
            kind: WireName::parse("Removed").unwrap(),
            ..envelope
        })
    };
    let failure = connect_rewritten(&hub, identity, agent_id(1), another_kind).await;
    assert_eq!(failure, Err(ConnectFailure::OutOfTurn));
}

/// A signed `Welcome` the agent can't act on: past its window, with the
/// agent's protocols changed, or of a generation below the pinned one. A
/// higher generation raises the pin's.
#[tokio::test]
async fn a_welcome_is_checked_before_it_is_acted_on() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let past_the_window: Rewrite = |forge, frame| {
        let Ok(HubMessage::Signed(envelope)) = hub_message(&frame) else {
            return frame;
        };
        let welcome = read_welcome(&envelope.payload).unwrap();
        forge.sign(HubEnvelope {
            expires_at: EpochSeconds::from_seconds(welcome.signed_at.seconds() + 901),
            ..envelope
        })
    };
    let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), past_the_window).await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::Expiry))
    );
    let stripped: Rewrite = |forge, frame| {
        forge.rewelcome(frame, |welcome| {
            welcome.agent_protocols = Protocols::new(1, 2).unwrap();
        })
    };
    let failure = connect_rewritten(&hub, identity.clone(), agent_id(1), stripped).await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::Protocols))
    );
    let mut pin = identity.pin().await.unwrap().unwrap();
    pin.generation = 2;
    identity.write_pin(&pin).await.unwrap();
    let failure = connecting(
        identity.clone(),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    )
    .connect(agent_id(1), hostname(1), None)
    .await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Welcome(WelcomeRefused::GenerationLower))
    );
    let raised: Rewrite = |forge, frame| forge.rewelcome(frame, |welcome| welcome.generation = 3);
    let connected = connect_rewritten(&hub, identity.clone(), agent_id(1), raised)
        .await
        .unwrap();
    assert_eq!(connected.pin.generation, 3);
    assert_eq!(identity.pin().await.unwrap().unwrap().generation, 3);
}

/// A connector to `hub` whose link lets `delay` pass on the agent's clock
/// before it hands over a signed message, as a link that held the
/// `Welcome` in a buffer would.
struct Delaying {
    hub: Arc<HubPairing>,
    clock: Arc<FakeClock>,
    delay: SignedDuration,
}

struct DelayingLink {
    inner: Box<dyn HubLink>,
    clock: Arc<FakeClock>,
    delay: SignedDuration,
}

#[async_trait]
impl HubConnector for Delaying {
    async fn connect(&self, route: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let inner = LoopbackHubConnector::to(self.hub.clone())
            .connect(route)
            .await?;
        Ok(Box::new(DelayingLink {
            inner,
            clock: self.clock.clone(),
            delay: self.delay,
        }))
    }
}

#[async_trait]
impl HubLink for DelayingLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        self.inner.exporter()
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        self.inner.send(frame).await
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        let frame = self.inner.receive().await?;
        if let Ok(HubMessage::Signed(_)) = hub_message(&frame) {
            self.clock.advance(self.delay);
        }
        Ok(frame)
    }
}

/// A `Welcome` that reaches the agent as long after its `Hello` as the
/// message lives had expired by then, so the agent acts on none of it,
/// whatever its own wall clock says; one a little sooner is taken
/// (decision 120, "Expiry is judged on the hub's time").
#[tokio::test]
async fn a_welcome_that_arrives_after_its_life_is_not_acted_on() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let life = SignedDuration::from_secs(i64::try_from(domain::fleet::MESSAGE_LIFE).unwrap());
    let connect = |delay: SignedDuration| {
        let clock = Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        ));
        ConnectToHub::new(
            identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(77)),
            clock.clone(),
            Arc::new(Delaying {
                hub: hub.pairing.clone(),
                clock,
                delay,
            }),
            "0.5.0-agent".to_owned(),
            None,
        )
    };
    let late = connect(life).connect(agent_id(1), hostname(1), None).await;
    assert_eq!(late, Err(ConnectFailure::Welcome(WelcomeRefused::Expiry)));
    let in_time = connect(life - SignedDuration::from_secs(1))
        .connect(agent_id(1), hostname(1), None)
        .await;
    assert!(in_time.is_ok(), "{in_time:?}");
}

/// The hub's side on its own: a proof is checked against the key the hub
/// registered, and a `Hello` must be the agent's, first in sequence, and
/// of its kind; the `Welcome` it gets is signed and lives 10 minutes.
#[tokio::test]
async fn the_hub_welcomes_only_a_proved_agents_first_hello() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let keys = hub.pairing.keys().await.unwrap();
    let agent_seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    let agent_nonce = Nonce::from_bytes([3; KEY_BYTES]);
    let connect = domain::fleet::Connect {
        agent_id: agent_id(1),
        agent_nonce,
        binding_kind: domain::fleet::BindingKind::TlsExporter,
    };
    let (challenge, handshake) = hub
        .pairing
        .challenge(&keys, &connect, Some(LOOPBACK_EXPORTER))
        .unwrap();
    let binding = Binding::TlsExporter(LOOPBACK_EXPORTER);
    let proof_bytes = SignedMessage::AgentProof {
        hub_key: *keys.key(),
        agent_id: agent_id(1),
        agent_key: agent_key(&agent_seed),
        agent_nonce,
        hub_nonce: challenge.hub_nonce,
        binding,
        counter: 40,
    }
    .to_bytes();
    let wrong = domain::fleet::Proof {
        counter: 40,
        agent_proof: FakeSigning
            .sign(&SigningSeed::from_bytes([5; KEY_BYTES]), &proof_bytes)
            .unwrap(),
    };
    let refused = hub.pairing.prove(&keys, &handshake, &wrong).await.unwrap();
    assert!(matches!(refused, ProofAnswer::Refused(_)), "{refused:?}");
    let proof = domain::fleet::Proof {
        counter: 40,
        agent_proof: FakeSigning.sign(&agent_seed, &proof_bytes).unwrap(),
    };
    let ProofAnswer::Proved(agent) = hub.pairing.prove(&keys, &handshake, &proof).await.unwrap()
    else {
        panic!("the proof was refused");
    };
    assert_eq!(agent.counter, 40);
    let hello = |seq: u64, kind: WireName, seed: &SigningSeed| {
        let payload = crate::pairing::wire::hello_payload(&domain::fleet::Hello {
            agent_version: "0.5.0-agent".to_owned(),
            hostname: hostname(1),
            protocols: domain::fleet::PROTOCOLS,
            clock: EpochSeconds::from_seconds(1_800_000_000),
            zone: None,
            machine: None,
        });
        let signed = SignedMessage::AgentMessage {
            hub_key: *keys.key(),
            agent_id: agent_id(1),
            agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            sequence: seq,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        domain::fleet::AgentEnvelope {
            signature: FakeSigning.sign(seed, &signed.to_bytes()).unwrap(),
            kind,
            seq,
            payload,
        }
    };
    let other_seed = SigningSeed::from_bytes([5; KEY_BYTES]);
    for (sent, refusal) in [
        (
            hello(1, hello_kind(), &other_seed),
            MessageRefused::Unsigned,
        ),
        (
            hello(2, hello_kind(), &agent_seed),
            MessageRefused::OutOfSequence,
        ),
        (
            hello(1, WireName::parse("Goodbye").unwrap(), &agent_seed),
            MessageRefused::Unexpected,
        ),
    ] {
        assert_eq!(
            hub.pairing
                .welcome(&keys, &handshake, &agent, &sent)
                .await
                .unwrap(),
            Err(refusal)
        );
    }
    let Ok(HelloAnswer::Welcome(welcome)) = hub
        .pairing
        .welcome(
            &keys,
            &handshake,
            &agent,
            &hello(1, hello_kind(), &agent_seed),
        )
        .await
        .unwrap()
    else {
        panic!("a welcome");
    };
    assert_eq!(welcome.seq, 1);
    assert_eq!(welcome.kind, domain::fleet::welcome_kind());
    assert_eq!(
        welcome.expires_at,
        EpochSeconds::from_seconds(1_800_000_600)
    );
    let signed = SignedMessage::HubMessage {
        hub_key: *keys.key(),
        agent_id: agent_id(1),
        agent_nonce,
        hub_nonce: challenge.hub_nonce,
        binding,
        sequence: 1,
        expires_at: welcome.expires_at,
        kind: &welcome.kind,
        payload: PayloadDigest::of(&welcome.payload),
    };
    assert!(FakeSigning.verify(keys.key(), &signed.to_bytes(), &welcome.signature));
    let read = read_welcome(&welcome.payload).unwrap();
    assert_eq!(read.approval, Approval::Pending);
    assert_eq!(read.hub_version, "0.5.0-hub");
    assert_eq!(read.protocol, Some(1));
}

/// The hub's new address in these tests; the token's is
/// `https://hub.example:9443`.
fn new_address() -> HubAddress {
    HubAddress::parse("https://hub-new.example:9443").unwrap()
}

/// The host and port of each route connected to, in order.
fn hosts(connector: &LoopbackHubConnector) -> Vec<String> {
    connector
        .routes()
        .into_iter()
        .map(|route| format!("{}:{}", route.host, route.port))
        .collect()
}

fn proofs(connector: &LoopbackHubConnector) -> usize {
    connector
        .sent()
        .iter()
        .filter(|text| matches!(AgentFrame::from_text(text), Ok(AgentFrame::Proof { .. })))
        .count()
}

/// Decision 120, "Addresses and the transport statement": an address the
/// person gives is kept once the pinned hub proved itself there, and tried
/// first from then on; the token's address stays the fallback.
#[tokio::test]
async fn an_address_the_person_gave_is_kept_once_the_hub_proves_itself_there() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let moved =
        || Arc::new(LoopbackHubConnector::to(hub.pairing.clone()).only_at("hub-new.example", 9443));
    let given = moved();
    let connected = connecting(identity.clone(), given.clone())
        .connect_at(agent_id(1), hostname(1), None, new_address())
        .await
        .unwrap();
    assert_eq!(connected.approval, Approval::Pending);
    let pin = identity.pin().await.unwrap().unwrap();
    assert_eq!(pin.given_address, Some(new_address()));
    assert_eq!(connected.pin, pin);
    assert_eq!(hosts(&given), ["hub-new.example:9443"]);
    let again = moved();
    connecting(identity.clone(), again.clone())
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    assert_eq!(hosts(&again), ["hub-new.example:9443"]);
    let back = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()).only_at("hub.example", 9443));
    connecting(identity, back.clone())
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    assert_eq!(hosts(&back), ["hub-new.example:9443", "hub.example:9443"]);
    assert_eq!(proofs(&back), 1);
}

/// An address where the pinned hub doesn't prove itself is not kept:
/// another hub's, or one where nothing answers. The agent tries only the
/// address it was given.
#[tokio::test]
async fn an_address_where_the_pinned_hub_doesnt_prove_itself_is_not_kept() {
    let pinned = hub(1);
    let identity = paired(&pinned, 1).await;
    let before = identity.pin().await.unwrap().unwrap();
    let other = hub(2);
    let failure = connecting(
        identity.clone(),
        Arc::new(LoopbackHubConnector::to(other.pairing.clone())),
    )
    .connect_at(agent_id(1), hostname(1), None, new_address())
    .await;
    assert_eq!(
        failure,
        Err(ConnectFailure::Challenge(ChallengeRefused::HubKeyMismatch))
    );
    let unanswered = Arc::new(LoopbackHubConnector::unanswered());
    let failure = connecting(identity.clone(), unanswered.clone())
        .connect_at(agent_id(1), hostname(1), None, new_address())
        .await;
    assert!(
        matches!(
            failure,
            Err(ConnectFailure::Link(LinkError::Unreachable { .. }))
        ),
        "{failure:?}"
    );
    assert_eq!(hosts(&unanswered), ["hub-new.example:9443"]);
    assert_eq!(identity.pin().await.unwrap().unwrap(), before);
}

/// Once the pinned hub has proved itself at an address, its answer stands:
/// the agent tries no other address after it.
#[tokio::test]
async fn once_the_pinned_hub_proves_itself_no_other_address_is_tried() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    connecting(
        identity.clone(),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    )
    .connect_at(agent_id(1), hostname(1), None, new_address())
    .await
    .unwrap();
    let stranger = with_pin_of(&identity).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let failure = connecting(stranger, connector.clone())
        .connect(agent_id(9), hostname(9), None)
        .await;
    assert!(
        matches!(failure, Err(ConnectFailure::Refused(_))),
        "{failure:?}"
    );
    assert_eq!(hosts(&connector), ["hub-new.example:9443"]);
}

/// When no address leads to the pinned hub, the agent says what went wrong
/// at the first, the one the person gave, and proved itself to no one.
#[tokio::test]
async fn when_no_address_reaches_the_hub_the_first_failure_is_said() {
    let pinned = hub(1);
    let identity = paired(&pinned, 1).await;
    connecting(
        identity.clone(),
        Arc::new(LoopbackHubConnector::to(pinned.pairing.clone())),
    )
    .connect_at(agent_id(1), hostname(1), None, new_address())
    .await
    .unwrap();
    let other = hub(2);
    let connector =
        Arc::new(LoopbackHubConnector::to(other.pairing.clone()).only_at("hub.example", 9443));
    let failure = connecting(identity, connector.clone())
        .connect(agent_id(1), hostname(1), None)
        .await;
    assert!(
        matches!(
            failure,
            Err(ConnectFailure::Link(LinkError::Unreachable { .. }))
        ),
        "{failure:?}"
    );
    assert_eq!(
        hosts(&connector),
        ["hub-new.example:9443", "hub.example:9443"]
    );
    assert_eq!(proofs(&connector), 0);
}

/// The pinned hub proving itself at an address isn't enough: an address
/// is kept only once the hub's `Welcome` holds, so a hub that refuses the
/// proof leaves the pin as it was.
#[tokio::test]
async fn an_address_where_the_hub_refuses_the_proof_is_not_kept() {
    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let before = stranger.pin().await.unwrap().unwrap();
    let failure = connecting(
        stranger.clone(),
        Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
    )
    .connect_at(agent_id(9), hostname(9), None, new_address())
    .await;
    assert!(
        matches!(failure, Err(ConnectFailure::Refused(_))),
        "{failure:?}"
    );
    assert_eq!(stranger.pin().await.unwrap().unwrap(), before);
}

/// The `Ack` in `frame`, sent as `sent` and signed as `signed` change it;
/// any other frame as it is.
fn rewrite_ack(
    forge: &Forge,
    frame: HubFrame,
    signed: fn(&mut HubEnvelope),
    sent: fn(&mut HubEnvelope),
) -> HubFrame {
    let Ok(HubMessage::Signed(mut envelope)) = hub_message(&frame) else {
        return frame;
    };
    if envelope.kind != domain::fleet::ack_kind() {
        return frame;
    }
    signed(&mut envelope);
    let Ok(HubMessage::Signed(mut envelope)) = hub_message(&forge.sign(envelope)) else {
        panic!("a signed frame");
    };
    sent(&mut envelope);
    hub_envelope_frame(&envelope)
}

/// One report, of place 1 in a new epoch.
fn one_report() -> crate::ports::OutboxEntry {
    use domain::fleet::{AgentReport, Happening, OutboxEpoch, ReportSeq};
    crate::ports::OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(1).unwrap(),
        report: AgentReport {
            op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0d)),
            happening: Happening::Started,
            at: EpochSeconds::from_seconds(1_800_000_000),
        },
    }
}

/// Agent 1, paired with `hub` and approved there.
async fn approved(hub: &Hub) -> Arc<InMemoryIdentity> {
    let identity = paired(hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    identity
}

fn keeping_reports(seed: u64) -> Hub {
    hub_with(seed, |pairing| {
        pairing.with_inbox(Arc::new(crate::fakes::InMemoryReportInbox::default()))
    })
}

/// An acknowledgement signed on this connection but out of sequence, or in
/// sequence but not signed for it, is a dropped connection and never acted
/// on (decision 120, "Signed messages after the handshake").
#[tokio::test]
async fn an_ack_out_of_sequence_or_not_signed_for_it_is_not_acted_on() {
    use crate::pairing::ChannelError;

    let hub = keeping_reports(1);
    let identity = approved(&hub).await;
    let entry = one_report();
    let as_sent: Rewrite = |forge, frame| rewrite_ack(forge, frame, |_| {}, |_| {});
    let out_of_sequence: Rewrite = |forge, frame| {
        rewrite_ack(
            forge,
            frame,
            |ack| ack.seq = ack.seq.saturating_add(1),
            |_| {},
        )
    };
    let not_signed_for_it: Rewrite = |forge, frame| {
        rewrite_ack(
            forge,
            frame,
            |ack| ack.seq = ack.seq.saturating_add(1),
            |ack| ack.seq = ack.seq.saturating_sub(1),
        )
    };
    for (rewrite, acted_on) in [
        (as_sent, true),
        (out_of_sequence, false),
        (not_signed_for_it, false),
    ] {
        let connector = Rewriting {
            hub: hub.pairing.clone(),
            hub_identity: hub.identity.clone(),
            rewrite,
        };
        let (_, mut channel) = connecting(identity.clone(), Arc::new(connector))
            .connect_channel(agent_id(1), hostname(1), None)
            .await
            .unwrap();
        channel.send_report(&entry).await.unwrap();
        let ack = channel.receive_ack().await;
        if acted_on {
            assert!(ack.is_ok(), "{ack:?}");
        } else {
            assert_eq!(ack, Err(ChannelError::Unsigned));
        }
    }
}

/// A connector to `hub` that notes, after each frame the agent sends,
/// whether the hub's session then takes the agent's signed messages, and
/// each reason the hub closes with. When `twice`, each report reaches the
/// hub twice, as a replayed frame would.
#[derive(Default)]
struct Watching {
    hub: Option<Arc<HubPairing>>,
    syncing: Arc<std::sync::Mutex<Vec<bool>>>,
    closes: Arc<std::sync::Mutex<Vec<&'static str>>>,
    twice: bool,
}

struct WatchingLink {
    hub: Arc<HubPairing>,
    session: crate::pairing::HubSession,
    answers: std::collections::VecDeque<HubFrame>,
    syncing: Arc<std::sync::Mutex<Vec<bool>>>,
    closes: Arc<std::sync::Mutex<Vec<&'static str>>>,
    twice: bool,
}

#[async_trait]
impl HubConnector for Watching {
    async fn connect(&self, _: &HubRoute) -> Result<Box<dyn HubLink>, LinkError> {
        let hub = self.hub.clone().expect("a hub to watch");
        Ok(Box::new(WatchingLink {
            session: hub.session(Some(LOOPBACK_EXPORTER)).await.unwrap(),
            hub,
            answers: std::collections::VecDeque::new(),
            syncing: self.syncing.clone(),
            closes: self.closes.clone(),
            twice: self.twice,
        }))
    }
}

#[async_trait]
impl HubLink for WatchingLink {
    fn exporter(&self) -> Option<[u8; KEY_BYTES]> {
        Some(LOOPBACK_EXPORTER)
    }

    async fn send(&mut self, frame: &AgentFrame) -> Result<(), LinkError> {
        use crate::pairing::HubStep;
        let report = matches!(
            agent_message(frame),
            Ok(AgentMessage::Signed(envelope)) if envelope.kind == domain::fleet::report_kind()
        );
        let times = if self.twice && report { 2 } else { 1 };
        for _ in 0..times {
            match self.hub.step(&mut self.session, frame).await {
                HubStep::Reply(answer) | HubStep::Last(answer) => self.answers.push_back(answer),
                HubStep::Close(reason) => self.closes.lock().unwrap().push(reason),
                HubStep::Wait | HubStep::Fail(_) => {}
            }
        }
        self.syncing
            .lock()
            .unwrap()
            .push(self.session.syncing_agent().is_some());
        Ok(())
    }

    async fn receive(&mut self) -> Result<HubFrame, LinkError> {
        self.answers
            .pop_front()
            .ok_or(LinkError::Closed { reason: None })
    }
}

/// Whether the hub's session took the agent's signed messages after each
/// frame of agent 1's connection to `hub`.
async fn syncing_after(hub: &Hub, identity: Arc<InMemoryIdentity>) -> Vec<bool> {
    let connector = Arc::new(Watching {
        hub: Some(hub.pairing.clone()),
        ..Watching::default()
    });
    connecting(identity, connector.clone())
        .connect(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    connector.syncing.lock().unwrap().clone()
}

/// A report the hub gets again on its connection, with the sequence number
/// it was signed with, closes the connection as a sequence error (decision
/// 120, "Signed messages after the handshake").
#[tokio::test]
async fn a_report_sent_again_on_its_connection_closes_it_as_a_sequence_error() {
    let hub = keeping_reports(1);
    let identity = approved(&hub).await;
    let connector = Arc::new(Watching {
        hub: Some(hub.pairing.clone()),
        twice: true,
        ..Watching::default()
    });
    let (_, mut channel) = connecting(identity, connector.clone())
        .connect_channel(agent_id(1), hostname(1), None)
        .await
        .unwrap();
    channel.send_report(&one_report()).await.unwrap();
    assert_eq!(
        *connector.closes.lock().unwrap(),
        [crate::pairing::SEQUENCE_ERROR]
    );
}

/// A connection goes on past the `Welcome`, taking the agent's reports,
/// only for an agent a person approved and on a hub that keeps reports;
/// only then does the transport hold it open past the handshake (§7.2).
#[tokio::test]
async fn a_connection_syncs_only_for_an_approved_agent_on_a_hub_that_keeps_reports() {
    let keeping = hub_with(1, |pairing| {
        pairing.with_inbox(Arc::new(crate::fakes::InMemoryReportInbox::default()))
    });
    let identity = paired(&keeping, 1).await;
    assert_eq!(
        syncing_after(&keeping, identity.clone()).await,
        [false, false, false]
    );
    let waiting = keeping.registry.agent(agent_id(1)).await.unwrap().unwrap();
    keeping.pairing.approve(&waiting).await.unwrap().unwrap();
    assert_eq!(
        syncing_after(&keeping, identity).await,
        [false, false, true]
    );

    let keeping_none = hub(2);
    let identity = paired(&keeping_none, 1).await;
    let waiting = keeping_none
        .registry
        .agent(agent_id(1))
        .await
        .unwrap()
        .unwrap();
    keeping_none
        .pairing
        .approve(&waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        syncing_after(&keeping_none, identity).await,
        [false, false, false]
    );
}
