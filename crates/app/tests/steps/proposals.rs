//! Steps for `features/planning/proposals.feature` and
//! `features/planning/proposal_approval.feature`: AI agents, each with its
//! own API token, propose changes to the configuration the scheduler
//! world's loop reads, through the use case the proposal routes call, while
//! a person edits that configuration through the Plans page's use case, and
//! reviews, approves or rejects the proposals on the "Pending changes" page.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use api::{ProposalStatusDto, UiEvent};
use app::fakes::{FakeChangeCodec, InMemoryConfigStore, InMemoryProposalStore, SeededRandom};
use app::ports::{ConfigDrafts, ConfigStore, ProposalStore};
use app::proposals::{
    ApprovalAsk, Made, ProposalError, ProposalPorts, Proposals, RepositoryForget, Review,
};
use cucumber::{given, then, when};
use domain::access::TOKEN_BYTES;
use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::maintenance::{ForgetComparison, ForgetPreview, ForgetSide, PlanForget};
use domain::planning::KeepCount;
use domain::planning::{
    ChangeRefused, ConfigChange, ConfigRevision, HookEntry, MAX_PER_HOUR, Plan, Proposal,
    ProposalId, ProposalRefused, ProposalStatus, ProposalWarning, Proposer, QUIET_AFTER_REJECTION,
    Rationale, Source, SummaryLine, TooSoon, short_proposal_id,
};
use domain::{OperationId, RepositoryAlias, SignedDuration, Timestamp};

use super::Agent;
use super::scheduler_steps::{RecordingEvents, plan_id, plan_named};

/// The proposals kept, and what the AI agent's last proposal came to.
pub struct ProposalsWorld {
    store: Arc<InMemoryProposalStore>,
    codec: Arc<FakeChangeCodec>,
    random: Arc<SeededRandom>,
    events: Arc<RecordingEvents>,
    /// What the AI agent's last proposal came to.
    made: Option<Result<Proposal, ProposalError>>,
    /// What the submission checks warned the AI agent of, last.
    warnings: Vec<ProposalWarning>,
    /// What the AI agent's last withdrawal came to.
    withdrawn: Option<Result<Proposal, ProposalError>>,
    /// The proposal as the "Pending changes" page last showed it.
    review: Option<Review>,
    /// What the person's last approval came to.
    approved: Option<Result<Proposal, ProposalError>>,
    /// What the person's last rejection came to.
    rejected: Option<Result<Proposal, ProposalError>>,
    /// The configuration the proposals are checked against, when not the
    /// scheduler world's: the forget preview's scenarios use the forget
    /// world's, whose repository the scripted engine lists.
    config: Option<Arc<InMemoryConfigStore>>,
    /// What the person's last forget preview came to.
    forget: Option<Result<Vec<RepositoryForget>, ProposalError>>,
}

impl Default for ProposalsWorld {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryProposalStore::new()),
            codec: Arc::new(FakeChangeCodec::new()),
            random: Arc::new(SeededRandom::new(89)),
            events: Arc::default(),
            made: None,
            warnings: Vec::new(),
            withdrawn: None,
            review: None,
            approved: None,
            rejected: None,
            config: None,
            forget: None,
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

    /// The page as it last showed the proposal.
    fn shown(&self) -> &Review {
        self.review
            .as_ref()
            .expect("a person has opened the proposal")
    }

    /// Why the person's last approval was refused.
    fn approval_refusal(&self) -> &ProposalError {
        match &self.approved {
            Some(Err(error)) => error,
            other => panic!("the approval was refused, not {other:?}"),
        }
    }
}

/// The proposals use case over the configuration the scheduler world's
/// loop reads, on the scenario's clock, with sign-in off. Built again for
/// each step, as one daemon's: under the same revision key.
pub(crate) fn proposals(agent: &Agent) -> Proposals {
    let config = proposals_config(agent);
    Proposals::new(ProposalPorts {
        store: agent.proposals.store.clone(),
        codec: agent.proposals.codec.clone(),
        config: config.clone(),
        drafts: config,
        writes: Arc::default(),
        changed: app::schedule::config_changes().0,
        events: agent.proposals.events.clone(),
        clock: agent.clock.clone(),
        random: agent.proposals.random.clone(),
        revision_key: Ok(domain::planning::RevisionKey::new(
            [5; domain::planning::REVISION_KEY_BYTES],
        )),
    })
}

/// The configuration the scenario's proposals are checked against.
fn proposals_config(agent: &Agent) -> Arc<InMemoryConfigStore> {
    agent
        .proposals
        .config
        .clone()
        .unwrap_or_else(|| agent.scheduler.config())
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

/// The configuration file's revision as it is now.
async fn revision(config: &InMemoryConfigStore) -> ConfigRevision {
    let loaded = config.load().await.expect("the configuration loads");
    config
        .draft(&loaded)
        .await
        .expect("the configuration drafts")
        .revision
}

async fn propose_with(
    agent: &Agent,
    proposer: u8,
    changes: Vec<ConfigChange>,
    rationale: Rationale,
) -> Result<Made, ProposalError> {
    let base = revision(&proposals_config(agent)).await;
    proposals(agent)
        .propose(ai_agent(proposer), base, changes, rationale)
        .await
}

async fn propose(
    agent: &Agent,
    proposer: u8,
    changes: Vec<ConfigChange>,
) -> Result<Proposal, ProposalError> {
    propose_with(agent, proposer, changes, Rationale::default())
        .await
        .map(|made| made.proposal)
}

/// Keeps what the AI agent's proposal came to.
fn made(agent: &mut Agent, made: Result<Made, ProposalError>) {
    agent.proposals.warnings = made
        .as_ref()
        .map(|made| made.warnings.clone())
        .unwrap_or_default();
    agent.proposals.made = Some(made.map(|made| made.proposal));
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

/// The plan `name` backing up `source` alone.
fn backing_up(name: &str, source: &str) -> Plan {
    let mut plan = plan_named(name);
    plan.sources = vec![Source::parse(source).expect("the scenario's source is valid")];
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
    let result = propose_with(agent, THE_AI_AGENT, adding(&name), Rationale::default()).await;
    made(agent, result);
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

/// The browser a person reviews proposals from.
const HOME: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The configuration as it is now.
async fn loaded(agent: &Agent) -> domain::planning::AgentConfig {
    agent
        .scheduler
        .config()
        .load()
        .await
        .expect("the configuration loads")
}

#[given("a person has opened the proposal")]
#[when("a person opens the proposal")]
async fn opens(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let review = proposals(agent)
        .review(&id)
        .await
        .expect("the page shows the proposal");
    agent.proposals.review = Some(review);
}

/// Approves the AI agent's last proposal as the page showed it, confirmed
/// by `confirmation`.
async fn approve(agent: &mut Agent, confirmation: String) {
    let id = agent.proposals.kept().id.clone();
    let ask = ApprovalAsk {
        shown: agent.proposals.shown().shown.clone(),
        confirmation: domain::access::step_up::StepUpConfirmation::parse(confirmation)
            .expect("a confirmation"),
        address: HOME,
    };
    agent.proposals.approved = Some(
        proposals(agent)
            .approve(&id, ask)
            .await
            .map(|applied| applied.proposal),
    );
}

#[when("the person approves the proposal as it was shown")]
async fn approves(agent: &mut Agent) {
    let short = short_proposal_id(&agent.proposals.kept().id).to_owned();
    approve(agent, short).await;
}

#[when("the person approves the proposal confirming it with another proposal's ID")]
async fn approves_with_another_id(agent: &mut Agent) {
    let other = ProposalId::from_bytes(&[0; TOKEN_BYTES]);
    assert_ne!(
        short_proposal_id(&other),
        short_proposal_id(&agent.proposals.kept().id),
        "the scenario's proposals differ in their short IDs"
    );
    approve(agent, short_proposal_id(&other).to_owned()).await;
}

#[then("the proposal is applied")]
async fn is_applied(agent: &mut Agent) {
    match &agent.proposals.approved {
        Some(Ok(answered)) => assert_eq!(answered.status, ProposalStatus::Applied),
        other => panic!("the proposal was applied, not {other:?}"),
    }
    assert_eq!(status(agent).await, ProposalStatus::Applied);
}

#[then(expr = "the configuration has the plan {string}")]
async fn has_plan(agent: &mut Agent, name: String) {
    assert!(
        loaded(agent).await.plan(&plan_id(&name)).is_some(),
        "the configuration has the plan {name}"
    );
}

#[then("the configuration's history names the proposal as the reason for the save")]
async fn history_names_proposal(agent: &mut Agent) {
    let applied = read(agent).await;
    let history = agent.scheduler.config().history();
    let last = history
        .last()
        .expect("the save kept a version in the history");
    assert!(
        last.contains(applied.id.as_str()),
        "the history's version {last} names the proposal {}",
        applied.id
    );
    assert_eq!(applied.applied_as.as_ref(), Some(last));
}

#[then("the approval is refused as not confirmed")]
fn refused_not_confirmed(agent: &mut Agent) {
    assert_eq!(
        agent.proposals.approval_refusal(),
        &ProposalError::StepUpFailed
    );
}

#[when("a comment is edited in the configuration file")]
fn comment_edited(agent: &mut Agent) {
    agent.scheduler.config().edit_comment("edited by hand");
}

#[given("the configuration file will be edited by hand just before the apply writes it")]
fn edited_before_apply(agent: &mut Agent) {
    agent
        .scheduler
        .config()
        .edit_comment_before_next_save("edited by hand");
}

#[then("the approval is refused because the configuration changed since it was shown")]
fn refused_changed(agent: &mut Agent) {
    assert_eq!(agent.proposals.approval_refusal(), &ProposalError::Changed);
}

#[then("the hand edit is kept")]
fn hand_edit_kept(agent: &mut Agent) {
    assert_eq!(agent.scheduler.config().current_comment(), "edited by hand");
}

#[then("the page says the configuration has changed since the proposal was drafted")]
fn page_says_changed(agent: &mut Agent) {
    assert!(agent.proposals.shown().changed_since_drafted);
}

#[then("the approval is refused because the proposal has expired")]
fn refused_expired(agent: &mut Agent) {
    assert_eq!(
        agent.proposals.approval_refusal(),
        &ProposalError::NotPending(ProposalStatus::Expired)
    );
}

#[then("the approval is refused because the proposal could delete snapshots")]
fn refused_needs_forget_preview(agent: &mut Agent) {
    assert_eq!(
        agent.proposals.approval_refusal(),
        &ProposalError::NeedsForgetPreview
    );
}

#[then("the page says approving the proposal waits for the forget preview")]
fn page_says_needs_forget_preview(agent: &mut Agent) {
    assert!(agent.proposals.shown().needs_forget_preview);
}

#[given(expr = "an AI agent has proposed adding the plans {string} and {string}")]
async fn has_proposed_adding_two(agent: &mut Agent, first: String, second: String) {
    let changes = vec![
        ConfigChange::AddPlan(plan_named(&first)),
        ConfigChange::AddPlan(plan_named(&second)),
    ];
    agent.proposals.made = Some(propose(agent, THE_AI_AGENT, changes).await);
    agent.proposals.kept();
}

/// Rejects the AI agent's last proposal for `reasons`.
async fn reject(agent: &mut Agent, reasons: Vec<String>) {
    let id = agent.proposals.kept().id.clone();
    agent.proposals.rejected = Some(proposals(agent).reject(&id, reasons).await);
}

#[when(expr = "a person rejects the proposal with the reasons {string} and {string}")]
async fn rejects_with(agent: &mut Agent, first: String, second: String) {
    reject(agent, vec![first, second]).await;
}

#[given("a person has rejected the proposal")]
async fn has_rejected(agent: &mut Agent) {
    let changes = agent.proposals.kept().changes.len();
    reject(agent, vec![String::new(); changes]).await;
    assert!(
        matches!(agent.proposals.rejected, Some(Ok(_))),
        "the proposal was rejected: {:?}",
        agent.proposals.rejected
    );
}

#[then("the proposal is rejected")]
async fn is_rejected(agent: &mut Agent) {
    match &agent.proposals.rejected {
        Some(Ok(answered)) => assert_eq!(answered.status, ProposalStatus::Rejected),
        other => panic!("the proposal was rejected, not {other:?}"),
    }
    assert_eq!(status(agent).await, ProposalStatus::Rejected);
}

#[then(expr = "the AI agent reads the reason {string} for change {int} and none for change {int}")]
async fn reads_reasons(agent: &mut Agent, reason: String, given: usize, none: usize) {
    let read = read(agent).await;
    let reasons: Vec<&str> = read
        .rejection
        .reasons()
        .iter()
        .map(|one| one.as_str())
        .collect();
    assert_eq!(reasons.get(given - 1), Some(&reason.as_str()));
    assert_eq!(reasons.get(none - 1), Some(&""));
    assert_eq!(reasons.len(), read.changes.len());
}

/// When the person rejected the proposal.
fn rejected_at(agent: &Agent) -> Timestamp {
    match &agent.proposals.rejected {
        Some(Ok(rejected)) => rejected.decided_at.expect("a rejection is dated"),
        other => panic!("the proposal was rejected, not {other:?}"),
    }
}

#[then(expr = "the proposal is refused until {int} minutes after the rejection")]
fn refused_after_rejection(agent: &mut Agent, minutes: i64) {
    assert_eq!(SignedDuration::from_mins(minutes), QUIET_AFTER_REJECTION);
    let until = rejected_at(agent) + SignedDuration::from_mins(minutes);
    assert_eq!(
        agent.proposals.refusal(),
        &ProposalError::TooSoon(TooSoon::AfterRejection { until })
    );
}

#[given(expr = "an AI agent has made and withdrawn {int} proposals in the last hour")]
async fn made_and_withdrawn(agent: &mut Agent, count: usize) {
    for n in 1..=count {
        let made = propose(agent, THE_AI_AGENT, adding(&format!("demo-{n}")))
            .await
            .unwrap_or_else(|error| panic!("proposal {n} is kept: {error:?}"));
        proposals(agent)
            .withdraw(&ai_agent(THE_AI_AGENT), &made.id)
            .await
            .unwrap_or_else(|error| panic!("proposal {n} is withdrawn: {error:?}"));
        agent.clock.advance(SignedDuration::from_mins(1));
    }
}

#[then(expr = "the proposal is refused as one over the {int} an hour")]
fn refused_hourly(agent: &mut Agent, limit: usize) {
    assert_eq!(limit, MAX_PER_HOUR);
    assert!(
        matches!(
            agent.proposals.refusal(),
            ProposalError::TooSoon(TooSoon::Hourly { .. })
        ),
        "refused as over the hourly limit: {:?}",
        agent.proposals.refusal()
    );
}

#[given(expr = "the plan {string} backs up {string}")]
async fn plan_backs_up(agent: &mut Agent, name: String, source: String) {
    let config = agent.scheduler.config();
    let with_plan = loaded(agent)
        .await
        .apply(&ConfigChange::AddPlan(backing_up(&name, &source)))
        .expect("the configuration takes the plan");
    config
        .save(&with_plan)
        .await
        .expect("the configuration saves");
}

#[given(expr = "an AI agent has proposed removing the plan {string}, with the rationale {string}")]
async fn has_proposed_removing(agent: &mut Agent, name: String, rationale: String) {
    let rationale = Rationale::parse(rationale).expect("the scenario's rationale is valid");
    let changes = vec![ConfigChange::RemovePlan(plan_id(&name))];
    let result = propose_with(agent, THE_AI_AGENT, changes, rationale).await;
    made(agent, result);
    agent.proposals.kept();
}

/// The plan `name` as the forget world's configuration holds it.
async fn forget_plan(agent: &Agent, name: &str) -> Plan {
    agent
        .forget
        .config()
        .load()
        .await
        .expect("the configuration loads")
        .plan(&plan_id(name))
        .expect("the scenario gave the plan")
        .clone()
}

/// The AI agent proposes `changes` to the forget world's configuration.
async fn propose_to_the_forget_world(agent: &mut Agent, changes: Vec<ConfigChange>) {
    agent.proposals.config = Some(agent.forget.config());
    let result = propose_with(agent, THE_AI_AGENT, changes, Rationale::default()).await;
    made(agent, result);
    agent.proposals.kept();
}

fn keep_last(plan: &mut Plan, keep: i64) {
    plan.retention.keep_last = Some(KeepCount::new(keep).expect("a count"));
}

#[given(expr = "an AI agent has proposed keeping the last {int} snapshot(s) of the plan {string}")]
async fn has_proposed_keeping(agent: &mut Agent, keep: i64, name: String) {
    let mut plan = forget_plan(agent, &name).await;
    keep_last(&mut plan, keep);
    propose_to_the_forget_world(agent, vec![ConfigChange::ChangePlan(plan)]).await;
}

#[given(
    expr = "an AI agent has proposed adding the plan {string} keeping the last {int} snapshot(s)"
)]
async fn has_proposed_adding_keeping(agent: &mut Agent, name: String, keep: i64) {
    let mut plan = Plan::new(
        plan_id(&name),
        RepositoryAlias::parse("demo").expect("an alias"),
        vec![Source::parse(&format!("/home/demo/{name}")).expect("a source")],
    );
    keep_last(&mut plan, keep);
    propose_to_the_forget_world(agent, vec![ConfigChange::AddPlan(plan)]).await;
}

#[given(expr = "an AI agent has proposed removing the plan {string}")]
async fn has_proposed_removing_alone(agent: &mut Agent, name: String) {
    propose_to_the_forget_world(agent, vec![ConfigChange::RemovePlan(plan_id(&name))]).await;
}

#[given(expr = "an AI agent has proposed giving the plan {string} the label {string}")]
async fn has_proposed_labelling(agent: &mut Agent, name: String, label: String) {
    let mut plan = forget_plan(agent, &name).await;
    plan.label = Some(label);
    propose_to_the_forget_world(agent, vec![ConfigChange::ChangePlan(plan)]).await;
}

#[when("a person previews what the proposal would forget")]
async fn previews_forget(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let maintenance = super::forget_steps::use_case(agent);
    let op = OperationId::from_uuid(domain::Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_0f0f,
    ));
    agent.proposals.forget = Some(proposals(agent).forget_preview(&id, op, &maintenance).await);
}

/// The forget preview of the one repository the proposal touches, which
/// was read.
fn compared(agent: &Agent) -> &ForgetComparison {
    let previewed = match &agent.proposals.forget {
        Some(Ok(previewed)) => previewed,
        other => panic!("the forget preview was answered, not {other:?}"),
    };
    let [only] = previewed.as_slice() else {
        panic!("the preview covers the one repository: {previewed:?}");
    };
    match &only.forget {
        Ok(comparison) => comparison,
        Err(error) => panic!("the repository was read, not refused: {error:?}"),
    }
}

/// What `side` of the comparison removes of the plan `name`, and how many
/// snapshots it owns there; none of a plan with no history on that side.
fn removes_of(side: Option<&ForgetPreview>, name: &str) -> Option<(usize, usize)> {
    side.expect("this agent maintains the repository")
        .plans
        .iter()
        .find(|plan| plan.plan.as_str() == name)
        .map(|plan| (plan.remove.len(), plan.total))
}

fn side<'a>(comparison: &'a ForgetComparison, when: &str) -> Option<&'a ForgetPreview> {
    side_named(comparison, when).preview()
}

fn side_named<'a>(comparison: &'a ForgetComparison, when: &str) -> &'a ForgetSide {
    match when {
        "before" => &comparison.before,
        "after" => &comparison.after,
        other => panic!("before or after, not {other}"),
    }
}

#[then(
    regex = r#"^(before|after) the proposal, a forget would be refused because the policy of the plan "([^"]*)" keeps nothing$"#
)]
fn would_be_refused(agent: &mut Agent, when: String, name: String) {
    let comparison = compared(agent);
    assert_eq!(
        side_named(comparison, &when),
        &ForgetSide::KeepsNothing {
            plan: plan_id(&name)
        },
        "{comparison:?}"
    );
}

#[then(
    regex = r#"^(before|after) the proposal, retention would forget (\d+) of the (\d+) snapshots of the plan "([^"]*)"$"#
)]
fn would_forget(agent: &mut Agent, when: String, remove: usize, total: usize, name: String) {
    let comparison = compared(agent);
    assert_eq!(
        removes_of(side(comparison, &when), &name),
        Some((remove, total)),
        "{comparison:?}"
    );
}

#[then(
    regex = r#"^(before|after) the proposal, retention would forget nothing of the plan "([^"]*)"$"#
)]
fn would_forget_nothing(agent: &mut Agent, when: String, name: String) {
    let comparison = compared(agent);
    let removes = removes_of(side(comparison, &when), &name);
    assert!(
        removes.is_none_or(|(remove, _)| remove == 0),
        "{comparison:?}"
    );
}

#[then(
    expr = "after the proposal, the repository leaves alone {int} group(s) of orphaned history holding {int} snapshots"
)]
fn leaves_orphaned_alone_after(agent: &mut Agent, groups: usize, snapshots: usize) {
    let after = compared(agent)
        .after
        .preview()
        .expect("this agent maintains the repository");
    assert_eq!(
        (
            after.left_alone.orphaned.groups,
            after.left_alone.orphaned.snapshots
        ),
        (groups, snapshots)
    );
}

/// How many snapshots of the plan `name` the difference `planned` names.
fn of_the_plan(planned: &[PlanForget], name: &str) -> usize {
    planned
        .iter()
        .filter(|plan| plan.plan.as_str() == name)
        .map(|plan| plan.remove.len())
        .sum()
}

#[then(
    expr = "the proposal would forget {int} snapshots of the plan {string} that nothing forgets now"
)]
fn forgets_more(agent: &mut Agent, count: usize, name: String) {
    let comparison = compared(agent);
    assert_eq!(of_the_plan(&comparison.forgets_more(), &name), count);
    assert!(comparison.forgets_less().is_empty(), "{comparison:?}");
}

#[then(expr = "the proposal would no longer forget {int} snapshots of the plan {string}")]
fn forgets_less(agent: &mut Agent, count: usize, name: String) {
    let comparison = compared(agent);
    assert_eq!(of_the_plan(&comparison.forgets_less(), &name), count);
    assert!(comparison.forgets_more().is_empty(), "{comparison:?}");
}

#[then(expr = "the preview covers the repository {string}")]
fn covers(agent: &mut Agent, repository: String) {
    let Some(Ok(previewed)) = &agent.proposals.forget else {
        panic!(
            "the forget preview was answered: {:?}",
            agent.proposals.forget
        );
    };
    let covered: Vec<&str> = previewed
        .iter()
        .map(|one| one.repository.as_str())
        .collect();
    assert_eq!(covered, [repository.as_str()]);
}

#[then("the proposal changes nothing retention forgets")]
fn changes_nothing(agent: &mut Agent) {
    let comparison = compared(agent);
    assert!(comparison.forgets_more().is_empty(), "{comparison:?}");
    assert!(comparison.forgets_less().is_empty(), "{comparison:?}");
}

#[then(
    expr = "the preview says this agent doesn't maintain the repository {string}, so its retention forgets nothing there before or after the proposal"
)]
fn not_maintained_here(agent: &mut Agent, repository: String) {
    covers(agent, repository);
    assert_eq!(
        compared(agent),
        &ForgetComparison {
            before: ForgetSide::NotMaintained,
            after: ForgetSide::NotMaintained
        }
    );
}

#[then("the engine was never asked to list the repository")]
fn never_listed(agent: &mut Agent) {
    assert!(
        agent.forget.jobs().is_empty(),
        "the engine was given {:?}",
        agent.forget.jobs()
    );
}

#[then("the forget preview is refused because the proposal is withdrawn")]
fn preview_refused_withdrawn(agent: &mut Agent) {
    assert_eq!(
        agent.proposals.forget,
        Some(Err(ProposalError::NotPending(ProposalStatus::Withdrawn)))
    );
}

#[then(expr = "the summary says the proposal removes the plan {string}")]
fn summary_removes(agent: &mut Agent, name: String) {
    assert_eq!(
        agent.proposals.shown().summary,
        [SummaryLine::RemovesPlan(plan_id(&name))]
    );
}

#[when(expr = "an AI agent proposes adding the plan {string} backing up {string}")]
async fn proposes_adding_backing_up(agent: &mut Agent, name: String, source: String) {
    let changes = vec![ConfigChange::AddPlan(backing_up(&name, &source))];
    let result = propose_with(agent, THE_AI_AGENT, changes, Rationale::default()).await;
    made(agent, result);
}

#[then(
    expr = "the AI agent is warned that the plans {string} and {string} back up the same sources"
)]
fn warned_same_sources(agent: &mut Agent, plan: String, other: String) {
    assert_eq!(
        agent.proposals.warnings,
        [ProposalWarning::SameSources {
            index: 0,
            plan: plan_id(&plan),
            other: plan_id(&other),
        }]
    );
}

/// The events announcing a change of the AI agent's last proposal, oldest
/// first.
fn announced(agent: &Agent) -> Vec<UiEvent> {
    let id = agent.proposals.kept().id.to_string();
    agent
        .proposals
        .events
        .published()
        .into_iter()
        .filter(
            |event| matches!(event, UiEvent::ProposalChanged { proposal, .. } if *proposal == id),
        )
        .collect()
}

#[then("an event announces the proposal as withdrawn, with nothing of its rationale")]
fn announced_withdrawn(agent: &mut Agent) {
    // The event names the proposal and its status, nothing else: no
    // field could carry the rationale.
    assert_eq!(
        announced(agent).last(),
        Some(&UiEvent::ProposalChanged {
            proposal: agent.proposals.kept().id.to_string(),
            status: ProposalStatusDto::Withdrawn,
        })
    );
}

/// Read from the store and the events alone: a read of the proposal
/// through the use case would bring it up to date itself.
#[then("the proposal is stale and announced as stale, before anyone reads it")]
async fn stale_unread(agent: &mut Agent) {
    let id = agent.proposals.kept().id.clone();
    let kept = agent
        .proposals
        .store
        .get(&id)
        .await
        .expect("the store reads");
    assert_eq!(kept.map(|one| one.status), Some(ProposalStatus::Stale));
    assert_eq!(
        announced(agent).last(),
        Some(&UiEvent::ProposalChanged {
            proposal: id.to_string(),
            status: ProposalStatusDto::Stale,
        })
    );
}
