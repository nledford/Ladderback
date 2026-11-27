use std::sync::Mutex;

use api::ErrorCode;
use async_trait::async_trait;
use domain::Uuid;
use domain::access::SessionTag;
use domain::fleet::{Accepts, COMMAND_LIFE, CommandId, HUB_RUNS_PER_HOUR, HubCommand, HubRun};
use domain::planning::{AgentConfig, AgentSettings, HubCommands};

use super::*;
use crate::fakes::{FakeClock, InMemoryCommandLedger, InMemoryConfigStore};
use crate::ports::{ConfigStore, Remembered, StoreError};

fn start() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn plan(name: &str) -> PlanId {
    PlanId::parse(name).unwrap()
}

fn operation(number: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128((0x0190_u128 << 112) | number))
}

/// The backups the hub starts, each a new operation, none going before.
#[derive(Default)]
struct Started(Mutex<Vec<(PlanId, Option<SessionTag>)>>);

#[async_trait]
impl HubRuns for Started {
    async fn going(&self, _plan: &PlanId) -> Option<OperationId> {
        None
    }

    async fn start(
        &self,
        plan: &PlanId,
        asked_by: Option<SessionTag>,
    ) -> Result<OperationId, HubRunRefused> {
        let mut started = self.0.lock().unwrap();
        started.push((plan.clone(), asked_by));
        Ok(operation(started.len() as u128))
    }

    async fn cancel(&self, _operation: &OperationId) -> Result<(), HubCancelRefused> {
        Ok(())
    }
}

/// A database that can't be read.
struct Unreadable;

fn gone() -> StoreError {
    StoreError::Unavailable {
        detail: "the disk is gone".to_owned(),
    }
}

#[async_trait]
impl CommandLedger for Unreadable {
    async fn remember(&self, _id: CommandId, _now: Timestamp) -> Result<Remembered, StoreError> {
        Err(gone())
    }

    async fn ran(&self, _run: &HubRun) -> Result<(), StoreError> {
        Err(gone())
    }

    async fn runs(&self) -> Result<Vec<HubRun>, StoreError> {
        Err(gone())
    }
}

struct Machine {
    config: Arc<InMemoryConfigStore>,
    runs: Arc<Started>,
    clock: Arc<FakeClock>,
    answer: AnswerHubCommands,
}

fn machine(ledger: Arc<dyn CommandLedger>) -> Machine {
    let config = Arc::new(InMemoryConfigStore::new());
    let runs = Arc::new(Started::default());
    let clock = Arc::new(FakeClock::new(start()));
    let answer = AnswerHubCommands::new(config.clone(), ledger, runs.clone(), clock.clone());
    Machine {
        config,
        runs,
        clock,
        answer,
    }
}

/// A command asked now on the hub's time, on a connection welcomed now,
/// whose `Hello` went at the start of the machine's monotonic clock.
fn run_of(name: &str, number: u128) -> ReceivedCommand {
    let now = EpochSeconds::from_timestamp(start()).unwrap();
    ReceivedCommand {
        request: CommandRequest {
            id: CommandId::from_uuid(Uuid::from_u128(number)),
            requested_at: now,
            asked_by: None,
            command: HubCommand::RunPlan { plan: plan(name) },
        },
        welcomed_at: now,
        since: SignedDuration::ZERO,
        hello_sent: SignedDuration::ZERO,
    }
}

/// A command fresh when it arrived, but acted on more than
/// [`COMMAND_LIFE`] after on the hub's time, as one held through a long
/// replay, is refused expired: no backup starts that late.
#[tokio::test]
async fn a_command_held_past_its_life_is_refused_expired() {
    let machine = machine(Arc::new(InMemoryCommandLedger::default()));
    machine
        .clock
        .advance(COMMAND_LIFE + SignedDuration::from_secs(1));
    assert_eq!(
        machine.answer.answer(&run_of("photos", 1)).await,
        Some(CommandAnswer::Refused {
            code: ErrorCode::CommandExpired,
            retry_at: None,
        })
    );
    assert!(machine.runs.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_machine_takes_every_command_unless_its_configuration_turns_them_off() {
    let machine = machine(Arc::new(InMemoryCommandLedger::default()));
    assert_eq!(machine.answer.accepts().await, Accepts::all());
    let off = AgentConfig::new(
        AgentSettings {
            hub_commands: HubCommands::Off,
            ..AgentSettings::default()
        },
        Default::default(),
        Default::default(),
        Vec::new(),
    )
    .unwrap();
    machine.config.save(&off).await.unwrap();
    assert_eq!(machine.answer.accepts().await, Accepts::none());
}

/// A machine that can't tell whether it took a command before runs
/// nothing and answers nothing: the hub says it had no answer.
#[tokio::test]
async fn a_ledger_that_cant_be_read_runs_nothing_and_answers_nothing() {
    let machine = machine(Arc::new(Unreadable));
    assert_eq!(machine.answer.answer(&run_of("photos", 1)).await, None);
    assert!(machine.runs.0.lock().unwrap().is_empty());
}

/// An expired command isn't remembered: its ID refuses nothing later.
#[tokio::test]
async fn an_expired_command_is_refused_unremembered() {
    let ledger = Arc::new(InMemoryCommandLedger::default());
    let machine = machine(ledger.clone());
    let mut expired = run_of("photos", 1);
    expired.welcomed_at = EpochSeconds::from_seconds(
        expired.request.requested_at.seconds() + COMMAND_LIFE.as_secs().unsigned_abs() + 1,
    );
    assert_eq!(
        machine.answer.answer(&expired).await,
        Some(CommandAnswer::Refused {
            code: ErrorCode::CommandExpired,
            retry_at: None,
        })
    );
    assert_eq!(
        ledger.remember(expired.request.id, start()).await,
        Ok(Remembered::New)
    );
}

/// The hub starts at most [`HUB_RUNS_PER_HOUR`] backups an hour, of any
/// plans: the next may start an hour after the oldest of them.
#[tokio::test]
async fn the_hub_starts_at_most_its_runs_an_hour() {
    let ledger = Arc::new(InMemoryCommandLedger::default());
    let machine = machine(ledger.clone());
    for number in 0..HUB_RUNS_PER_HOUR {
        let at = start() - SignedDuration::from_mins(50 - number as i64);
        let run = HubRun {
            plan: plan(&format!("plan-{number}")),
            at,
        };
        ledger.ran(&run).await.unwrap();
    }
    assert_eq!(
        machine.answer.answer(&run_of("photos", 1)).await,
        Some(CommandAnswer::Refused {
            code: ErrorCode::CommandRateLimited,
            retry_at: Some(start() + SignedDuration::from_mins(10)),
        })
    );
    assert!(machine.runs.0.lock().unwrap().is_empty());
}
