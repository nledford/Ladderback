//! Steps for `features/execution/recovery.feature`: they leave operations in
//! the in-memory operation log the world's `store` holds, as a daemon that
//! stopped would, start the daemon again through `RecoverOperations`, and read
//! the log back through its port.

use std::sync::Arc;

use app::fakes::InMemoryOperationStore;
use app::ports::{
    EngineResult, HistoryPage, HistoryQuery, OperationDetail, OperationStore, Start, StoreError,
};
use app::recovery::RecoverOperations;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::{OperationId, PlanId, RepositoryId, Timestamp, Uuid};

use super::Agent;

/// An operation the scenario put in the log, and how it names its target.
struct Seeded {
    id: OperationId,
    kind: OperationKind,
    plan: Option<String>,
    repository: Option<String>,
    /// The log's entry for it as it ended, for an operation that had ended
    /// before the daemon started again.
    ended: Option<OperationDetail>,
}

/// What the scenario left in the log, and what each start of the daemon
/// reported.
#[derive(Default)]
pub struct RecoveryWorld {
    /// How many operations the scenario has made, probes included.
    made: u128,
    seeded: Vec<Seeded>,
    /// What each start of the daemon reported, oldest first.
    starts: Vec<Result<u64, StoreError>>,
}

impl RecoveryWorld {
    fn next_id(&mut self) -> OperationId {
        self.made += 1;
        OperationId::from_uuid(Uuid::from_u128(0x7ec0_0000 + self.made))
    }

    /// A new queued operation of `kind` on the plan or repository the
    /// scenario names.
    fn queue(
        &mut self,
        kind: OperationKind,
        plan: Option<&str>,
        repository: Option<&str>,
    ) -> Operation {
        Operation::queue(
            self.next_id(),
            kind,
            Trigger::Schedule,
            Target {
                plan: plan.map(plan_id),
                repository: repository.map(repository_id),
            },
            at(0),
        )
    }

    fn remember(
        &mut self,
        op: &Operation,
        plan: Option<&str>,
        repository: Option<&str>,
        ended: Option<OperationDetail>,
    ) {
        self.seeded.push(Seeded {
            id: op.id(),
            kind: op.kind(),
            plan: plan.map(str::to_owned),
            repository: repository.map(str::to_owned),
            ended,
        });
    }

    fn backup(&self, plan: &str) -> &Seeded {
        self.seeded
            .iter()
            .find(|seeded| {
                seeded.kind == OperationKind::Backup && seeded.plan.as_deref() == Some(plan)
            })
            .unwrap_or_else(|| {
                panic!("the scenario never put a backup of the plan {plan} in the log")
            })
    }

    fn maintenance(&self, kind: OperationKind, repository: &str) -> &Seeded {
        self.seeded
            .iter()
            .find(|seeded| seeded.kind == kind && seeded.repository.as_deref() == Some(repository))
            .unwrap_or_else(|| {
                panic!(
                    "the scenario never put a {kind:?} of the repository {repository} in the log"
                )
            })
    }

    fn last_start(&self) -> &Result<u64, StoreError> {
        self.starts
            .last()
            .expect("the daemon started again in the scenario")
    }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).expect("a time in range")
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("the scenario's plan ID is valid")
}

/// The repository this module calls `alias`: always the same 64 hex digits
/// for the same name, as every repository's ID is.
fn repository_id(alias: &str) -> RepositoryId {
    let folded = alias.bytes().fold(0x2ec0_u128, |acc, byte| {
        acc.wrapping_mul(257).wrapping_add(u128::from(byte))
    });
    RepositoryId::parse(&format!("{folded:064x}")).expect("64 hex digits are a repository ID")
}

fn maintenance_kind(name: &str) -> OperationKind {
    match name {
        "prune" => OperationKind::Prune,
        "check" => OperationKind::Check,
        "forget" => OperationKind::Forget,
        "adopt" => OperationKind::Adopt,
        other => panic!("the scenario names a maintenance operation that doesn't exist: {other}"),
    }
}

fn warning() -> Warning {
    Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some("/home/demo/locked.txt".to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

/// How an operation that ended before the daemon stopped is shown.
fn ending_status(ending: &str) -> OperationStatus {
    match ending {
        "succeeded" => OperationStatus::Succeeded,
        "succeeded with warnings" => OperationStatus::SucceededWithWarnings,
        "failed because the repository was unreachable" => OperationStatus::Failed {
            reason: unreachable_repository(),
        },
        "cancelled" => OperationStatus::Cancelled,
        other => panic!("the scenario ends an operation in a way that doesn't exist: {other}"),
    }
}

fn unreachable_repository() -> FailureReason {
    FailureReason::EngineFailed {
        detail: "repository unreachable".to_owned(),
    }
}

/// Starts `op` on the log and checks the log took it as running.
async fn start(agent: &mut Agent, op: &mut Operation) {
    op.start(at(10)).expect("a queued operation starts");
    let decided = agent
        .store
        .try_start(op)
        .await
        .expect("the log answers a start");
    assert_eq!(
        decided,
        Start::Started,
        "the scenario starts its operation on a free repository"
    );
}

async fn detail(agent: &Agent, id: OperationId) -> OperationDetail {
    agent
        .store
        .get(id)
        .await
        .expect("the log answers")
        .expect("the log holds the operation")
}

#[given(expr = "the log shows a backup of the plan {string} as running")]
async fn backup_running(agent: &mut Agent, plan: String) {
    let mut op = agent
        .recovery
        .queue(OperationKind::Backup, Some(&plan), None);
    start(agent, &mut op).await;
    agent.recovery.remember(&op, Some(&plan), None, None);
}

#[given(
    expr = "the log shows a backup of the plan {string} as running with {int} warning(s) recorded"
)]
async fn backup_running_with_warnings(agent: &mut Agent, plan: String, count: usize) {
    let mut op = agent
        .recovery
        .queue(OperationKind::Backup, Some(&plan), None);
    op.start(at(10)).expect("a queued operation starts");
    for _ in 0..count {
        op.record_warning(warning())
            .expect("a running operation records a warning");
    }
    agent
        .store
        .save(&op, None)
        .await
        .expect("the log answers a save");
    agent.recovery.remember(&op, Some(&plan), None, None);
}

#[given(expr = "the log shows a backup of the plan {string} as queued")]
async fn backup_queued(agent: &mut Agent, plan: String) {
    let op = agent
        .recovery
        .queue(OperationKind::Backup, Some(&plan), None);
    agent
        .store
        .save(&op, None)
        .await
        .expect("the log answers a save");
    agent.recovery.remember(&op, Some(&plan), None, None);
}

#[given(expr = "the log shows a {word} of the repository {string} as running")]
async fn maintenance_running(agent: &mut Agent, kind: String, repository: String) {
    let kind = maintenance_kind(&kind);
    let mut op = agent.recovery.queue(kind, None, Some(&repository));
    start(agent, &mut op).await;
    agent.recovery.remember(&op, None, Some(&repository), None);

    // The repository is held: a maintenance operation of another kind that
    // tries to start on it is refused, and nothing of it is stored.
    let other = if kind == OperationKind::Check {
        OperationKind::Prune
    } else {
        OperationKind::Check
    };
    let mut probe = agent.recovery.queue(other, None, Some(&repository));
    probe.start(at(11)).expect("a queued operation starts");
    let decided = agent
        .store
        .try_start(&probe)
        .await
        .expect("the log answers a start");
    assert_eq!(
        decided,
        Start::Refused { running: op.id() },
        "the running {kind:?} holds the repository before the daemon starts again"
    );
}

#[given(
    regex = r#"^the log shows a backup of the plan "([^"]+)" that ended as (succeeded with warnings|succeeded|failed because the repository was unreachable|cancelled)$"#
)]
async fn backup_ended(agent: &mut Agent, plan: String, ending: String) {
    let mut op = agent
        .recovery
        .queue(OperationKind::Backup, Some(&plan), None);
    op.start(at(10)).expect("a queued operation starts");
    let ended = match ending.as_str() {
        "succeeded" => op.succeed(at(70)),
        "succeeded with warnings" => {
            op.record_warning(warning())
                .expect("a running operation records a warning");
            op.succeed(at(70))
        }
        "failed because the repository was unreachable" => {
            op.fail(unreachable_repository(), at(70))
        }
        "cancelled" => op.cancel(at(70)),
        other => panic!("the scenario ends an operation in a way that doesn't exist: {other}"),
    };
    ended.expect("a running operation ends");
    agent
        .store
        .save(&op, None)
        .await
        .expect("the log answers a save");
    let snapshot = detail(agent, op.id()).await;
    assert_eq!(snapshot.record.status, ending_status(&ending));
    agent
        .recovery
        .remember(&op, Some(&plan), None, Some(snapshot));
}

#[given("the log shows no operation at all")]
async fn empty_log(agent: &mut Agent) {
    let page = agent
        .store
        .list(&HistoryQuery::default())
        .await
        .expect("the log answers a list");
    assert!(
        page.operations.is_empty(),
        "the scenario's log starts empty"
    );
}

#[when("the daemon starts again")]
async fn daemon_starts_again(agent: &mut Agent) {
    let recovery = RecoverOperations::new(agent.store.clone());
    agent.recovery.starts.push(recovery.recover().await);
}

#[when("the daemon starts again on a log that cannot be written")]
async fn daemon_starts_on_unwritable_log(agent: &mut Agent) {
    let recovery = RecoverOperations::new(Arc::new(UnwritableLog(agent.store.clone())));
    agent.recovery.starts.push(recovery.recover().await);
}

#[then(expr = "the log shows the backup of the plan {string} as failed because the daemon stopped")]
async fn backup_failed_daemon_stopped(agent: &mut Agent, plan: String) {
    let id = agent.recovery.backup(&plan).id;
    assert_failed_daemon_stopped(agent, id).await;
}

#[then(
    expr = "the log shows the {word} of the repository {string} as failed because the daemon stopped"
)]
async fn maintenance_failed_daemon_stopped(agent: &mut Agent, kind: String, repository: String) {
    let id = agent
        .recovery
        .maintenance(maintenance_kind(&kind), &repository)
        .id;
    assert_failed_daemon_stopped(agent, id).await;
}

async fn assert_failed_daemon_stopped(agent: &Agent, id: OperationId) {
    let status = detail(agent, id).await.record.status;
    assert_eq!(
        status,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
    match status {
        OperationStatus::Failed { reason } => assert_eq!(reason.code(), "daemon-stopped"),
        other => panic!("the operation is not failed: {other:?}"),
    }
}

#[then(expr = "the log never shows the backup of the plan {string} as succeeded")]
async fn backup_never_succeeded(agent: &mut Agent, plan: String) {
    let id = agent.recovery.backup(&plan).id;
    let status = detail(agent, id).await.record.status;
    assert!(
        !matches!(
            status,
            OperationStatus::Succeeded | OperationStatus::SucceededWithWarnings
        ),
        "the log shows the backup as {status:?}"
    );
}

#[then(expr = "the log still shows {int} warning(s) on the backup of the plan {string}")]
async fn backup_keeps_warnings(agent: &mut Agent, count: usize, plan: String) {
    let id = agent.recovery.backup(&plan).id;
    let held = detail(agent, id).await;
    assert_eq!(held.record.warning_count, count as u64);
    assert_eq!(held.warnings.len(), count);
}

#[then(expr = "the log still shows the backup of the plan {string} as running")]
async fn backup_still_running(agent: &mut Agent, plan: String) {
    let id = agent.recovery.backup(&plan).id;
    assert_eq!(
        detail(agent, id).await.record.status,
        OperationStatus::Running
    );
}

#[then(
    regex = r#"^the log still shows the backup of the plan "([^"]+)" as (succeeded with warnings|succeeded|failed because the repository was unreachable|cancelled)$"#
)]
async fn backup_keeps_its_ending(agent: &mut Agent, plan: String, ending: String) {
    let seeded = agent.recovery.backup(&plan);
    let id = seeded.id;
    let before = seeded
        .ended
        .clone()
        .expect("the scenario put an ended backup in the log");
    let now = detail(agent, id).await;
    assert_eq!(now.record.status, ending_status(&ending));
    assert_eq!(now, before, "nothing of an ended operation changed");
}

#[then(expr = "the repository {string} is free for a prune and for a check")]
async fn repository_is_free(agent: &mut Agent, repository: String) {
    for kind in [OperationKind::Prune, OperationKind::Check] {
        let mut op = agent.recovery.queue(kind, None, Some(&repository));
        start(agent, &mut op).await;
        // Ending it frees the repository again for the next kind.
        op.succeed(at(80)).expect("a running operation ends");
        agent
            .store
            .save(&op, None)
            .await
            .expect("the log answers a save");
    }
}

#[then(expr = "the daemon's start ended {int} operation(s)")]
fn start_ended(agent: &mut Agent, count: u64) {
    assert_eq!(agent.recovery.last_start(), &Ok(count));
}

#[then(expr = "the daemon's first start ended {int} operation(s)")]
fn first_start_ended(agent: &mut Agent, count: u64) {
    let first = agent
        .recovery
        .starts
        .first()
        .expect("the daemon started again in the scenario");
    assert_eq!(first, &Ok(count));
}

#[then("no operation the scenario put in the log is left queued or running")]
async fn none_unfinished(agent: &mut Agent) {
    let ids: Vec<OperationId> = agent
        .recovery
        .seeded
        .iter()
        .map(|seeded| seeded.id)
        .collect();
    for id in ids {
        let status = detail(agent, id).await.record.status;
        assert!(status.is_finished(), "an operation is still {status:?}");
    }
}

#[then("the daemon's start reports that the log could not be written")]
fn start_reports_unwritable(agent: &mut Agent) {
    assert!(
        matches!(
            agent.recovery.last_start(),
            Err(StoreError::Unavailable { .. })
        ),
        "the start reported {:?}",
        agent.recovery.last_start()
    );
}

/// The scenario's log, but one that refuses to end operations.
struct UnwritableLog(Arc<InMemoryOperationStore>);

#[async_trait]
impl OperationStore for UnwritableLog {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        self.0.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.0.try_start(operation).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.0.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.0.list(query).await
    }

    async fn ever_succeeded(&self, plan: &domain::PlanId) -> Result<bool, StoreError> {
        self.0.ever_succeeded(plan).await
    }

    async fn fail_unfinished(&self, _reason: &FailureReason) -> Result<u64, StoreError> {
        Err(StoreError::Unavailable {
            detail: "the log cannot be written".to_owned(),
        })
    }
}
