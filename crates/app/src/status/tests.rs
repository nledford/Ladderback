//! What the summary takes each operation for: which counts as a plan's last
//! success, and which repository an operation tells of reading.

use std::collections::BTreeMap;
use std::sync::Arc;

use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::planning::{
    AgentConfig, AgentSettings, CronExpression, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, Schedule, Source,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, PlanId, RepositoryAlias, SignedDuration, TimeZone, Timestamp, Uuid};

use super::{ReadStatus, Status};
use crate::fakes::{FakeClock, InMemoryConfigStore, InMemoryOperationStore};
use crate::ports::{ConfigStore, OperationStore};

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap_or_else(|e| panic!("a time: {e:?}"))
}

fn alias(text: &str) -> RepositoryAlias {
    RepositoryAlias::parse(text).unwrap_or_else(|e| panic!("an alias: {e:?}"))
}

fn plan_id(text: &str) -> PlanId {
    PlanId::parse(text).unwrap_or_else(|e| panic!("a plan ID: {e:?}"))
}

fn repository(name: &str) -> RepositoryConfig {
    RepositoryConfig {
        locations: Default::default(),
        settings: RepositorySettings {
            repository: format!("/home/demo/scratch/{name}"),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        id: None,
        maintenance: Maintenance::None,
        group_by: Default::default(),
        hooks: Default::default(),
    }
}

/// The repositories "nas" and "office", and the plan "home" on "nas".
fn config() -> AgentConfig {
    let repositories = ["nas", "office"]
        .into_iter()
        .map(|name| (alias(name), repository(name)))
        .collect();
    let home = Plan::new(
        plan_id("home"),
        alias("nas"),
        vec![Source::parse("/home/demo/home").unwrap_or_else(|e| panic!("a source: {e:?}"))],
    );
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        repositories,
        vec![home],
    )
    .unwrap_or_else(|e| panic!("a valid configuration: {e:?}"))
}

/// The same, with the plan "home" run daily at 03:00 UTC.
fn daily_config() -> AgentConfig {
    let base = config();
    let home = Plan {
        schedule: Some(Schedule::new(
            CronExpression::parse("0 3 * * *").unwrap_or_else(|e| panic!("a cron: {e:?}")),
        )),
        ..base.plans()[0].clone()
    };
    AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        base.repositories().clone(),
        vec![home],
    )
    .unwrap_or_else(|e| panic!("a valid configuration: {e:?}"))
}

/// More operations than the summary reads in one page.
const MORE_THAN_A_PAGE: usize = 205;

/// The operation log, each operation queued a minute after the one before,
/// with time-ordered IDs as the log's are.
#[derive(Default)]
struct Log {
    store: Arc<InMemoryOperationStore>,
    made: u64,
}

impl Log {
    /// An operation of `kind` on `plan`, or on no plan, started; its
    /// repository's alias recorded when `on` names one.
    fn started(&mut self, kind: OperationKind, plan: Option<&str>, on: Option<&str>) -> Operation {
        self.made += 1;
        let minutes = i64::try_from(self.made).unwrap_or(i64::MAX);
        let at = now() - SignedDuration::from_hours(5) + SignedDuration::from_mins(minutes);
        let seconds = u128::try_from(at.as_second()).unwrap_or(0);
        let id = OperationId::from_uuid(Uuid::from_u128((seconds << 64) | u128::from(self.made)));
        let target = Target {
            plan: plan.map(plan_id),
            repository: None,
        };
        let mut operation = Operation::queue(id, kind, Trigger::Schedule, target, at);
        if let Some(on) = on {
            operation = operation.with_repository_alias(alias(on));
        }
        operation
            .start(at)
            .unwrap_or_else(|e| panic!("a queued operation starts: {e:?}"));
        operation
    }

    async fn keep(&self, operation: &Operation) {
        self.store
            .save(operation, None)
            .await
            .unwrap_or_else(|e| panic!("the log keeps it: {e:?}"));
    }

    async fn succeeded(&mut self, kind: OperationKind, plan: Option<&str>, on: Option<&str>) {
        let mut operation = self.started(kind, plan, on);
        let at = operation.queued_at();
        operation
            .succeed(at)
            .unwrap_or_else(|e| panic!("a running operation succeeds: {e:?}"));
        self.keep(&operation).await;
    }

    async fn failed(&mut self, plan: Option<&str>, on: Option<&str>, reason: FailureReason) {
        let kind = if plan.is_some() {
            OperationKind::Backup
        } else {
            OperationKind::Check
        };
        let mut operation = self.started(kind, plan, on);
        let at = operation.queued_at();
        operation
            .fail(reason, at)
            .unwrap_or_else(|e| panic!("a running operation fails: {e:?}"));
        self.keep(&operation).await;
    }

    async fn read(&self, config: &AgentConfig) -> Status {
        let store = InMemoryConfigStore::new();
        store
            .save(config)
            .await
            .unwrap_or_else(|e| panic!("the configuration is kept: {e:?}"));
        ReadStatus::new(
            Arc::new(store),
            Arc::clone(&self.store) as Arc<dyn OperationStore>,
            Arc::new(FakeClock::new(now())),
            TimeZone::UTC,
        )
        .read()
        .await
        .unwrap_or_else(|e| panic!("the summary is read: {e:?}"))
    }
}

fn unreadable(status: &Status) -> Vec<(&str, OperationKind)> {
    status
        .unreadable
        .iter()
        .map(|entry| (entry.repository.as_str(), entry.operation.kind))
        .collect()
}

#[tokio::test]
async fn a_restore_or_a_check_of_a_plan_that_succeeded_is_not_its_last_success() {
    let mut log = Log::default();
    log.succeeded(OperationKind::Restore, Some("home"), Some("nas"))
        .await;
    log.succeeded(OperationKind::Check, Some("home"), Some("nas"))
        .await;
    let status = log.read(&config()).await;
    assert_eq!(status.plans[0].last_success, None);
    assert_eq!(
        status.plans[0].last.as_ref().map(|last| last.kind),
        Some(OperationKind::Check)
    );
}

#[tokio::test]
async fn a_backup_with_warnings_is_a_last_success() {
    let mut log = Log::default();
    let mut backup = log.started(OperationKind::Backup, Some("home"), None);
    let at = backup.queued_at();
    backup
        .record_warning(Warning {
            kind: WarningKind::SourceUnreadable,
            item: None,
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        })
        .unwrap_or_else(|e| panic!("a running backup warns: {e:?}"));
    backup
        .succeed(at)
        .unwrap_or_else(|e| panic!("a running backup succeeds: {e:?}"));
    log.keep(&backup).await;
    let status = log.read(&config()).await;
    let success = status.plans[0]
        .last_success
        .as_ref()
        .map(|s| &s.operation.status);
    assert_eq!(success, Some(&OperationStatus::SucceededWithWarnings));
}

#[tokio::test]
async fn an_operation_names_the_repository_it_recorded_before_its_plans() {
    let mut log = Log::default();
    // A check of "office" that couldn't find it, run for no plan; and one
    // recorded on "office" though the plan "home" is on "nas".
    log.failed(None, Some("office"), FailureReason::RepositoryNotFound)
        .await;
    let mut restore = log.started(OperationKind::Restore, Some("home"), Some("office"));
    let at = restore.queued_at();
    restore
        .fail(FailureReason::PasswordNotAccepted, at)
        .unwrap_or_else(|e| panic!("a running restore fails: {e:?}"));
    log.keep(&restore).await;
    let status = log.read(&config()).await;
    assert_eq!(
        unreadable(&status),
        vec![("office", OperationKind::Restore)]
    );
}

#[tokio::test]
async fn a_repository_or_a_plan_the_configuration_no_longer_holds_is_not_named() {
    let mut log = Log::default();
    log.failed(None, Some("attic"), FailureReason::RepositoryNotFound)
        .await;
    log.failed(Some("gone"), None, FailureReason::RepositoryNotFound)
        .await;
    let status = log.read(&config()).await;
    assert_eq!(unreadable(&status), Vec::new());
}

#[tokio::test]
async fn an_operation_that_tells_nothing_of_reading_leaves_the_last_that_did() {
    let mut log = Log::default();
    log.failed(Some("home"), None, FailureReason::RepositoryNotFound)
        .await;
    let mut canceled = log.started(OperationKind::Backup, Some("home"), None);
    let at = canceled.queued_at();
    canceled
        .cancel(at)
        .unwrap_or_else(|e| panic!("a running backup is canceled: {e:?}"));
    log.keep(&canceled).await;
    log.failed(
        Some("home"),
        None,
        FailureReason::EngineFailed {
            detail: "a source folder is gone".to_owned(),
        },
    )
    .await;
    let status = log.read(&config()).await;
    assert_eq!(unreadable(&status), vec![("nas", OperationKind::Backup)]);
    assert_eq!(
        status.unreadable[0].operation.status,
        OperationStatus::Failed {
            reason: FailureReason::RepositoryNotFound
        }
    );
}

#[tokio::test]
async fn a_plan_whose_last_success_is_older_than_the_newest_page_is_not_overdue() {
    let mut log = Log::default();
    log.succeeded(OperationKind::Backup, Some("home"), None)
        .await;
    for _ in 0..MORE_THAN_A_PAGE {
        log.failed(Some("home"), None, FailureReason::DeadlinePassed)
            .await;
    }
    let status = log.read(&daily_config()).await;
    let standing = &status.plans[0];
    assert_eq!(
        standing.last_success.as_ref().map(|s| s.ago),
        Some(SignedDuration::from_hours(5) - SignedDuration::from_mins(1)),
        "the last success is the one older than the page"
    );
    assert!(!standing.overdue, "its next run isn't due");
}

#[tokio::test]
async fn a_scheduled_plan_that_never_succeeded_stays_overdue_through_a_long_history() {
    let mut log = Log::default();
    for _ in 0..MORE_THAN_A_PAGE {
        log.failed(Some("home"), None, FailureReason::DeadlinePassed)
            .await;
    }
    let status = log.read(&daily_config()).await;
    assert_eq!(status.plans[0].last_success, None);
    assert!(status.plans[0].overdue);
}

#[tokio::test]
async fn a_repository_whose_last_reading_failed_before_the_newest_page_is_still_unreadable() {
    let mut log = Log::default();
    log.failed(Some("home"), None, FailureReason::RepositoryNotFound)
        .await;
    for _ in 0..MORE_THAN_A_PAGE {
        log.succeeded(OperationKind::Check, None, Some("office"))
            .await;
    }
    let status = log.read(&config()).await;
    assert_eq!(unreadable(&status), vec![("nas", OperationKind::Backup)]);
}

/// The log is read back only until every configured repository has been
/// told of: what is older can't change what the summary says.
#[tokio::test]
async fn every_configured_repository_told_of_completes_the_reading() {
    let mut log = Log::default();
    let mut record = async |plan: Option<&str>, on: Option<&str>| {
        let operation = log.started(OperationKind::Check, plan, on);
        log.keep(&operation).await;
        log.store
            .get(operation.id())
            .await
            .unwrap_or_else(|e| panic!("the log reads it: {e:?}"))
            .unwrap_or_else(|| panic!("the log holds it"))
            .record
    };
    let mut nas = record(None, Some("nas")).await;
    let mut office = record(None, Some("office")).await;
    for record in [&mut nas, &mut office] {
        record.status = OperationStatus::Succeeded;
    }
    let config = config();
    let mut told = super::Told::new(&config);
    assert!(!told.complete(), "nothing told of yet");
    told.take(std::slice::from_ref(&nas));
    assert!(!told.complete(), "office not told of yet");
    told.take(std::slice::from_ref(&office));
    assert!(told.complete());
}
