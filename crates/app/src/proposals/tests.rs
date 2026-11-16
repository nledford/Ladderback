use std::collections::BTreeMap;

use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::planning::{
    AgentConfig, ChangeRefused, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, Source,
};
use domain::secrets::SecretRef;
use domain::{PlanId, RepositoryAlias, SignedDuration, Timestamp};

use super::*;
use crate::fakes::{
    FakeChangeCodec, FakeClock, InMemoryConfigStore, InMemoryProposalStore, SeededRandom,
};
use crate::ports::Added;

struct World {
    proposals: Proposals,
    store: Arc<InMemoryProposalStore>,
    config: Arc<InMemoryConfigStore>,
    clock: Arc<FakeClock>,
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").unwrap()
}

fn plan(name: &str) -> Plan {
    Plan::new(
        PlanId::parse(name).unwrap(),
        alias(),
        vec![Source::parse(&format!("/home/demo/{name}")).unwrap()],
    )
}

async fn world() -> World {
    let entry = RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: RepositorySettings {
            repository: "/srv/demo-repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let config = Arc::new(InMemoryConfigStore::new());
    config
        .save(
            &AgentConfig::new(
                Default::default(),
                Hooks::default(),
                BTreeMap::from([(alias(), entry)]),
                Vec::new(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let store = Arc::new(InMemoryProposalStore::new());
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let proposals = Proposals::new(
        store.clone(),
        Arc::new(FakeChangeCodec::new()),
        config.clone(),
        clock.clone(),
        Arc::new(SeededRandom::new(7)),
    );
    World {
        proposals,
        store,
        config,
        clock,
    }
}

async fn propose(world: &World, change: ConfigChange) -> Result<Proposal, ProposalError> {
    propose_as(world, Proposer::NoToken, change).await
}

async fn propose_as(
    world: &World,
    proposer: Proposer,
    change: ConfigChange,
) -> Result<Proposal, ProposalError> {
    world
        .proposals
        .propose(
            proposer,
            ConfigRevision::of(b"base"),
            vec![change],
            Rationale::default(),
        )
        .await
}

fn token(id: &str) -> Proposer {
    Proposer::Token {
        id: ApiTokenId::parse(id).unwrap(),
        label: ApiTokenLabel::new(format!("demo-agent-{id}")).unwrap(),
    }
}

#[tokio::test]
async fn a_refused_proposal_keeps_nothing_and_changes_nothing() {
    let world = world().await;
    let before = world.config.load().await.unwrap();
    let refused = propose(&world, ConfigChange::ChangePlan(plan("home"))).await;
    assert_eq!(
        refused,
        Err(ProposalError::Refused(ProposalRefused::Change {
            index: 0,
            refused: ChangeRefused::UnknownPlan(PlanId::parse("home").unwrap()),
        }))
    );
    assert!(world.store.all().await.unwrap().is_empty());
    assert_eq!(world.config.load().await.unwrap(), before);
}

#[tokio::test]
async fn a_kept_proposal_reads_back_with_its_changes_and_leaves_the_configuration() {
    let world = world().await;
    let before = world.config.load().await.unwrap();
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    assert_eq!(made.status, ProposalStatus::Pending);
    let read = world
        .proposals
        .get(&Proposer::NoToken, &made.id)
        .await
        .unwrap();
    assert_eq!(read, made);
    assert_eq!(world.config.load().await.unwrap(), before);
}

#[tokio::test]
async fn reading_an_expired_proposal_keeps_it_expired() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    world.clock.advance(SignedDuration::from_hours(48));
    let listed = world.proposals.list(&Proposer::NoToken).await.unwrap();
    assert_eq!(
        listed.iter().map(|one| one.status).collect::<Vec<_>>(),
        [ProposalStatus::Expired]
    );
    let kept = world.store.get(&made.id).await.unwrap().unwrap();
    assert_eq!(kept.status, ProposalStatus::Expired);
}

#[tokio::test]
async fn a_proposal_being_applied_is_not_withdrawn() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    assert!(
        world
            .store
            .change_status(&made.id, ProposalStatus::Pending, ProposalStatus::Applying)
            .await
            .unwrap()
    );
    assert_eq!(
        world.proposals.withdraw(&Proposer::NoToken, &made.id).await,
        Err(ProposalError::NotPending(ProposalStatus::Applying))
    );
}

#[tokio::test]
async fn a_proposal_whose_changes_cant_be_read_back_goes_stale_and_the_others_still_list() {
    let world = world().await;
    let readable = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let now = world.clock.now();
    let garbled = StoredProposal {
        id: ProposalId::parse("bbbbbbbbbbbb").unwrap(),
        proposer: Proposer::NoToken,
        base: ConfigRevision::of(b"base"),
        changes: "garbled".to_owned(),
        rationale: Rationale::default(),
        status: ProposalStatus::Pending,
        created_at: now,
        expires_at: now.saturating_add(SignedDuration::from_hours(48)).unwrap(),
    };
    assert_eq!(
        world
            .store
            .add(&garbled, MAX_PENDING, MAX_ENDED)
            .await
            .unwrap(),
        Added::Kept
    );
    let listed = world.proposals.list(&Proposer::NoToken).await.unwrap();
    let unreadable = listed
        .iter()
        .find(|one| one.id == garbled.id)
        .expect("the unreadable one is listed");
    assert_eq!(unreadable.status, ProposalStatus::Stale);
    assert!(unreadable.changes.is_empty());
    assert!(listed.contains(&readable), "{listed:?}");
    let kept = world.store.get(&garbled.id).await.unwrap().unwrap();
    assert_eq!(kept.status, ProposalStatus::Stale);
}

#[tokio::test]
async fn a_token_sees_only_its_own_proposals_and_no_token_sees_them_all() {
    let world = world().await;
    let by_token = propose_as(
        &world,
        token("aaaaaaaaaaaa"),
        ConfigChange::AddPlan(plan("home")),
    )
    .await
    .unwrap();
    let by_other_token = propose_as(
        &world,
        token("cccccccccccc"),
        ConfigChange::AddPlan(plan("media")),
    )
    .await
    .unwrap();
    let by_no_token = propose(&world, ConfigChange::AddPlan(plan("work")))
        .await
        .unwrap();

    let seen_by_token = world.proposals.list(&token("aaaaaaaaaaaa")).await.unwrap();
    assert_eq!(seen_by_token, std::slice::from_ref(&by_token));
    assert_eq!(
        world
            .proposals
            .get(&token("aaaaaaaaaaaa"), &by_no_token.id)
            .await,
        Err(ProposalError::Unknown)
    );
    assert_eq!(
        world
            .proposals
            .withdraw(&token("aaaaaaaaaaaa"), &by_no_token.id)
            .await,
        Err(ProposalError::Unknown)
    );

    let seen_by_no_token = world.proposals.list(&Proposer::NoToken).await.unwrap();
    assert_eq!(seen_by_no_token.len(), 3);
    assert!(seen_by_no_token.contains(&by_token));
    assert!(seen_by_no_token.contains(&by_other_token));
    assert!(seen_by_no_token.contains(&by_no_token));
    assert_eq!(
        world.proposals.get(&Proposer::NoToken, &by_token.id).await,
        Ok(by_token)
    );
}
