//! Steps for `features/fleet/hub_commands.feature`: a machine connected to
//! its hub answers the hub's commands to run a plan's backup or cancel one,
//! and the hub tells the person who asked what the machine answered, or why
//! it sent nothing (technical design §7.2, §10.3; §16, decisions 120 and
//! 176). The hub sends each command on the machine's connection through the
//! loopback connector, signed as the hub signs its messages; the machine
//! answers it through its use case, whose backups a double of the plan
//! runner starts. The machine's clock and its command ledger are kept for
//! the scenario, so a restart builds the use case and the connection again
//! on the same database.

use std::sync::{Arc, Mutex, PoisonError};

use api::sync::{AgentFrame, CancelOpPayload, CommandResultPayload, HelloPayload, RunPlanPayload};
use app::fakes::{
    FakeClock, FakeSigning, InMemoryCommandLedger, InMemoryConfigStore, InMemoryOutbox,
    LoopbackHubConnector, SeededRandom,
};
use app::hub_commands::{
    AnswerHubCommands, CommandAnswer, HubCancelRefused, HubRunRefused, HubRuns,
};
use app::pairing::{AgentChannel, ChannelError, CommandAsk, CommandAskRefused, ConnectToHub};
use app::ports::{Clock, ConfigStore};
use app::sync::SyncWithHub;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cucumber::{given, then, when};
use domain::access::SessionTag;
use domain::fleet::{
    COMMAND_ANSWER_WITHIN, EpochSeconds, HubCommand, OutboxEpoch, PlansReport, ReportedPlan,
    WireName, cancel_op_kind, run_plan_kind,
};
use domain::planning::{AgentConfig, AgentSettings, HubCommands};
use domain::{OperationId, PlanId, RepositoryId, SignedDuration, Uuid};
use futures::future::{Either, join, select};

use super::Agent;
use crate::scheduler_steps::config_with_plans;

/// The hub's session that asks for each command, by its tag.
const ASKED_BY: &str = "0123456789abcdef";

#[derive(Default)]
pub(super) struct CommandsWorld {
    /// On the heap, so the scenario's world stays small on the runner's
    /// stack.
    side: Option<Box<Side>>,
}

/// The machine's side: its configuration, its command ledger, its backups,
/// its clock, the connection it holds to its hub and the use case that
/// answers on it.
struct Side {
    config: Arc<InMemoryConfigStore>,
    ledger: Arc<InMemoryCommandLedger>,
    runs: Arc<Runs>,
    clock: Arc<FakeClock>,
    connector: Arc<LoopbackHubConnector>,
    channel: Option<AgentChannel>,
    answer: Option<AnswerHubCommands>,
    /// The last command the hub sent: its kind and its payload.
    last: Option<(WireName, Vec<u8>)>,
    /// How the machine's last answer went.
    answered: Option<Result<(), ChannelError>>,
    commands: u128,
    /// Whether the machine runs a release whose `Hello` doesn't say which
    /// commands it takes.
    older: bool,
    /// Whether the hub holds the machine's connection as live.
    live: bool,
    /// The machine's sync with its hub, run while a person on the hub asks;
    /// on the heap, as it is too large to keep in the world on the stack.
    sync: Option<Box<SyncWithHub>>,
    /// What the hub told the person who asked last.
    heard: Option<Result<CommandAnswer, CommandAskRefused>>,
}

fn side(agent: &mut Agent) -> &mut Side {
    let world = &mut agent.pairing;
    if world.commands.side.is_none() {
        let connector = world.connector_to(world.hub());
        let config = Arc::new(InMemoryConfigStore::new());
        world.commands.side = Some(Box::new(Side {
            runs: Arc::new(Runs {
                config: config.clone(),
                state: Mutex::default(),
            }),
            config,
            ledger: Arc::new(InMemoryCommandLedger::default()),
            clock: Arc::new(FakeClock::new(world.hub().clock.now())),
            connector,
            channel: None,
            answer: None,
            last: None,
            answered: None,
            commands: 0,
            older: false,
            live: false,
            sync: None,
            heard: None,
        }));
    }
    world.commands.side.as_mut().expect("the machine's side")
}

/// One operation the double of the plan runner holds.
struct Running {
    id: OperationId,
    plan: Option<PlanId>,
    backup: bool,
    running: bool,
    canceled: bool,
}

#[derive(Default)]
struct RunsState {
    operations: Vec<Running>,
    /// Each backup the hub started: its plan, the hub's session that asked
    /// and its operation.
    started: Vec<(PlanId, Option<SessionTag>, OperationId)>,
}

/// The plan runner's double: it starts a backup of a plan the machine's
/// configuration has, as a running operation, and cancels a running one.
struct Runs {
    config: Arc<InMemoryConfigStore>,
    state: Mutex<RunsState>,
}

impl Runs {
    fn state(&self) -> std::sync::MutexGuard<'_, RunsState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn add(&self, plan: Option<PlanId>, backup: bool) -> OperationId {
        let mut state = self.state();
        let number = state.operations.len() as u128 + 1;
        let id = OperationId::from_uuid(Uuid::from_u128((0x0190_u128 << 112) | number));
        state.operations.push(Running {
            id,
            plan,
            backup,
            running: true,
            canceled: false,
        });
        id
    }

    fn last(&self) -> OperationId {
        self.state().operations.last().expect("an operation").id
    }
}

#[async_trait]
impl HubRuns for Runs {
    async fn going(&self, plan: &PlanId) -> Option<OperationId> {
        self.state()
            .operations
            .iter()
            .find(|op| op.backup && op.running && op.plan.as_ref() == Some(plan))
            .map(|op| op.id)
    }

    async fn queue(
        &self,
        plan: &PlanId,
        asked_by: Option<SessionTag>,
    ) -> Result<OperationId, HubRunRefused> {
        let config = self
            .config
            .load()
            .await
            .map_err(|_| HubRunRefused::Refused)?;
        if config.plans().iter().all(|known| known.id != *plan) {
            return Err(HubRunRefused::UnknownPlan);
        }
        let id = self.add(Some(plan.clone()), true);
        self.state().started.push((plan.clone(), asked_by, id));
        Ok(id)
    }

    async fn cancel(&self, operation: &OperationId) -> Result<(), HubCancelRefused> {
        let mut state = self.state();
        let op = state
            .operations
            .iter_mut()
            .find(|op| op.id == *operation)
            .ok_or(HubCancelRefused::Unknown)?;
        if !op.running {
            return Err(HubCancelRefused::NotRunning);
        }
        if !op.backup {
            return Err(HubCancelRefused::NotABackup);
        }
        op.running = false;
        op.canceled = true;
        Ok(())
    }
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("the scenario's plan ID is valid")
}

fn repository_id(alias: &str) -> RepositoryId {
    let folded = alias.bytes().fold(0x6c62_u128, |acc, byte| {
        acc.wrapping_mul(257).wrapping_add(u128::from(byte))
    });
    RepositoryId::parse(&format!("{folded:064x}")).expect("64 hex digits are a repository ID")
}

fn payload_of(text: &str) -> Option<(String, Vec<u8>)> {
    match AgentFrame::from_text(text) {
        Ok(AgentFrame::Signed { kind, payload, .. }) => Some((
            kind,
            URL_SAFE_NO_PAD
                .decode(payload)
                .expect("a signed message's payload is base64"),
        )),
        _ => None,
    }
}

impl Side {
    /// The use case that answers the hub's commands, on the database the
    /// machine keeps.
    fn answering(&self) -> AnswerHubCommands {
        AnswerHubCommands::new(
            self.config.clone(),
            self.ledger.clone(),
            self.runs.clone(),
            self.clock.clone(),
        )
    }

    /// How the machine connects to its hub.
    fn connecting(&self, machine: &super::Machine) -> ConnectToHub {
        ConnectToHub::new(
            machine.identity.clone(),
            Arc::new(FakeSigning),
            Arc::new(SeededRandom::new(machine.seed + 1)),
            self.clock.clone(),
            self.connector.clone(),
            "0.6.0-agent".to_owned(),
            machine.identifier.clone(),
        )
    }

    /// The machine connects to its hub, built as after a start: its use
    /// case on the database it keeps, and a new connection whose `Hello`
    /// says which commands it takes.
    async fn connect(&mut self, machine: &super::Machine) {
        let answer = self.answering();
        let connect = self.connecting(machine);
        let accepts = if self.older {
            None
        } else {
            Some(answer.accepts().await)
        };
        let (_, channel) = connect
            .connect_accepting(
                machine.agent_id,
                machine.hostname.clone(),
                machine.zone.clone(),
                None,
                accepts,
            )
            .await
            .expect("the machine connects to its hub");
        self.channel = Some(channel);
        self.answer = Some(answer);
        self.live = false;
    }

    /// The machine tells its hub `plans`, its first message on this
    /// connection, by which the hub holds the connection as live.
    async fn tell_plans(&mut self, plans: &PlansReport) {
        let channel = self.channel.as_mut().expect("a connection to the hub");
        channel.send_plans(plans).await.expect("the plans are sent");
        channel
            .receive_plans_ack()
            .await
            .expect("the hub took the plans");
        self.live = true;
    }

    /// The hub sends `kind` holding `payload`, and the machine answers it.
    async fn send(&mut self, kind: WireName, payload: Vec<u8>) {
        self.last = Some((kind.clone(), payload.clone()));
        self.connector.send_command(kind, payload);
        self.answer_next().await;
    }

    async fn answer_next(&mut self) {
        let channel = self.channel.as_mut().expect("a connection to the hub");
        let answer = self.answer.as_ref().expect("the machine's use case");
        self.answered = Some(answer.answer_next(channel).await);
    }

    /// A new command's ID, as the hub makes one.
    fn command_id(&mut self) -> String {
        self.commands += 1;
        Uuid::from_u128((0x0191_u128 << 112) | self.commands)
            .hyphenated()
            .to_string()
    }

    /// The `Hello` the machine sent last.
    fn hello(&self) -> HelloPayload {
        let payload = self
            .connector
            .sent()
            .iter()
            .filter_map(|text| payload_of(text))
            .filter(|(kind, _)| kind == "Hello")
            .map(|(_, payload)| payload)
            .next_back()
            .expect("the machine said Hello");
        HelloPayload::from_bytes(&payload).expect("a Hello")
    }

    /// The machine's answers to the hub's commands, in order.
    fn results(&self) -> Vec<CommandResultPayload> {
        self.connector
            .sent()
            .iter()
            .filter_map(|text| payload_of(text))
            .filter(|(kind, _)| kind == "CommandResult")
            .map(|(_, payload)| CommandResultPayload::from_bytes(&payload).expect("an answer"))
            .collect()
    }

    /// The machine's answer to the last command, which it answered without
    /// fault.
    fn result(&self) -> CommandResultPayload {
        assert!(
            matches!(self.answered, Some(Ok(()))),
            "the machine answered: {:?}",
            self.answered
        );
        let (_, payload) = self.last.as_ref().expect("a command");
        let id = RunPlanPayload::from_bytes(payload)
            .map(|run| run.command_id)
            .or_else(|_| CancelOpPayload::from_bytes(payload).map(|cancel| cancel.command_id))
            .expect("a command's payload");
        let result = self.results().last().cloned().expect("an answer");
        assert_eq!(result.command_id, id, "the answer names the command");
        result
    }

    fn requested_at(&self, ago: SignedDuration) -> u64 {
        let at = self.clock.now() - ago;
        u64::try_from(at.as_second()).expect("after 1970")
    }
}

async fn run_plan(agent: &mut Agent, plan: &str, ago: SignedDuration) {
    let side = side(agent);
    let payload = RunPlanPayload {
        command_id: side.command_id(),
        requested_at: side.requested_at(ago),
        plan_id: plan.to_owned(),
        session: Some(ASKED_BY.to_owned()),
    }
    .to_bytes();
    side.send(run_plan_kind(), payload).await;
}

async fn cancel(agent: &mut Agent, operation: OperationId) {
    let side = side(agent);
    let payload = CancelOpPayload {
        command_id: side.command_id(),
        requested_at: side.requested_at(SignedDuration::ZERO),
        op_id: operation.as_uuid().hyphenated().to_string(),
        session: Some(ASKED_BY.to_owned()),
    }
    .to_bytes();
    side.send(cancel_op_kind(), payload).await;
}

#[given(expr = "the machine has the plan {string}")]
async fn has_plan(agent: &mut Agent, plan: String) {
    let config = config_with_plans(&[(&plan, "nas")], repository_id);
    side(agent)
        .config
        .save(&config)
        .await
        .expect("the configuration is kept");
}

#[given("the machine's configuration turns its hub's commands off")]
async fn commands_off(agent: &mut Agent) {
    let side = side(agent);
    let config = side.config.load().await.expect("the configuration");
    let off = AgentConfig::new(
        AgentSettings {
            hub_commands: HubCommands::Off,
            ..config.agent().clone()
        },
        config.hooks().clone(),
        config.repositories().clone(),
        config.plans().to_vec(),
    )
    .expect("the configuration is valid");
    side.config
        .save(&off)
        .await
        .expect("the configuration is kept");
}

#[given("the machine is connected to its hub")]
#[when("the machine connects to its hub")]
async fn connected(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    side(agent).connect(&machine).await;
    agent.pairing.first = Some(machine);
}

#[given("the machine syncs with its hub")]
async fn syncs(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    let side = side(agent);
    let sync = SyncWithHub::new(
        side.connecting(&machine),
        Arc::new(InMemoryOutbox::new(OutboxEpoch::from_uuid(
            Uuid::from_u128(0xe1),
        ))),
        side.clock.clone(),
        machine.agent_id,
        machine.hostname.clone(),
        machine.zone.clone(),
    )
    .with_commands(Arc::new(side.answering()));
    side.sync = Some(Box::new(sync));
    agent.pairing.first = Some(machine);
}

#[when("the machine restarts and connects to its hub again")]
async fn restarts(agent: &mut Agent) {
    let machine = agent.pairing.first.take().expect("the first agent");
    let side = side(agent);
    side.channel = None;
    side.answer = None;
    side.connect(&machine).await;
    agent.pairing.first = Some(machine);
}

#[then("the machine told its hub it takes its commands")]
async fn takes_commands(agent: &mut Agent) {
    assert_eq!(
        side(agent).hello().accepts,
        Some(vec!["run-plan".to_owned(), "cancel-op".to_owned()])
    );
}

#[then("the machine told its hub it takes no commands")]
async fn takes_none(agent: &mut Agent) {
    assert_eq!(side(agent).hello().accepts, Some(Vec::new()));
}

#[given(expr = "a backup of {string} is running on the machine")]
async fn backup_running(agent: &mut Agent, plan: String) {
    side(agent).runs.add(Some(plan_id(&plan)), true);
}

#[given("a check is running on the machine")]
async fn check_running(agent: &mut Agent) {
    side(agent).runs.add(None, false);
}

#[given(expr = "the hub asked the machine to run the plan {string}")]
#[when(expr = "the hub asks the machine to run the plan {string}")]
async fn asks_to_run(agent: &mut Agent, plan: String) {
    run_plan(agent, &plan, SignedDuration::ZERO).await;
}

#[when(expr = "the hub asks the machine to run the plan {string}, asked {int} minutes ago")]
async fn asks_late(agent: &mut Agent, plan: String, minutes: i64) {
    run_plan(agent, &plan, SignedDuration::from_mins(minutes)).await;
}

#[when("the hub sends the machine that command again")]
async fn sends_again(agent: &mut Agent) {
    let side = side(agent);
    let (kind, payload) = side.last.clone().expect("a command");
    side.send(kind, payload).await;
}

#[when(
    expr = "a command to run the plan {string} reaches the machine with a signature its hub didn't make"
)]
async fn forged(agent: &mut Agent, plan: String) {
    side(agent).connector.forge_next_command();
    run_plan(agent, &plan, SignedDuration::ZERO).await;
}

#[when("the hub asks the machine to cancel that backup")]
#[when("the hub asks the machine to cancel that check")]
async fn asks_to_cancel(agent: &mut Agent) {
    let operation = side(agent).runs.last();
    cancel(agent, operation).await;
}

#[given("that backup ended")]
async fn backup_ended(agent: &mut Agent) {
    let side = side(agent);
    let last = side.runs.last();
    let mut state = side.runs.state();
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.id == last)
        .expect("the backup");
    op.running = false;
}

#[when(expr = "{int} minutes pass on the machine and its hub")]
async fn minutes_pass(agent: &mut Agent, minutes: i64) {
    let passed = SignedDuration::from_mins(minutes);
    agent.pairing.hub().clock.advance(passed);
    side(agent).clock.advance(passed);
}

#[then(expr = "the machine started a backup of {string} for the hub's session")]
async fn started_for_session(agent: &mut Agent, plan: String) {
    let side = side(agent);
    let started = &side.runs.state().started;
    assert_eq!(started.len(), 1, "one backup started");
    let (on, asked_by, _) = started.first().expect("the backup");
    assert_eq!(*on, plan_id(&plan));
    assert_eq!(asked_by.as_ref().map(SessionTag::as_str), Some(ASKED_BY));
}

#[then("the machine started no backup")]
async fn started_none(agent: &mut Agent) {
    assert!(side(agent).runs.state().started.is_empty());
}

#[then(expr = "the machine started {int} backup(s)")]
async fn started_count(agent: &mut Agent, count: usize) {
    assert_eq!(side(agent).runs.state().started.len(), count);
}

#[then("the machine answered that it started that backup")]
async fn answered_started(agent: &mut Agent) {
    let side = side(agent);
    let started = side.runs.state().started.last().expect("a backup").2;
    let result = side.result();
    assert_eq!(result.result, "started");
    assert_eq!(
        result.op_id,
        Some(started.as_uuid().hyphenated().to_string())
    );
}

#[then("the machine answered that it started the backup already running")]
async fn answered_running(agent: &mut Agent) {
    let side = side(agent);
    let running = side.runs.last();
    let result = side.result();
    assert_eq!(result.result, "started");
    assert_eq!(
        result.op_id,
        Some(running.as_uuid().hyphenated().to_string())
    );
}

#[then(expr = "the machine refused the command with {string}")]
async fn refused(agent: &mut Agent, code: String) {
    let result = side(agent).result();
    assert_eq!(result.result, "refused");
    assert_eq!(result.code, Some(code));
    assert_eq!(result.op_id, None);
}

#[then(
    expr = "the machine refused the command with {string}, to be asked again {int} minutes from now"
)]
async fn refused_until(agent: &mut Agent, code: String, minutes: i64) {
    let side = side(agent);
    let result = side.result();
    assert_eq!(result.result, "refused");
    assert_eq!(result.code, Some(code));
    let again = side.clock.now() + SignedDuration::from_mins(minutes);
    assert_eq!(
        result.retry_at,
        Some(u64::try_from(again.as_second()).expect("after 1970"))
    );
}

#[then("the machine answered nothing and dropped the connection")]
async fn answered_nothing(agent: &mut Agent) {
    let side = side(agent);
    assert_eq!(side.answered, Some(Err(ChannelError::Unsigned)));
    assert!(side.results().is_empty(), "no answer: {:?}", side.results());
}

#[then("that backup was canceled")]
async fn backup_canceled(agent: &mut Agent) {
    let side = side(agent);
    let last = side.runs.last();
    let state = side.runs.state();
    let op = state
        .operations
        .iter()
        .find(|op| op.id == last)
        .expect("the backup");
    assert!(op.canceled && !op.running);
}

#[then("the machine answered that it canceled that backup")]
async fn answered_canceled(agent: &mut Agent) {
    let side = side(agent);
    let last = side.runs.last();
    let result = side.result();
    assert_eq!(result.result, "canceled");
    assert_eq!(result.op_id, Some(last.as_uuid().hyphenated().to_string()));
}

#[then("the check runs on")]
async fn check_runs_on(agent: &mut Agent) {
    let side = side(agent);
    let state = side.runs.state();
    let check = state.operations.last().expect("the check");
    assert!(check.running && !check.canceled);
}

/// The ask of a person on the hub, signed in as the session [`ASKED_BY`]
/// tags, that the first machine take `command`.
fn person_ask(agent: &Agent, command: HubCommand) -> CommandAsk {
    CommandAsk {
        agent: agent
            .pairing
            .first
            .as_ref()
            .expect("the first agent")
            .agent_id,
        command,
        asked_by: Some(SessionTag::parse(ASKED_BY).expect("a session's tag")),
        beyond_loopback: false,
    }
}

/// The machine's connection, live on the hub once it told the hub its
/// plans, none of them.
async fn live_side(agent: &mut Agent) -> &mut Side {
    let side = side(agent);
    if side.channel.is_some() && !side.live {
        side.tell_plans(&PlansReport::of(Vec::new())).await;
    }
    side
}

/// A person on the hub asks the machine to take `command`; the machine,
/// while connected, answers what reaches it, and the hub tells the person
/// what it heard.
async fn person_asks(agent: &mut Agent, command: HubCommand) {
    let ask = person_ask(agent, command);
    let pairing = agent.pairing.hub().pairing.clone();
    let side = live_side(agent).await;
    if let Some(sync) = side.sync.as_ref() {
        let agent_id = ask.agent;
        let asked = async {
            while !pairing.is_online(agent_id) {
                tokio::task::yield_now().await;
            }
            pairing.command_asked(ask).await
        };
        // On the heap, as the sync is: its future is large.
        let running = Box::pin(sync.run());
        let asked = std::pin::pin!(asked);
        let Either::Right((heard, _)) = select(running, asked).await else {
            panic!("the machine's sync with its hub ended");
        };
        side.heard = Some(heard);
        return;
    }
    let asked = std::pin::pin!(pairing.command_asked(ask));
    let heard = match (side.channel.as_mut(), side.answer.as_ref()) {
        (Some(channel), Some(answer)) => {
            let answering = std::pin::pin!(answer.answer_next(channel));
            match select(asked, answering).await {
                Either::Left((heard, _)) => heard,
                Either::Right((answered, asked)) => {
                    side.answered = Some(answered);
                    asked.await
                }
            }
        }
        _ => asked.await,
    };
    side.heard = Some(heard);
}

#[then("the machine heard no command")]
async fn heard_no_command(agent: &mut Agent) {
    let side = live_side(agent).await;
    let channel = side.channel.as_mut().expect("a connection to the hub");
    assert!(channel.buffered_command().is_none(), "a command waited");
    assert!(side.results().is_empty(), "{:?}", side.results());
}

#[given("the machine isn't connected to its hub")]
async fn not_connected(agent: &mut Agent) {
    assert!(side(agent).channel.is_none());
}

#[given("the machine runs a release that takes no commands and is connected to its hub")]
async fn older_connected(agent: &mut Agent) {
    side(agent).older = true;
    connected(agent).await;
}

#[given(expr = "the machine told its hub the plan {string} runs next in an hour")]
async fn told_next_run(agent: &mut Agent, plan: String) {
    connected(agent).await;
    let side = side(agent);
    let next = side.clock.now() + SignedDuration::from_hours(1);
    let plans = PlansReport::of(vec![ReportedPlan {
        plan: plan_id(&plan),
        last: None,
        next_run: EpochSeconds::from_timestamp(next),
    }]);
    side.tell_plans(&plans).await;
}

#[given("the machine's connection to its hub ended")]
async fn connection_ended(agent: &mut Agent) {
    let side = side(agent);
    side.connector.drop_links().await;
    side.channel = None;
    side.answer = None;
    side.live = false;
}

#[given(expr = "a person on the hub asked the machine to run the plan {string}")]
#[when(expr = "a person on the hub asks the machine to run the plan {string}")]
async fn person_asks_to_run(agent: &mut Agent, plan: String) {
    person_asks(
        agent,
        HubCommand::RunPlan {
            plan: plan_id(&plan),
        },
    )
    .await;
}

#[when(
    expr = "a person on the hub asks the machine to run the plan {string}, which the machine hears and doesn't answer for 20 seconds"
)]
async fn person_asks_unanswered(agent: &mut Agent, plan: String) {
    let ask = person_ask(
        agent,
        HubCommand::RunPlan {
            plan: plan_id(&plan),
        },
    );
    let hub = agent.pairing.hub();
    let (pairing, clock) = (hub.pairing.clone(), hub.clock.clone());
    let side = live_side(agent).await;
    let channel = side.channel.as_mut().expect("a connection to the hub");
    let silent = async {
        channel
            .listen_command()
            .await
            .expect("the machine hears the command");
        clock.advance(COMMAND_ANSWER_WITHIN);
    };
    let (heard, ()) = join(pairing.command_asked(ask), silent).await;
    side.heard = Some(heard);
}

/// The code of what the hub told the person, by its name on the wire: its
/// refusal, or the machine's.
fn heard_code(side: &Side) -> Option<String> {
    let code = match side.heard.as_ref().expect("the hub told the person") {
        Err(refused) => refused.code(),
        Ok(CommandAnswer::Refused { code, .. }) => *code,
        Ok(_) => return None,
    };
    serde_json::to_value(code)
        .ok()
        .and_then(|name| name.as_str().map(str::to_owned))
}

#[then(expr = "the hub told the person {string}")]
async fn person_told(agent: &mut Agent, code: String) {
    let side = side(agent);
    assert_eq!(heard_code(side), Some(code), "{:?}", side.heard);
}

#[then(expr = "the hub told the person {string}, with the plan's next run in an hour")]
async fn person_told_next_run(agent: &mut Agent, code: String) {
    let side = side(agent);
    assert_eq!(heard_code(side), Some(code), "{:?}", side.heard);
    let next = side.clock.now() + SignedDuration::from_hours(1);
    assert_eq!(
        side.heard,
        Some(Err(CommandAskRefused::MachineOfflineNextRun {
            next_run: next
        }))
    );
}

#[then(expr = "the hub told the person {string}, to be asked again {int} minutes from now")]
async fn person_told_until(agent: &mut Agent, code: String, minutes: i64) {
    let side = side(agent);
    assert_eq!(heard_code(side), Some(code), "{:?}", side.heard);
    let again = side.clock.now() + SignedDuration::from_mins(minutes);
    let retry_at = match &side.heard {
        Some(Ok(CommandAnswer::Refused { retry_at, .. })) => *retry_at,
        Some(Err(CommandAskRefused::Busy { retry_at })) => Some(*retry_at),
        _ => None,
    };
    assert_eq!(retry_at, Some(again));
}

#[then("the hub told the person the machine started that backup")]
async fn person_told_started(agent: &mut Agent) {
    let side = side(agent);
    let started = side.runs.state().started.last().expect("a backup").2;
    assert_eq!(side.heard, Some(Ok(CommandAnswer::Started(started))));
}
