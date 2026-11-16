//! Steps for `features/planning/proposals.feature`: AI agents, each with
//! its own API token, propose changes to the configuration the scheduler
//! world's loop reads, through the use case the proposal routes call, while
//! a person edits that configuration through the Plans page's use case.

use std::sync::Arc;

use app::fakes::{FakeChangeCodec, InMemoryProposalStore, SeededRandom};
use app::ports::{ConfigStore, ProposalStore};
use app::proposals::{ProposalError, Proposals};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::access::TOKEN_BYTES;
use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::planning::{
    ChangeRefused, ConfigChange, ConfigRevision, HookEntry, Plan, Proposal, ProposalRefused,
    ProposalStatus, Proposer, Rationale,
};

use super::Agent;
use super::scheduler_steps::{plan_id, plan_named};

/// The proposals kept, and what the AI agent's last proposal came to.
pub struct ProposalsWorld {
    store: Arc<InMemoryProposalStore>,
    codec: Arc<FakeChangeCodec>,
    random: Arc<SeededRandom>,
    /// What the AI agent's last proposal came to.
    made: Option<Result<Proposal, ProposalError>>,
    /// What the AI agent's last withdrawal came to.
    withdrawn: Option<Result<Proposal, ProposalError>>,
}

impl Default for ProposalsWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryProposalStore::new()),
            codec: Arc::new(FakeChangeCodec::new()),
            random: Arc::new(SeededRandom::new(89)),
            made: None,
            withdrawn: None,
        }
    }
}

impl ProposalsWorld {
    /// The AI agent's last proposal, which was kept.
    fn kept(&self) -> &Proposal {
        match &self.made {
            Some(Ok(proposal)) => proposal,
            other => panic!("the AI agent's proposal was kept, not {other:?}"),
        }
    }

    /// Why the AI agent's last proposal was refused.
    fn refusal(&self) -> &ProposalError {
        match &self.made {
            Some(Err(error)) => error,
            other => panic!("the AI agent's proposal was refused, not {other:?}"),
        }
    }
}

/// The proposals use case over the configuration the scheduler world's
/// loop reads, on the scenario's clock.
fn proposals(agent: &Agent) -> Proposals {
    Proposals::new(
        agent.proposals.store.clone(),
        agent.proposals.codec.clone(),
        agent.scheduler.config(),
        agent.clock.clone(),
        agent.proposals.random.clone(),
    )
}

/// The AI agent `n`, by its own API token: 1 is the AI agent, 2 another.
fn ai_agent(n: u8) -> Proposer {
    Proposer::Token {
        id: ApiTokenId::from_bytes(&[n; TOKEN_BYTES]),
        label: ApiTokenLabel::new(format!("demo-assistant-{n}")).expect("a valid label"),
    }
}

const THE_AI_AGENT: u8 = 1;
const ANOTHER_AI_AGENT: u8 = 2;

async fn propose(
    agent: &Agent,
    proposer: u8,
    changes: Vec<ConfigChange>,
) -> Result<Proposal, ProposalError> {
    proposals(agent)
        .propose(
            ai_agent(proposer),
            ConfigRevision::new("demo-revision".to_owned()),
            changes,
            Rationale::default(),
        )
        .await
}

fn adding(name: &str) -> Vec<ConfigChange> {
    vec![ConfigChange::AddPlan(plan_named(name))]
}

/// The plan `name` with a hook that runs before each of its backups.
fn hooked(name: &str) -> Plan {
    let mut plan = plan_named(name);
    plan.hooks
        .run_before
        .push(HookEntry::parse("demo-dump --all").expect("a hook"));
    plan
}

/// The AI agent's last proposal as the AI agent reads it now.
async fn read(agent: &Agent) -> Proposal {
    let id = agent.proposals.kept().id.clone();
    proposals(agent)
        .get(&ai_agent(THE_AI_AGENT), &id)
        .await
        .expect("the AI agent reads its proposal")
}

async fn status(agent: &Agent) -> ProposalStatus {
    read(agent).await.status
}

#[when(expr = "an AI agent proposes adding the plan {string}")]
#[when(expr = "the AI agent proposes adding the plan {string}")]
async fn proposes_adding(agent: &mut Agent, name: String) {
    agent.proposals.made = Some(propose(agent, THE_AI_AGENT, adding(&name)).await);
}

#[given(expr = "an AI agent has proposed adding the plan {string}")]
async fn has_proposed_adding(agent: &mut Agent, name: String) {
    proposes_adding(agent, name).await;
    agent.proposals.kept();
}

#[when(expr = "an AI agent proposes changing the plan {string}")]
async fn proposes_changing(agent: &mut Agent, name: String) {
    let changes = vec![ConfigChange::ChangePlan(plan_named(&name))];
    agent.proposals.made = Some(propose(agent, THE_AI_AGENT, changes).await);
}

#[when(
    expr = "an AI agent proposes adding the plan {string} with a hook that runs before each backup"
)]
async fn proposes_adding_hooked(agent: &mut Agent, name: String) {
    let changes = vec![ConfigChange::AddPlan(hooked(&name))];
    agent.proposals.made = Some(propose(agent, THE_AI_AGENT, changes).await);
}

#[given(expr = "the plan {string} has a hook that runs before each backup")]
async fn plan_has_hook(agent: &mut Agent, name: String) {
    let config = agent.scheduler.config();
    let hooked = config
        .load()
        .await
        .expect("the configuration loads")
        .apply(&ConfigChange::AddPlan(hooked(&name)))
        .expect("the configuration takes the plan");
    config.save(&hooked).await.expect("the configuration saves");
}

#[when(expr = "an AI agent proposes changing the plan {string} to have no hooks")]
async fn proposes_changing_without_hooks(agent: &mut Agent, name: String) {
    proposes_changing(agent, name).await;
}

#[when(expr = "an AI agent proposes adding {int} plans")]
async fn proposes_adding_many(agent: &mut Agent, count: usize) {
    let changes = (1..=count)
        .map(|n| ConfigChange::AddPlan(plan_named(&format!("demo-{n}"))))
        .collect();
    agent.proposals.made = Some(propose(agent, THE_AI_AGENT, changes).await);
}

#[given(expr = "an AI agent has {int} proposals pending")]
async fn has_pending(agent: &mut Agent, count: usize) {
    for n in 1..=count {
        let made = propose(agent, THE_AI_AGENT, adding(&format!("demo-{n}"))).await;
        assert!(made.is_ok(), "proposal {n} is kept: {made:?}");
    }
}

#[when("the AI agent withdraws the proposal")]
async fn withdraws(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let withdrawn = proposals(agent)
        .withdraw(&ai_agent(THE_AI_AGENT), &id)
        .await;
    agent.proposals.withdrawn = Some(withdrawn);
}

#[then(expr = "the proposal is pending, to expire {int} hours after it was made")]
async fn pending_to_expire(agent: &mut Agent, hours: i64) {
    let made = agent.proposals.kept().clone();
    assert_eq!(made.status, ProposalStatus::Pending);
    assert_eq!(
        made.expires_at.duration_since(made.created_at),
        SignedDuration::from_hours(hours)
    );
    assert_eq!(
        read(agent).await,
        made,
        "the proposal is kept as it was made"
    );
}

#[then(expr = "the configuration has no plan {string}")]
async fn has_no_plan(agent: &mut Agent, name: String) {
    let config = agent
        .scheduler
        .config()
        .load()
        .await
        .expect("the configuration loads");
    assert_eq!(config.plan(&plan_id(&name)), None);
}

#[then(
    expr = "the proposal is refused because its change {int} names a plan the configuration doesn't have"
)]
fn refused_unknown_plan(agent: &mut Agent, change: usize) {
    assert!(
        matches!(
            agent.proposals.refusal(),
            ProposalError::Refused(ProposalRefused::Change {
                index,
                refused: ChangeRefused::UnknownPlan(_),
            }) if *index + 1 == change
        ),
        "refused at change {change} for its plan: {:?}",
        agent.proposals.refusal()
    );
}

#[then(expr = "the proposal is refused as holding more than {int} changes")]
fn refused_too_many(agent: &mut Agent, limit: usize) {
    assert_eq!(limit, domain::planning::MAX_CHANGES);
    assert_eq!(
        agent.proposals.refusal(),
        &ProposalError::Refused(ProposalRefused::TooManyChanges)
    );
}

#[then(expr = "the proposal is refused because its change {int} changes a plan's hooks")]
fn refused_hooks(agent: &mut Agent, change: usize) {
    assert!(
        matches!(
            agent.proposals.refusal(),
            ProposalError::Refused(ProposalRefused::Hooks { index }) if *index + 1 == change
        ),
        "refused at change {change} for its hooks: {:?}",
        agent.proposals.refusal()
    );
}

#[then(expr = "the proposal is refused as one over the {int} pending")]
fn refused_full(agent: &mut Agent, limit: usize) {
    assert_eq!(limit, domain::planning::MAX_PENDING);
    assert_eq!(agent.proposals.refusal(), &ProposalError::Full);
}

#[then("no proposal is kept")]
async fn none_kept(agent: &mut Agent) {
    let kept = agent.proposals.store.all().await.expect("the store reads");
    assert_eq!(kept, Vec::new());
}

#[then("the proposal is pending")]
async fn is_pending(agent: &mut Agent) {
    assert_eq!(status(agent).await, ProposalStatus::Pending);
}

#[then("the proposal is stale")]
async fn is_stale(agent: &mut Agent) {
    assert_eq!(status(agent).await, ProposalStatus::Stale);
}

#[then("the proposal has expired")]
async fn has_expired(agent: &mut Agent) {
    assert_eq!(status(agent).await, ProposalStatus::Expired);
}

#[then("the proposal is withdrawn")]
async fn is_withdrawn(agent: &mut Agent) {
    match &agent.proposals.withdrawn {
        Some(Ok(answered)) => assert_eq!(answered.status, ProposalStatus::Withdrawn),
        other => panic!("the proposal was withdrawn, not {other:?}"),
    }
    assert_eq!(status(agent).await, ProposalStatus::Withdrawn);
}

#[then("the AI agent can't withdraw it, because it has expired")]
async fn cant_withdraw_expired(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let withdrawn = proposals(agent)
        .withdraw(&ai_agent(THE_AI_AGENT), &id)
        .await;
    assert_eq!(
        withdrawn,
        Err(ProposalError::NotPending(ProposalStatus::Expired))
    );
}

#[then("the AI agent's proposals list it as withdrawn")]
async fn lists_withdrawn(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let listed = proposals(agent)
        .list(&ai_agent(THE_AI_AGENT))
        .await
        .expect("the AI agent lists its proposals");
    let statuses: Vec<_> = listed.iter().map(|one| (&one.id, one.status)).collect();
    assert_eq!(statuses, [(&id, ProposalStatus::Withdrawn)]);
}

#[then(expr = "another AI agent's proposal of the plan {string} is kept")]
async fn another_kept(agent: &mut Agent, name: String) {
    let made = propose(agent, ANOTHER_AI_AGENT, adding(&name))
        .await
        .expect("another AI agent's proposal is kept");
    let kept = agent
        .proposals
        .store
        .get(&made.id)
        .await
        .expect("the store reads");
    assert_eq!(
        kept.map(|one| one.proposer),
        Some(ai_agent(ANOTHER_AI_AGENT))
    );
}

#[then("another AI agent's proposals list none")]
async fn another_lists_none(agent: &mut Agent) {
    let listed = proposals(agent)
        .list(&ai_agent(ANOTHER_AI_AGENT))
        .await
        .expect("another AI agent lists its proposals");
    assert_eq!(listed, Vec::new());
}

#[then("another AI agent can't withdraw it, as it can't see it")]
async fn another_cant_withdraw(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let use_case = proposals(agent);
    let other = ai_agent(ANOTHER_AI_AGENT);
    assert_eq!(
        use_case.withdraw(&other, &id).await,
        Err(ProposalError::Unknown)
    );
    assert_eq!(use_case.get(&other, &id).await, Err(ProposalError::Unknown));
    assert_eq!(status(agent).await, ProposalStatus::Pending);
}
