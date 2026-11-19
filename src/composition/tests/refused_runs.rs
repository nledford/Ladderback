use std::assert_matches;

use super::captured_log::{self, capture};
use std::sync::PoisonError;

use api::ErrorCode;
use app::fakes::{FakeSecrets, InMemoryConfigStore};
use domain::RepositoryId;

use super::service_support::*;
use super::*;

/// A backup started by hand whose repository's secret can't be read is
/// refused, and the daemon's log says so, as it does for a scheduled run
/// (#483): the plan, the refusal's code, the setting and where the
/// secret was looked for, a name and never a secret (I4). The engine is
/// never asked.
#[tokio::test]
async fn a_backup_started_by_hand_and_refused_for_its_secret_says_why_in_the_log() {
    capture();
    let variable = "LADDERBACK_TEST_PASSWORD_NEVER_SET";
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        Default::default(),
        std::collections::BTreeMap::from([(
            alias.clone(),
            domain::planning::RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    repository: "s3:s3.example.net/demo".to_owned(),
                    repo_hot: None,
                    password: SecretRef::Env(variable.to_owned()),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                },
                maintenance: domain::planning::Maintenance::None,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        )]),
        vec![domain::planning::Plan::new(
            domain::PlanId::parse("home").expect("a plan ID"),
            alias,
            vec![domain::planning::Source::parse("/home/demo").expect("a source")],
        )],
    )
    .expect("a valid configuration");
    let config_store = Arc::new(InMemoryConfigStore::new());
    app::ports::ConfigStore::save(&*config_store, &config)
        .await
        .expect("the config saves");
    let scripted = engine();
    let runner = PlanRunner {
        config: config_store,
        resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
        agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
        engine: scripted.clone(),
        locations: None,
        retention: Arc::new(engine::RusticRetention),
        store: Arc::new(app::fakes::InMemoryOperationStore::new()),
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(ProcessHookRunner::new()),
        events: Arc::new(BroadcastEventBus::new()),
        queue: Arc::new(RunQueue::new()),
        ends: app::retry::run_ends().0,
        runs: crate::composition::runs::Runs::default(),
        readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
        full_disk_access: None,
        left_out: KeyFilesLeftOut::default(),
        writes: Arc::default(),
    };

    let refused = runner
        .start_backup("home".to_owned(), Trigger::Manual)
        .await;

    assert_matches!(
        &refused,
        Err(StartError::SecretRefused(SecretRefusal {
            code: ErrorCode::SecretMissing,
            ..
        }))
    );
    assert_eq!(jobs(&scripted), 0);
    let said = captured_log::lines();
    let line = said
        .iter()
        .find(|(level, line)| {
            *level == log::Level::Warn
                && line.contains("the manual run of plan home didn't start")
                && line.contains(variable)
        })
        .map(|(_, line)| line)
        .unwrap_or_else(|| panic!("no warning names the plan and the variable: {said:?}"));
    assert!(line.contains("(secret-missing)"), "{line}");
    assert!(line.contains("password"), "{line}");
}

/// A scheduled run whose repository's password can't be read doesn't
/// start, and nobody is there to hear a refusal: it ends as one failed
/// operation that says why, never started, the engine never asked (§5).
/// The schedule hears it failed, so it is retried as any failed backup
/// the schedule started (decision 76), and its place in the queue is
/// given up. The log names the plan, the operation, the code, the setting
/// and where the secret was looked for, as a refusal by hand does (#483).
#[tokio::test]
async fn a_scheduled_run_refused_for_a_secret_ends_as_one_failed_operation_that_says_why() {
    capture();
    const UNSET: &str = "LADDERBACK_TEST_REFUSED_RUN_PASSWORD_UNSET";
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, plan) =
        refused_password_runner(UNSET, store.clone(), Arc::new(BroadcastEventBus::new())).await;

    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place).await;

    assert_eq!(jobs(&scripted), 0, "the engine was asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}");
    };
    assert_eq!(
        record.status,
        OperationStatus::Failed {
            reason: FailureReason::SecretRefused {
                field: "password".to_owned(),
                refusal: domain::execution::refusal::SettingRefusal::SecretMissing {
                    source: UNSET.to_owned(),
                },
            },
        }
    );
    assert_eq!(record.kind, OperationKind::Backup);
    assert_eq!(record.trigger, Trigger::Schedule);
    assert_eq!(record.target.plan.as_ref(), Some(&plan));
    assert_eq!(record.started_at, None, "a refused run never started");

    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended");
    assert_eq!(
        end,
        Some(app::retry::RunEnd {
            operation: record.id,
            plan: plan.clone(),
            trigger: Trigger::Schedule,
            outcome: app::retry::RunOutcome::Failed,
        })
    );

    let mut next = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    assert!(
        futures::FutureExt::now_or_never(next.turn()).is_some(),
        "the refused run gave up its place"
    );

    let operation = record.id.as_uuid().hyphenated().to_string();
    let said = captured_log::lines();
    let line = said
        .iter()
        .find(|(level, line)| *level == log::Level::Warn && line.contains(&operation))
        .map(|(_, line)| line)
        .unwrap_or_else(|| panic!("no warning names the operation: {said:?}"));
    assert!(
        line.contains("the scheduled run of plan home didn't start"),
        "{line}"
    );
    assert!(line.contains("(secret-missing)"), "{line}");
    // Where the secret was looked for is a name, never a secret (#483).
    assert!(
        line.contains(&format!("its password, from {UNSET}")),
        "{line}"
    );
}

/// A run the schedule starts while the daemon starts, before the
/// session's keyring service is up, waits for it and runs with the
/// password the keyring holds. The daemon says so once in its log,
/// naming the run, the plan and the keyring entry, never the password
/// (§5, I4).
#[tokio::test]
async fn a_scheduled_run_while_the_daemon_starts_waits_for_the_keyring_service_and_logs_it_once() {
    capture();
    let (runner, heard, scripted, plan) = keyring_runner().await;

    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place).await;

    tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended")
        .expect("a run ended");
    let seen = scripted.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let [Job::Backup(backup)] = seen.as_slice() else {
        panic!("the run's backup reached the engine: {seen:?}");
    };
    assert_eq!(backup.repository.password.expose(), KEYRING_PASSWORD);
    let waits: Vec<String> = captured_log::infos()
        .into_iter()
        .filter(|line| line.contains("keyring service"))
        .collect();
    let [line] = waits.as_slice() else {
        panic!("the wait is logged once: {waits:?}");
    };
    assert!(line.contains("the scheduled run of plan home"), "{line}");
    assert!(line.contains("ladderback/demo"), "{line}");
    assert!(!line.contains(KEYRING_PASSWORD), "{line}");
}

/// A run a person starts while the daemon starts is answered at once
/// when the session's keyring service isn't up yet: it never waits, and
/// the log says nothing of a wait.
#[tokio::test]
async fn a_run_by_hand_while_the_daemon_starts_never_waits_for_the_keyring_service() {
    capture();
    let (runner, _heard, scripted, plan) = keyring_runner().await;

    let refused = Operations::start_backup(&runner, plan.as_str().to_owned(), Trigger::Manual)
        .await
        .expect_err("a run by hand with no keyring service to ask is refused");

    assert_matches!(
        refused,
        StartError::SecretRefused(SecretRefusal {
            code: ErrorCode::SecretKeyringUnavailable,
            ..
        })
    );
    assert_eq!(jobs(&scripted), 0);
    let waits: Vec<String> = captured_log::infos()
        .into_iter()
        .filter(|line| line.contains("keyring service"))
        .collect();
    assert_eq!(waits, Vec::<String>::new());
}

/// The password the keyring holds for the repository `demo`.
const KEYRING_PASSWORD: &str = "canary-keyring-password";

/// A runner for the plan `home` on the repository `demo`, as the daemon
/// builds it, a moment after it started: the repository's password is
/// in a keyring whose session's service starts 25 s after the daemon,
/// on a clock that jumps to the end of each wait.
async fn keyring_runner() -> (
    PlanRunner,
    app::retry::RunEnds,
    Arc<Scripted>,
    domain::PlanId,
) {
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(app::fakes::FakeClock::auto_advancing(started));
    let secrets = Arc::new(FakeSecrets::new());
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.start_keyring_service_at(
        clock.clone(),
        started + domain::SignedDuration::from_secs(25),
    );
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Keyring(entry),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let (mut runner, heard, scripted, plan) = refused_runner(
        settings,
        Arc::new(app::fakes::InMemoryOperationStore::new()),
        Arc::new(BroadcastEventBus::new()),
    )
    .await;
    // The fake clock's monotonic time is zero at `started`.
    runner.resolve = Arc::new(daemon_resolve(secrets, clock, domain::SignedDuration::ZERO));
    (runner, heard, scripted, plan)
}

/// A runner for the plan `home` on the repository `demo`, whose password
/// is the environment variable `unset`, which nothing sets.
async fn refused_password_runner(
    unset: &str,
    store: Arc<dyn OperationStore>,
    events: Arc<BroadcastEventBus>,
) -> (
    PlanRunner,
    app::retry::RunEnds,
    Arc<Scripted>,
    domain::PlanId,
) {
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Env(unset.to_owned()),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    refused_runner(settings, store, events).await
}

/// A scheduled run whose password command fails with the other secrets
/// of the repository in its stderr keeps the command's words in its
/// history row's failure masked as the refusal of a run by hand masks
/// them, and no secret reaches the row (I4).
#[tokio::test]
async fn a_refused_runs_history_row_says_what_the_command_said_masked_as_its_refusal() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let referenced = scratch.path().join("token");
    std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Command(
            domain::secrets::CommandLine::parse(
                "sh -c 'echo canary-inline canary-from-a-file >&2; exit 3'",
            )
            .expect("a command"),
        ),
        options: [
            ("secret_access_key".to_owned(), "canary-inline".to_owned()),
            (
                "application_key".to_owned(),
                format!("${{file:{}}}", referenced.display()),
            ),
        ]
        .into(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, _, plan) =
        refused_runner(settings, store, Arc::new(BroadcastEventBus::new())).await;

    let by_hand = Operations::start_backup(&runner, plan.as_str().to_owned(), Trigger::Manual)
        .await
        .expect_err("a run by hand is refused");
    let StartError::SecretRefused(alert) = by_hand else {
        panic!("a secret refusal, not {by_hand:?}");
    };
    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place).await;

    let page = Operations::history(&runner, None, None, None)
        .await
        .expect("the history reads");
    let [row] = page.operations.as_slice() else {
        panic!("one operation is recorded: {page:?}");
    };
    let failure = row.failure.as_ref().expect("the row says why it failed");
    assert_eq!(failure.code, ErrorCode::SecretCommandFailed);
    let detail = failure
        .detail
        .as_deref()
        .expect("the command's words are kept");
    assert_eq!(Some(detail), alert.detail.as_deref());
    assert!(detail.contains(domain::secrets::REDACTED), "{detail}");
    let wire = serde_json::to_string(row).expect("the row serializes");
    assert!(!wire.contains("canary-inline"), "{wire}");
    assert!(!wire.contains("canary-from-a-file"), "{wire}");
}

/// A runner for the plan `home` on the repository `demo` of `settings`.
async fn refused_runner(
    settings: domain::planning::RepositorySettings,
    store: Arc<dyn OperationStore>,
    events: Arc<BroadcastEventBus>,
) -> (
    PlanRunner,
    app::retry::RunEnds,
    Arc<Scripted>,
    domain::PlanId,
) {
    let plan = domain::PlanId::parse("home").expect("a plan ID");
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        Default::default(),
        std::collections::BTreeMap::from([(
            alias.clone(),
            domain::planning::RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings,
                maintenance: domain::planning::Maintenance::Local,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        )]),
        vec![domain::planning::Plan::new(
            plan.clone(),
            alias,
            vec![domain::planning::Source::parse("/home/demo").expect("a source")],
        )],
    )
    .expect("a valid configuration");
    let config_store = Arc::new(InMemoryConfigStore::new());
    app::ports::ConfigStore::save(&*config_store, &config)
        .await
        .expect("the config saves");
    let scripted = engine();
    let (ends, heard) = app::retry::run_ends();
    let runner = PlanRunner {
        config: config_store,
        resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
        agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
        engine: scripted.clone(),
        locations: None,
        retention: Arc::new(engine::RusticRetention),
        store,
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(ProcessHookRunner::new()),
        events,
        queue: Arc::new(RunQueue::new()),
        ends,
        runs: crate::composition::runs::Runs::default(),
        readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
        full_disk_access: None,
        left_out: KeyFilesLeftOut::default(),
        writes: Arc::default(),
    };
    (runner, heard, scripted, plan)
}

/// A retry the schedule started that is refused for its secret ends as a
/// failed retry: the schedule hears it with its own trigger, so the
/// retry's count goes on rather than starting over (decision 76).
#[tokio::test]
async fn a_refused_retry_is_heard_as_a_failed_retry() {
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, _, plan) = refused_password_runner(
        "LADDERBACK_TEST_REFUSED_RETRY_PASSWORD_UNSET",
        store.clone(),
        Arc::new(BroadcastEventBus::new()),
    )
    .await;

    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Retry, place).await;

    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}");
    };
    assert_eq!(record.trigger, Trigger::Retry);
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the retry ended");
    assert_eq!(
        end,
        Some(app::retry::RunEnd {
            operation: record.id,
            plan,
            trigger: Trigger::Retry,
            outcome: app::retry::RunOutcome::Failed,
        })
    );
}

/// A refused run whose write the operation log refuses still ends: the
/// schedule hears it failed and a page is told, and the daemon's log says
/// the write failed by its code, never in the store's own words (I4).
#[tokio::test]
async fn a_refused_run_the_log_cannot_keep_still_ends_and_is_announced() {
    capture();
    let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let store = Arc::new(super::support::RefusingSaves {
        inner: Arc::new(app::fakes::InMemoryOperationStore::new()),
        refused: refused.clone(),
    });
    let events = Arc::new(BroadcastEventBus::new());
    let mut page = events.subscribe().await;
    let (runner, heard, scripted, plan) = refused_password_runner(
        "LADDERBACK_TEST_REFUSED_UNKEPT_PASSWORD_UNSET",
        store,
        events,
    )
    .await;

    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place).await;

    assert_eq!(jobs(&scripted), 0, "the engine was asked");
    assert_eq!(refused.load(std::sync::atomic::Ordering::SeqCst), 1);
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended")
        .expect("an end");
    assert_eq!(end.outcome, app::retry::RunOutcome::Failed);
    let told = futures::FutureExt::now_or_never(page.next()).flatten();
    assert!(
        matches!(
            told,
            Some(UiEvent::OperationFailed {
                code: ErrorCode::SecretMissing,
                ..
            })
        ),
        "{told:?}"
    );
    let operation = end.operation.as_uuid().hyphenated().to_string();
    let said = captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains(&operation)
            && line.contains("a write to the operation log failed")),
        "{said:?}"
    );
    assert!(
        said.iter()
            .all(|(_, line)| !line.contains(super::support::REFUSAL)),
        "{said:?}"
    );
}

/// A backup of the plan `home` on `repository`, as the log holds it
/// queued under `id` when the daemon starts again.
fn queued_home_backup(id: u128, repository: Option<RepositoryId>) -> domain::execution::Operation {
    domain::execution::Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(id)),
        OperationKind::Backup,
        Trigger::Schedule,
        Target {
            plan: Some(domain::PlanId::parse("home").expect("a plan ID")),
            repository,
        },
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    )
}

/// A backup left queued when the daemon stopped, whose repository's
/// password can't be read when the next start takes it up again, ends
/// failed with the refusal, not `daemon-stopped` (#490): under its own
/// ID, never started, the engine never asked. The schedule hears it
/// failed, so its retries follow it as they follow a refused scheduled
/// run (decision 76), a page is told, and the daemon's log names the
/// operation and the refusal's code.
#[tokio::test]
async fn a_resumed_backup_refused_for_its_secret_ends_with_the_refusal() {
    capture();
    const UNSET: &str = "LADDERBACK_TEST_RESUMED_PASSWORD_UNSET";
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let events = Arc::new(BroadcastEventBus::new());
    let mut page = events.subscribe().await;
    let (runner, heard, scripted, plan) =
        refused_password_runner(UNSET, store.clone(), events).await;
    let queued = queued_home_backup(0x0490, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    PlanRuns::resume(&runner, queued.clone()).await;

    assert_eq!(jobs(&scripted), 0, "the engine was asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}");
    };
    assert_eq!(record.id, queued.id());
    assert_eq!(
        record.status,
        OperationStatus::Failed {
            reason: FailureReason::SecretRefused {
                field: "password".to_owned(),
                refusal: domain::execution::refusal::SettingRefusal::SecretMissing {
                    source: UNSET.to_owned(),
                },
            },
        }
    );
    assert_eq!(record.started_at, None, "a refused run never started");
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended");
    assert_eq!(
        end,
        Some(app::retry::RunEnd {
            operation: queued.id(),
            plan,
            trigger: Trigger::Schedule,
            outcome: app::retry::RunOutcome::Failed,
        })
    );
    let told = futures::FutureExt::now_or_never(page.next()).flatten();
    assert!(
        matches!(
            told,
            Some(UiEvent::OperationFailed {
                code: ErrorCode::SecretMissing,
                ..
            })
        ),
        "{told:?}"
    );
    let operation = queued.id().as_uuid().hyphenated().to_string();
    let said = captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains(&operation)
            && line.contains("(secret-missing)")),
        "no warning names the operation and the code: {said:?}"
    );
}

/// The history row of such a backup keeps what a failing password
/// command said masked of the repository's secrets, as a refused
/// scheduled run's row does, and no secret reaches it (I4).
#[tokio::test]
async fn a_resumed_backups_refusal_is_kept_masked_of_the_repositorys_secrets() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let referenced = scratch.path().join("token");
    std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Command(
            domain::secrets::CommandLine::parse(
                "sh -c 'echo canary-inline canary-from-a-file >&2; exit 3'",
            )
            .expect("a command"),
        ),
        options: [
            ("secret_access_key".to_owned(), "canary-inline".to_owned()),
            (
                "application_key".to_owned(),
                format!("${{file:{}}}", referenced.display()),
            ),
        ]
        .into(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, _, _) =
        refused_runner(settings, store.clone(), Arc::new(BroadcastEventBus::new())).await;
    let queued = queued_home_backup(0x0491, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    PlanRuns::resume(&runner, queued).await;

    let page = Operations::history(&runner, None, None, None)
        .await
        .expect("the history reads");
    let [row] = page.operations.as_slice() else {
        panic!("one operation is recorded: {page:?}");
    };
    let failure = row.failure.as_ref().expect("the row says why it failed");
    assert_eq!(failure.code, ErrorCode::SecretCommandFailed);
    let detail = failure
        .detail
        .as_deref()
        .expect("the command's words are kept");
    assert!(detail.contains(domain::secrets::REDACTED), "{detail}");
    let wire = serde_json::to_string(row).expect("the row serializes");
    assert!(!wire.contains("canary-inline"), "{wire}");
    assert!(!wire.contains("canary-from-a-file"), "{wire}");
}

/// A backup carries from its start the warnings reading its repository's
/// secrets met (#386): one the schedule starts now and one the queue held
/// from before the start alike, each recorded with a password file other
/// users can read (#539).
#[cfg(unix)]
#[tokio::test]
async fn started_and_resumed_backups_carry_the_warnings_their_secrets_gave() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = tempfile::tempdir().expect("a scratch folder");
    let password = scratch.path().join("password");
    std::fs::write(&password, "demo-readable-password\n").expect("the password file");
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o644))
        .expect("the file's mode");
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::File {
            path: password
                .to_str()
                .expect("a scratch path is text")
                .to_owned(),
            read: domain::secrets::FileRead::FirstLine,
        },
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, _, plan) =
        refused_runner(settings, store.clone(), Arc::new(BroadcastEventBus::new())).await;
    let next_end = || async {
        tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
            .await
            .expect("the run ends")
            .expect("an end")
    };

    let place = runner
        .queue
        .push(&plan, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place).await;
    let started = next_end().await.operation;
    let queued = queued_home_backup(0x0539, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    PlanRuns::resume(&runner, queued.clone()).await;
    let resumed = next_end().await.operation;
    assert_eq!(resumed, queued.id());

    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    for (run, id) in [("started", started), ("resumed", resumed)] {
        let record = history
            .operations
            .iter()
            .find(|record| record.id == id)
            .unwrap_or_else(|| panic!("the {run} backup is recorded: {history:?}"));
        assert_eq!(
            record.warning_kinds,
            [WarningKind::SecretFileReadable],
            "the {run} backup: {record:?}"
        );
    }
}

/// A refused backup whose plan now backs up to another repository than
/// the one it was queued for ends `daemon-stopped`, as any backup whose
/// plan moved does (decision 73): the refusal is of a repository it was
/// never queued for.
#[tokio::test]
async fn a_resumed_backup_whose_plan_moved_ends_as_daemon_stopped_though_its_secret_is_refused() {
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, _, _) = refused_password_runner(
        "LADDERBACK_TEST_MOVED_PASSWORD_UNSET",
        store.clone(),
        Arc::new(BroadcastEventBus::new()),
    )
    .await;
    let queued = queued_home_backup(
        0x0492,
        Some(RepositoryId::parse(&"b".repeat(64)).expect("a repository ID")),
    );
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    PlanRuns::resume(&runner, queued.clone()).await;

    let record = store
        .get(queued.id())
        .await
        .expect("the log reads")
        .expect("the backup is in the log")
        .record;
    assert_eq!(
        record.status,
        OperationStatus::Failed {
            reason: FailureReason::DaemonStopped
        }
    );
}
