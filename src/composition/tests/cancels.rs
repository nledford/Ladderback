use super::support::*;
use super::*;

#[tokio::test]
async fn a_running_operation_is_canceled_once() {
    let registry = CancelRegistry::new();
    let id = OperationId::from_uuid(Uuid::new_v4());
    let cancel = registry.register(id);
    assert!(registry.cancel(&id), "the running operation cancels");
    let sleep = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(sleep);
    tokio::select! {
        _ = cancel => {}
        _ = &mut sleep => panic!("canceling fired the run's cancel future"),
    }
    assert!(!registry.cancel(&id), "one handle, one cancel");
    registry.finish(&id);
    assert!(
        !registry.cancel(&id),
        "a finished operation no longer cancels"
    );
}

/// A cancel that arrives the instant the route answers finds the
/// operation, not an "already ended" refusal (§4.2): the cancel switch
/// is parked before the run starts, while the run is still in its
/// before-hooks.
#[tokio::test]
async fn a_cancel_in_the_start_window_stops_the_operation() {
    let (runner, mut entered, open_gate, _scratch) = runner_at_a_gate().await;
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");
    // The run is in its before-hooks now, parked at the gate and not yet
    // at the engine, so the switch must already be parked for a cancel.
    entered.changed().await.expect("the run reaches its hooks");
    // The cancel is answered while the gate is still closed: a switch
    // parked any later than the route's answer is a refusal here.
    let cancel = runner.cancel(id, http_adapter::Canceller::Session).await;
    open_gate.send(true).expect("the gate opens");
    cancel.expect("a cancel in the start window is accepted");
}

/// A client that cancels the instant it has the answer to its start is
/// not told the operation is unknown or not running: the operation is
/// recorded and its cancel switch parked before the route answers, and
/// the run it cancels ends canceled.
#[tokio::test]
async fn a_cancel_right_after_the_answer_is_accepted_and_ends_the_operation_canceled() {
    let (runner, _entered, open_gate, _scratch) = runner_at_a_gate().await;
    open_gate.send(true).expect("the gate opens");
    let id = runner
        .start_backup("home".into(), Trigger::Manual)
        .await
        .expect("the plan starts");
    runner
        .cancel(id.clone(), http_adapter::Canceller::Session)
        .await
        .expect("a cancel straight after the answer is accepted");
    let operation = operation_of(&id).expect("the answer is an operation ID");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}

fn an_api_token_trigger() -> Trigger {
    Trigger::ApiToken {
        id: domain::access::api_token::ApiTokenId::parse("abcdefghijkl").expect("an ID"),
        label: domain::access::api_token::ApiTokenLabel::new("nightly-mcp".to_owned())
            .expect("a label"),
    }
}

/// A backup an API token started is logged with the token's trigger, and
/// the token may cancel it (decision 93).
#[tokio::test]
async fn a_backup_an_api_token_started_is_logged_as_its_and_the_token_cancels_it() {
    let (runner, _entered, open_gate, _scratch) = runner_at_a_gate().await;
    open_gate.send(true).expect("the gate opens");
    let id = runner
        .start_backup("home".into(), an_api_token_trigger())
        .await
        .expect("the plan starts");
    let operation = operation_of(&id).expect("the answer is an operation ID");
    let logged = GetOperation::new(Arc::clone(&runner.store))
        .get(operation)
        .await
        .expect("the log reads")
        .expect("the operation is logged");
    assert_eq!(logged.record.trigger, an_api_token_trigger());
    runner
        .cancel(id.clone(), http_adapter::Canceller::ApiToken)
        .await
        .expect("a token cancels a backup");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}

/// An API token stops backups only: a running check is refused to it as
/// needing a session, and goes on until a session cancels it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_api_token_cancels_no_maintenance_run_and_a_session_still_can() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(IdentifiesThenParks);
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts")
        .operation;
    assert_eq!(
        runner
            .cancel(id.clone(), http_adapter::Canceller::ApiToken)
            .await,
        Err(http_adapter::CancelError::SessionOnly)
    );
    runner
        .cancel(id.clone(), http_adapter::Canceller::Session)
        .await
        .expect("a session cancels the check");
    let operation = operation_of(&id).expect("the answer is an operation ID");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}

/// A client that gives up on its start while the log is still being
/// written (the connection drops, and the handler with it) leaves a run
/// that goes on: nothing is left queued and never run, or registered and
/// never cancellable.
#[tokio::test]
async fn a_start_whose_client_gave_up_still_runs_and_still_cancels() {
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let (open_log, log_is_open) = tokio::sync::watch::channel(false);
    runner.store = Arc::new(GatedSaves {
        inner: Arc::clone(&runner.store),
        open: log_is_open,
    });
    let answered = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        runner.start_backup("home".into(), Trigger::Manual),
    )
    .await;
    assert!(answered.is_err(), "the start waits for the log");
    open_log.send(true).expect("the log opens");
    let operation = *runner
        .registry
        .running()
        .keys()
        .next()
        .expect("the start parked a cancel switch");
    let id = operation.as_uuid().hyphenated().to_string();
    let accepted = async {
        loop {
            match runner
                .cancel(id.clone(), http_adapter::Canceller::Session)
                .await
            {
                Err(http_adapter::CancelError::Unknown) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                other => return other,
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), accepted)
        .await
        .expect("the operation ends up in the log")
        .expect("the operation that was started is cancellable");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}

/// A client that cancels the instant it has the answer to a maintenance
/// start is not told the operation is unknown or not running: the run
/// starts on a task of its own, and that task parks the run's cancel
/// switch before the route answers (§4.2). The run it cancels ends
/// canceled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_right_after_a_maintenance_start_is_accepted_and_ends_the_run_canceled() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(IdentifiesThenParks);
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts")
        .operation;
    runner
        .cancel(id.clone(), http_adapter::Canceller::Session)
        .await
        .expect("a cancel straight after the answer is accepted");
    let operation = operation_of(&id).expect("the answer is an operation ID");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}
