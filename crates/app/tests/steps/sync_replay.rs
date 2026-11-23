//! Steps for `features/fleet/sync_replay.feature`: an agent's reports kept
//! in its outbox while the hub is out of reach, replayed in order once it is
//! back, and taken once however often they come (technical design §7.2).
//! A backup is its two reports, as the agent's event recorder makes them;
//! a restart is a new recorder and a new sync over the same outbox, as the
//! store's file outlives the daemon. An agent that keeps syncing runs the
//! daemon's sync loop on a clock of its own, which the steps move from one
//! of the loop's waits to the next.

use std::sync::Arc;

use api::UiEvent;
use api::sync::{AgentFrame, ReportPayload};
use app::fakes::{FakeClock, InMemoryOutbox, LoopbackHubConnector};
use app::ports::{Appended, Outbox, ReportInbox};
use app::sync::{RecordReports, SyncFailure, SyncWithHub, Synced};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cucumber::{given, then, when};
use domain::fleet::{AgentReport, Happening, OutboxEpoch};
use domain::{Timestamp, Uuid};

use super::{Agent, Machine, PairingWorld};

/// The sync world: each agent's outbox, recorder and connection.
#[derive(Default)]
pub(super) struct SyncWorld {
    first: Option<Side>,
    other: Option<Side>,
    /// Whether the hub can't be reached.
    down: bool,
}

struct Side {
    outbox: Arc<InMemoryOutbox>,
    record: RecordReports,
    connector: Arc<LoopbackHubConnector>,
    /// The reports recorded, in order.
    recorded: Vec<AgentReport>,
    /// How many reports were kept while the hub couldn't be reached.
    kept_while_down: usize,
    /// How many backups ran.
    backups: u128,
    /// Each sync's outcome, in order.
    synced: Vec<Result<Synced, SyncFailure>>,
    /// The sync loop, once the agent keeps syncing.
    running: Option<Running>,
}

/// How many of the sync loop's waits a step moves through before it gives
/// up on what it waits for.
const WAKES: usize = 20;

/// The agent's sync loop, running beside its operations as the daemon runs
/// it, stopped with the scenario.
struct Running {
    clock: Arc<FakeClock>,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    /// Once the loop waits on its clock, moves the clock to the end of that
    /// wait; `false` when it never waits, as a loop that stopped.
    async fn wake(&self) -> bool {
        for _ in 0..1_000 {
            if let Some(next) = self.clock.until_next_sleeper() {
                self.clock.advance(next);
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        false
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Side {
    fn new(world: &PairingWorld, n: u128) -> Self {
        let outbox = Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe0 + n),
        )));
        let connector = world.connector_to(world.hub());
        connector.set_down(world.syncing.down);
        Self {
            record: RecordReports::new(outbox.clone()),
            outbox,
            connector,
            recorded: Vec::new(),
            kept_while_down: 0,
            backups: 0,
            synced: Vec::new(),
            running: None,
        }
    }

    /// One backup: its start and its end, recorded as the agent's
    /// operations make them.
    async fn backup(&mut self, agent: u128, down: bool) {
        self.backups += 1;
        let operation = Uuid::from_u128((agent << 64) | self.backups).to_string();
        let started = 1_800_000_000 + 600 * i64::try_from(self.backups).unwrap();
        let time = |second: i64| Timestamp::from_second(second).unwrap().to_string();
        let events = [
            UiEvent::OperationStarted {
                operation: operation.clone(),
                repository: Some("nas".to_owned()),
                started_at: time(started),
            },
            UiEvent::OperationSucceeded {
                operation,
                finished_at: time(started + 42),
            },
        ];
        for event in &events {
            let kept = self.record.record(event).await.expect("recorded");
            assert!(matches!(kept, Some(Appended::Kept(_))), "{kept:?}");
            if down {
                self.kept_while_down += 1;
            }
            self.recorded
                .push(RecordReports::report_of(event).expect("a report"));
        }
    }

    async fn sync(&mut self, machine: &Machine) -> &Result<Synced, SyncFailure> {
        let sync = SyncWithHub::new(
            machine.connecting(self.connector.clone()),
            self.outbox.clone(),
            Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).unwrap(),
            )),
            machine.agent_id,
            machine.hostname.clone(),
            machine.zone.clone(),
        );
        let outcome = sync.sync_once().await;
        self.synced.push(outcome);
        self.synced.last().expect("a sync")
    }

    async fn waiting(&self) -> usize {
        self.outbox
            .unacknowledged(None, usize::MAX)
            .await
            .expect("the reports waiting")
            .len()
    }
}

fn first(agent: &mut Agent) -> &mut Side {
    let world = &mut agent.pairing;
    if world.syncing.first.is_none() {
        world.syncing.first = Some(Side::new(world, 1));
    }
    world
        .syncing
        .first
        .as_mut()
        .expect("the first agent's side")
}

fn other(agent: &mut Agent) -> &mut Side {
    let world = &mut agent.pairing;
    if world.syncing.other.is_none() {
        world.syncing.other = Some(Side::new(world, 2));
    }
    world
        .syncing
        .other
        .as_mut()
        .expect("the other agent's side")
}

fn set_down(agent: &mut Agent, down: bool) {
    agent.pairing.syncing.down = down;
    first(agent).connector.set_down(down);
}

#[given("the hub can't be reached")]
async fn hub_down(agent: &mut Agent) {
    set_down(agent, true);
}

#[when("the hub can be reached again")]
async fn hub_back(agent: &mut Agent) {
    set_down(agent, false);
}

#[given("the agent keeps syncing with its hub while it runs")]
async fn keeps_syncing(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    let side = first(agent);
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let sync = SyncWithHub::new(
        machine.connecting(side.connector.clone()),
        side.outbox.clone(),
        clock.clone(),
        machine.agent_id,
        machine.hostname.clone(),
        machine.zone.clone(),
    );
    side.running = Some(Running {
        clock,
        task: tokio::spawn(async move { sync.run().await }),
    });
    agent.pairing.first = Some(machine);
}

#[given(regex = r"^the agent (?:runs|ran) (\d+) backups?$")]
#[when(regex = r"^the agent (?:runs|ran) (\d+) backups?$")]
async fn runs_backups(agent: &mut Agent, count: u32) {
    let down = agent.pairing.syncing.down;
    let side = first(agent);
    for _ in 0..count {
        side.backup(1, down).await;
    }
}

#[given(regex = r"^the other agent ran (\d+) backups?$")]
async fn other_runs_backups(agent: &mut Agent, count: u32) {
    let down = agent.pairing.syncing.down;
    let side = other(agent);
    for _ in 0..count {
        side.backup(2, down).await;
    }
}

#[when("the agent tries to sync with its hub")]
async fn tries_to_sync(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    first(agent).sync(&machine).await;
    agent.pairing.first = Some(machine);
}

#[when("the agent syncs with its hub")]
async fn syncs(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    let outcome = first(agent).sync(&machine).await.clone();
    agent.pairing.first = Some(machine);
    assert!(
        matches!(outcome, Ok(Synced::Replayed { .. })),
        "{outcome:?}"
    );
}

#[when("the other agent syncs with its hub")]
async fn other_syncs(agent: &mut Agent) {
    let machine = agent.pairing.second.take().expect("the other agent");
    other(agent).sync(&machine).await;
    agent.pairing.second = Some(machine);
}

#[when("the agent restarts")]
async fn restarts(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let side = world
        .syncing
        .first
        .as_mut()
        .expect("the first agent's side");
    side.record = RecordReports::new(side.outbox.clone());
    let connector = world.connector_to(world.hub());
    connector.set_down(world.syncing.down);
    world
        .syncing
        .first
        .as_mut()
        .expect("the first agent's side")
        .connector = connector;
}

#[given("the hub's next acknowledgement is lost with its connection")]
async fn ack_lost(agent: &mut Agent) {
    first(agent).connector.lose_acks(1);
}

#[given("another agent paired with the hub and waiting for approval")]
async fn other_waiting(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.new_token().await;
    let machine = Machine::new(2, "desk");
    world.pair_and_keep(&machine).await;
    assert!(matches!(world.outcome, Some(Ok(_))), "{:?}", world.outcome);
    world.second = Some(machine);
}

#[then(regex = r"^the (\d+) backups ran while the hub couldn't be reached$")]
async fn ran_while_down(agent: &mut Agent, count: u128) {
    let side = first(agent);
    assert_eq!(side.backups, count);
    assert_eq!(side.kept_while_down, usize::try_from(count * 2).unwrap());
    assert!(
        side.synced.iter().all(Result::is_err),
        "no sync held: {:?}",
        side.synced
    );
}

#[then("the backup's 2 reports were kept while the agent's sync couldn't reach the hub")]
async fn kept_while_unreachable(agent: &mut Agent) {
    let id = agent.pairing.first().agent_id;
    let inbox = agent.pairing.hub().inbox.clone();
    let side = first(agent);
    let running = side.running.as_ref().expect("the agent keeps syncing");
    for _ in 0..WAKES {
        if !side.connector.routes().is_empty() || !running.wake().await {
            break;
        }
    }
    assert!(
        !side.connector.routes().is_empty(),
        "the agent's sync never tried its hub"
    );
    let ended = side.recorded.last().map(|report| &report.happening);
    assert_eq!(ended, Some(&Happening::Succeeded));
    assert_eq!(side.kept_while_down, 2);
    assert_eq!(side.waiting().await, 2);
    assert!(inbox.reports(id).await.unwrap().is_empty());
}

#[then(regex = r"^the agent's outbox holds (\d+) reports? the hub hasn't acknowledged$")]
async fn outbox_holds(agent: &mut Agent, count: usize) {
    assert_eq!(first(agent).waiting().await, count);
}

#[then(regex = r"^the other agent's outbox holds (\d+) reports? the hub hasn't acknowledged$")]
async fn other_outbox_holds(agent: &mut Agent, count: usize) {
    assert_eq!(other(agent).waiting().await, count);
}

#[then("the agent's outbox holds no report the hub hasn't acknowledged")]
async fn outbox_empty(agent: &mut Agent) {
    assert_eq!(first(agent).waiting().await, 0);
}

#[then(regex = r"^the hub holds the agent's (\d+) reports, each once, in the order they happened$")]
async fn hub_holds(agent: &mut Agent, count: usize) {
    let id = agent.pairing.first().agent_id;
    let held = agent.pairing.hub().inbox.reports(id).await.unwrap();
    let reports: Vec<AgentReport> = held.iter().map(|entry| entry.report.clone()).collect();
    assert_eq!(reports.len(), count, "{held:?}");
    assert_eq!(reports, first(agent).recorded);
    let places: Vec<_> = held.iter().map(|entry| entry.seq).collect();
    assert!(places.is_sorted_by(|a, b| a < b), "{held:?}");
}

#[then(
    regex = r"^the hub soon holds the agent's (\d+) reports, each once, in the order they happened$"
)]
async fn hub_soon_holds(agent: &mut Agent, count: usize) {
    let id = agent.pairing.first().agent_id;
    let inbox = agent.pairing.hub().inbox.clone();
    let side = first(agent);
    let running = side.running.as_ref().expect("the agent keeps syncing");
    for _ in 0..WAKES {
        let held = inbox.reports(id).await.unwrap().len();
        if held >= count || !running.wake().await {
            break;
        }
    }
    hub_holds(agent, count).await;
}

#[then("the hub holds no report of the other agent")]
async fn hub_holds_none(agent: &mut Agent) {
    let id = agent
        .pairing
        .second
        .as_ref()
        .expect("the other agent")
        .agent_id;
    let held = agent.pairing.hub().inbox.reports(id).await.unwrap();
    assert!(held.is_empty(), "{held:?}");
}

#[then("the hub received the agent's first report twice")]
async fn first_report_twice(agent: &mut Agent) {
    let sent = first(agent).connector.sent();
    let firsts = sent
        .iter()
        .filter_map(|text| match AgentFrame::from_text(text) {
            Ok(AgentFrame::Signed { kind, payload, .. }) if kind == "OpEvent" => {
                URL_SAFE_NO_PAD.decode(payload).ok()
            }
            _ => None,
        })
        .filter_map(|bytes| ReportPayload::from_bytes(&bytes).ok())
        .filter(|report| report.seq == 1)
        .count();
    assert_eq!(firsts, 2);
}

#[then("the other agent is told it waits for approval")]
async fn other_pending(agent: &mut Agent) {
    let side = other(agent);
    assert_eq!(side.synced.last(), Some(&Ok(Synced::Pending)));
}
