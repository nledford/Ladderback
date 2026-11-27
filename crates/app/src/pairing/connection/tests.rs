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
        Arc::new(crate::fakes::InMemoryLeaseCache::default()),
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
            new_epoch: false,
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

/// The key files of `inner`, counting each write of the pin.
struct CountingPinWrites {
    inner: Arc<InMemoryIdentity>,
    writes: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl crate::ports::Identity for CountingPinWrites {
    async fn seed(
        &self,
        file: SeedFile,
    ) -> Result<Option<SigningSeed>, crate::ports::IdentityError> {
        self.inner.seed(file).await
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, crate::ports::IdentityError> {
        self.inner.keep_seed(file, seed).await
    }

    async fn forget_recovery_seed(&self) -> Result<(), crate::ports::IdentityError> {
        self.inner.forget_recovery_seed().await
    }

    async fn renew_agent_key(&self, seed: SigningSeed) -> Result<(), crate::ports::IdentityError> {
        self.inner.renew_agent_key(seed).await
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, crate::ports::IdentityError> {
        self.inner.recovery_key().await
    }

    async fn keep_recovery_key(
        &self,
        key: PublicKey,
    ) -> Result<PublicKey, crate::ports::IdentityError> {
        self.inner.keep_recovery_key(key).await
    }

    async fn tls_key(&self) -> Result<Option<domain::fleet::TlsKey>, crate::ports::IdentityError> {
        self.inner.tls_key().await
    }

    async fn keep_tls_key(
        &self,
        key: domain::fleet::TlsKey,
    ) -> Result<domain::fleet::TlsKey, crate::ports::IdentityError> {
        self.inner.keep_tls_key(key).await
    }

    async fn pin(&self) -> Result<Option<domain::fleet::HubPin>, crate::ports::IdentityError> {
        self.inner.pin().await
    }

    async fn write_pin(
        &self,
        pin: &domain::fleet::HubPin,
    ) -> Result<(), crate::ports::IdentityError> {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.write_pin(pin).await
    }

    async fn forget_pin(&self) -> Result<(), crate::ports::IdentityError> {
        self.inner.forget_pin().await
    }

    async fn begin_attempt(&self) -> Result<crate::ports::Attempt, crate::ports::IdentityError> {
        self.inner.begin_attempt().await
    }
}

/// A welcome at the generation the pin already holds writes no pin: the
/// key files are written only when the hub's pin changes.
#[tokio::test]
async fn a_welcome_at_the_pins_own_generation_writes_no_pin() {
    let hub = hub(1);
    let identity = approved(&hub).await;
    let pinned = identity.pin().await.unwrap();
    let counting = Arc::new(CountingPinWrites {
        inner: identity.clone(),
        writes: std::sync::atomic::AtomicUsize::new(0),
    });
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let connected = ConnectToHub::new(
        counting.clone(),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(77)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        connector,
        "0.5.0-agent".to_owned(),
        None,
    )
    .connect(agent_id(1), hostname(1), None)
    .await
    .unwrap();
    assert_eq!(connected.approval, Approval::Approved);
    assert_eq!(counting.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(identity.pin().await.unwrap(), pinned);
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
    // Each report its own operation's, as the hub holds an operation's
    // happening once (#1513).
    let report = |n: u64| AgentReport {
        op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0b_0000_0000 + u128::from(n))),
        happening: Happening::Started,
        at: EpochSeconds::from_seconds(1_800_000_000),
    };
    let earlier = OutboxEpoch::from_uuid(Uuid::from_u128(0xe0));
    for place in 1..=MAX_REPORTS_HELD {
        let entry = OutboxEntry {
            epoch: earlier,
            seq: ReportSeq::new(place).unwrap(),
            report: report(place),
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
    let mut fresh = MAX_REPORTS_HELD;
    for round in [2, 1] {
        for _ in 0..round {
            fresh += 1;
            outbox.append(&report(fresh)).await.unwrap();
        }
        assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: round }));
    }
    let next = OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(4).unwrap(),
        report: report(fresh + 1),
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
    for n in MAX_REPORTS_HELD + 1..=fresh {
        let operation = Uuid::from_u128(0x0b_0000_0000 + u128::from(n));
        assert!(!warned.concat().contains(&operation.to_string()));
    }
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

    async fn keep_plans(
        &self,
        agent: domain::AgentId,
        plans: &domain::fleet::PlansReport,
    ) -> Result<(), crate::ports::StoreError> {
        self.inner.keep_plans(agent, plans).await
    }

    async fn plans(
        &self,
        agent: domain::AgentId,
    ) -> Result<Option<domain::fleet::PlansReport>, crate::ports::StoreError> {
        self.inner.plans(agent).await
    }

    async fn keep_progress(
        &self,
        agent: domain::AgentId,
        progress: &domain::fleet::ReportedProgress,
    ) -> Result<(), crate::ports::StoreError> {
        self.inner.keep_progress(agent, progress).await
    }

    async fn progress(
        &self,
        agent: domain::AgentId,
    ) -> Result<Option<domain::fleet::ReportedProgress>, crate::ports::StoreError> {
        self.inner.progress(agent).await
    }

    async fn taken_through(
        &self,
        agent: domain::AgentId,
        epoch: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::ReportSeq>, crate::ports::StoreError> {
        self.inner.taken_through(agent, epoch).await
    }

    async fn diverged(
        &self,
        agent: domain::AgentId,
        epoch: domain::fleet::OutboxEpoch,
    ) -> Result<bool, crate::ports::StoreError> {
        self.inner.diverged(agent, epoch).await
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
        .connect_channel(agent_id(1), hostname(1), None, None)
        .await
        .unwrap();
    // Cut short at the step's first wait after the ingest, if it has one.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        cut.send_report(&entry(1)),
    )
    .await;
    let (_, mut next) = connecting
        .connect_channel(agent_id(1), hostname(1), None, None)
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

/// A 0.6.0 agent restored from an older database, whose `Hello` says
/// nothing of its outbox, numbers its new reports again from places of its
/// epoch the hub took before, further behind than a resend: the hub
/// acknowledges them as taken, keeps none, and warns once for that agent
/// while it runs, naming nothing a report says (I4; #1387, decision 177).
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
    // Each report its own operation's, as the hub holds an operation's
    // happening once (#1513).
    let report = |n: u64| AgentReport {
        op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0c_0000_0000 + u128::from(n))),
        happening: Happening::Succeeded,
        at: EpochSeconds::from_seconds(1_800_000_000),
    };
    let epoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
    let taken_before = u64::try_from(SEND_WINDOW).unwrap() + 8;
    for place in 1..=taken_before {
        let entry = OutboxEntry {
            epoch,
            seq: ReportSeq::new(place).unwrap(),
            report: report(place),
        };
        inbox
            .ingest(agent_id(1), &entry, hub.clock.now())
            .await
            .unwrap();
    }
    // The restored outbox: the same epoch, numbering from its first place,
    // stating no position, as 0.6.0's `Hello`.
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(epoch));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity, connector),
        Arc::new(UnstatedOutbox(outbox.clone())),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    );
    crate::test_log::capture_the_log();
    let mut resent = taken_before;
    for round in [1, 2] {
        for _ in 0..round {
            resent += 1;
            outbox.append(&report(resent)).await.unwrap();
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
    for n in taken_before + 1..=resent {
        let operation = Uuid::from_u128(0x0c_0000_0000 + u128::from(n));
        assert!(!warned.concat().contains(&operation.to_string()));
    }
}

/// An outbox that states no position, as a 0.6.0 agent's `Hello` doesn't,
/// and otherwise is the in-memory outbox it wraps.
struct UnstatedOutbox(Arc<crate::fakes::InMemoryOutbox>);

#[async_trait]
impl crate::ports::Outbox for UnstatedOutbox {
    async fn append(
        &self,
        report: &domain::fleet::AgentReport,
    ) -> Result<crate::ports::Appended, crate::ports::StoreError> {
        self.0.append(report).await
    }

    async fn unacknowledged(
        &self,
        after: Option<domain::fleet::ReportSeq>,
        limit: usize,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        self.0.unacknowledged(after, limit).await
    }

    async fn acknowledge(
        &self,
        epoch: domain::fleet::OutboxEpoch,
        through: domain::fleet::ReportSeq,
    ) -> Result<(), crate::ports::StoreError> {
        self.0.acknowledge(epoch, through).await
    }

    async fn position(
        &self,
    ) -> Result<Option<domain::fleet::OutboxPosition>, crate::ports::StoreError> {
        Ok(None)
    }

    async fn start_epoch(
        &self,
        from: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::OutboxEpoch>, crate::ports::StoreError> {
        self.0.start_epoch(from).await
    }
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

    async fn position(
        &self,
    ) -> Result<Option<domain::fleet::OutboxPosition>, crate::ports::StoreError> {
        Err(unreadable())
    }

    async fn start_epoch(
        &self,
        _: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::OutboxEpoch>, crate::ports::StoreError> {
        Err(unreadable())
    }
}

fn unreadable() -> crate::ports::StoreError {
    crate::ports::StoreError::Unavailable {
        detail: "disk I/O error".to_owned(),
    }
}

/// A configuration the agent can't read.
struct UnreadableConfig;

#[async_trait]
impl crate::ports::ConfigStore for UnreadableConfig {
    async fn load(&self) -> Result<domain::planning::AgentConfig, crate::ports::ConfigError> {
        Err(crate::ports::ConfigError::Unavailable {
            file: "ladderback.toml".to_owned(),
            detail: "unreadable".to_owned(),
        })
    }

    async fn save(
        &self,
        _: &domain::planning::AgentConfig,
    ) -> Result<(), crate::ports::ConfigError> {
        Ok(())
    }

    async fn record_id(
        &self,
        _: &domain::RepositoryAlias,
        _: &domain::planning::RepositoryConfig,
        _: &domain::RepositoryId,
    ) -> Result<(), crate::ports::RecordError> {
        Ok(())
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

    /// It says nothing of where it stands, so the hub never tells it to
    /// start a new epoch.
    async fn position(
        &self,
    ) -> Result<Option<domain::fleet::OutboxPosition>, crate::ports::StoreError> {
        Ok(None)
    }

    async fn start_epoch(
        &self,
        _: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::OutboxEpoch>, crate::ports::StoreError> {
        Ok(None)
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

/// An outbox that gives its position and reports as `InMemoryOutbox` does
/// but can't start a new epoch, as on a full or read-only disk.
struct StuckEpochOutbox(crate::fakes::InMemoryOutbox);

#[async_trait]
impl crate::ports::Outbox for StuckEpochOutbox {
    async fn append(
        &self,
        report: &domain::fleet::AgentReport,
    ) -> Result<crate::ports::Appended, crate::ports::StoreError> {
        self.0.append(report).await
    }

    async fn unacknowledged(
        &self,
        after: Option<domain::fleet::ReportSeq>,
        limit: usize,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        self.0.unacknowledged(after, limit).await
    }

    async fn acknowledge(
        &self,
        epoch: domain::fleet::OutboxEpoch,
        through: domain::fleet::ReportSeq,
    ) -> Result<(), crate::ports::StoreError> {
        self.0.acknowledge(epoch, through).await
    }

    async fn position(
        &self,
    ) -> Result<Option<domain::fleet::OutboxPosition>, crate::ports::StoreError> {
        self.0.position().await
    }

    async fn start_epoch(
        &self,
        _: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::OutboxEpoch>, crate::ports::StoreError> {
        Err(unreadable())
    }
}

/// An outbox whose reports can be read but whose position can't, as after
/// one busy moment of the store.
struct UnreadablePositionOutbox(crate::fakes::InMemoryOutbox);

#[async_trait]
impl crate::ports::Outbox for UnreadablePositionOutbox {
    async fn append(
        &self,
        report: &domain::fleet::AgentReport,
    ) -> Result<crate::ports::Appended, crate::ports::StoreError> {
        self.0.append(report).await
    }

    async fn unacknowledged(
        &self,
        after: Option<domain::fleet::ReportSeq>,
        limit: usize,
    ) -> Result<Vec<crate::ports::OutboxEntry>, crate::ports::StoreError> {
        self.0.unacknowledged(after, limit).await
    }

    async fn acknowledge(
        &self,
        epoch: domain::fleet::OutboxEpoch,
        through: domain::fleet::ReportSeq,
    ) -> Result<(), crate::ports::StoreError> {
        self.0.acknowledge(epoch, through).await
    }

    async fn position(
        &self,
    ) -> Result<Option<domain::fleet::OutboxPosition>, crate::ports::StoreError> {
        Err(unreadable())
    }

    async fn start_epoch(
        &self,
        from: domain::fleet::OutboxEpoch,
    ) -> Result<Option<domain::fleet::OutboxEpoch>, crate::ports::StoreError> {
        self.0.start_epoch(from).await
    }
}

/// A restored machine whose outbox position can't be read ends the session
/// as a store failure with nothing sent, rather than connecting as a 0.6.0
/// machine whose reports at places the hub took are acknowledged and let
/// go of (decision 177).
#[tokio::test]
async fn an_outbox_position_that_cant_be_read_sends_nothing() {
    use crate::ports::{Outbox as _, ReportInbox as _};
    use crate::sync::{SyncFailure, SyncWithHub};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = approved(&hub).await;
    let taken = one_report();
    let now = Timestamp::from_second(1_800_000_030).unwrap();
    inbox.ingest(agent_id(1), &taken, now).await.unwrap();
    let restored = Arc::new(UnreadablePositionOutbox(crate::fakes::InMemoryOutbox::new(
        taken.epoch,
    )));
    restored
        .append(&domain::fleet::AgentReport {
            op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x10)),
            happening: domain::fleet::Happening::Succeeded,
            at: EpochSeconds::from_seconds(1_800_000_000),
        })
        .await
        .unwrap();
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        restored.clone(),
        Arc::new(FakeClock::new(now)),
        agent_id(1),
        hostname(1),
        None,
    );
    assert!(matches!(sync.sync_once().await, Err(SyncFailure::Store(_))));
    assert_eq!(
        restored.unacknowledged(None, 10).await.unwrap().len(),
        1,
        "the restored report is kept to send under a new epoch"
    );
}

/// A machine told to start a new epoch whose outbox can't start one ends
/// the session as a store failure and sends nothing under the old epoch,
/// so the hub drops none of its reports as taken (decision 177).
#[tokio::test]
async fn a_new_epoch_that_cant_be_started_sends_nothing() {
    use crate::ports::{Outbox as _, ReportInbox as _};
    use crate::sync::{SyncFailure, SyncWithHub};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let epoch = domain::fleet::OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
    let report = |n: u128| domain::fleet::AgentReport {
        op_id: domain::OperationId::from_uuid(Uuid::from_u128(n)),
        happening: domain::fleet::Happening::Succeeded,
        at: domain::fleet::EpochSeconds::from_seconds(1_800_000_000),
    };
    let now = Timestamp::from_second(1_800_000_030).unwrap();
    for n in 1..=3 {
        let entry = crate::ports::OutboxEntry {
            epoch,
            seq: domain::fleet::ReportSeq::new(n).unwrap(),
            report: report(u128::from(n)),
        };
        inbox.ingest(agent_id(1), &entry, now).await.unwrap();
    }
    let restored = crate::fakes::InMemoryOutbox::new(epoch);
    restored.append(&report(0x10)).await.unwrap();
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(StuckEpochOutbox(restored)),
        Arc::new(FakeClock::new(now)),
        agent_id(1),
        hostname(1),
        None,
    );
    assert!(
        matches!(sync.sync_once().await, Err(SyncFailure::Store(_))),
        "the new epoch isn't started"
    );
    assert_eq!(inbox.reports(agent_id(1)).await.unwrap().len(), 3);
    assert_eq!(
        inbox.taken_through(agent_id(1), epoch).await.unwrap(),
        domain::fleet::ReportSeq::new(3),
        "nothing was sent under the old epoch"
    );
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
/// it can't reach, an outbox it can't read, and a configuration it can't
/// read its leases from, aren't logged at every retry.
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

    // An outbox it can't read ends each session before it connects, as its
    // position can't be stated (decision 177): one streak, one warning.
    let hub = keeping_reports(2);
    let identity = approved(&hub).await;
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(UnreadableOutbox),
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    );
    let warned = warned_by_the_sync_loop(sync, &clock, 3).await;
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].starts_with("the agent's reports didn't reach its hub: Store"));

    // A configuration it can't read leaves the leases unasked at every look
    // of the one connection it holds to a hub that approved it.
    let Leasing {
        hub,
        leases,
        operations,
        others,
        identity,
        machine_clock,
        ..
    } = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(Arc::new(UnreadableConfig), leases, operations, others);
    let warned = warned_by_the_sync_loop(sync, &machine_clock, 3).await;
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].starts_with("the leases weren't asked for"));
}

/// The agent's sync loop opens a session for a progress alone: with no
/// report waiting in its outbox and no lease to ask for, the progress of
/// the operation it runs still reaches the hub (§7.2, #1440).
#[tokio::test]
async fn the_sync_loop_tells_the_hub_a_progress_with_no_report_waiting() {
    use crate::ports::ReportInbox as _;
    use crate::sync::{LiveProgress, SyncWithHub};
    use api::{OperationStep, ProgressUnit, UiEvent};
    use domain::fleet::OutboxEpoch;

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = approved(&hub).await;
    let progress = Arc::new(LiveProgress::new());
    progress.hear(&UiEvent::OperationProgress {
        operation: Uuid::from_u128(0x0d).to_string(),
        step: Some(OperationStep::BackingUp),
        unit: ProgressUnit::Bytes,
        current: 40,
        total: Some(100),
        at: Some("2027-01-15T08:00:05Z".to_owned()),
    });
    let told = progress.latest().expect("the progress is kept");
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_030).unwrap(),
    ));
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_progress(progress);

    let warned = warned_by_the_sync_loop(sync, &clock, 2).await;
    assert!(warned.is_empty(), "{warned:?}");
    assert_eq!(inbox.progress(agent_id(1)).await.unwrap(), Some(told));
}

/// The agent's sync loop tells the hub a report waiting in its outbox once
/// its connection holds, with nothing else due: no progress, no plans and
/// no lease to ask for.
#[tokio::test]
async fn the_sync_loop_tells_the_hub_a_report_waiting_in_its_outbox() {
    use crate::ports::{Outbox as _, ReportInbox as _};
    use crate::sync::SyncWithHub;
    use domain::fleet::OutboxEpoch;

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let identity = approved(&hub).await;
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(0xe1),
    )));
    let entry = one_report();
    outbox.append(&entry.report).await.unwrap();
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_030).unwrap(),
    ));
    let sync = SyncWithHub::new(
        connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        outbox,
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    );

    let warned = warned_by_the_sync_loop(sync, &clock, 2).await;
    assert!(warned.is_empty(), "{warned:?}");
    let told: Vec<_> = inbox
        .reports(agent_id(1))
        .await
        .unwrap()
        .into_iter()
        .map(|told| told.report)
        .collect();
    assert_eq!(told, vec![entry.report]);
}

/// A channel shows its sequence numbers in a log and leaves out its keys.
#[tokio::test]
async fn a_channel_shows_its_sequence_numbers_and_no_key() {
    let hub = hub_with(1, |pairing| pairing);
    let identity = paired(&hub, 1).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let (_, channel) = connecting(identity, connector)
        .connect_channel(agent_id(1), hostname(1), None, None)
        .await
        .unwrap();
    let next = domain::fleet::FIRST_SEQUENCE + 1;
    assert_eq!(
        format!("{channel:?}"),
        format!("AgentChannel {{ next_sent: {next}, next_received: {next}, .. }}")
    );
}

/// An acknowledgement the hub never sent is never taken: a link the hub
/// drops before its plans or heartbeat acknowledgement ends the wait for
/// it with the link's failure.
#[tokio::test]
async fn a_link_dropped_before_an_acknowledgement_fails_the_wait_for_it() {
    use crate::pairing::ChannelError;

    let hub = hub_with(1, |pairing| pairing);
    let identity = approved(&hub).await;
    for kind in ["plans", "heartbeat"] {
        let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
        let (_, mut channel) = connecting(identity.clone(), connector.clone())
            .connect_channel(agent_id(1), hostname(1), None, None)
            .await
            .unwrap();
        connector.drop_links().await;
        let waited = if kind == "plans" {
            channel.receive_plans_ack().await
        } else {
            channel.receive_heartbeat_ack().await.map(|_| ())
        };
        assert!(
            matches!(waited, Err(ChannelError::Link(_))),
            "{kind}: {waited:?}"
        );
    }
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

/// Only a refusal as a second computer stands the machine refused by its
/// hub: another signed refusal, as a key the hub doesn't hold gets, leaves
/// it as it was (#1462).
#[tokio::test]
async fn only_a_signed_identity_conflict_stands_the_machine_refused() {
    use crate::machine::{MachineStanding, Standing};
    use crate::sync::{SyncFailure, SyncWithHub};
    use domain::fleet::OutboxEpoch;

    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let stranger = with_pin_of(&identity).await;
    let standing = Arc::new(MachineStanding::new(Standing::Same));
    let sync = SyncWithHub::new(
        connecting(
            stranger,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        ),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_standing(standing.clone());
    let failure = sync.sync_once().await;
    let Err(SyncFailure::Connect(ConnectFailure::Refused(refused))) = failure else {
        panic!("{failure:?}");
    };
    assert_eq!(refused.code.as_str(), "proof-refused");
    assert_eq!(standing.get(), Standing::Same);
}

/// Only the paired hub's signed `machine-removed` stands the machine removed
/// by its hub, so it may take a new machine ID: the same refusal with a
/// signature the hub didn't make leaves it as it was (#1489).
#[tokio::test]
async fn only_a_signed_agent_removed_stands_the_machine_removed() {
    use crate::machine::{MachineStanding, Standing};
    use crate::sync::{SyncFailure, SyncWithHub};
    use domain::fleet::OutboxEpoch;

    let hub = hub(1);
    let identity = paired(&hub, 1).await;
    let record = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.registry
        .remove(agent_id(1), record.key, hub.clock.now())
        .await
        .unwrap()
        .unwrap();
    let standing = Arc::new(MachineStanding::new(Standing::Same));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity.clone(), connector.clone()),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_standing(standing.clone());

    connector.forge_refusals();
    let failure = sync.sync_once().await;
    assert!(
        matches!(failure, Err(SyncFailure::Connect(ConnectFailure::Unsigned))),
        "{failure:?}"
    );
    assert_eq!(standing.get(), Standing::Same);

    let signed = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity, signed),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_030).unwrap(),
        )),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_standing(standing.clone());
    let failure = sync.sync_once().await;
    let Err(SyncFailure::Connect(ConnectFailure::Refused(refused))) = failure else {
        panic!("{failure:?}");
    };
    assert_eq!(refused.code.as_str(), "machine-removed");
    assert_eq!(standing.get(), Standing::RemovedByHub);
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
            outbox: None,
            accepts: None,
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
    let Ok(HelloAnswer::Welcome {
        envelope: welcome, ..
    }) = hub
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
            .connect_channel(agent_id(1), hostname(1), None, None)
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
    /// The hub's answer to a release is lost on the way, as when the
    /// connection drops after the hub took it.
    drop_released: bool,
}

struct WatchingLink {
    hub: Arc<HubPairing>,
    session: crate::pairing::HubSession,
    answers: std::collections::VecDeque<HubFrame>,
    syncing: Arc<std::sync::Mutex<Vec<bool>>>,
    closes: Arc<std::sync::Mutex<Vec<&'static str>>>,
    twice: bool,
    drop_released: bool,
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
            drop_released: self.drop_released,
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
        let release = matches!(
            agent_message(frame),
            Ok(AgentMessage::Signed(envelope)) if envelope.kind == domain::fleet::release_kind()
        );
        for _ in 0..times {
            match self.hub.step(&mut self.session, frame).await {
                HubStep::Reply(_) | HubStep::Last(_) if self.drop_released && release => {}
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
        .connect_channel(agent_id(1), hostname(1), None, None)
        .await
        .unwrap();
    channel.send_report(&one_report()).await.unwrap();
    assert_eq!(
        *connector.closes.lock().unwrap(),
        [crate::pairing::SEQUENCE_ERROR]
    );
}

/// A report unlike the one the hub took at its place closes the connection
/// as `outbox-diverged` (decision 177, §7.2).
#[tokio::test]
async fn a_report_unlike_the_one_taken_at_its_place_closes_as_outbox_diverged() {
    let hub = keeping_reports(1);
    let identity = approved(&hub).await;
    let connector = Arc::new(Watching {
        hub: Some(hub.pairing.clone()),
        ..Watching::default()
    });
    let taken = one_report();
    let (_, mut channel) = connecting(identity, connector.clone())
        .connect_channel(
            agent_id(1),
            hostname(1),
            None,
            Some(domain::fleet::OutboxPosition {
                epoch: taken.epoch,
                next: domain::fleet::ReportSeq::new(2).unwrap(),
            }),
        )
        .await
        .unwrap();
    channel.send_report(&taken).await.unwrap();
    channel.receive_ack().await.unwrap();
    let mut unlike = taken;
    unlike.report.happening = domain::fleet::Happening::Succeeded;
    let _ = channel.send_report(&unlike).await;
    assert_eq!(
        *connector.closes.lock().unwrap(),
        [crate::pairing::OUTBOX_DIVERGED]
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

/// The repository `photos`, as its entry records it.
fn photos_config(maintenance: domain::planning::Maintenance) -> domain::planning::AgentConfig {
    use std::collections::BTreeMap;

    use domain::planning::{
        AgentConfig, AgentSettings, GroupBy, Hooks, RepositoryConfig, RepositorySettings,
    };
    use domain::secrets::{SecretRef, SecretValue};

    let repository = RepositoryConfig {
        locations: Default::default(),
        id: Some(photos()),
        settings: RepositorySettings {
            repository: "/srv/photos".to_owned(),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        BTreeMap::from([(
            domain::RepositoryAlias::parse("photos").unwrap(),
            repository,
        )]),
        Vec::new(),
    )
    .unwrap()
}

fn photos() -> domain::RepositoryId {
    domain::RepositoryId::parse(&"0a".repeat(32)).unwrap()
}

/// An approved agent on a hub that decides leases, with the repository
/// `photos`, which its entry asks to maintain, and a sync that asks for its
/// leases, on a clock of the machine's own from `machine_start`.
struct Leasing {
    hub: Hub,
    owners: Arc<crate::fakes::InMemoryOwnershipRegistry>,
    configs: Arc<crate::fakes::InMemoryConfigStore>,
    cache: Arc<crate::fakes::InMemoryLeaseCache>,
    leases: Arc<crate::leases::MaintenanceLeases>,
    machine_clock: Arc<FakeClock>,
    sync: crate::sync::SyncWithHub,
    identity: Arc<InMemoryIdentity>,
    operations: Arc<crate::fakes::InMemoryOperationStore>,
    others: Arc<crate::others_snapshots::OthersSnapshots>,
}

async fn leasing(machine_start: Timestamp) -> Leasing {
    use domain::fleet::OutboxEpoch;
    use domain::planning::Maintenance;

    use crate::fakes::{InMemoryConfigStore, InMemoryLeaseCache, InMemoryOwnershipRegistry};
    use crate::leases::MaintenanceLeases;
    use crate::ownership::RepositoryOwners;
    use crate::ports::ConfigStore as _;
    use crate::sync::SyncWithHub;

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let owners = Arc::new(InMemoryOwnershipRegistry::new());
    let hub = hub(1);
    let hub = Hub {
        pairing: Arc::new(
            HubPairing::new(
                hub.registry.clone(),
                hub.identity.clone(),
                Arc::new(FakeSigning),
                Arc::new(SeededRandom::new(1)),
                hub.clock.clone(),
                "0.5.0-hub".to_owned(),
            )
            .with_inbox(inbox)
            .with_owners(Arc::new(RepositoryOwners::new(
                hub.registry.clone(),
                owners.clone(),
                hub.clock.clone(),
            ))),
        ),
        ..hub
    };
    let identity = paired(&hub, 1).await;
    let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
    hub.pairing.approve(&waiting).await.unwrap().unwrap();
    let configs = Arc::new(InMemoryConfigStore::new());
    configs
        .save(&photos_config(Maintenance::Local))
        .await
        .unwrap();
    let machine_clock = Arc::new(FakeClock::new(machine_start));
    let cache = Arc::new(InMemoryLeaseCache::default());
    let leases = Arc::new(MaintenanceLeases::new(
        identity.clone(),
        cache.clone(),
        machine_clock.clone(),
    ));
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let operations = Arc::new(crate::fakes::InMemoryOperationStore::new());
    // The machine listed `photos` and found no other agent's snapshots, so
    // its claims are the hub's to grant.
    let others = Arc::new(crate::others_snapshots::OthersSnapshots::default());
    others.listed(photos(), agent_id(1), machine_start, &[]);
    let sync = SyncWithHub::new(
        connecting(identity.clone(), connector),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(
        configs.clone(),
        leases.clone(),
        operations.clone(),
        others.clone(),
    );
    Leasing {
        hub,
        owners,
        configs,
        cache,
        leases,
        machine_clock,
        sync,
        identity,
        operations,
        others,
    }
}

/// A sync of the machine `leasing` sets up, on a connection that loses the
/// hub's answer to each release.
fn losing_released(leasing: &Leasing) -> crate::sync::SyncWithHub {
    use domain::fleet::OutboxEpoch;

    let connector = Arc::new(Watching {
        hub: Some(leasing.hub.pairing.clone()),
        drop_released: true,
        ..Watching::default()
    });
    crate::sync::SyncWithHub::new(
        connecting(leasing.identity.clone(), connector),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe2),
        ))),
        leasing.machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(
        leasing.configs.clone(),
        leasing.leases.clone(),
        leasing.operations.clone(),
        leasing.others.clone(),
    )
}

/// A machine lets go of a lease before it sends its release, so a release
/// the hub took but whose answer was lost leaves the machine holding
/// nothing: were the entry to ask to maintain the repository again, it
/// runs nothing under a lease the hub may have granted elsewhere (§7.3,
/// I1).
#[tokio::test]
async fn i1_a_release_whose_answer_is_lost_leaves_the_machine_holding_nothing() {
    use domain::maintenance::NoLease;
    use domain::planning::Maintenance;

    use crate::ports::ConfigStore as _;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    assert_eq!(
        leasing
            .leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
    leasing
        .configs
        .save(&photos_config(Maintenance::Hub))
        .await
        .unwrap();
    assert!(losing_released(&leasing).sync_once().await.is_err());
    assert_eq!(leasing.leases.held().await.unwrap(), vec![]);
    leasing
        .configs
        .save(&photos_config(Maintenance::Local))
        .await
        .unwrap();
    assert_eq!(
        leasing.leases.permits(Some(&photos())).await,
        Err(NoLease::NoneHeld)
    );
}

/// A machine releases nothing while a maintenance of the repository runs
/// here, though its entry no longer asks to maintain it: the hub would
/// otherwise grant the repository to another machine mid-run, so it counts
/// the lease until it runs out instead (§7.3, I1).
#[tokio::test]
async fn i1_a_lease_is_not_released_while_maintenance_runs_on_its_repository() {
    use domain::execution::{Operation, OperationKind, Target, Trigger};
    use domain::planning::Maintenance;

    use crate::ports::{ConfigStore as _, OperationStore as _, OwnershipRegistry as _, Start};

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    let at = leasing.machine_clock.now();
    let target = Target {
        plan: None,
        repository: Some(photos()),
    };
    let id = domain::OperationId::from_uuid(Uuid::from_u128(0x0619));
    let mut prune = Operation::queue(id, OperationKind::Prune, Trigger::Manual, target, at);
    prune.start(at).unwrap();
    assert_eq!(
        leasing.operations.try_start(&prune).await,
        Ok(Start::Started)
    );
    leasing
        .configs
        .save(&photos_config(Maintenance::Hub))
        .await
        .unwrap();
    leasing.sync.sync_once().await.unwrap();
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    assert_eq!(
        held.binding(leasing.hub.clock.now())
            .map(|lease| lease.holder),
        Some(agent_id(1)),
        "the hub still counts the lease"
    );
}

/// When the machine's lease on `photos` runs out on its own clock, as its
/// cache keeps it.
async fn kept_end(cache: &crate::fakes::InMemoryLeaseCache) -> Option<Timestamp> {
    use crate::ports::LeaseCache as _;
    cache
        .load()
        .await
        .unwrap()
        .leases()
        .find(|(repository, _)| **repository == photos())
        .map(|(_, lease)| lease.ends_at)
}

/// With no report to send, the agent's sync loop asks for its grants once
/// it starts and again every 15 minutes, so a lease its hub renews never
/// runs out while the hub can be reached (§7.3, I1).
#[tokio::test]
async fn i1_the_sync_loop_asks_for_grants_at_its_start_and_every_15_minutes() {
    use crate::sync::{GRANTS_EVERY, POLL};

    let Leasing {
        cache,
        machine_clock,
        sync,
        ..
    } = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let polls = usize::try_from(GRANTS_EVERY.as_secs() / POLL.as_secs()).unwrap();
    let cache_seen = cache.clone();
    let clock = machine_clock.clone();
    // The first session, at the loop's start, takes the hub's grant.
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let runner = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::select! {
                () = sync.run() => unreachable!("the sync loop runs until it is stopped"),
                _ = stopped => {}
            }
        });
    });
    let mut ends = Vec::new();
    for _ in 0..=polls {
        let mut slept = false;
        for _ in 0..1_000 {
            if let Some(next) = clock.until_next_sleeper() {
                ends.push(kept_end(&cache_seen).await);
                clock.advance(next);
                slept = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(slept, "the sync loop stopped waiting");
    }
    stop.send(()).unwrap();
    runner.join().unwrap();
    let first = ends
        .first()
        .copied()
        .flatten()
        .expect("granted at the start");
    // Until 15 minutes have passed the loop asks for nothing more; at 15
    // minutes it asks again, and its lease reaches 15 minutes further.
    assert!(
        ends.iter().take(polls).all(|end| *end == Some(first)),
        "{ends:?}"
    );
    assert_eq!(
        ends.last().copied().flatten(),
        first.checked_add(GRANTS_EVERY).ok(),
        "{ends:?}"
    );
}

/// A configuration the agent can't read is its own trouble, not its
/// hub's: the sync loop asks for no leases but keeps its one connection,
/// with its heartbeats, so the machine stays online (§7.2; §16, decision
/// 159).
#[tokio::test]
async fn an_unreadable_configuration_keeps_the_connection_to_the_hub() {
    use domain::fleet::OutboxEpoch;

    use crate::sync::{POLL, SyncWithHub};

    let start = Timestamp::from_second(1_800_000_000).unwrap();
    let Leasing {
        hub,
        leases,
        operations,
        others,
        identity,
        machine_clock,
        ..
    } = leasing(start).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let sync = SyncWithHub::new(
        connecting(identity, connector.clone()),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(Arc::new(UnreadableConfig), leases, operations, others);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let runner = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::select! {
                () = sync.run() => unreachable!("the sync loop runs until it is stopped"),
                _ = stopped => {}
            }
        });
    });
    // A minute of polls, two heartbeats among them.
    let polls = 60 / POLL.as_secs();
    for _ in 0..polls {
        let mut slept = false;
        for _ in 0..1_000 {
            if let Some(next) = machine_clock.until_next_sleeper() {
                machine_clock.advance(next);
                slept = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(slept, "the sync loop stopped waiting");
    }
    stop.send(()).unwrap();
    runner.join().unwrap();
    assert_eq!(connector.routes().len(), 1, "one connection held");
    assert!(hub.pairing.is_online(agent_id(1)));
}

/// An approved agent's session asks for its leases: the hub grants the
/// lease on the repository it maintains, signed at the hub's time, and the
/// agent holds it for the lease's life on that time though its own clock
/// runs 3 days behind; once its entry no longer asks to maintain the
/// repository, the next session releases the lease and the hub ends it
/// (§7.2, §7.3, I1).
#[tokio::test]
async fn i1_a_session_takes_the_hubs_grants_on_its_time_and_releases_what_isnt_asked_for() {
    use domain::maintenance::NoLease;
    use domain::planning::Maintenance;

    use crate::ports::{ConfigStore as _, OwnershipRegistry as _};
    use crate::sync::Synced;

    // The machine's clock runs 3 days behind the hub's.
    let behind = Timestamp::from_second(1_800_000_000 - 3 * 24 * 3600).unwrap();
    let Leasing {
        hub,
        owners,
        configs,
        leases,
        machine_clock,
        sync,
        ..
    } = leasing(behind).await;
    assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: 0 }));
    let held = owners.ownership(&photos()).await.unwrap();
    let lease = held
        .binding(hub.clock.now())
        .expect("the hub granted a lease");
    assert_eq!(lease.holder, agent_id(1));
    assert_eq!(
        leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
    // 7 days less a minute later on both clocks, the lease still holds; at
    // 7 days it ran out, whatever the machine's own clock says.
    machine_clock.advance(SignedDuration::from_secs(7 * 24 * 3600 - 60));
    assert_eq!(
        leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
    machine_clock.advance(SignedDuration::from_secs(60));
    assert_eq!(leases.permits(Some(&photos())).await, Err(NoLease::RanOut));
    // Back within the lease, the entry gives maintenance to the hub: the
    // next session releases the lease, and the hub ends it.
    machine_clock.set_back(SignedDuration::from_secs(3600));
    configs
        .save(&photos_config(Maintenance::Hub))
        .await
        .unwrap();
    assert_eq!(sync.sync_once().await, Ok(Synced::Replayed { sent: 0 }));
    assert_eq!(leases.held().await.unwrap(), vec![]);
    let ended = owners.ownership(&photos()).await.unwrap();
    assert_eq!(ended.binding(hub.clock.now()), None);
}

/// A snapshot the agent `agent` tagged, made at `time`.
fn tagged_by(agent: AgentId, time: Timestamp) -> crate::ports::SnapshotSummary {
    use domain::execution::plan_tags::{MACHINE_TAG_PREFIX, PLAN_TAG_PREFIX};

    crate::ports::SnapshotSummary {
        id: domain::SnapshotId::parse(&"5".repeat(64)).unwrap(),
        time,
        offset: domain::Offset::UTC,
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: vec![
            format!("{PLAN_TAG_PREFIX}home"),
            format!("{MACHINE_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
        ],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
        tags_truncated: false,
    }
}

/// A machine whose listing of a repository it asks to maintain showed
/// another agent's snapshot of 3 days ago tells its hub so, and the hub
/// grants it no lease, though no other paired machine reports the
/// repository (§7.3, I1).
#[tokio::test]
async fn i1_a_session_reports_another_agents_recent_snapshots_and_is_granted_no_lease() {
    use domain::maintenance::NoLease;

    use crate::ports::OwnershipRegistry as _;

    let start = Timestamp::from_second(1_800_000_000).unwrap();
    let leasing = leasing(start).await;
    leasing.others.listed(
        photos(),
        agent_id(1),
        start,
        &[tagged_by(
            agent_id(9),
            start - SignedDuration::from_hours(3 * 24),
        )],
    );
    leasing.sync.sync_once().await.unwrap();
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    assert_eq!(held.binding(leasing.hub.clock.now()), None);
    assert_eq!(
        leasing.leases.permits(Some(&photos())).await,
        Err(NoLease::NoneHeld)
    );
}

/// A repository no listing has shown since the daemon started may hold
/// another agent's snapshots, so the session reports that it does, and
/// the hub grants no lease on it (§7.3, I1).
#[tokio::test]
async fn i1_a_session_reports_a_repository_never_listed_as_holding_another_agents_snapshots() {
    use domain::fleet::OutboxEpoch;

    use crate::ports::OwnershipRegistry as _;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let never_listed = crate::sync::SyncWithHub::new(
        connecting(
            leasing.identity.clone(),
            Arc::new(LoopbackHubConnector::to(leasing.hub.pairing.clone())),
        ),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe3),
        ))),
        leasing.machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(
        leasing.configs.clone(),
        leasing.leases.clone(),
        leasing.operations.clone(),
        Arc::default(),
    );
    never_listed.sync_once().await.unwrap();
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    assert_eq!(held.binding(leasing.hub.clock.now()), None);
}

/// A sync of the machine `leasing` sets up that has listed nothing since it
/// started, and lists through `engine` before each session, on `connector`.
fn listing_first(
    leasing: &Leasing,
    engine: Arc<dyn crate::ports::Engine>,
    connector: Arc<LoopbackHubConnector>,
) -> crate::sync::SyncWithHub {
    use domain::fleet::OutboxEpoch;

    use crate::others_snapshots::{ListingsDue, NoteListings, OthersSnapshots};

    let seen = Arc::new(OthersSnapshots::default());
    let listing = crate::snapshots::ListSnapshots::new(
        Arc::new(NoteListings::new(
            engine,
            seen.clone(),
            agent_id(1),
            leasing.machine_clock.clone(),
        )),
        leasing.machine_clock.clone(),
        crate::secrets::ResolveRepository::new(Arc::new(crate::fakes::FakeSecrets::default())),
        Arc::default(),
    );
    let listings = Arc::new(ListingsDue::new(
        leasing.configs.clone(),
        leasing.operations.clone(),
        seen.clone(),
        listing,
        Arc::new(SeededRandom::new(3)),
        agent_id(1),
        leasing.machine_clock.clone(),
    ));
    crate::sync::SyncWithHub::new(
        connecting(leasing.identity.clone(), connector),
        Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe4),
        ))),
        leasing.machine_clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_leases(
        leasing.configs.clone(),
        leasing.leases.clone(),
        leasing.operations.clone(),
        Arc::clone(listings.seen()),
    )
    .with_listings(listings)
}

/// A machine that hasn't listed a repository it asks to maintain since it
/// started lists it before its first session, so that session reports
/// what the repository holds and the hub grants the lease, with no person
/// opening anything (§7.3; §16, decision 156; I1).
#[tokio::test]
async fn i1_a_session_lists_an_unlisted_repository_first_and_is_granted_its_lease() {
    use crate::ports::OwnershipRegistry as _;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let engine = Arc::new(crate::fakes::ListingEngine::default());
    let sync = listing_first(
        &leasing,
        engine.clone(),
        Arc::new(LoopbackHubConnector::to(leasing.hub.pairing.clone())),
    );
    sync.sync_once().await.unwrap();
    assert_eq!(engine.listings(&photos()), 1);
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    let lease = held
        .binding(leasing.hub.clock.now())
        .expect("the hub granted a lease");
    assert_eq!(lease.holder, agent_id(1));
    assert_eq!(
        leasing
            .leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
}

/// A listing before a session that fails leaves the repository reported as
/// holding another agent's snapshots, so the hub grants nothing; the next
/// session lists it again, and is granted the lease once a listing succeeds
/// (decision 156, I1).
#[tokio::test]
async fn i1_a_failed_listing_before_a_session_is_refused_and_tried_again_at_the_next() {
    use crate::ports::OwnershipRegistry as _;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let engine = Arc::new(crate::fakes::ListingEngine::default());
    engine.fails(photos(), true);
    let sync = listing_first(
        &leasing,
        engine.clone(),
        Arc::new(LoopbackHubConnector::to(leasing.hub.pairing.clone())),
    );
    sync.sync_once().await.unwrap();
    assert_eq!(engine.listings(&photos()), 1);
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    assert_eq!(held.binding(leasing.hub.clock.now()), None);
    engine.fails(photos(), false);
    sync.sync_once().await.unwrap();
    assert_eq!(engine.listings(&photos()), 2);
    assert_eq!(
        leasing
            .leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
}

/// The machine's report of `photos`, as its sync sends it, with the floor
/// it holds and that floor's proof.
async fn reported_photos(leasing: &Leasing) -> domain::maintenance::RepositoryReports {
    use domain::maintenance::{RepositoryReport, RepositoryReports};
    use domain::planning::Maintenance;

    let report = RepositoryReport {
        repository: photos(),
        request: Maintenance::Local,
        holds_other_agents_snapshots: false,
        floor: None,
    };
    RepositoryReports::new(leasing.leases.with_floors(vec![report]).await.unwrap())
}

/// Has the machine take a grant of `photos` at epoch `number`, signed by
/// `keys` at the hub's time for 7 days, as a hub granted it before the copy
/// it was restored from lost the grant; under the recovery number of the
/// floor the machine holds.
async fn take_grant_at(leasing: &Leasing, keys: &crate::pairing::HubKeys, number: u64) {
    use domain::fleet::GENERATION;
    use domain::maintenance::{Lease, LeaseEpoch};

    use crate::ports::LeaseCache as _;

    let held = leasing.cache.load().await.unwrap();
    let recovery_number = held
        .floors()
        .find(|(repository, _)| **repository == photos())
        .map(|(_, floor)| floor.recovery_number)
        .expect("the machine was granted photos before");
    let signed_at = leasing.hub.clock.now();
    let lease = Lease {
        holder: agent_id(1),
        epoch: LeaseEpoch::new(number).unwrap(),
        expires_at: signed_at
            .checked_add(SignedDuration::from_hours(7 * 24))
            .unwrap(),
    };
    let grants = crate::pairing::signed_grants(
        &FakeSigning,
        keys,
        agent_id(1),
        GENERATION,
        signed_at,
        vec![(photos(), lease)],
    )
    .unwrap();
    let refused = leasing
        .leases
        .take(&grants, recovery_number, leasing.machine_clock.now())
        .await
        .unwrap();
    assert_eq!(refused, vec![]);
}

/// A machine keeps each grant with the hub's signature over its entry
/// alone, and shows it back with its floor: the hub that signed it takes it
/// as proof of the epoch (decision 137).
#[tokio::test]
async fn i1_a_grant_is_kept_with_a_proof_its_hub_takes_back() {
    use domain::maintenance::LeaseEpoch;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    let reports = reported_photos(&leasing).await;
    let keys = leasing.hub.pairing.keys().await.unwrap();
    let proven = crate::pairing::proven_floors(&FakeSigning, &keys, agent_id(1), &reports);
    assert_eq!(
        proven
            .iter()
            .map(|floor| (floor.repository.clone(), floor.epoch, floor.held))
            .collect::<Vec<_>>(),
        vec![(photos(), LeaseEpoch::new(1).unwrap(), true)]
    );
}

/// A hub restored from an older copy hears a floor it signed above the
/// epoch it counts: it enters recovery mode and withholds its grants
/// (`GrantsWithheld`), and the machine keeps the lease it holds, which
/// runs out on its own (decision 137, I1).
#[tokio::test]
async fn i1_a_hub_that_hears_a_proven_floor_above_its_counter_withholds_its_grants() {
    use crate::ports::OwnershipRegistry as _;

    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    let keys = leasing.hub.pairing.keys().await.unwrap();
    take_grant_at(&leasing, &keys, 5).await;
    let end = kept_end(&leasing.cache).await;
    assert!(!leasing.hub.pairing.recovering().await.unwrap());
    leasing.machine_clock.advance(SignedDuration::from_secs(60));
    leasing.hub.clock.advance(SignedDuration::from_secs(60));
    leasing.sync.sync_once().await.unwrap();
    assert!(leasing.hub.pairing.recovering().await.unwrap());
    assert_eq!(kept_end(&leasing.cache).await, end, "nothing renewed");
    assert_eq!(
        leasing
            .leases
            .permits(Some(&photos()))
            .await
            .map(|epoch| epoch.is_some()),
        Ok(true)
    );
    let held = leasing.owners.ownership(&photos()).await.unwrap();
    assert_eq!(
        held.binding(leasing.hub.clock.now())
            .map(|lease| lease.epoch.get()),
        Some(1),
        "the hub granted nothing above its counter"
    );
}

/// A hub in recovery mode warns once that a machine's report found it
/// there, however many reports do; a person's confirmation ends it once
/// and says so, and a second confirmation finds nothing to end (decision
/// 137).
#[tokio::test]
async fn i1_recovery_mode_is_warned_of_once_and_a_confirmation_ends_it_once() {
    use std::net::{IpAddr, Ipv4Addr};

    use domain::access::step_up::StepUpConfirmation;

    use crate::pairing::RecoveryAsk;

    crate::test_log::capture_the_log();
    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    let keys = leasing.hub.pairing.keys().await.unwrap();
    take_grant_at(&leasing, &keys, 5).await;
    for _ in 0..3 {
        leasing.machine_clock.advance(SignedDuration::from_secs(60));
        leasing.hub.clock.advance(SignedDuration::from_secs(60));
        leasing.sync.sync_once().await.unwrap();
    }
    assert!(leasing.hub.pairing.recovering().await.unwrap());
    let warned = |text: &str| {
        crate::test_log::logged_by("app::pairing")
            .into_iter()
            .filter(|(level, line)| *level == log::Level::Warn && line.contains(text))
            .count()
    };
    assert_eq!(warned("proved a lease above what this hub counts"), 1);
    let ask = || RecoveryAsk {
        peer: IpAddr::V4(Ipv4Addr::LOCALHOST),
        beyond_loopback: false,
        confirmation: Some(
            StepUpConfirmation::parse(Fingerprint::of_key(keys.key()).to_string()).unwrap(),
        ),
    };
    assert_eq!(
        leasing.hub.pairing.end_recovery_asked(ask()).await,
        Ok(true)
    );
    assert!(!leasing.hub.pairing.recovering().await.unwrap());
    assert_eq!(
        leasing.hub.pairing.end_recovery_asked(ask()).await,
        Ok(false)
    );
    assert_eq!(warned("recovery mode ended"), 1);
}

/// A floor whose proof another hub's key signed doesn't count: the hub
/// logs it, stays out of recovery mode and grants as before (decision 137,
/// "A report counts only with its proof").
#[tokio::test]
async fn i1_a_floor_another_hubs_key_signed_is_logged_and_doesnt_count() {
    crate::test_log::capture_the_log();
    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    leasing.sync.sync_once().await.unwrap();
    let other = hub(2).pairing.keys().await.unwrap();
    take_grant_at(&leasing, &other, 5).await;
    let reports = reported_photos(&leasing).await;
    let keys = leasing.hub.pairing.keys().await.unwrap();
    assert_eq!(
        crate::pairing::proven_floors(&FakeSigning, &keys, agent_id(1), &reports),
        vec![]
    );
    leasing.sync.sync_once().await.unwrap();
    assert!(!leasing.hub.pairing.recovering().await.unwrap());
    let warned: Vec<String> = crate::test_log::logged_by("app::pairing")
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, line)| line)
        .collect();
    let unproven = format!(
        "agent {} reported an epoch floor of 5 on repository {}",
        agent_id(1).as_uuid(),
        photos().as_str()
    );
    assert!(
        warned.iter().any(|line| line.starts_with(&unproven)),
        "{warned:?}"
    );
}

/// The hub keeps the commands a machine's `Hello` said it takes with its
/// live connection (decision 176); a machine whose `Hello` doesn't say, as
/// a 0.6.0 one, is kept apart from one that takes none.
#[tokio::test]
async fn the_hub_keeps_the_commands_a_live_machine_takes() {
    use crate::ports::OutboxEntry;
    use domain::fleet::{Accepts, AgentReport, Happening, OutboxEpoch, ReportSeq};

    let entry = |place| OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        seq: ReportSeq::new(place).unwrap(),
        report: AgentReport {
            op_id: domain::OperationId::from_uuid(Uuid::from_u128(0x0b)),
            happening: Happening::Started,
            at: EpochSeconds::from_seconds(1_800_000_000),
        },
    };
    for (accepts, kept) in [
        (Some(Accepts::all()), Some(Accepts::all())),
        (Some(Accepts::none()), Some(Accepts::none())),
        (None, None),
    ] {
        let inbox = Arc::new(FullInbox::default());
        let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
        let identity = paired(&hub, 1).await;
        let waiting = hub.registry.agent(agent_id(1)).await.unwrap().unwrap();
        hub.pairing.approve(&waiting).await.unwrap().unwrap();
        let connecting = connecting(
            identity,
            Arc::new(LoopbackHubConnector::to(hub.pairing.clone())),
        );
        let (_, mut channel) = connecting
            .connect_accepting(agent_id(1), hostname(1), None, None, accepts)
            .await
            .unwrap();
        channel.send_report(&entry(1)).await.unwrap();
        channel.receive_ack().await.unwrap();
        assert!(hub.pairing.is_online(agent_id(1)));
        assert_eq!(hub.pairing.accepts_of(agent_id(1)), kept);
    }
}

/// The hub's `run-plan` of `photos`.
fn photos_plan() -> domain::fleet::HubCommand {
    domain::fleet::HubCommand::RunPlan {
        plan: domain::PlanId::parse("photos").unwrap(),
    }
}

/// A person's ask on loopback that machine `agent` run `photos`.
fn photos_asked_of(agent: AgentId) -> crate::pairing::CommandAsk {
    crate::pairing::CommandAsk {
        agent,
        command: photos_plan(),
        asked_by: None,
        beyond_loopback: false,
    }
}

/// The hub sends a live machine that takes commands the one a person asked
/// for, and hears the machine's answer: the backup it started (decision
/// 176).
#[tokio::test]
async fn the_hub_hears_the_backup_a_live_machine_started_for_it() {
    let hub = keeping_reports(1);
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let clock = Arc::new(FakeClock::new(hub.clock.now()));
    let sync = sync_taking_commands(&hub, connector, clock).await;
    let asked = async {
        for _ in 0..10_000 {
            if hub.pairing.is_online(agent_id(1)) {
                break;
            }
            tokio::task::yield_now().await;
        }
        hub.pairing
            .command_asked(photos_asked_of(agent_id(1)))
            .await
    };
    let running = std::pin::pin!(sync.run());
    let asked = std::pin::pin!(asked);
    let answered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::future::select(running, asked),
    )
    .await;
    let Ok(futures::future::Either::Right((answer, _))) = answered else {
        panic!("the hub heard no answer");
    };
    assert_eq!(
        answer,
        Ok(crate::hub_commands::CommandAnswer::Started(
            domain::OperationId::from_uuid(Uuid::from_u128(0x0e))
        ))
    );
}

/// A command no live machine takes is refused at once, and nothing is kept
/// to send later (decision 176).
#[tokio::test]
async fn the_hub_refuses_at_once_a_command_no_live_machine_takes() {
    use crate::pairing::CommandAskRefused;
    use domain::fleet::Accepts;

    let hub = keeping_reports(1);
    let _identity = approved(&hub).await;
    let ask = |agent| hub.pairing.command_asked(photos_asked_of(agent));
    assert_eq!(
        ask(agent_id(2)).await,
        Err(CommandAskRefused::MachineUnknown)
    );
    assert_eq!(
        ask(agent_id(1)).await,
        Err(CommandAskRefused::MachineOffline)
    );
    let _older = hub
        .pairing
        .went_live_taking(agent_id(1), 1, Box::new(|| {}), None)
        .await;
    assert_eq!(
        ask(agent_id(1)).await,
        Err(CommandAskRefused::CommandsUnsupported)
    );
    let _newer = hub
        .pairing
        .went_live_taking(agent_id(1), 2, Box::new(|| {}), Some(Accepts::none()))
        .await;
    assert_eq!(ask(agent_id(1)).await, Err(CommandAskRefused::CommandsOff));
}

/// A run of a plan on a machine offline names the plan's next run on its
/// schedule, as the machine last reported it, when it is still to come; a
/// cancel, a plan reported without one, or one already past, is only
/// offline (decision 176).
#[tokio::test]
async fn a_run_on_a_machine_offline_names_its_next_run_still_to_come() {
    use crate::pairing::CommandAskRefused;
    use crate::ports::ReportInbox as _;
    use domain::fleet::{HubCommand, PlansReport, ReportedPlan};

    let inbox = Arc::new(crate::fakes::InMemoryReportInbox::default());
    let hub = hub_with(1, |pairing| pairing.with_inbox(inbox.clone()));
    let _identity = approved(&hub).await;
    let ask = || hub.pairing.command_asked(photos_asked_of(agent_id(1)));
    assert_eq!(ask().await, Err(CommandAskRefused::MachineOffline));
    let now = hub.clock.now();
    let next = now + SignedDuration::from_hours(1);
    let plans = |plan: &str, next_run: Option<Timestamp>| {
        PlansReport::of(vec![ReportedPlan {
            plan: domain::PlanId::parse(plan).unwrap(),
            last: None,
            next_run: next_run.and_then(EpochSeconds::from_timestamp),
        }])
    };
    inbox
        .keep_plans(agent_id(1), &plans("photos", Some(next)))
        .await
        .unwrap();
    assert_eq!(
        ask().await,
        Err(CommandAskRefused::MachineOfflineNextRun { next_run: next })
    );
    let cancel = crate::pairing::CommandAsk {
        command: HubCommand::CancelOp {
            op: domain::OperationId::from_uuid(Uuid::from_u128(0x0e)),
        },
        ..photos_asked_of(agent_id(1))
    };
    assert_eq!(
        hub.pairing.command_asked(cancel).await,
        Err(CommandAskRefused::MachineOffline)
    );
    for report in [
        plans("music", Some(next)),
        plans("photos", None),
        plans("photos", Some(now)),
        plans("photos", Some(now - SignedDuration::from_mins(1))),
    ] {
        inbox.keep_plans(agent_id(1), &report).await.unwrap();
        assert_eq!(
            ask().await,
            Err(CommandAskRefused::MachineOffline),
            "{report:?}"
        );
    }
}

/// The hub sends the command signed on the machine's live connection and
/// waits 20 seconds for its answer, then says the machine didn't answer
/// (decision 176).
#[tokio::test]
async fn the_hub_waits_20_seconds_for_a_machines_answer() {
    use crate::pairing::CommandAskRefused;
    use domain::fleet::{Accepts, COMMAND_ANSWER_WITHIN};
    use futures::StreamExt as _;

    let hub = keeping_reports(1);
    let _identity = approved(&hub).await;
    let mut queue = hub
        .pairing
        .went_live_taking(agent_id(1), 1, Box::new(|| {}), Some(Accepts::all()))
        .await;
    let asked = hub.pairing.command_asked(photos_asked_of(agent_id(1)));
    let waited = async {
        let sent = queue.next().await.expect("the hub sent the command");
        let request = crate::pairing::wire::read_command(&sent.kind, &sent.payload)
            .unwrap()
            .unwrap();
        assert_eq!(request.command, photos_plan());
        assert_eq!(
            request.requested_at,
            EpochSeconds::from_timestamp(hub.clock.now()).unwrap()
        );
        hub.clock
            .advance(COMMAND_ANSWER_WITHIN.saturating_sub(SignedDuration::from_secs(1)));
        tokio::task::yield_now().await;
        hub.clock.advance(SignedDuration::from_secs(1));
    };
    let (answer, ()) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::future::join(asked, waited),
    )
    .await
    .expect("the hub waited past 20 seconds");
    assert_eq!(answer, Err(CommandAskRefused::NoAnswer));
}

/// A command whose asker stopped waiting, as when the person closed the
/// page, no longer waits for the machine's answer: the hub's limit on the
/// commands waiting counts only those still waited on (decision 176).
#[tokio::test]
async fn a_command_no_one_waits_for_leaves_room_for_the_next() {
    use domain::fleet::{Accepts, MAX_COMMANDS_IN_FLIGHT};
    use futures::{FutureExt as _, StreamExt as _};

    let hub = keeping_reports(1);
    let _identity = approved(&hub).await;
    let mut queue = hub
        .pairing
        .went_live_taking(agent_id(1), 1, Box::new(|| {}), Some(Accepts::all()))
        .await;
    for ask in 0..=MAX_COMMANDS_IN_FLIGHT {
        let mut asked = Box::pin(hub.pairing.command_asked(photos_asked_of(agent_id(1))));
        assert!(
            futures::poll!(asked.as_mut()).is_pending(),
            "the hub refused ask {ask}"
        );
        assert!(
            queue.next().now_or_never().flatten().is_some(),
            "the hub sent nothing for ask {ask}"
        );
        drop(asked);
    }
}

/// The hub's runs for a sync that takes its commands: no plan runs, and
/// each start is the operation `0x0e`.
struct StartsOne;

#[async_trait]
impl crate::hub_commands::HubRuns for StartsOne {
    async fn going(&self, _plan: &domain::PlanId) -> Option<domain::OperationId> {
        None
    }

    async fn start(
        &self,
        _plan: &domain::PlanId,
        _asked_by: Option<domain::access::SessionTag>,
    ) -> Result<domain::OperationId, crate::hub_commands::HubRunRefused> {
        Ok(domain::OperationId::from_uuid(Uuid::from_u128(0x0e)))
    }

    async fn cancel(
        &self,
        _operation: &domain::OperationId,
    ) -> Result<(), crate::hub_commands::HubCancelRefused> {
        Err(crate::hub_commands::HubCancelRefused::Unknown)
    }
}

/// A sync of machine 1 with `hub` on `connector` and `clock`, taking the
/// hub's commands.
async fn sync_taking_commands(
    hub: &Hub,
    connector: Arc<LoopbackHubConnector>,
    clock: Arc<FakeClock>,
) -> crate::sync::SyncWithHub {
    let identity = approved(hub).await;
    crate::sync::SyncWithHub::new(
        connecting(identity, connector),
        Arc::new(crate::fakes::InMemoryOutbox::new(
            domain::fleet::OutboxEpoch::from_uuid(Uuid::from_u128(0xe1)),
        )),
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_commands(answering(clock))
}

/// Answers to the hub's commands on `clock`, with the runs [`StartsOne`].
fn answering(clock: Arc<FakeClock>) -> Arc<crate::hub_commands::AnswerHubCommands> {
    Arc::new(crate::hub_commands::AnswerHubCommands::new(
        Arc::new(crate::fakes::InMemoryConfigStore::new()),
        Arc::new(crate::fakes::InMemoryCommandLedger::default()),
        Arc::new(StartsOne),
        clock,
    ))
}

/// A listing engine whose listings wait to be let through, after saying
/// one has begun.
struct HeldListings {
    engine: crate::fakes::ListingEngine,
    begun: tokio::sync::Notify,
    through: tokio::sync::Notify,
}

#[async_trait]
impl crate::ports::Engine for HeldListings {
    async fn run(
        &self,
        op: domain::OperationId,
        job: crate::ports::Job,
    ) -> Result<crate::ports::RunningJob, crate::ports::EngineError> {
        self.begun.notify_one();
        self.through.notified().await;
        self.engine.run(op, job).await
    }
}

/// The hub's `run-plan` of `photos`, the command `seq`, asked at `at`.
fn run_photos(seq: u128, at: Timestamp) -> Vec<u8> {
    api::sync::RunPlanPayload {
        command_id: Uuid::from_u128((0x0191_u128 << 112) | seq)
            .hyphenated()
            .to_string(),
        requested_at: u64::try_from(at.as_second()).unwrap(),
        plan_id: "photos".to_owned(),
        session: Some("0123456789abcdef".to_owned()),
    }
    .to_bytes()
}

/// A machine answers the hub's command while a listing holds its session,
/// as it beats its heart then (decision 176).
#[tokio::test]
async fn a_session_answers_the_hubs_command_while_a_listing_holds_it() {
    let leasing = leasing(Timestamp::from_second(1_800_000_000).unwrap()).await;
    let held = Arc::new(HeldListings {
        engine: crate::fakes::ListingEngine::default(),
        begun: tokio::sync::Notify::new(),
        through: tokio::sync::Notify::new(),
    });
    let connector = Arc::new(LoopbackHubConnector::to(leasing.hub.pairing.clone()));
    let sync = listing_first(&leasing, held.clone(), connector.clone())
        .with_commands(answering(leasing.machine_clock.clone()));
    let result_kind = domain::fleet::command_result_kind();
    let watched = async {
        held.begun.notified().await;
        connector.send_command(
            domain::fleet::run_plan_kind(),
            run_photos(1, leasing.hub.clock.now()),
        );
        // The listing holds the session until the command is answered.
        for _ in 0..200 {
            if !sent_of(&connector, &result_kind).is_empty() {
                return true;
            }
            leasing
                .machine_clock
                .advance(domain::fleet::HEARTBEAT_EVERY);
            tokio::task::yield_now().await;
        }
        false
    };
    let running = std::pin::pin!(sync.run());
    let watched = std::pin::pin!(watched);
    let answered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::future::select(running, watched),
    )
    .await;
    assert!(
        matches!(answered, Ok(futures::future::Either::Right((true, _)))),
        "the machine didn't answer while it listed"
    );
    assert_eq!(held.engine.listings(&photos()), 0, "the listing went on");
    let results = sent_of(&connector, &result_kind);
    let result = api::sync::CommandResultPayload::from_bytes(&results[0]).unwrap();
    assert_eq!(result.result, "started", "{result:?}");
}

/// The signed messages of `kind` the agent sent on `connector`, as their
/// payloads.
fn sent_of(connector: &LoopbackHubConnector, kind: &WireName) -> Vec<Vec<u8>> {
    connector
        .sent()
        .iter()
        .filter_map(
            |text| match agent_message(&AgentFrame::from_text(text).ok()?) {
                Ok(AgentMessage::Signed(envelope)) if envelope.kind == *kind => {
                    Some(envelope.payload)
                }
                _ => None,
            },
        )
        .collect()
}

/// A sync that takes its hub's commands says in its `Hello` which it
/// takes, so the hub sends them (decision 176).
#[tokio::test]
async fn a_sync_taking_commands_names_them_in_its_hello() {
    let hub = hub(1);
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let clock = Arc::new(FakeClock::new(hub.clock.now()));
    let sync = sync_taking_commands(&hub, connector.clone(), clock).await;
    sync.sync_once().await.unwrap();
    let hellos = sent_of(&connector, &hello_kind());
    let hello = api::sync::HelloPayload::from_bytes(hellos.last().unwrap()).unwrap();
    assert_eq!(
        hello.accepts,
        Some(vec!["run-plan".to_owned(), "cancel-op".to_owned()])
    );
}

/// A connection the machine holds answers the hub's command between its
/// own messages, while it waits for its next poll (decision 176).
#[tokio::test]
async fn a_held_connection_answers_the_hubs_command() {
    let hub = keeping_reports(1);
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let clock = Arc::new(FakeClock::new(hub.clock.now()));
    let sync = sync_taking_commands(&hub, connector.clone(), clock.clone()).await;
    let result_kind = domain::fleet::command_result_kind();
    let heartbeat = domain::fleet::heartbeat_kind();
    let payload = api::sync::RunPlanPayload {
        command_id: Uuid::from_u128((0x0191_u128 << 112) | 1)
            .hyphenated()
            .to_string(),
        requested_at: u64::try_from(hub.clock.now().as_second()).unwrap(),
        plan_id: "photos".to_owned(),
        session: Some("0123456789abcdef".to_owned()),
    }
    .to_bytes();
    let watched = async {
        for _ in 0..10_000 {
            if !sent_of(&connector, &heartbeat).is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        connector.send_command(domain::fleet::run_plan_kind(), payload);
        for _ in 0..200 {
            if !sent_of(&connector, &result_kind).is_empty() {
                return true;
            }
            clock.advance(crate::sync::POLL);
            tokio::task::yield_now().await;
        }
        false
    };
    let running = std::pin::pin!(sync.run());
    let watched = std::pin::pin!(watched);
    let answered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::future::select(running, watched),
    )
    .await;
    assert!(
        matches!(answered, Ok(futures::future::Either::Right((true, _)))),
        "the machine didn't answer the hub's command"
    );
    let results = sent_of(&connector, &result_kind);
    let result = api::sync::CommandResultPayload::from_bytes(&results[0]).unwrap();
    assert_eq!(result.result, "started", "{result:?}");
}

/// The kinds of the signed messages the agent sent on `connector`, in
/// order.
fn kinds_sent(connector: &LoopbackHubConnector) -> Vec<WireName> {
    connector
        .sent()
        .iter()
        .filter_map(
            |text| match agent_message(&AgentFrame::from_text(text).ok()?) {
                Ok(AgentMessage::Signed(envelope)) => Some(envelope.kind),
                _ => None,
            },
        )
        .collect()
}

/// A machine answers the hub's commands that came while it replayed its
/// outbox between its windows of reports, not after the whole replay, so
/// none waits on a long backlog past the hub's 20 seconds (decision 176).
#[tokio::test]
async fn a_replay_answers_the_hubs_commands_between_its_windows() {
    use crate::ports::Outbox as _;
    use domain::fleet::{AgentReport, Happening, OutboxEpoch, SEND_WINDOW, report_kind};

    let hub = keeping_reports(1);
    let identity = approved(&hub).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let clock = Arc::new(FakeClock::new(hub.clock.now()));
    let outbox = Arc::new(crate::fakes::InMemoryOutbox::new(OutboxEpoch::from_uuid(
        Uuid::from_u128(0xe1),
    )));
    for number in 0..=SEND_WINDOW {
        outbox
            .append(&AgentReport {
                op_id: domain::OperationId::from_uuid(Uuid::from_u128(
                    0x0b00 + u128::try_from(number).unwrap(),
                )),
                happening: Happening::Started,
                at: EpochSeconds::from_seconds(1_800_000_000),
            })
            .await
            .unwrap();
    }
    let sync = crate::sync::SyncWithHub::new(
        connecting(identity, connector.clone()),
        outbox,
        clock.clone(),
        agent_id(1),
        hostname(1),
        None,
    )
    .with_commands(answering(clock));
    connector.send_command(
        domain::fleet::run_plan_kind(),
        run_photos(1, hub.clock.now()),
    );
    connector.send_commands_ahead();
    sync.sync_once().await.unwrap();
    let kinds = kinds_sent(&connector);
    let answered = kinds
        .iter()
        .position(|kind| *kind == domain::fleet::command_result_kind());
    let last_report = kinds.iter().rposition(|kind| *kind == report_kind());
    assert!(
        matches!((answered, last_report), (Some(answered), Some(last)) if answered < last),
        "{kinds:?}"
    );
}

/// A machine holding as many of the hub's commands unanswered as the hub
/// waits on, as through a long replay, keeps its connection when the hub
/// sends another: it drops the oldest, which the hub has stopped waiting
/// for, answers it expired, and answers the rest (decision 176).
#[tokio::test]
async fn a_machine_holding_its_limit_of_commands_answers_the_oldest_expired() {
    use domain::fleet::{Accepts, MAX_COMMANDS_IN_FLIGHT, command_result_kind, run_plan_kind};

    let hub = keeping_reports(1);
    let identity = approved(&hub).await;
    let connector = Arc::new(LoopbackHubConnector::to(hub.pairing.clone()));
    let (_, mut channel) = connecting(identity, connector.clone())
        .connect_accepting(agent_id(1), hostname(1), None, None, Some(Accepts::all()))
        .await
        .unwrap();
    channel.send_report(&one_report()).await.unwrap();
    channel.receive_ack().await.unwrap();
    let sent = u128::try_from(MAX_COMMANDS_IN_FLIGHT).unwrap() + 1;
    for number in 1..=sent {
        connector.send_command(run_plan_kind(), run_photos(number, hub.clock.now()));
    }
    connector.send_commands_ahead();
    channel.send_heartbeat(None).await.unwrap();
    channel
        .receive_heartbeat_ack()
        .await
        .expect("the machine keeps its connection");
    answering(Arc::new(FakeClock::new(hub.clock.now())))
        .answer_held(&mut channel)
        .await
        .unwrap();
    let results: Vec<(String, String, Option<String>)> =
        sent_of(&connector, &command_result_kind())
            .iter()
            .map(|bytes| {
                let result = api::sync::CommandResultPayload::from_bytes(bytes).unwrap();
                (result.command_id, result.result, result.code)
            })
            .collect();
    let id = |number: u128| {
        Uuid::from_u128((0x0191_u128 << 112) | number)
            .hyphenated()
            .to_string()
    };
    assert_eq!(
        results.first().map(|(id, _, code)| (id.clone(), code.clone())),
        Some((id(1), Some("command-expired".to_owned())))
    );
    let answered: Vec<String> = results.into_iter().map(|(id, ..)| id).collect();
    assert_eq!(answered, (1..=sent).map(id).collect::<Vec<_>>());
}
