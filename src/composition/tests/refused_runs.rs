use std::assert_matches;

use super::captured_log::{self, capture};
use std::sync::PoisonError;

use api::ErrorCode;
use app::fakes::{FakeClock, FakeSecrets, InMemoryConfigStore};
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
        machine: Arc::default(),
        leases: None,
        running: Arc::default(),
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
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;

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

/// A start by hand of a plan that has a run waiting is refused before its
/// job is built (§16, decision 75, point 4; #1140): the queue answers
/// first, so a flood of starts reads no secret, runs no password command
/// and records nothing, and the refusal is the queue's, whatever the
/// secret would have said. The answer says why; the log doesn't, so a
/// client that retries in a loop can't fill it, as a cancel of an operation
/// that isn't running leaves no line either.
#[tokio::test]
async fn a_start_by_hand_while_its_plan_has_a_run_waiting_is_refused_before_its_secret_is_read() {
    capture();
    const UNSET: &str = "LADDERBACK_TEST_WAITING_RUN_PASSWORD_UNSET";
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, plan) =
        refused_password_runner(UNSET, store.clone(), Arc::new(BroadcastEventBus::new())).await;
    let limit = domain::planning::Concurrency::default();
    let _running = runner.queue.push(&plan, limit);
    let _waiting = runner.queue.push(&plan, limit);

    let refused =
        Operations::start_backup(&runner, plan.as_str().to_owned(), Trigger::Manual).await;

    assert_matches!(refused, Err(StartError::RunWaiting));
    assert_eq!(jobs(&scripted), 0, "the engine was asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    assert!(history.operations.is_empty(), "{history:?}");
    let said: Vec<String> = captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .chain(captured_log::infos())
        .collect();
    assert!(
        !said.iter().any(|line| line.contains("(plan-run-waiting)")),
        "the refusal was logged: {said:?}"
    );
    assert!(
        !said.iter().any(|line| line.contains(UNSET)),
        "the secret was looked for: {said:?}"
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
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;

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

/// A run that waits for the keyring to be unlocked doesn't hold the
/// scheduler with it: another plan's run starts and reaches the engine
/// while the first waits, no run is refused while the keyring is locked,
/// and once the keyring is unlocked the waiting run backs up with the
/// password the keyring holds (#1221).
#[tokio::test]
async fn another_plan_runs_while_a_run_waits_for_the_keyring_to_be_unlocked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    // A clock that moves only when told, so the run is still waiting when
    // the other plan's run starts.
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.lock_keyring_until(
        clock.clone(),
        started + domain::SignedDuration::from_secs(60),
    );
    let demo = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let plain = domain::RepositoryAlias::parse("plain").expect("a repository alias");
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        Default::default(),
        std::collections::BTreeMap::from([
            (
                demo.clone(),
                domain::planning::RepositoryConfig {
                    locations: Default::default(),
                    id: None,
                    settings: RepositorySettings {
                        repository: "/srv/demo-repo".to_owned(),
                        repo_hot: None,
                        password: SecretRef::Keyring(entry),
                        options: Default::default(),
                        options_hot: Default::default(),
                        options_cold: Default::default(),
                    },
                    maintenance: domain::planning::Maintenance::Local,
                    group_by: Default::default(),
                    hooks: Default::default(),
                },
            ),
            (
                plain.clone(),
                domain::planning::RepositoryConfig {
                    locations: Default::default(),
                    id: None,
                    settings: RepositorySettings {
                        repository: "/srv/plain-repo".to_owned(),
                        repo_hot: None,
                        password: SecretRef::Inline(domain::secrets::SecretValue::new(
                            "plain-password",
                        )),
                        options: Default::default(),
                        options_hot: Default::default(),
                        options_cold: Default::default(),
                    },
                    maintenance: domain::planning::Maintenance::Local,
                    group_by: Default::default(),
                    hooks: Default::default(),
                },
            ),
        ]),
        vec![
            domain::planning::Plan::new(
                domain::PlanId::parse("home").expect("a plan ID"),
                demo,
                vec![domain::planning::Source::parse("/home/demo").expect("a source")],
            ),
            domain::planning::Plan::new(
                domain::PlanId::parse("other").expect("a plan ID"),
                plain,
                vec![domain::planning::Source::parse("/home/other").expect("a source")],
            ),
        ],
    )
    .expect("a valid configuration");
    let config_store = Arc::new(InMemoryConfigStore::new());
    app::ports::ConfigStore::save(&*config_store, &config)
        .await
        .expect("the config saves");
    let scripted = engine();
    let (ends, heard) = app::retry::run_ends();
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let runner = PlanRunner {
        config: config_store,
        resolve: Arc::new(daemon_resolve(
            secrets,
            clock.clone(),
            domain::SignedDuration::ZERO,
        )),
        agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
        engine: scripted.clone(),
        locations: None,
        retention: Arc::new(engine::RusticRetention),
        store: store.clone(),
        registry: Arc::new(CancelRegistry::new()),
        hooks: Arc::new(ProcessHookRunner::new()),
        events: Arc::new(BroadcastEventBus::new()),
        queue: Arc::new(RunQueue::new()),
        ends,
        runs: crate::composition::runs::Runs::default(),
        readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
        full_disk_access: None,
        machine: Arc::default(),
        leases: None,
        running: Arc::default(),
        left_out: KeyFilesLeftOut::default(),
        writes: Arc::default(),
    };
    let home = domain::PlanId::parse("home").expect("a plan ID");
    let other = domain::PlanId::parse("other").expect("a plan ID");

    let waiting = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    // The plan's next run is due 10 minutes in: the waiting run never
    // waits past it (#1221).
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, waiting, Some(unlock)).await;

    // While the first run waits for the keyring, the other plan's run
    // starts, reaches the engine and ends, and nothing is refused.
    let place = runner
        .queue
        .push(&other, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &other, Trigger::Schedule, place, None).await;
    let other_end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the other run ends")
        .expect("an end");
    assert_eq!(other_end.plan, other, "{other_end:?}");
    assert_eq!(jobs(&scripted), 1, "only the plan that isn't waiting ran");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let refused: Vec<_> = history
        .operations
        .iter()
        .filter(|row| {
            matches!(
                row.status,
                domain::execution::OperationStatus::Failed {
                    reason: domain::execution::FailureReason::SecretRefused { .. },
                }
            )
        })
        .collect();
    assert!(
        refused.is_empty(),
        "no run is refused for its secret while the keyring is locked: {history:?}"
    );

    // The keyring unlocks, and the waiting run backs up.
    clock.advance(domain::SignedDuration::from_secs(60));
    let home_end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the waiting run ends")
        .expect("an end");
    assert_eq!(home_end.plan, home, "{home_end:?}");
    let seen = scripted.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let passwords: Vec<&str> = seen
        .iter()
        .filter_map(|job| match job {
            Job::Backup(backup) => Some(backup.repository.password.expose()),
            _ => None,
        })
        .collect();
    assert!(
        passwords.contains(&KEYRING_PASSWORD),
        "the waiting run backed up with the keyring's password: {passwords:?}"
    );
}

/// The password the keyring holds for the repository `demo`.
const KEYRING_PASSWORD: &str = "canary-keyring-password";

/// A runner for the plan `home` on the repository `demo`, its password in
/// a keyring `secrets` answers, on a clock that moves only when told, so
/// a run of it can be held waiting for the keyring to be unlocked
/// (#1221).
async fn keyring_parked_runner(
    secrets: Arc<FakeSecrets>,
    clock: Arc<FakeClock>,
    store: Arc<app::fakes::InMemoryOperationStore>,
) -> (
    PlanRunner,
    app::retry::RunEnds,
    Arc<Scripted>,
    domain::PlanId,
) {
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    let settings = domain::planning::RepositorySettings {
        repository: "/srv/demo-repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Keyring(entry),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let (mut runner, heard, scripted, plan) =
        refused_runner(settings, store, Arc::new(BroadcastEventBus::new())).await;
    runner.resolve = Arc::new(daemon_resolve(secrets, clock, domain::SignedDuration::ZERO));
    (runner, heard, scripted, plan)
}

/// A locked keyring over the entry the repository `demo`'s password sits
/// in, until `clock` reaches `unlock_at`.
fn keyring_locked_until(
    secrets: &Arc<FakeSecrets>,
    clock: &Arc<FakeClock>,
    unlock_at: domain::Timestamp,
) {
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.lock_keyring_until(clock.clone(), unlock_at);
}

/// A queued backup the daemon takes up again whose repository password is
/// in a locked keyring waits for the keyring to be unlocked — its
/// operation shows as queued while it waits — and runs once it is, never
/// past the next run its plan's planner kept, which is its bound
/// (#1221, decision 135).
#[tokio::test]
async fn a_resumed_run_waits_for_the_keyring_to_be_unlocked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1221, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        PlanRuns::resume(&runner, queued.clone(), Some(unlock)),
    )
    .await
    .expect("resume returns without waiting for the keyring");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("the queued backup is kept while its run waits: {history:?}")
    };
    assert_eq!(
        record.status,
        domain::execution::OperationStatus::Queued,
        "the operation shows as queued while its run waits: {history:?}"
    );

    clock.advance(domain::SignedDuration::from_secs(60));
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the resumed run ends")
        .expect("an end");
    assert_eq!(end.plan, home, "{end:?}");
    assert_eq!(jobs(&scripted), 1, "the resumed run reached the engine");
    let seen = scripted.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let passwords: Vec<&str> = seen
        .iter()
        .filter_map(|job| match job {
            Job::Backup(backup) => Some(backup.repository.password.expose()),
            _ => None,
        })
        .collect();
    assert!(
        passwords.contains(&KEYRING_PASSWORD),
        "the resumed run backed up with the keyring's password: {passwords:?}"
    );
}

/// A queued backup of a plan whose planner kept no next run — the plan
/// left the schedule while it sat queued — is refused at once when its
/// keyring is locked: there is no run to wait within (#1221, decision
/// 135).
#[tokio::test]
async fn a_resumed_run_of_a_plan_with_no_next_run_is_refused_at_once() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, home) =
        keyring_parked_runner(secrets, clock, store.clone()).await;
    let queued = queued_home_backup(0x1222, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");

    PlanRuns::resume(&runner, queued.clone(), None).await;
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended")
        .expect("an end");
    assert_eq!(end.plan, home, "{end:?}");
    assert_eq!(end.outcome, app::retry::RunOutcome::Failed, "{end:?}");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}")
    };
    assert!(
        matches!(
            &record.status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused {
                    refusal: domain::execution::refusal::SettingRefusal::SecretKeyringLocked { .. },
                    ..
                },
            }
        ),
        "refused as locked, at once: {record:?}"
    );
}

/// A run parked waiting for its keyring gives way to a newer run of its
/// plan that is going when the keyring is unlocked: one backup runs, not
/// two behind each other (#1221, decision 135).
#[tokio::test]
async fn a_waiting_run_gives_way_to_a_newer_run_of_its_plan() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(55),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store).await;

    let waiting = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, waiting, Some(unlock)).await;

    // The run waits, asking every 10 s while the keyring is locked, until
    // it sleeps from 50 s to 60 s.
    for _ in 0..5 {
        until_it_waits(&clock).await;
        clock.advance(domain::SignedDuration::from_secs(10));
    }
    until_it_waits(&clock).await;

    // The keyring unlocks at 55 s, and a newer run of the plan starts and
    // reaches the engine; its job never answers, so it is going.
    clock.advance(domain::SignedDuration::from_secs(5));
    *scripted
        .hangs
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = 1;
    let newer = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &home, Trigger::Schedule, newer, None).await;

    // The waiting run asks at its next tick, finds the keyring unlocked,
    // and gives way to the newer run.
    clock.advance(domain::SignedDuration::from_secs(5));
    until_logged(&[
        "the scheduled run of plan home didn't start",
        "plan-run-waiting",
    ])
    .await;
    assert_eq!(jobs(&scripted), 1, "only the newer run reached the engine");
}

/// A queued backup a restart resumed, parked waiting for its keyring, gives
/// way to a newer run of its plan that is going when the keyring is
/// unlocked, and ends failed as superseded: it never ran, and the daemon
/// didn't stop (#1245, decision 135).
#[tokio::test]
async fn a_resumed_run_that_gives_way_to_a_newer_run_ends_superseded() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(55),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1245, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    for _ in 0..5 {
        until_it_waits(&clock).await;
        clock.advance(domain::SignedDuration::from_secs(10));
    }
    until_it_waits(&clock).await;

    // The keyring unlocks at 55 s, and a newer run of the plan starts and
    // reaches the engine; its job never answers, so it is going.
    clock.advance(domain::SignedDuration::from_secs(5));
    *scripted
        .hangs
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = 1;
    let newer = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &home, Trigger::Schedule, newer, None).await;

    clock.advance(domain::SignedDuration::from_secs(5));
    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::Superseded,
            }
        ),
        "the queued backup ends superseded: {status:?}"
    );
    assert_eq!(jobs(&scripted), 1, "only the newer run reached the engine");
}

/// A queued backup recorded on a repository its plan no longer backs up to
/// isn't parked to wait for the keyring of the one it backs up to now: it
/// ends at once, as it would with the keyring unlocked (#1221).
#[tokio::test]
async fn a_resumed_run_whose_plan_moved_is_not_parked_for_the_keyring() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1225, Some(repository_id()));
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued, Some(unlock)).await;

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let history = store
                .list(&app::ports::HistoryQuery::default())
                .await
                .expect("the log reads");
            if history
                .operations
                .iter()
                .all(|record| record.status.is_finished())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the queued backup ends at once, never waiting for the keyring");
    assert_eq!(clock.sleepers(), 0, "nothing waits for the keyring");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// Waits until a run parked on `clock` sleeps there: time moved before it
/// does would skip its wake.
async fn until_it_waits(clock: &FakeClock) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while clock.sleepers() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the run waits for the keyring");
}

/// Waits until the daemon's log has a line that holds every one of `words`.
async fn until_logged(words: &[&str]) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let said = captured_log::lines();
            if said
                .iter()
                .any(|(_, line)| words.iter().all(|word| line.contains(word)))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the daemon logs a line with {words:?}"));
}

/// Waits until the operation log holds `id` with a status that is final,
/// and returns it: the daemon's log can say a run ended before the log
/// has its end.
async fn until_ended(
    store: &app::fakes::InMemoryOperationStore,
    id: OperationId,
) -> domain::execution::OperationStatus {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let history = store
                .list(&app::ports::HistoryQuery::default())
                .await
                .expect("the log reads");
            if let Some(record) = history
                .operations
                .iter()
                .find(|record| record.id == id && record.status.is_finished())
            {
                break record.status.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the operation log ends operation {id:?}"))
}

/// The plan `home` of `runner`'s configuration, rewritten by `change`, is
/// saved in its place.
async fn change_home(runner: &PlanRunner, change: impl FnOnce(&mut domain::planning::Plan)) {
    let home = domain::PlanId::parse("home").expect("a plan ID");
    let config = runner.config.load().await.expect("the configuration reads");
    let mut plan = config.plan(&home).expect("the plan").clone();
    change(&mut plan);
    let changed = config.change_plan(plan).expect("a valid change");
    app::ports::ConfigStore::save(&*runner.config, &changed)
        .await
        .expect("the configuration saves");
}

/// A run that waited for its keyring backs up its plan as the
/// configuration stands once the keyring is unlocked, not as it stood
/// when the run began to wait: the wait can last hours (#1221, decision
/// 135).
#[tokio::test]
async fn a_run_that_waited_backs_up_its_plan_as_it_stands_once_the_keyring_is_unlocked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;
    until_it_waits(&clock).await;

    change_home(&runner, |plan| {
        plan.sources =
            vec![domain::planning::Source::parse("/home/demo/photos").expect("a source")];
    })
    .await;
    clock.advance(domain::SignedDuration::from_secs(60));
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the run ends")
        .expect("an end");
    assert_eq!(end.plan, home, "{end:?}");
    let seen = scripted.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let sources: Vec<&[String]> = seen
        .iter()
        .filter_map(|job| match job {
            Job::Backup(backup) => Some(backup.sources.as_slice()),
            _ => None,
        })
        .collect();
    assert_eq!(
        sources,
        [["/home/demo/photos".to_owned()].as_slice()],
        "the run backed up the plan's sources as they stand now"
    );
}

/// A run that waited for its keyring whose plan was deleted while it
/// waited doesn't start once the keyring is unlocked (#1221).
#[tokio::test]
async fn a_run_whose_plan_was_deleted_while_it_waited_does_not_start() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;
    until_it_waits(&clock).await;

    let config = runner.config.load().await.expect("the configuration reads");
    let without = config.remove_plan(&home).expect("a valid change");
    app::ports::ConfigStore::save(&*runner.config, &without)
        .await
        .expect("the configuration saves");
    clock.advance(domain::SignedDuration::from_secs(60));
    until_logged(&["home", "didn't start", "isn't configured"]).await;
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// The keyring entry of the repository `other`, where [`move_home_to_other`]
/// moves the plan `home`.
fn other_entry() -> domain::secrets::KeyringEntry {
    domain::secrets::KeyringEntry::parse("ladderback/other").expect("an entry")
}

/// The ID of the repository `other`, once the daemon has opened it.
fn other_id() -> RepositoryId {
    RepositoryId::parse(&"b".repeat(64)).expect("a repository ID")
}

/// `config` with the repository `demo` rewritten by `change`.
fn with_demo(
    config: &domain::planning::AgentConfig,
    change: impl FnOnce(&mut domain::planning::RepositoryConfig),
) -> domain::planning::AgentConfig {
    let demo = domain::RepositoryAlias::parse("demo").expect("an alias");
    let mut repositories = config.repositories().clone();
    change(repositories.get_mut(&demo).expect("the repository"));
    let home = PlanId::parse("home").expect("a plan id");
    let plan = config.plan(&home).expect("the plan").clone();
    config
        .replacing(
            config.agent().clone(),
            config.hooks().clone(),
            repositories,
            vec![plan],
        )
        .expect("a valid configuration")
}

/// `config` saved in place of the runner's.
async fn save_config(runner: &PlanRunner, config: &domain::planning::AgentConfig) {
    app::ports::ConfigStore::save(&*runner.config, config)
        .await
        .expect("the configuration saves");
}

/// The repository `demo` as the daemon found it once it opened it: it
/// records its ID.
fn demo_opened(entry: &mut domain::planning::RepositoryConfig) {
    entry.id = Some(repository_id());
}

/// The repository `demo` as the person rewrote it to name the repository
/// `other`, under the same alias: another ID, another password.
fn demo_now_other(entry: &mut domain::planning::RepositoryConfig) {
    entry.id = Some(other_id());
    entry.settings.repository = "/srv/other".to_owned();
    entry.settings.password = SecretRef::Keyring(other_entry());
}

/// A configuration store that answers its first load with `first` and
/// every later one with `later`: the person changed the configuration
/// between a run's first look and its next.
struct ChangesAfterFirstLoad {
    first: domain::planning::AgentConfig,
    later: domain::planning::AgentConfig,
    loads: std::sync::atomic::AtomicUsize,
}

impl ChangesAfterFirstLoad {
    fn new(first: domain::planning::AgentConfig, later: domain::planning::AgentConfig) -> Self {
        Self {
            first,
            later,
            loads: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl app::ports::ConfigStore for ChangesAfterFirstLoad {
    async fn load(&self) -> Result<domain::planning::AgentConfig, app::ports::ConfigError> {
        let loads = self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if loads == 0 {
            self.first.clone()
        } else {
            self.later.clone()
        })
    }

    async fn save(
        &self,
        _config: &domain::planning::AgentConfig,
    ) -> Result<(), app::ports::ConfigError> {
        Ok(())
    }

    async fn record_id(
        &self,
        _alias: &domain::RepositoryAlias,
        _read: &domain::planning::RepositoryConfig,
        _id: &RepositoryId,
    ) -> Result<(), app::ports::RecordError> {
        Ok(())
    }
}

/// A configuration store that reads `config` the first `readable` times
/// it is loaded, and then can't be read.
struct UnreadableAfter {
    config: domain::planning::AgentConfig,
    readable: usize,
    loads: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl app::ports::ConfigStore for UnreadableAfter {
    async fn load(&self) -> Result<domain::planning::AgentConfig, app::ports::ConfigError> {
        let loads = self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if loads < self.readable {
            Ok(self.config.clone())
        } else {
            Err(app::ports::ConfigError::Unavailable {
                file: "config.toml".to_owned(),
                detail: "disk I/O error".to_owned(),
            })
        }
    }

    async fn save(
        &self,
        _config: &domain::planning::AgentConfig,
    ) -> Result<(), app::ports::ConfigError> {
        Ok(())
    }

    async fn record_id(
        &self,
        _alias: &domain::RepositoryAlias,
        _read: &domain::planning::RepositoryConfig,
        _id: &RepositoryId,
    ) -> Result<(), app::ports::RecordError> {
        Ok(())
    }
}

/// `config` with the plan `home` moved to the repository `other`, whose
/// password is in the keyring under [`other_entry`].
fn home_on_other(config: &domain::planning::AgentConfig) -> domain::planning::AgentConfig {
    let other = domain::RepositoryAlias::parse("other").expect("an alias");
    let mut repositories = config.repositories().clone();
    repositories.insert(
        other.clone(),
        domain::planning::RepositoryConfig {
            locations: Default::default(),
            id: None,
            settings: domain::planning::RepositorySettings {
                repository: "/srv/other".to_owned(),
                repo_hot: None,
                password: SecretRef::Keyring(other_entry()),
                options: Default::default(),
                options_hot: Default::default(),
                options_cold: Default::default(),
            },
            maintenance: domain::planning::Maintenance::Local,
            group_by: Default::default(),
            hooks: Default::default(),
        },
    );
    let home = PlanId::parse("home").expect("a plan id");
    let mut plan = config.plan(&home).expect("the plan").clone();
    plan.repository = other;
    config
        .replacing(
            config.agent().clone(),
            config.hooks().clone(),
            repositories,
            vec![plan],
        )
        .expect("a valid configuration")
}

/// The person adds the repository `other`, whose password is in the
/// keyring under [`other_entry`], and moves the plan `home` there.
async fn move_home_to_other(runner: &PlanRunner, secrets: &FakeSecrets) {
    secrets.set_keyring_entry(&other_entry(), "canary-other");
    let config = runner.config.load().await.expect("the configuration reads");
    save_config(runner, &home_on_other(&config)).await;
}

/// A scheduled run that waited for its keyring, and whose plan moved to
/// another repository while it waited, doesn't start: the engine is never
/// asked, and the new repository's password isn't read for a run that
/// won't run there (#1221).
#[tokio::test]
async fn a_run_whose_plan_moved_while_it_waited_does_not_start() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;
    until_it_waits(&clock).await;

    move_home_to_other(&runner, &secrets).await;
    clock.advance(domain::SignedDuration::from_secs(60));
    until_logged(&["home", "didn't start", "another repository"]).await;
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the new repository's password isn't read"
    );
}

/// A queued backup whose run waited for its keyring, and whose plan moved
/// to another repository while it waited, isn't run there under a record
/// of the old: it ends once the keyring is unlocked, the engine is never
/// asked, and the new repository's password isn't read (#1221).
#[tokio::test]
async fn a_resumed_run_whose_plan_moved_while_it_waited_ends_unrun() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1223, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    until_it_waits(&clock).await;

    move_home_to_other(&runner, &secrets).await;
    clock.advance(domain::SignedDuration::from_secs(60));
    until_logged(&["can't run again", "another repository"]).await;
    // The log line comes before the status is saved: the operation itself
    // ends, and ends as failed, not left queued.
    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::PlanMoved,
            }
        ),
        "the queued backup ends failed: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the new repository's password isn't read"
    );
}

/// A queued backup whose run waited for its keyring, and whose plan was
/// deleted while it waited, ends once the keyring is unlocked as
/// `plan-removed`, not as a stopped daemon, and the engine is never asked
/// (#1293).
#[tokio::test]
async fn a_resumed_run_whose_plan_was_deleted_while_it_waited_ends_plan_removed() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1293, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    until_it_waits(&clock).await;

    let config = runner.config.load().await.expect("the configuration reads");
    save_config(&runner, &config.remove_plan(&home).expect("a valid change")).await;
    clock.advance(domain::SignedDuration::from_secs(60));
    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::PlanRemoved,
            }
        ),
        "the queued backup ends plan-removed: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A queued backup whose run waited for its keyring, whose configuration
/// can't be read once the keyring is unlocked, ends as `config-unusable`,
/// not as a stopped daemon, and the engine is never asked (#1293).
#[tokio::test]
async fn a_resumed_run_whose_configuration_cannot_be_read_once_it_waited_ends_config_unusable() {
    // The start's build and the parked task's first read find it; the read
    // once the keyring is unlocked doesn't.
    ends_config_unusable_when_readable(2, 0x1294).await;
}

/// A queued backup parked for its keyring whose configuration can't be
/// read when its task first looks ends as `config-unusable` without
/// waiting (#1293).
#[tokio::test]
async fn a_parked_resumed_run_whose_configuration_cannot_be_read_ends_config_unusable() {
    // The start's build finds it; the parked task's first read doesn't.
    ends_config_unusable_when_readable(1, 0x1295).await;
}

/// Takes up a queued backup parked for its keyring under a configuration
/// that reads `readable` times and then can't be read, and asserts that it
/// ends as `config-unusable` with the engine never asked.
async fn ends_config_unusable_when_readable(readable: usize, id: u128) {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (mut runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    runner.config = Arc::new(UnreadableAfter {
        config: runner.config.load().await.expect("the configuration reads"),
        readable,
        loads: std::sync::atomic::AtomicUsize::new(0),
    });
    let queued = queued_home_backup(id, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    if readable > 1 {
        until_it_waits(&clock).await;
        clock.advance(domain::SignedDuration::from_secs(60));
    }
    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::ConfigUnusable,
            }
        ),
        "the queued backup ends config-unusable: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A queued backup parked for its keyring whose plan was deleted before
/// its task looked ends as `plan-removed` even when its bound has already
/// passed: the plan is looked for before the bound, so it isn't refused
/// for a secret of a plan nobody has (#1293).
#[tokio::test]
async fn a_parked_resumed_run_whose_plan_was_deleted_ends_plan_removed_past_its_bound() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (mut runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let config = runner.config.load().await.expect("the configuration reads");
    runner.config = Arc::new(ChangesAfterFirstLoad::new(
        config.clone(),
        config.remove_plan(&home).expect("a valid change"),
    ));
    let queued = queued_home_backup(0x1296, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    // The bound is the start's own time: it has passed when the task looks.
    let unlock = app::secrets::UnlockWait::new(clock.clone(), started);
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;

    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::PlanRemoved,
            }
        ),
        "the queued backup ends plan-removed: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A scheduled run parked for its keyring whose plan was deleted before
/// its task looked doesn't start, even when its bound has already
/// passed: the plan is looked for before the bound, as a queued backup's
/// is, so nothing is recorded of a refusal for a secret of a plan nobody
/// has (#1317).
#[tokio::test]
async fn a_parked_run_whose_plan_was_deleted_does_not_start_past_its_bound() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (mut runner, _heard, scripted, home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let config = runner.config.load().await.expect("the configuration reads");
    runner.config = Arc::new(ChangesAfterFirstLoad::new(
        config.clone(),
        config.remove_plan(&home).expect("a valid change"),
    ));
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    // The bound is the start's own time: it has passed when the task looks.
    let unlock = app::secrets::UnlockWait::new(clock.clone(), started);
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;

    until_logged(&["home", "didn't start", "isn't configured"]).await;
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    assert!(
        history.operations.is_empty(),
        "nothing is recorded: {history:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// Each refusal a queued backup's job can be built again with, once its
/// wait for its keyring is over, ends it with its own reason, never as a
/// stopped daemon (#1293).
#[test]
fn a_refused_build_after_the_wait_ends_with_its_own_reason() {
    use crate::composition::plan_runner::never_ran;
    use domain::execution::FailureReason as F;
    assert_eq!(never_ran(&StartError::UnknownPlan), F::PlanRemoved);
    assert_eq!(
        never_ran(&StartError::Unavailable {
            code: ErrorCode::ConfigUnavailable,
            args: Vec::new(),
            detail: None,
        }),
        F::ConfigUnusable
    );
    assert_eq!(
        never_ran(&StartError::KeyFilesNotLeftOut),
        F::KeyFilesNotLeftOut
    );
    assert_eq!(never_ran(&StartError::RunWaiting), F::Superseded);
}

/// A scheduled run that waited for its keyring, whose plan kept its alias
/// while the repository behind the alias changed — another recorded ID —
/// doesn't start, and the other repository's password isn't read for it:
/// the comparison of the recorded IDs comes before the build that
/// resolves it (#1221, decision 135).
#[tokio::test]
async fn a_run_whose_alias_now_names_another_repository_reads_no_new_password() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store).await;
    let config = runner.config.load().await.expect("the configuration reads");
    save_config(&runner, &with_demo(&config, demo_opened)).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;
    until_it_waits(&clock).await;

    secrets.set_keyring_entry(&other_entry(), "canary-other");
    let config = runner.config.load().await.expect("the configuration reads");
    save_config(&runner, &with_demo(&config, demo_now_other)).await;
    clock.advance(domain::SignedDuration::from_secs(60));
    until_logged(&["home", "didn't start", "another repository"]).await;
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the other repository's password isn't read"
    );
}

/// A queued backup whose run waited for its keyring, whose plan kept its
/// alias while the repository behind the alias changed, ends unrun, and
/// the other repository's password isn't read for it (#1221, decision
/// 135).
#[tokio::test]
async fn a_resumed_run_whose_alias_now_names_another_repository_reads_no_new_password() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(60),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let config = runner.config.load().await.expect("the configuration reads");
    save_config(&runner, &with_demo(&config, demo_opened)).await;
    let queued = queued_home_backup(0x1226, Some(repository_id()));
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    until_it_waits(&clock).await;

    secrets.set_keyring_entry(&other_entry(), "canary-other");
    let config = runner.config.load().await.expect("the configuration reads");
    save_config(&runner, &with_demo(&config, demo_now_other)).await;
    clock.advance(domain::SignedDuration::from_secs(60));
    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::PlanMoved,
            }
        ),
        "the queued backup ends failed: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the other repository's password isn't read"
    );
}

/// A scheduled run refused as locked for one repository, whose plan moved
/// to another before the parked run looked at the configuration again,
/// resolves no secret of the other: the repository it was refused for
/// is compared with the plan's before anything is read (#1221, decision
/// 135). Whether it then runs on the new repository is open (#1247).
#[tokio::test]
async fn a_parked_run_whose_plan_moved_before_it_waited_reads_no_new_password() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    secrets.set_keyring_entry(&other_entry(), "canary-other");
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (mut runner, _heard, scripted, home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store).await;
    let config = runner.config.load().await.expect("the configuration reads");
    runner.config = Arc::new(ChangesAfterFirstLoad::new(
        config.clone(),
        home_on_other(&config),
    ));
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;

    until_logged(&["home", "didn't start", "another repository"]).await;
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the new repository's password isn't read"
    );
}

/// A queued backup refused as locked on the repository it was recorded
/// on, whose plan moved before the parked run looked at the configuration
/// again, ends unrun and resolves no secret of the other repository: its
/// recorded repository is compared with the plan's before anything is
/// read (#1221, decision 135).
#[tokio::test]
async fn a_parked_resumed_run_whose_plan_moved_before_it_waited_reads_no_new_password() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    secrets.set_keyring_entry(&other_entry(), "canary-other");
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (mut runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let config = with_demo(
        &runner.config.load().await.expect("the configuration reads"),
        demo_opened,
    );
    runner.config = Arc::new(ChangesAfterFirstLoad::new(
        config.clone(),
        home_on_other(&config),
    ));
    let queued = queued_home_backup(0x1227, Some(repository_id()));
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;

    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::PlanMoved,
            }
        ),
        "the queued backup ends failed: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(other_entry())),
        0,
        "the new repository's password isn't read"
    );
}

/// A scheduled run whose plan's next run came due before its parked
/// task began is refused as locked, as the schedule's run is at the
/// bound, even when the keyring was unlocked in between: a read taken
/// after the bound isn't one the run goes on (#1221, decision 135).
#[tokio::test]
async fn a_parked_run_whose_bound_passed_stays_refused_as_locked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    // Locked for the first read, the schedule's own; unlocked after it.
    secrets.lock_keyring_for_reads(1);
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    // The plan's next run is due already.
    let unlock = app::secrets::UnlockWait::new(clock.clone(), started);
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;

    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended")
        .expect("an end");
    assert_eq!(end.outcome, app::retry::RunOutcome::Failed, "{end:?}");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}")
    };
    assert!(
        matches!(
            &record.status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused {
                    refusal: domain::execution::refusal::SettingRefusal::SecretKeyringLocked { .. },
                    ..
                },
            }
        ),
        "refused as locked: {record:?}"
    );
}

/// A queued backup whose plan's next run came due before its parked task
/// began ends refused as locked, even when the keyring was unlocked in
/// between (#1221, decision 135).
#[tokio::test]
async fn a_parked_resumed_run_whose_bound_passed_stays_refused_as_locked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.lock_keyring_for_reads(1);
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1228, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(clock.clone(), started);
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;

    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            &status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused {
                    refusal: domain::execution::refusal::SettingRefusal::SecretKeyringLocked { .. },
                    ..
                },
            }
        ),
        "refused as locked: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A scheduled run whose keyring's unlock prompt was dismissed is refused
/// as locked at once, and never parked, though its plan's next run is far
/// off: a dismissed prompt is a no, so the entry is read once and the
/// daemon never asks whether the keyring is locked (#993, decision 135).
#[tokio::test]
async fn a_scheduled_run_whose_unlock_prompt_was_dismissed_is_refused_as_locked_and_never_parked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.dismiss_unlock_prompt();
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, heard, scripted, home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::start(&runner, &home, Trigger::Schedule, place, Some(unlock)).await;

    let end = tokio::time::timeout(std::time::Duration::from_secs(5), heard.next())
        .await
        .expect("the schedule hears how the run ended")
        .expect("an end");
    assert_eq!(end.outcome, app::retry::RunOutcome::Failed, "{end:?}");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    let history = store
        .list(&app::ports::HistoryQuery::default())
        .await
        .expect("the log reads");
    let [record] = history.operations.as_slice() else {
        panic!("one operation is recorded: {history:?}")
    };
    assert!(
        matches!(
            &record.status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused {
                    refusal: domain::execution::refusal::SettingRefusal::SecretKeyringLocked { .. },
                    ..
                },
            }
        ),
        "refused as locked: {record:?}"
    );
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(entry)),
        1,
        "the entry is read once, never again"
    );
    assert_eq!(secrets.lock_query_count(), 0, "the run never waited");
}

/// A queued backup the daemon takes up again whose keyring's unlock prompt
/// was dismissed ends refused as locked at once, and is never parked: the
/// entry is read once, and the daemon never asks whether the keyring is
/// locked (#993, decision 135).
#[tokio::test]
async fn a_resumed_run_whose_unlock_prompt_was_dismissed_is_refused_as_locked_and_never_parked() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    let entry = domain::secrets::KeyringEntry::parse("ladderback/demo").expect("an entry");
    secrets.set_keyring_entry(&entry, KEYRING_PASSWORD);
    secrets.dismiss_unlock_prompt();
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets.clone(), clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x993, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;

    let status = until_ended(&store, queued.id()).await;
    assert!(
        matches!(
            &status,
            domain::execution::OperationStatus::Failed {
                reason: domain::execution::FailureReason::SecretRefused {
                    refusal: domain::execution::refusal::SettingRefusal::SecretKeyringLocked { .. },
                    ..
                },
            }
        ),
        "refused as locked: {status:?}"
    );
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
    assert_eq!(
        secrets.read_count(&SecretRef::Keyring(entry)),
        1,
        "the entry is read once, never again"
    );
    assert_eq!(secrets.lock_query_count(), 0, "the run never waited");
}

/// A queued backup whose run waits for its keyring can be cancelled while
/// it waits, as any queued backup can: it ends cancelled, and the engine
/// is never asked (#1221, §4.2).
#[tokio::test]
async fn a_resumed_run_waiting_for_its_keyring_can_be_cancelled() {
    capture();
    let started = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let clock = Arc::new(FakeClock::new(started));
    let secrets = Arc::new(FakeSecrets::new());
    keyring_locked_until(
        &secrets,
        &clock,
        started + domain::SignedDuration::from_secs(3600),
    );
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, _home) =
        keyring_parked_runner(secrets, clock.clone(), store.clone()).await;
    let queued = queued_home_backup(0x1224, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let unlock = app::secrets::UnlockWait::new(
        clock.clone(),
        started + domain::SignedDuration::from_secs(600),
    );
    PlanRuns::resume(&runner, queued.clone(), Some(unlock)).await;
    until_it_waits(&clock).await;

    let cancel = runner
        .cancel(
            queued.id().as_uuid().hyphenated().to_string(),
            http_adapter::Canceller::Session,
        )
        .await;
    assert_eq!(cancel, Ok(()), "the waiting run can be cancelled");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let history = store
                .list(&app::ports::HistoryQuery::default())
                .await
                .expect("the log reads");
            if history
                .operations
                .iter()
                .any(|record| record.status == domain::execution::OperationStatus::Cancelled)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the queued backup ends cancelled");
    assert_eq!(clock.sleepers(), 0, "the wait for the keyring stopped");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A cancel the route answers `Ok` in the instant a queued backup's wait
/// for its keyring ends, before the run gives up its cancel switch, ends
/// the backup cancelled, whatever the wait answered (#1248).
#[tokio::test]
async fn a_cancel_as_a_resumed_runs_wait_ends_ends_it_cancelled() {
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, scripted, home) = refused_password_runner(
        "LADDERBACK_TEST_CANCEL_AS_THE_WAIT_ENDS_UNSET",
        store.clone(),
        Arc::new(BroadcastEventBus::new()),
    )
    .await;
    let queued = queued_home_backup(0x1248, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let switch = runner.registry.register(queued.id());

    let cancel = runner
        .cancel(
            queued.id().as_uuid().hyphenated().to_string(),
            http_adapter::Canceller::Session,
        )
        .await;
    assert_eq!(cancel, Ok(()), "the waiting run can be cancelled");
    runner
        .after_wait(
            queued.clone(),
            &home,
            crate::composition::plan_runner::Waited::Unrunnable(
                FailureReason::PlanRemoved,
                "its plan was deleted".to_owned(),
            ),
            async move {
                let _ = switch.await;
            },
        )
        .await;

    let status = until_ended(&store, queued.id()).await;
    assert_eq!(status, OperationStatus::Cancelled, "the cancel it was told");
    assert_eq!(jobs(&scripted), 0, "the engine was never asked");
}

/// A queued backup whose wait for its keyring ended with no cancel ends
/// as the wait answered, and a cancel after that is refused: nothing of
/// it is running (#1248).
#[tokio::test]
async fn a_resumed_run_whose_wait_ended_unrun_ends_as_it_answered_and_refuses_a_later_cancel() {
    let store = Arc::new(app::fakes::InMemoryOperationStore::new());
    let (runner, _heard, _scripted, home) = refused_password_runner(
        "LADDERBACK_TEST_WAIT_ENDED_UNRUN_UNSET",
        store.clone(),
        Arc::new(BroadcastEventBus::new()),
    )
    .await;
    let queued = queued_home_backup(0x1249, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    let switch = runner.registry.register(queued.id());

    runner
        .after_wait(
            queued.clone(),
            &home,
            crate::composition::plan_runner::Waited::Unrunnable(
                FailureReason::PlanRemoved,
                "its plan was deleted".to_owned(),
            ),
            async move {
                let _ = switch.await;
            },
        )
        .await;

    let status = until_ended(&store, queued.id()).await;
    assert_eq!(
        status,
        OperationStatus::Failed {
            reason: FailureReason::PlanRemoved
        }
    );
    assert!(
        !runner.registry.cancel(&queued.id()),
        "the run gave up its cancel switch"
    );
    let cancel = runner
        .cancel(
            queued.id().as_uuid().hyphenated().to_string(),
            http_adapter::Canceller::Session,
        )
        .await;
    assert_eq!(cancel, Err(http_adapter::CancelError::NotRunning));
}

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
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;

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
        machine: Arc::default(),
        leases: None,
        running: Arc::default(),
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
    PlanRuns::start(&runner, &plan, Trigger::Retry, place, None).await;

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
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;

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

    PlanRuns::resume(&runner, queued.clone(), None).await;
    until_ended(&store, queued.id()).await;

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

    let id = queued.id();
    PlanRuns::resume(&runner, queued, None).await;
    until_ended(&store, id).await;

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
    PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;
    let started = next_end().await.operation;
    let queued = queued_home_backup(0x0539, None);
    store
        .save(&queued, None)
        .await
        .expect("the queued backup is saved");
    PlanRuns::resume(&runner, queued.clone(), None).await;
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

    PlanRuns::resume(&runner, queued.clone(), None).await;
    until_ended(&store, queued.id()).await;

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
