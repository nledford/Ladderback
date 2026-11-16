//! Steps for `features/planning/plan_editor.feature`: a person creates,
//! changes and deletes plans through the use case the Plans page calls, in
//! the configuration the scheduler world's loop reads, bringing the
//! proposals up to date as the daemon does, or in the one the forget
//! world's maintenance reads.

use std::sync::Arc;

use app::fakes::InMemoryConfigStore;
use app::plans::{EditError, EditPlans};
use app::ports::ConfigStore;
use app::repositories::ConfigWrites;
use cucumber::{given, then, when};
use domain::RepositoryAlias;
use domain::planning::{AgentConfig, ChangeRefused, CronExpression, Plan, Schedule, Violation};

use super::Agent;
use super::proposals_steps::proposals;
use super::scheduler_steps::{plan_id, plan_named, plan_on};

/// The lock every write of the configuration holds, and what the last
/// change a person made came to.
#[derive(Default)]
pub struct PlansWorld {
    writes: Arc<ConfigWrites>,
    edited: Option<Result<(), EditError>>,
}

impl PlansWorld {
    fn edits(&self, config: Arc<InMemoryConfigStore>) -> EditPlans {
        EditPlans::new(
            config,
            self.writes.clone(),
            app::schedule::config_changes().0,
        )
    }

    fn refusal(&mut self) -> ChangeRefused {
        match self.edited.take() {
            Some(Err(EditError::Refused(refused))) => refused,
            other => panic!("the change was refused, not {other:?}"),
        }
    }
}

/// The edits of the configuration the scheduler world's loop reads, which
/// bring the proposals world's proposals up to date, as the daemon's do.
fn serving(agent: &Agent) -> EditPlans {
    agent
        .plans
        .edits(agent.scheduler.config())
        .with_proposals(Arc::new(proposals(agent)))
}

fn scheduled(mut plan: Plan, cron: &str) -> Plan {
    plan.schedule = Some(Schedule::new(
        CronExpression::parse(cron).expect("a cron expression"),
    ));
    plan
}

async fn loaded(config: &InMemoryConfigStore) -> AgentConfig {
    config.load().await.expect("the configuration loads")
}

#[given("the agent has its repository and no plans")]
async fn no_plans(agent: &mut Agent) {
    agent.scheduler.save().await;
}

#[when(expr = "a person creates the plan {string} scheduled {string}")]
async fn creates(agent: &mut Agent, name: String, cron: String) {
    let edits = serving(agent);
    let created = edits.create(scheduled(plan_named(&name), &cron)).await;
    agent.plans.edited = Some(created);
}

#[when(expr = "a person creates the plan {string} into the repository {string}")]
async fn creates_into(agent: &mut Agent, name: String, repository: String) {
    let repository = RepositoryAlias::parse(&repository).expect("the scenario's alias is valid");
    let edits = serving(agent);
    let created = edits.create(plan_on(&name, repository)).await;
    agent.plans.edited = Some(created);
}

/// The plan as the configuration holds it, or as a form still shows it once
/// the configuration no longer does, with its schedule changed.
#[when(expr = "a person changes the schedule of the plan {string} to {string}")]
async fn changes_schedule(agent: &mut Agent, name: String, cron: String) {
    let config = agent.scheduler.config();
    let shown = loaded(&config)
        .await
        .plan(&plan_id(&name))
        .cloned()
        .unwrap_or_else(|| plan_named(&name));
    let changed = serving(agent).change(scheduled(shown, &cron)).await;
    agent.plans.edited = Some(changed);
}

#[when(expr = "a person deletes the plan {string} while the agent serves")]
async fn deletes_while_serving(agent: &mut Agent, name: String) {
    let edits = serving(agent);
    let deleted = edits.delete(&plan_id(&name)).await;
    assert_eq!(deleted, Ok(()), "the plan is deleted");
}

#[when(expr = "a person deletes the plan {string}")]
async fn deletes(agent: &mut Agent, name: String) {
    let edits = agent.plans.edits(agent.forget.store());
    let deleted = edits.delete(&plan_id(&name)).await;
    assert_eq!(deleted, Ok(()), "the plan is deleted");
}

#[then(expr = "the change is refused at the setting {string} because another plan has the ID")]
fn refused_taken_id(agent: &mut Agent, setting: String) {
    assert_eq!(setting, "id", "a taken ID is the plan's ID at fault");
    assert!(
        matches!(
            agent.plans.refusal(),
            ChangeRefused::Violation(Violation::DuplicatePlan { .. })
        ),
        "refused for its ID"
    );
}

#[then(
    expr = "the change is refused at the setting {string} because the configuration has no such repository"
)]
fn refused_unknown_repository(agent: &mut Agent, setting: String) {
    assert_eq!(
        setting, "repository",
        "an unknown repository is the plan's repository at fault"
    );
    assert!(
        matches!(
            agent.plans.refusal(),
            ChangeRefused::Violation(Violation::UnknownRepository { .. })
        ),
        "refused for its repository"
    );
}

#[then(expr = "the change is refused because the configuration has no plan {string}")]
fn refused_unknown_plan(agent: &mut Agent, name: String) {
    assert_eq!(
        agent.plans.refusal(),
        ChangeRefused::UnknownPlan(plan_id(&name))
    );
}

#[then(expr = "the configuration still holds only the plan {string} scheduled {string}")]
async fn holds_only(agent: &mut Agent, name: String, cron: String) {
    let config = loaded(&agent.scheduler.config()).await;
    assert_eq!(config.plans(), [scheduled(plan_named(&name), &cron)]);
}

#[then("the configuration holds no plans")]
async fn holds_no_plans(agent: &mut Agent) {
    let config = loaded(&agent.scheduler.config()).await;
    assert!(config.plans().is_empty(), "plans: {:?}", config.plans());
}
