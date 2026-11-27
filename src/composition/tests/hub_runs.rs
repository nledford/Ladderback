//! The plan runner as the hub's runs (§16, decision 176): a backup the hub
//! asked for is Back up now's, logged under the hub's session; the hub
//! finds the plan's backup going and cancels backups only.

use app::hub_commands::{HubCancelRefused, HubRunRefused, HubRuns};
use domain::access::SessionTag;

use super::support::*;
use super::*;

fn plan(name: &str) -> PlanId {
    PlanId::parse(name).expect("a plan ID")
}

fn tag() -> SessionTag {
    SessionTag::parse("0123456789abcdef").expect("a session tag")
}

/// The hub's backup is logged with the session that asked, is the plan's
/// backup going while it waits or runs, and the hub cancels it.
#[tokio::test]
async fn the_hubs_backup_is_logged_as_its_found_going_and_canceled() {
    let (runner, _entered, open_gate, _scratch) = runner_at_a_gate().await;
    let operation = HubRuns::queue(&runner, &plan("home"), Some(tag()))
        .await
        .expect("the plan is queued");
    let logged = GetOperation::new(Arc::clone(&runner.store))
        .get(operation)
        .await
        .expect("the log reads")
        .expect("the operation is logged");
    assert_eq!(
        logged.record.trigger,
        Trigger::Hub {
            asked_by: Some(tag())
        }
    );
    assert_eq!(runner.going(&plan("home")).await, Some(operation));
    assert_eq!(runner.going(&plan("other")).await, None);
    HubRuns::cancel(&runner, &operation)
        .await
        .expect("the hub cancels a backup");
    open_gate.send(true).expect("the gate opens");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
    assert_eq!(runner.going(&plan("home")).await, None);
    assert_eq!(
        HubRuns::cancel(&runner, &operation).await,
        Err(HubCancelRefused::NotRunning)
    );
}

/// A plan the machine doesn't have isn't run, and an operation it never
/// logged isn't canceled.
#[tokio::test]
async fn the_hub_is_refused_an_unknown_plan_and_operation() {
    let (runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    assert_eq!(
        HubRuns::queue(&runner, &plan("missing"), None).await,
        Err(HubRunRefused::UnknownPlan)
    );
    assert_eq!(
        HubRuns::cancel(&runner, &OperationId::from_uuid(Uuid::from_u128(7))).await,
        Err(HubCancelRefused::Unknown)
    );
}

/// Points every repository's password at an environment variable that is
/// never set, read by `secrets`.
async fn password_read_by(runner: &mut PlanRunner, secrets: Arc<dyn app::ports::SecretResolver>) {
    let config = app::ports::ConfigStore::load(&*runner.config)
        .await
        .expect("the config loads");
    let repositories = config
        .repositories()
        .iter()
        .map(|(alias, repository)| {
            let mut repository = repository.clone();
            repository.settings.password =
                domain::secrets::SecretRef::Env("LADDERBACK_TEST_PASSWORD_NEVER_SET".to_owned());
            (alias.clone(), repository)
        })
        .collect();
    let unreadable = domain::planning::AgentConfig::new(
        config.agent().clone(),
        config.hooks().clone(),
        repositories,
        config.plans().to_vec(),
    )
    .expect("a valid configuration");
    app::ports::ConfigStore::save(&*runner.config, &unreadable)
        .await
        .expect("the config saves");
    runner.resolve = Arc::new(ResolveRepository::new(secrets));
}

/// A password source that never answers, as a password command that
/// hangs or a keyring waiting on a prompt nobody sees.
struct NeverAnswers;

#[async_trait]
impl app::ports::SecretResolver for NeverAnswers {
    async fn resolve(
        &self,
        _secret: &domain::secrets::SecretRef,
    ) -> Result<app::ports::Resolved, domain::secrets::SecretError> {
        std::future::pending().await
    }

    async fn keyring_lock(&self) -> app::ports::KeyringLock {
        app::ports::KeyringLock::Unknown
    }
}

/// The machine answers the hub's backup once it is queued, while its
/// repository's password is still being read, so a password source slow
/// to answer holds neither the answer nor the machine's heartbeats; a
/// cancel meanwhile ends the backup canceled before any hook runs
/// (decision 176).
#[tokio::test]
async fn the_hubs_backup_is_answered_while_its_password_is_read_and_a_cancel_then_ends_it() {
    let (mut runner, entered, _gate, _scratch) = runner_at_a_gate().await;
    password_read_by(&mut runner, Arc::new(NeverAnswers)).await;
    let operation = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        HubRuns::queue(&runner, &plan("home"), Some(tag())),
    )
    .await
    .expect("the machine answers while the password is read")
    .expect("the plan is queued");
    assert_eq!(runner.going(&plan("home")).await, Some(operation));
    HubRuns::cancel(&runner, &operation)
        .await
        .expect("the hub cancels the backup");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
    assert_eq!(*entered.borrow(), 0, "a hook ran");
}

/// A hub whose connection drops while the machine writes the hub's
/// backup to its log, and the answer with it, leaves a backup that goes
/// on: nothing is left queued and never run, or registered and never
/// cancellable, as for Back up now.
#[tokio::test]
async fn the_hubs_backup_whose_answer_nobody_waits_for_still_runs_and_still_cancels() {
    let (mut runner, _entered, open_hooks, _scratch) = runner_at_a_gate().await;
    open_hooks.send(true).expect("the gate opens");
    let (open_log, log_is_open) = tokio::sync::watch::channel(false);
    runner.store = Arc::new(GatedSaves {
        inner: Arc::clone(&runner.store),
        open: log_is_open,
    });
    let answered = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        HubRuns::queue(&runner, &plan("home"), Some(tag())),
    )
    .await;
    assert!(answered.is_err(), "the answer waits for the log");
    open_log.send(true).expect("the log opens");
    let operation = *runner
        .registry
        .running()
        .keys()
        .next()
        .expect("the start parked a cancel switch");
    let accepted = async {
        loop {
            match HubRuns::cancel(&runner, &operation).await {
                Err(HubCancelRefused::Unknown) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                other => return other,
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), accepted)
        .await
        .expect("the operation ends up in the log")
        .expect("the backup that was started is cancellable");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}

/// The hub's backup is queued before its repository's password is read:
/// the log holds it when the machine answers, and a password that can't
/// be read ends that run failed for it, before any hook runs, as an
/// unattended run's does (decision 176). The daemon's log says the hub's
/// run didn't start, not that a queued backup can't run again.
#[tokio::test]
async fn the_hubs_backup_is_queued_and_a_password_it_cant_read_fails_it() {
    super::captured_log::capture();
    let (mut runner, entered, _gate, _scratch) = runner_at_a_gate().await;
    password_read_by(&mut runner, Arc::new(app::fakes::FakeSecrets::new())).await;
    let operation = HubRuns::queue(&runner, &plan("home"), Some(tag()))
        .await
        .expect("the plan is queued");
    let logged = GetOperation::new(Arc::clone(&runner.store))
        .get(operation)
        .await
        .expect("the log reads")
        .expect("the operation is logged");
    assert_eq!(
        logged.record.trigger,
        Trigger::Hub {
            asked_by: Some(tag())
        }
    );
    let status = ended(&runner, operation).await;
    assert!(
        matches!(
            status,
            OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused { .. }
            }
        ),
        "{status:?}"
    );
    assert_eq!(*entered.borrow(), 0, "a hook ran");
    let id = operation.as_uuid().hyphenated().to_string();
    let said: Vec<String> = super::captured_log::lines()
        .into_iter()
        .filter(|(level, line)| *level == log::Level::Warn && line.contains(&id))
        .map(|(_, line)| line)
        .collect();
    assert!(
        said.iter().any(|line| line.contains(
            "the hub's run, asked for by session 0123456789abcdef of plan home didn't start"
        )),
        "{said:?}"
    );
    assert!(
        said.iter().all(|line| !line.contains("can't run again")),
        "{said:?}"
    );
}

/// The hub cancels backups only: a running check goes on until a session
/// cancels it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hub_cancels_no_maintenance_run() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(IdentifiesThenParks);
    let id = runner
        .start_maintenance("demo".into(), http_adapter::MaintenanceRequest::Check, None)
        .await
        .expect("the check starts")
        .operation;
    let operation = operation_of(&id).expect("the answer is an operation ID");
    assert_eq!(
        HubRuns::cancel(&runner, &operation).await,
        Err(HubCancelRefused::NotABackup)
    );
    Operations::cancel(&runner, id, http_adapter::Canceller::Session)
        .await
        .expect("a session cancels the check");
    assert_eq!(ended(&runner, operation).await, OperationStatus::Canceled);
}
