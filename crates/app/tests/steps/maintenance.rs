//! Steps for `features/maintenance/*.feature`: they start and end operations
//! through the operation log's port, on the in-memory log the world's `store`
//! holds, and read the history back through its use cases.

use app::history::{GetOperation, ListOperations};
use app::ports::{OperationDetail, OperationRecord, OperationStore, Start};
use cucumber::{given, then, when};
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::{OperationId, RepositoryId, Timestamp, Uuid};
use futures::future::join;

use super::Agent;

/// The operations a scenario has made, and what its attempts to start them
/// decided.
#[derive(Default)]
pub struct MaintenanceWorld {
    /// The names the scenario gave its repositories, in order of first use.
    repositories: Vec<String>,
    /// How many operations the scenario has made.
    made: u128,
    /// The operation the scenario started and left running.
    running: Option<Operation>,
    /// The operation the scenario left in the queue.
    queued: Option<Operation>,
    /// Each attempt to start an operation, oldest first, with what it decided.
    attempts: Vec<(Operation, Start)>,
}

impl MaintenanceWorld {
    /// The ID of the repository the scenario calls `name`: 64 hex digits,
    /// as every repository's ID is.
    fn repository(&mut self, name: &str) -> RepositoryId {
        let position = match self.repositories.iter().position(|known| known == name) {
            Some(position) => position,
            None => {
                self.repositories.push(name.to_owned());
                self.repositories.len() - 1
            }
        };
        RepositoryId::parse(&format!("{:064x}", position + 1))
            .expect("64 hex digits are a repository ID")
    }

    /// A new queued operation of the kind called `kind` on the repository
    /// called `repository`.
    fn queue(&mut self, kind: &str, repository: &str) -> Operation {
        let repository = self.repository(repository);
        self.made += 1;
        Operation::queue(
            OperationId::from_uuid(Uuid::from_u128(self.made)),
            kind_named(kind),
            Trigger::Manual,
            Target {
                plan: None,
                repository: Some(repository),
            },
            at(0),
        )
    }

    fn last_attempt(&self) -> &(Operation, Start) {
        self.attempts
            .last()
            .expect("the scenario tried to start an operation")
    }

    fn running(&self) -> &Operation {
        self.running
            .as_ref()
            .expect("the scenario left an operation running")
    }

    fn queued(&self) -> &Operation {
        self.queued
            .as_ref()
            .expect("the scenario left an operation queued")
    }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).expect("a time in range")
}

fn kind_named(name: &str) -> OperationKind {
    match name {
        "prune" => OperationKind::Prune,
        "check" => OperationKind::Check,
        "backup" => OperationKind::Backup,
        "identify" => OperationKind::Identify,
        other => panic!("the scenario names an operation that doesn't exist: {other}"),
    }
}

/// An operation the scenario starts itself, so that it is running.
fn started(mut op: Operation) -> Operation {
    op.start(at(10)).expect("a queued operation starts");
    op
}

async fn try_start(agent: &mut Agent, op: Operation) {
    let decided = agent
        .store
        .try_start(&op)
        .await
        .expect("the log answers a start");
    agent.maintenance.attempts.push((op, decided));
}

/// What the history holds for the operation `id`.
async fn history_of(agent: &mut Agent, id: OperationId) -> Option<OperationDetail> {
    let store = agent.store().await;
    GetOperation::new(store)
        .get(id)
        .await
        .expect("the history answers")
}

#[given(expr = "a {word} of the repository {string} is running")]
async fn is_running(agent: &mut Agent, kind: String, repository: String) {
    let op = started(agent.maintenance.queue(&kind, &repository));
    let decided = agent
        .store
        .try_start(&op)
        .await
        .expect("the log answers a start");
    assert_eq!(
        decided,
        Start::Started,
        "the scenario starts its first operation on a free repository"
    );
    agent.maintenance.running = Some(op);
}

#[given(expr = "a {word} of the repository {string} is queued")]
async fn is_queued(agent: &mut Agent, kind: String, repository: String) {
    let op = agent.maintenance.queue(&kind, &repository);
    agent
        .store
        .save(&op, None)
        .await
        .expect("the log answers a save");
    agent.maintenance.queued = Some(op);
}

#[given(regex = r"^the running operation (succeeds with warnings|succeeds|fails|is cancelled)$")]
async fn running_operation_ends(agent: &mut Agent, ending: String) {
    let mut op = agent.maintenance.running().clone();
    let ended = match ending.as_str() {
        "succeeds" => op.succeed(at(70)),
        "succeeds with warnings" => {
            op.record_warning(Warning {
                kind: WarningKind::SourceUnreadable,
                item: Some("/home/demo/locked.txt".to_owned()),
                during: "prune".to_owned(),
                detail: "permission denied".to_owned(),
            })
            .expect("a running operation records a warning");
            op.succeed(at(70))
        }
        "fails" => op.fail(FailureReason::EngineExited { exit_code: Some(1) }, at(70)),
        "is cancelled" => op.cancel(at(70)),
        other => panic!("the scenario ends an operation in a way that doesn't exist: {other}"),
    };
    ended.expect("a running operation ends");
    agent
        .store
        .save(&op, None)
        .await
        .expect("the log answers a save");
    agent.maintenance.running = Some(op);
}

#[when(expr = "a {word} of the repository {string} tries to start")]
async fn tries_to_start(agent: &mut Agent, kind: String, repository: String) {
    let op = started(agent.maintenance.queue(&kind, &repository));
    try_start(agent, op).await;
}

#[when(expr = "the queued {word} tries to start")]
async fn queued_tries_to_start(agent: &mut Agent, kind: String) {
    let op = agent.maintenance.queued().clone();
    assert_eq!(op.kind(), kind_named(&kind), "the queued operation's kind");
    try_start(agent, started(op)).await;
}

#[when(expr = "the running {word} tries to start again")]
async fn running_tries_to_start_again(agent: &mut Agent, kind: String) {
    let op = agent.maintenance.running().clone();
    assert_eq!(op.kind(), kind_named(&kind), "the running operation's kind");
    try_start(agent, op).await;
}

#[when(expr = "a {word} and a {word} of the repository {string} try to start at the same moment")]
async fn try_to_start_at_the_same_moment(
    agent: &mut Agent,
    first: String,
    second: String,
    repository: String,
) {
    let first = started(agent.maintenance.queue(&first, &repository));
    let second = started(agent.maintenance.queue(&second, &repository));
    let (first_decided, second_decided) = join(
        agent.store.try_start(&first),
        agent.store.try_start(&second),
    )
    .await;
    let first_decided = first_decided.expect("the log answers a start");
    let second_decided = second_decided.expect("the log answers a start");
    agent.maintenance.attempts.push((first, first_decided));
    agent.maintenance.attempts.push((second, second_decided));
}

#[then("the start is allowed")]
fn start_is_allowed(agent: &mut Agent) {
    let (op, decided) = agent.maintenance.last_attempt();
    assert_eq!(
        decided,
        &Start::Started,
        "the {:?} wasn't allowed to start",
        op.kind()
    );
}

#[then("the start is refused")]
fn start_is_refused(agent: &mut Agent) {
    let (op, decided) = agent.maintenance.last_attempt();
    assert!(
        matches!(decided, Start::Refused { .. }),
        "the {:?} wasn't refused: {decided:?}",
        op.kind()
    );
}

#[then("the refusal names the running operation")]
fn refusal_names_the_running_operation(agent: &mut Agent) {
    let (_, decided) = agent.maintenance.last_attempt();
    assert_eq!(
        decided,
        &Start::Refused {
            running: agent.maintenance.running().id()
        }
    );
}

#[then("the history doesn't list the operation that tried to start")]
async fn history_doesnt_list_the_operation_that_tried(agent: &mut Agent) {
    let id = agent.maintenance.last_attempt().0.id();
    assert_eq!(history_of(agent, id).await, None);
    let store = agent.store().await;
    let listed = ListOperations::new(store)
        .list(None, None, None)
        .await
        .expect("the history answers");
    assert!(
        listed.operations.iter().all(|record| record.id != id),
        "the history lists it: {:?}",
        listed.operations
    );
}

#[then(expr = "the history still shows the queued {word} as queued")]
async fn history_shows_the_queued_operation(agent: &mut Agent, kind: String) {
    let op = agent.maintenance.queued().clone();
    assert_eq!(op.kind(), kind_named(&kind), "the queued operation's kind");
    let detail = history_of(agent, op.id()).await;
    let detail = detail.expect("the queued operation is in the history");
    assert_eq!(detail.record, OperationRecord::of(&op, None));
    assert_eq!(detail.record.status, OperationStatus::Queued);
}

#[then(expr = "the history still shows the running {word} as running")]
async fn history_shows_the_running_operation(agent: &mut Agent, kind: String) {
    let op = agent.maintenance.running().clone();
    assert_eq!(op.kind(), kind_named(&kind), "the running operation's kind");
    let detail = history_of(agent, op.id()).await;
    let detail = detail.expect("the running operation is in the history");
    assert_eq!(detail.record, OperationRecord::of(&op, None));
    assert_eq!(detail.record.status, OperationStatus::Running);
}

#[then("exactly one of them is started")]
fn exactly_one_is_started(agent: &mut Agent) {
    let attempts = &agent.maintenance.attempts;
    let started = attempts
        .iter()
        .filter(|(_, decided)| *decided == Start::Started)
        .count();
    assert_eq!(started, 1, "{attempts:?}");
}

#[then("the other is refused, naming the one that started")]
fn the_other_is_refused(agent: &mut Agent) {
    let attempts = &agent.maintenance.attempts;
    let winner = attempts
        .iter()
        .find(|(_, decided)| *decided == Start::Started)
        .map(|(op, _)| op.id())
        .expect("one of them started");
    let others: Vec<_> = attempts
        .iter()
        .filter(|(op, _)| op.id() != winner)
        .collect();
    assert_eq!(others.len(), 1, "{attempts:?}");
    for (_, decided) in others {
        assert_eq!(decided, &Start::Refused { running: winner });
    }
}
