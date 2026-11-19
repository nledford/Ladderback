//! Steps for `features/automation/status.feature`: the status summary read
//! through `ReadStatus` over an in-memory configuration and operation log of
//! the scenario's own, on a clock stopped on the hour.

use std::collections::BTreeMap;
use std::sync::Arc;

use app::fakes::{FakeClock, InMemoryConfigStore, InMemoryOperationStore};
use app::ports::{ConfigStore, OperationStore};
use app::status::{ReadStatus, Status};
use cucumber::{given, then, when};
use domain::execution::{
    FailureReason, Operation, OperationKind, OperationStatus, Target, Trigger,
};
use domain::planning::{AgentConfig, AgentSettings, CronExpression, Hooks, Plan, Schedule};
use domain::{OperationId, PlanId, RepositoryId, SignedDuration, TimeZone, Timestamp, Uuid};

use super::Agent;
use super::scheduler_steps::config_with_plans;

/// The scenario's configuration, operation log and summary.
#[derive(Default)]
pub struct StatusWorld {
    config: Option<AgentConfig>,
    store: Arc<InMemoryOperationStore>,
    /// How many operations the scenario has put in the log.
    made: u128,
    status: Option<Status>,
}

impl StatusWorld {
    /// An ID that sorts after every operation queued earlier, as the log's
    /// time-ordered IDs do.
    fn id_at(&mut self, at: Timestamp) -> OperationId {
        self.made += 1;
        let seconds = u128::try_from(at.as_second()).expect("a time after 1970");
        OperationId::from_uuid(Uuid::from_u128((seconds << 64) | self.made))
    }

    fn backup(&mut self, plan: &str, queued: Timestamp) -> Operation {
        Operation::queue(
            self.id_at(queued),
            OperationKind::Backup,
            Trigger::Schedule,
            Target {
                plan: Some(plan_id(plan)),
                repository: None,
            },
            queued,
        )
    }

    async fn keep(&self, operation: &Operation) {
        self.store
            .save(operation, None)
            .await
            .expect("the log keeps the operation");
    }

    fn status(&self) -> &Status {
        self.status
            .as_ref()
            .expect("the scenario read the status summary")
    }
}

/// The scenario's now: on the hour (08:00 UTC), the summary's clock.
fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).expect("a time")
}

fn ago(amount: i64, unit: &str) -> SignedDuration {
    match unit {
        "minute" | "minutes" => SignedDuration::from_mins(amount),
        "hour" | "hours" => SignedDuration::from_hours(amount),
        other => panic!("the scenario names a unit of time that isn't one: {other}"),
    }
}

fn plan_id(name: &str) -> PlanId {
    PlanId::parse(name).expect("the scenario's plan ID is valid")
}

fn repository_id(alias: &str) -> RepositoryId {
    let folded = alias.bytes().fold(0x5747_u128, |acc, byte| {
        acc.wrapping_mul(257).wrapping_add(u128::from(byte))
    });
    RepositoryId::parse(&format!("{folded:064x}")).expect("64 hex digits are a repository ID")
}

#[given(expr = "the plans {string} on {string} and {string} on {string}, each run every hour")]
fn the_plans(agent: &mut Agent, first: String, on: String, second: String, other: String) {
    let listed = config_with_plans(&[(&first, &on), (&second, &other)], repository_id);
    let hourly = Schedule::new(CronExpression::parse("0 * * * *").expect("a cron expression"));
    let plans: Vec<Plan> = listed
        .plans()
        .iter()
        .cloned()
        .map(|plan| Plan {
            schedule: Some(hourly.clone()),
            ..plan
        })
        .collect();
    agent.status.config = Some(
        AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            listed.repositories().clone(),
            plans,
        )
        .expect("the scenario's configuration is valid"),
    );
}

#[given(regex = r#"^the plan "([^"]+)" last backed up successfully (\d+) (minutes?|hours?) ago$"#)]
async fn backed_up(agent: &mut Agent, plan: String, amount: i64, unit: String) {
    let finished = now() - ago(amount, &unit);
    let started = finished - SignedDuration::from_mins(1);
    let mut backup = agent.status.backup(&plan, started);
    backup.start(started).expect("a queued backup starts");
    backup.succeed(finished).expect("a running backup succeeds");
    agent.status.keep(&backup).await;
}

#[given(
    regex = r#"^the plan "([^"]+)" last failed (\d+) (minutes?|hours?) ago( because no repository's config file was found)?$"#
)]
async fn failed(agent: &mut Agent, plan: String, amount: i64, unit: String, why: String) {
    let finished = now() - ago(amount, &unit);
    let started = finished - SignedDuration::from_mins(1);
    let reason = if why.is_empty() {
        FailureReason::EngineFailed {
            detail: "a source folder is gone".to_owned(),
        }
    } else {
        FailureReason::RepositoryConfigNotFound {
            detail: "no config file at the location".to_owned(),
        }
    };
    let mut backup = agent.status.backup(&plan, started);
    backup.start(started).expect("a queued backup starts");
    backup
        .fail(reason, finished)
        .expect("a running backup fails");
    agent.status.keep(&backup).await;
}

#[given(regex = r#"^a backup of the plan "([^"]+)" is (running|waiting its turn)$"#)]
async fn in_the_queue(agent: &mut Agent, plan: String, state: String) {
    let queued = now() - SignedDuration::from_mins(5);
    let mut backup = agent.status.backup(&plan, queued);
    if state == "running" {
        backup.start(queued).expect("a queued backup starts");
    }
    agent.status.keep(&backup).await;
}

#[when("an AI agent reads the status summary")]
async fn reads(agent: &mut Agent) {
    let config = InMemoryConfigStore::new();
    config
        .save(
            agent
                .status
                .config
                .as_ref()
                .expect("the scenario configured its plans"),
        )
        .await
        .expect("the configuration is kept");
    let read = ReadStatus::new(
        Arc::new(config),
        agent.status.store.clone(),
        Arc::new(FakeClock::new(now())),
        TimeZone::UTC,
    );
    agent.status.status = Some(read.read().await.expect("the summary is read"));
}

fn standing<'a>(agent: &'a Agent, plan: &str) -> &'a app::status::PlanStanding {
    agent
        .status
        .status()
        .plans
        .iter()
        .find(|standing| standing.plan == plan_id(plan))
        .unwrap_or_else(|| panic!("the summary has no plan {plan}"))
}

#[then(regex = r#"^the plan "([^"]+)" last backed up successfully (\d+) (minutes?|hours?) ago$"#)]
fn last_success(agent: &mut Agent, plan: String, amount: i64, unit: String) {
    let success = standing(agent, &plan)
        .last_success
        .as_ref()
        .unwrap_or_else(|| panic!("the plan {plan} has no successful backup in the summary"));
    assert_eq!(success.ago, ago(amount, &unit));
    assert_eq!(success.operation.status, OperationStatus::Succeeded);
}

#[then(expr = "the plan {string} has no successful backup")]
fn no_success(agent: &mut Agent, plan: String) {
    assert_eq!(standing(agent, &plan).last_success, None);
}

#[then(regex = r#"^the plan "([^"]+)" is (not )?overdue$"#)]
fn overdue(agent: &mut Agent, plan: String, not: String) {
    assert_eq!(standing(agent, &plan).overdue, not.is_empty(), "{plan}");
}

#[then("no plan is overdue")]
fn none_overdue(agent: &mut Agent) {
    let overdue: Vec<&PlanId> = agent
        .status
        .status()
        .plans
        .iter()
        .filter(|standing| standing.overdue)
        .map(|standing| &standing.plan)
        .collect();
    assert!(overdue.is_empty(), "overdue: {overdue:?}");
}

fn unreadable(agent: &Agent) -> BTreeMap<String, String> {
    agent
        .status
        .status()
        .unreadable
        .iter()
        .map(|unreadable| {
            let code = match &unreadable.operation.status {
                OperationStatus::Failed { reason } => reason.code(),
                other => panic!("an unreadable repository's operation {other:?} didn't fail"),
            };
            (unreadable.repository.as_str().to_owned(), code.to_owned())
        })
        .collect()
}

#[then("no repository is said to be unreadable")]
fn none_unreadable(agent: &mut Agent) {
    assert_eq!(unreadable(agent), BTreeMap::new());
}

#[then(expr = "the repository {string} is said to be unreadable with {string}")]
fn is_unreadable(agent: &mut Agent, repository: String, code: String) {
    assert_eq!(unreadable(agent).get(&repository), Some(&code));
}

#[then(expr = "the repository {string} is not said to be unreadable")]
fn is_not_unreadable(agent: &mut Agent, repository: String) {
    assert_eq!(unreadable(agent).get(&repository), None);
}

#[then("no backup is running or waiting")]
fn empty_queue(agent: &mut Agent) {
    assert_eq!(agent.status.status().queue, Vec::new());
}

#[then(regex = r#"^the backup of the plan "([^"]+)" is (running|waiting)$"#)]
fn queued(agent: &mut Agent, plan: String, state: String) {
    let status = if state == "running" {
        OperationStatus::Running
    } else {
        OperationStatus::Queued
    };
    let found = agent
        .status
        .status()
        .queue
        .iter()
        .find(|operation| operation.target.plan == Some(plan_id(&plan)))
        .unwrap_or_else(|| panic!("the queue holds no backup of the plan {plan}"));
    assert_eq!(found.kind, OperationKind::Backup);
    assert_eq!(found.status, status);
}
