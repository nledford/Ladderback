use std::assert_matches;

use super::support::*;
use super::*;

/// A backup started while another runs waits in the queue for its turn
/// (§5, the concurrency limit, 1 here): it is in the log as queued, and
/// nothing of it runs, its hooks included, until the running one ends.
#[tokio::test]
async fn a_backup_started_while_another_runs_waits_for_its_turn() {
    let (runner, mut entered, open_gate, _scratch) = runner_at_a_gate().await;
    let first = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");
    entered
        .changed()
        .await
        .expect("the first run reaches its hooks");
    let second = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts again");
    let second_id = operation_of(&second).expect("an operation ID");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), entered.changed())
            .await
            .is_err(),
        "the second run reached its hooks while the first one ran"
    );
    assert_eq!(status_of(&runner, second_id).await, OperationStatus::Queued);

    open_gate.send(true).expect("the gate opens");
    runner
        .cancel(first, http_adapter::Canceller::Session)
        .await
        .expect("the first run is canceled");
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        entered.wait_for(|entries| *entries >= 2),
    )
    .await
    .expect("the second run takes its turn once the first one ends")
    .expect("the gate stays open");
    runner
        .cancel(second, http_adapter::Canceller::Session)
        .await
        .expect("the second run is canceled");
}

/// The end the runner's next backup says, within a few seconds.
async fn said_end(ends: &app::retry::RunEnds) -> Option<app::retry::RunEnd> {
    tokio::time::timeout(std::time::Duration::from_secs(5), ends.next())
        .await
        .expect("the run says how it ended")
}

/// A backup's run says how it ended, so the scheduler can retry a
/// failed one (§5): here one started by hand, which a person cancels.
#[tokio::test]
async fn a_backup_started_now_says_how_it_ended() {
    let (mut runner, mut entered, open_gate, _scratch) = runner_at_a_gate().await;
    let (sender, ends) = app::retry::run_ends();
    runner.ends = sender;
    let started = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");
    let operation = operation_of(&started).expect("an operation ID");
    entered.changed().await.expect("the run reaches its hooks");
    open_gate.send(true).expect("the gate opens");
    runner
        .cancel(started, http_adapter::Canceller::Session)
        .await
        .expect("the run is canceled");
    assert_eq!(
        said_end(&ends).await,
        Some(app::retry::RunEnd {
            operation,
            plan: domain::PlanId::parse("home").expect("a plan ID"),
            trigger: Trigger::Manual,
            outcome: app::retry::RunOutcome::Canceled,
        })
    );
}

/// The same for a backup the queue held from before the start.
#[tokio::test]
async fn a_resumed_backup_says_how_it_ended() {
    let (mut runner, mut entered, open_gate, _scratch) = runner_at_a_gate().await;
    let (sender, ends) = app::retry::run_ends();
    runner.ends = sender;
    let operation = OperationId::from_uuid(Uuid::from_u128(0x7275));
    let queued = domain::execution::Operation::queue(
        operation,
        OperationKind::Backup,
        Trigger::Retry,
        domain::execution::Target {
            plan: Some(domain::PlanId::parse("home").expect("a plan ID")),
            repository: None,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    runner
        .store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    PlanRuns::resume(&runner, queued, None).await;
    entered.changed().await.expect("the run reaches its hooks");
    open_gate.send(true).expect("the gate opens");
    runner
        .cancel(
            operation.as_uuid().hyphenated().to_string(),
            http_adapter::Canceller::Session,
        )
        .await
        .expect("the run is canceled");
    assert_eq!(
        said_end(&ends).await,
        Some(app::retry::RunEnd {
            operation,
            plan: domain::PlanId::parse("home").expect("a plan ID"),
            trigger: Trigger::Retry,
            outcome: app::retry::RunOutcome::Canceled,
        })
    );
}

/// A runner as [`runner_at_a_gate`] makes it, whose log holds a backup of
/// "home" queued under `id`, over a configuration that can no longer be
/// read. Also the queued backup, and the scratch folder.
async fn a_queued_backup_over_a_spoiled_configuration(
    id: OperationId,
) -> (PlanRunner, domain::execution::Operation, tempfile::TempDir) {
    let (runner, _entered, _gate, scratch) = runner_at_a_gate().await;
    let queued = domain::execution::Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Manual,
        domain::execution::Target {
            plan: Some(domain::PlanId::parse("home").expect("a plan ID")),
            repository: None,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    runner
        .store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    std::fs::write(
        scratch.path().join("config").join("config.toml"),
        "not = [toml",
    )
    .expect("the configuration is spoiled");
    (runner, queued, scratch)
}

async fn status_of(runner: &PlanRunner, id: OperationId) -> OperationStatus {
    runner
        .store
        .get(id)
        .await
        .expect("the log reads")
        .expect("the backup is in the log")
        .record
        .status
}

/// How operation `id` ended, once the log has its end: a leftover that
/// won't run is ended on a task of its own (#1250).
async fn until_ended(runner: &PlanRunner, id: OperationId) -> OperationStatus {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let status = status_of(runner, id).await;
            if status.is_finished() {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the operation log ends operation {id:?}"))
}

/// A queued backup whose job can't be built at the restart ends as
/// failed, the log says why (decision 73), and the end is announced as
/// every end a start makes is (#547): left queued, it would show as
/// queued with nothing to run it until the next start. Here the
/// configuration becomes unreadable between the background task's read
/// and the runner's own.
#[tokio::test]
async fn a_queued_backup_ends_as_daemon_stopped_when_the_configuration_cannot_be_read() {
    super::captured_log::capture();
    let id = OperationId::from_uuid(Uuid::from_u128(0x2741));
    let (runner, queued, _scratch) = a_queued_backup_over_a_spoiled_configuration(id).await;
    let mut page = runner.events.subscribe().await;

    PlanRuns::resume(&runner, queued, None).await;
    // The end is on a task of its own (#1250): its news says it is done.
    let told = tokio::time::timeout(std::time::Duration::from_secs(5), page.next())
        .await
        .ok()
        .flatten();

    assert_eq!(
        status_of(&runner, id).await,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
    let hyphenated = id.as_uuid().hyphenated().to_string();
    let said = super::captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains(&hyphenated)
            && line.contains("the configuration can't be read")
            && line.contains("(config-invalid)")),
        "the log says why the backup ended: {said:?}"
    );
    assert_matches!(
        &told,
        Some(UiEvent::OperationFailed { operation, code: ErrorCode::DaemonStopped, .. })
            if *operation == hyphenated,
        "the end is announced"
    );
    assert!(
        futures::FutureExt::now_or_never(page.next()).is_none(),
        "the end is announced once"
    );
}

/// The same when the background task's own read of the configuration
/// fails: no job can be built, so every queued backup ends as failed,
/// the log says why, and each end is announced (#547).
#[tokio::test]
async fn the_queue_ends_as_daemon_stopped_when_the_background_task_cannot_read_the_configuration() {
    super::captured_log::capture();
    let id = OperationId::from_uuid(Uuid::from_u128(0x2742));
    let (runner, queued, _scratch) = a_queued_backup_over_a_spoiled_configuration(id).await;
    let mut page = runner.events.subscribe().await;

    super::resume_queued(
        vec![OperationRecord::of(&queued, None)],
        runner.config.as_ref(),
        Arc::clone(&runner.store),
        Arc::new(app::fakes::InMemoryScheduleStore::new()),
        Arc::clone(&runner.events),
        &runner,
    )
    .await;

    assert_eq!(
        status_of(&runner, id).await,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
    let said = super::captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains("the configuration can't be read, so any operation queued")),
        "the log says why the queue ended: {said:?}"
    );
    let hyphenated = id.as_uuid().hyphenated().to_string();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains(&hyphenated)
            && line.contains("ConfigUnreadable")),
        "the log names the backup that ended, and why: {said:?}"
    );
    let told = futures::FutureExt::now_or_never(page.next()).flatten();
    assert_matches!(
        &told,
        Some(UiEvent::OperationFailed { operation, code: ErrorCode::DaemonStopped, .. })
            if *operation == hyphenated,
        "the end is announced"
    );
}

/// A queued backup whose plan now backs up to another repository than
/// the one it was queued for isn't run there under the old record: it
/// ends as failed, and the log says why (decision 73).
#[tokio::test]
async fn a_queued_backup_whose_plan_moved_to_another_repository_ends_as_daemon_stopped() {
    super::captured_log::capture();
    let id = OperationId::from_uuid(Uuid::from_u128(0x2743));
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let queued = domain::execution::Operation::queue(
        id,
        OperationKind::Backup,
        Trigger::Schedule,
        domain::execution::Target {
            plan: Some(domain::PlanId::parse("home").expect("a plan ID")),
            repository: Some(
                domain::RepositoryId::parse(&"b".repeat(64)).expect("a repository ID"),
            ),
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    );
    runner
        .store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    PlanRuns::resume(&runner, queued, None).await;

    assert_eq!(
        until_ended(&runner, id).await,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
    let hyphenated = id.as_uuid().hyphenated().to_string();
    let said = super::captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains(&hyphenated)
            && line.contains("another repository")),
        "the log says why the backup ended: {said:?}"
    );
}
