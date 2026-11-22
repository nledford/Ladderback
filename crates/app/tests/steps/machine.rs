//! Steps for `features/fleet/copied_data_folder.feature`: the check at
//! start against a fake machine, the maintenance a copy refuses through the
//! forget steps' use case, and a new machine ID taken over the forget
//! steps' configuration.

use std::sync::Arc;

use app::fakes::FakeThisMachine;
use app::machine::{Found, MachineStanding, NewIdError, Standing, TakeNewId, check_at_start};
use app::maintenance::MaintenanceError;
use app::ports::{ConfigStore, OperationStore, Start, ThisMachine};
use cucumber::{given, then, when};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::fleet::MachineIdentifier;
use domain::{AgentId, OperationId, RepositoryAlias, Timestamp, Uuid};

use super::Agent;

/// This computer's identifier, and the one another computer has.
const THIS_COMPUTER: &str = "0123456789abcdef0123456789abcdef";
const OTHER_COMPUTER: &str = "fedcba9876543210fedcba9876543210";

/// The scenario's machine: what its system gives and its data folder
/// records, where it stands, and what the check and a new ID answered.
#[derive(Default)]
pub struct MachineWorld {
    machine: Option<Arc<FakeThisMachine>>,
    standing: Arc<MachineStanding>,
    found: Option<Found>,
    taken: Option<Result<AgentId, NewIdError>>,
    /// The check a scenario started, which keeps the repository maintained.
    running: Option<OperationId>,
}

impl MachineWorld {
    /// Where the machine stands, which the maintenance use case reads.
    pub fn standing(&self) -> Arc<MachineStanding> {
        Arc::clone(&self.standing)
    }

    fn machine(&self) -> Arc<FakeThisMachine> {
        Arc::clone(self.machine.as_ref().expect("a Given set up the machine"))
    }

    fn start(&mut self) {
        let found = check_at_start(self.machine().as_ref());
        self.standing = Arc::new(MachineStanding::new(found.standing()));
        self.found = Some(found);
    }
}

fn set_up(agent: &mut Agent, identifier: Option<&str>, recorded: Option<&str>) {
    let machine = FakeThisMachine::new(identifier);
    let machine = match recorded {
        Some(recorded) => machine.with_recorded(recorded),
        None => machine,
    };
    agent.machine.machine = Some(Arc::new(machine));
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("a repository alias")
}

#[given("the data folder was copied from another computer")]
fn copied(agent: &mut Agent) {
    set_up(agent, Some(THIS_COMPUTER), Some(OTHER_COMPUTER));
}

#[given("the data folder was made on this computer")]
fn made_here(agent: &mut Agent) {
    set_up(agent, Some(THIS_COMPUTER), Some(THIS_COMPUTER));
}

#[given("the data folder was made before the machine identifier was kept")]
fn made_before(agent: &mut Agent) {
    set_up(agent, Some(THIS_COMPUTER), None);
}

#[given("this computer's machine identifier can't be read")]
fn unread(agent: &mut Agent) {
    set_up(agent, None, Some(THIS_COMPUTER));
}

#[when("the agent starts on this computer")]
fn starts(agent: &mut Agent) {
    agent.machine.start();
}

#[when("the agent starts on this computer again")]
fn starts_again(agent: &mut Agent) {
    agent.machine.start();
}

#[given("a check of the repository is running here")]
async fn check_running(agent: &mut Agent) {
    let id = OperationId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0296));
    let at = Timestamp::from_second(1_800_000_000).expect("a time");
    let target = Target {
        plan: None,
        repository: Some(agent.forget.repository_id().clone()),
    };
    let mut check = Operation::queue(id, OperationKind::Check, Trigger::Manual, target, at);
    check.start(at).expect("a queued operation starts");
    assert_eq!(
        agent.store.try_start(&check).await,
        Ok(Start::Started),
        "the log starts the check"
    );
    agent.machine.running = Some(id);
}

#[when("the person takes a new machine ID")]
async fn takes_new_id(agent: &mut Agent) {
    let take = TakeNewId::new(
        agent.machine.machine(),
        agent.machine.standing(),
        agent.forget.store(),
        Arc::clone(&agent.store) as _,
        Arc::default(),
    );
    agent.machine.taken = Some(take.take().await);
}

#[then("it is refused because the data folder was copied from another computer")]
fn refused_as_copied(agent: &mut Agent) {
    assert_eq!(
        agent.forget.take_refused(),
        Some(MaintenanceError::DataFolderCopied),
        "the request is refused as one a copied data folder may not run"
    );
}

#[then("the agent finds its data folder was copied from another computer")]
fn finds_copied(agent: &mut Agent) {
    assert_eq!(agent.machine.found, Some(Found::Copied));
    assert_eq!(agent.machine.standing.get(), Standing::Copied);
}

#[then("the agent finds nothing new")]
fn finds_nothing_new(agent: &mut Agent) {
    assert!(
        matches!(agent.machine.found, Some(Found::Same | Found::Recorded)),
        "the check found {:?}",
        agent.machine.found
    );
    assert_eq!(agent.machine.standing.get(), Standing::Same);
}

#[then("the agent notes that this computer's machine identifier couldn't be read")]
fn notes_unread(agent: &mut Agent) {
    assert!(
        matches!(agent.machine.found, Some(Found::Unread(_))),
        "the check found {:?}",
        agent.machine.found
    );
    assert_eq!(agent.machine.standing.get(), Standing::Unread);
}

#[then("the agent maintains the repositories its configuration asks it to")]
fn maintains(agent: &mut Agent) {
    assert!(agent.machine.standing.maintains());
}

#[then("the agent doesn't maintain the repositories its configuration asks it to")]
fn maintains_nothing(agent: &mut Agent) {
    assert!(!agent.machine.standing.maintains());
}

#[then("maintenance stays refused until the agent starts again")]
fn refused_until_restart(agent: &mut Agent) {
    assert_eq!(agent.machine.standing.get(), Standing::NewIdTaken);
    assert!(!agent.machine.standing.maintains());
}

#[then("the machine has a new machine ID")]
fn has_new_id(agent: &mut Agent) {
    let taken = match &agent.machine.taken {
        Some(Ok(id)) => *id,
        other => panic!("no new machine ID was taken: {other:?}"),
    };
    assert_eq!(agent.machine.machine().renewed_id(), Some(taken));
}

#[then("the machine ID is unchanged")]
fn id_unchanged(agent: &mut Agent) {
    assert_eq!(agent.machine.machine().renewed_id(), None);
}

#[then("the data folder records this computer's machine identifier")]
fn records_this_computer(agent: &mut Agent) {
    assert_eq!(
        agent.machine.machine().recorded(),
        Ok(Some(
            MachineIdentifier::parse(THIS_COMPUTER).expect("an identifier")
        ))
    );
}

#[then("taking a new machine ID is refused, as the data folder isn't a copy")]
fn new_id_refused(agent: &mut Agent) {
    assert_eq!(agent.machine.taken, Some(Err(NewIdError::NotCopied)));
}

#[then("taking a new machine ID is refused because that check is running")]
fn new_id_busy(agent: &mut Agent) {
    let running = agent.machine.running.expect("a Given started the check");
    assert_eq!(agent.machine.taken, Some(Err(NewIdError::Busy { running })));
}

#[then(expr = "the repository's maintenance is now {string}")]
async fn maintenance_now(agent: &mut Agent, maintenance: String) {
    let config = agent
        .forget
        .store()
        .load()
        .await
        .expect("the configuration loads");
    let entry = config.repository(&alias()).expect("the repository");
    assert_eq!(entry.maintenance.as_str(), maintenance);
}
