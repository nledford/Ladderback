use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};

use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::planning::Location;
use domain::planning::{
    AgentConfig, ChangeRefused, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, Source, short_proposal_id,
};
use domain::secrets::SecretRef;
use domain::{PlanId, RepositoryAlias, SignedDuration, Timestamp};
use futures::StreamExt as _;

use super::*;
use crate::fakes::{
    FakeChangeCodec, FakeClock, InMemoryConfigStore, InMemoryEventBus, InMemoryProposalStore,
    SeededRandom,
};

struct World {
    proposals: Arc<Proposals>,
    store: Arc<InMemoryProposalStore>,
    codec: Arc<FakeChangeCodec>,
    config: Arc<InMemoryConfigStore>,
    clock: Arc<FakeClock>,
    events: Arc<InMemoryEventBus>,
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
    let events = Arc::new(InMemoryEventBus::new());
    let codec = Arc::new(FakeChangeCodec::new());
    World {
        proposals: Arc::new(Proposals::new(ProposalPorts {
            store: store.clone(),
            codec: codec.clone(),
            config: config.clone(),
            drafts: config.clone(),
            writes: Arc::default(),
            changed: crate::schedule::config_changes().0,
            events: events.clone(),
            clock: clock.clone(),
            random: Arc::new(SeededRandom::new(7)),
            revision_key: Ok(key(3)),
        })),
        store,
        codec,
        config,
        clock,
        events,
    }
}

fn key(byte: u8) -> RevisionKey {
    RevisionKey::new([byte; domain::planning::REVISION_KEY_BYTES])
}

/// The proposals of `world` as the daemon started again holds them, under
/// `revision_key`: the same store, codec and file.
fn restarted(world: &World, revision_key: Result<RevisionKey, RandomUnavailable>) -> Proposals {
    Proposals::new(ProposalPorts {
        store: world.store.clone(),
        codec: world.codec.clone(),
        config: world.config.clone(),
        drafts: world.config.clone(),
        writes: Arc::default(),
        changed: crate::schedule::config_changes().0,
        events: world.events.clone(),
        clock: world.clock.clone(),
        random: Arc::new(SeededRandom::new(7)),
        revision_key,
    })
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
        .map(|made| made.proposal)
}

fn token(id: &str) -> Proposer {
    Proposer::Token {
        id: ApiTokenId::parse(id).unwrap(),
        label: ApiTokenLabel::new(format!("demo-agent-{id}")).unwrap(),
    }
}

/// How long a test waits for an announcement: one never made fails the test
/// here instead of hanging it.
const ANNOUNCED_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// The next announcement on `events`, failing the test when none arrives
/// within [`ANNOUNCED_WITHIN`].
async fn announced(events: &mut futures::stream::BoxStream<'static, UiEvent>) -> UiEvent {
    tokio::time::timeout(ANNOUNCED_WITHIN, events.next())
        .await
        .unwrap_or_else(|_| panic!("nothing was announced within {ANNOUNCED_WITHIN:?}"))
        .expect("the bus outlives its subscribers")
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
        decided_at: None,
        reasons: Vec::new(),
        approved_from: None,
        applied_as: None,
    };
    assert_eq!(
        world
            .store
            .add(&garbled, MAX_PENDING, MAX_ENDED)
            .await
            .unwrap(),
        Admission::Kept
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

const HOME: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The approval of what the page shows of `id` now, confirmed by its short
/// ID, as with sign-in off.
async fn as_shown(world: &World, id: &ProposalId) -> ApprovalAsk {
    let review = world.proposals.review(id).await.unwrap();
    ApprovalAsk {
        shown: review.shown,
        confirmation: short_proposal_id(id).to_owned(),
        address: HOME,
    }
}

#[tokio::test]
async fn a_review_shows_the_diff_it_binds_an_approval_to_and_whether_the_file_changed() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let review = world.proposals.review(&made.id).await.unwrap();
    let config = world.config.load().await.unwrap();
    let draft = world
        .config
        .draft(&Proposal::check(&config, &made.changes).unwrap())
        .await
        .unwrap();
    let diff = unified_diff(&draft.before, &draft.after);
    assert!(diff.contains("+"), "{diff}");
    assert_eq!(review.diff, diff, "nothing here is a secret to mask");
    assert_eq!(
        review.shown,
        Shown {
            revision: ShownRevision::of(&key(3), &draft.revision),
            diff: DiffHash::of(&diff),
        }
    );
    assert_ne!(
        review.shown.revision.as_str(),
        draft.revision.as_str(),
        "never the file's plain revision, a hash of its secrets too"
    );
    assert_eq!(review.summary, [SummaryLine::AddsPlan(plan("home").id)]);
    // Drafted against the revision of "base", which the file never was.
    assert!(review.changed_since_drafted);
    assert_eq!(review.rejected, 0);
}

#[tokio::test]
async fn the_same_file_is_shown_another_revision_after_a_restart() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let before = world.proposals.review(&made.id).await.unwrap();
    let after = restarted(&world, Ok(key(4)))
        .review(&made.id)
        .await
        .unwrap();
    assert_ne!(before.shown.revision, after.shown.revision);
    assert_eq!(before.shown.diff, after.shown.diff);
}

#[tokio::test]
async fn an_approval_of_what_a_page_showed_before_a_restart_changes_nothing() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let ask = as_shown(&world, &made.id).await;
    assert_eq!(
        restarted(&world, Ok(key(4))).approve(&made.id, ask).await,
        Err(ProposalError::Changed)
    );
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(world.config.load().await.unwrap().plans().is_empty());
}

#[tokio::test]
async fn with_no_key_to_show_a_revision_under_a_review_or_an_approval_is_refused() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let ask = as_shown(&world, &made.id).await;
    let unavailable = RandomUnavailable("no entropy".to_owned());
    let keyless = restarted(&world, Err(unavailable.clone()));
    assert_eq!(
        keyless.review(&made.id).await.map(drop),
        Err(ProposalError::Random(unavailable.clone()))
    );
    assert_eq!(
        keyless.approve(&made.id, ask).await.map(drop),
        Err(ProposalError::Random(unavailable))
    );
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(world.config.load().await.unwrap().plans().is_empty());
}

#[tokio::test]
async fn an_approval_applies_the_proposal_tells_the_scheduler_and_announces_each_step() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let ask = as_shown(&world, &made.id).await;
    let mut events = world.events.subscribe().await;
    let applied = world.proposals.approve(&made.id, ask).await.unwrap();
    assert_eq!(applied.status, ProposalStatus::Applied);
    let kept_as = applied.applied_as.clone().unwrap();
    assert!(kept_as.contains(made.id.as_str()), "{kept_as}");
    assert!(
        world
            .config
            .load()
            .await
            .unwrap()
            .plan(&plan("home").id)
            .is_some()
    );
    let kept = world.store.get(&made.id).await.unwrap().unwrap();
    assert_eq!(kept.status, ProposalStatus::Applied);
    assert_eq!(kept.approved_from, Some(HOME));
    assert_eq!(kept.decided_at, Some(world.clock.now()));
    let steps = [announced(&mut events).await, announced(&mut events).await];
    assert_eq!(
        steps,
        [
            UiEvent::ProposalChanged {
                proposal: made.id.to_string(),
                status: ProposalStatusDto::Applying,
            },
            UiEvent::ProposalChanged {
                proposal: made.id.to_string(),
                status: ProposalStatusDto::Applied,
            },
        ]
    );
}

#[tokio::test]
async fn an_approval_not_confirmed_changes_nothing() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let mut ask = as_shown(&world, &made.id).await;
    ask.confirmation = "aaaaaa".to_owned();
    assert_eq!(
        world.proposals.approve(&made.id, ask).await,
        Err(ProposalError::StepUpFailed)
    );
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(world.config.load().await.unwrap().plans().is_empty());
}

#[tokio::test]
async fn an_approval_of_another_diff_than_the_one_a_rebuild_gives_changes_nothing() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let mut ask = as_shown(&world, &made.id).await;
    ask.shown.diff = DiffHash::of("+[plans.elsewhere]\n");
    assert_eq!(
        world.proposals.approve(&made.id, ask).await,
        Err(ProposalError::Changed)
    );
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(world.config.load().await.unwrap().plans().is_empty());
}

#[tokio::test]
async fn an_approval_of_a_proposal_that_could_delete_snapshots_changes_nothing() {
    let world = world().await;
    let mut kept = plan("home");
    kept.retention.keep_daily = Some(domain::planning::KeepCount::new(7).unwrap());
    let made = propose(&world, ConfigChange::AddPlan(kept)).await.unwrap();
    let review = world.proposals.review(&made.id).await.unwrap();
    assert!(review.needs_forget_preview);
    let ask = as_shown(&world, &made.id).await;
    assert_eq!(
        world.proposals.approve(&made.id, ask).await,
        Err(ProposalError::NeedsForgetPreview)
    );
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(world.config.load().await.unwrap().plans().is_empty());
    let plain = propose(&world, ConfigChange::AddPlan(plan("photos")))
        .await
        .unwrap();
    assert!(
        !world
            .proposals
            .review(&plain.id)
            .await
            .unwrap()
            .needs_forget_preview
    );
}

#[tokio::test]
async fn a_plan_saved_brings_the_proposals_up_to_date_and_announces_those_gone_stale() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let mut events = world.events.subscribe().await;
    let edits = crate::plans::EditPlans::new(
        world.config.clone(),
        Arc::default(),
        crate::schedule::config_changes().0,
    )
    .with_proposals(world.proposals.clone());
    edits.create(plan("home")).await.unwrap();
    // Kept stale by the save itself, before anyone reads it.
    assert_eq!(
        world.store.get(&made.id).await.unwrap().unwrap().status,
        ProposalStatus::Stale
    );
    assert_eq!(
        announced(&mut events).await,
        UiEvent::ProposalChanged {
            proposal: made.id.to_string(),
            status: ProposalStatusDto::Stale,
        }
    );
}

#[tokio::test]
async fn a_hand_edit_during_the_apply_is_kept_and_the_proposal_is_pending_again() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let ask = as_shown(&world, &made.id).await;
    world.config.edit_comment_before_next_save("edited by hand");
    assert_eq!(
        world.proposals.approve(&made.id, ask).await,
        Err(ProposalError::Changed)
    );
    assert_eq!(world.config.current_comment(), "edited by hand");
    assert!(world.config.load().await.unwrap().plans().is_empty());
    let kept = world.store.get(&made.id).await.unwrap().unwrap();
    assert_eq!(kept.status, ProposalStatus::Pending);
}

#[tokio::test]
async fn only_a_pending_proposal_is_approved_or_rejected() {
    let world = world().await;
    let made = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let ask = as_shown(&world, &made.id).await;
    world
        .proposals
        .withdraw(&Proposer::NoToken, &made.id)
        .await
        .unwrap();
    assert_eq!(
        world.proposals.approve(&made.id, ask).await,
        Err(ProposalError::NotPending(ProposalStatus::Withdrawn))
    );
    assert_eq!(
        world.proposals.reject(&made.id, vec![String::new()]).await,
        Err(ProposalError::NotPending(ProposalStatus::Withdrawn))
    );
}

#[tokio::test]
async fn a_rejection_keeps_a_reason_per_change_and_the_proposer_reads_it() {
    let world = world().await;
    let made = propose_as(
        &world,
        token("aaaaaaaaaaaa"),
        ConfigChange::AddPlan(plan("home")),
    )
    .await
    .unwrap();
    assert_eq!(
        world.proposals.reject(&made.id, Vec::new()).await,
        Err(ProposalError::Rejection(RejectionRefused::Count {
            changes: 1
        }))
    );
    let mut events = world.events.subscribe().await;
    let rejected = world
        .proposals
        .reject(&made.id, vec!["Back up weekly instead".to_owned()])
        .await
        .unwrap();
    assert_eq!(rejected.status, ProposalStatus::Rejected);
    let read = world
        .proposals
        .get(&token("aaaaaaaaaaaa"), &made.id)
        .await
        .unwrap();
    assert_eq!(
        read.rejection
            .reasons()
            .iter()
            .map(|reason| reason.as_str())
            .collect::<Vec<_>>(),
        ["Back up weekly instead"]
    );
    assert_eq!(read.decided_at, Some(world.clock.now()));
    assert_eq!(
        announced(&mut events).await,
        UiEvent::ProposalChanged {
            proposal: made.id.to_string(),
            status: ProposalStatusDto::Rejected,
        }
    );
    let again = propose_as(
        &world,
        token("aaaaaaaaaaaa"),
        ConfigChange::AddPlan(plan("home")),
    )
    .await;
    assert!(
        matches!(
            again,
            Err(ProposalError::TooSoon(TooSoon::AfterRejection { .. }))
        ),
        "{again:?}"
    );
    let review = world.proposals.review(&made.id).await.unwrap();
    assert_eq!(review.rejected, 1);
    assert_eq!(review.diff, "");
}

#[tokio::test]
async fn a_review_counts_only_its_own_proposers_rejections() {
    let world = world().await;
    let rejected = propose_as(
        &world,
        token("aaaaaaaaaaaa"),
        ConfigChange::AddPlan(plan("home")),
    )
    .await
    .unwrap();
    world
        .proposals
        .reject(&rejected.id, vec![String::new()])
        .await
        .unwrap();
    let other = propose_as(
        &world,
        token("bbbbbbbbbbbb"),
        ConfigChange::AddPlan(plan("photos")),
    )
    .await
    .unwrap();
    let untokened = propose(&world, ConfigChange::AddPlan(plan("music")))
        .await
        .unwrap();
    for made in [&other, &untokened] {
        assert_eq!(world.proposals.review(&made.id).await.unwrap().rejected, 0);
    }
    assert_eq!(
        world.proposals.review(&rejected.id).await.unwrap().rejected,
        1
    );
    // A client with no token is one proposer too. A rejection holds that
    // proposer back, so its second proposal comes first.
    let untokened_again = propose(&world, ConfigChange::AddPlan(plan("videos")))
        .await
        .unwrap();
    world
        .proposals
        .reject(&untokened.id, vec![String::new()])
        .await
        .unwrap();
    assert_eq!(
        world
            .proposals
            .review(&untokened_again.id)
            .await
            .unwrap()
            .rejected,
        1
    );
}

#[tokio::test]
async fn a_proposal_kept_comes_with_its_warnings() {
    let world = world().await;
    let mut copy = plan("copy");
    copy.sources = plan("home").sources;
    let made = world
        .proposals
        .propose(
            Proposer::NoToken,
            ConfigRevision::of(b"base"),
            vec![
                ConfigChange::AddPlan(plan("home")),
                ConfigChange::AddPlan(copy),
            ],
            Rationale::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        made.warnings,
        [ProposalWarning::SameSources {
            index: 1,
            plan: plan("copy").id,
            other: plan("home").id,
        }]
    );
}

#[tokio::test]
async fn an_expiry_found_by_a_read_and_a_withdrawal_are_announced() {
    let world = world().await;
    let first = propose(&world, ConfigChange::AddPlan(plan("home")))
        .await
        .unwrap();
    let mut events = world.events.subscribe().await;
    world
        .proposals
        .withdraw(&Proposer::NoToken, &first.id)
        .await
        .unwrap();
    assert_eq!(
        announced(&mut events).await,
        UiEvent::ProposalChanged {
            proposal: first.id.to_string(),
            status: ProposalStatusDto::Withdrawn,
        }
    );
    let second = propose(&world, ConfigChange::AddPlan(plan("media")))
        .await
        .unwrap();
    assert_eq!(
        announced(&mut events).await,
        UiEvent::ProposalChanged {
            proposal: second.id.to_string(),
            status: ProposalStatusDto::Pending,
        }
    );
    world.clock.advance(SignedDuration::from_hours(48));
    world.proposals.list(&Proposer::NoToken).await.unwrap();
    assert_eq!(
        announced(&mut events).await,
        UiEvent::ProposalChanged {
            proposal: second.id.to_string(),
            status: ProposalStatusDto::Expired,
        }
    );
}

#[test]
fn the_diff_shown_masks_every_secret_of_each_repository_and_location() {
    let settings = RepositorySettings {
        repository: "rest:https://demo:canary-url@example.com/repo".to_owned(),
        repo_hot: None,
        password: SecretRef::Inline(domain::secrets::SecretValue::new("canary-password")),
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
    };
    let further = Location {
        options: BTreeMap::from([("access-key".to_owned(), "canary-key".to_owned())]),
        ..Location::of(&settings)
    };
    let entry = RepositoryConfig {
        locations: domain::planning::Locations {
            first_when: None,
            further: vec![further],
        },
        id: None,
        settings,
        maintenance: Maintenance::None,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    };
    let config = AgentConfig::new(
        Default::default(),
        Hooks::default(),
        BTreeMap::from([(alias(), entry)]),
        Vec::new(),
    )
    .unwrap();
    let before = "url = \"canary-url\"\npassword = \"canary-password\"\n";
    let after = format!("{before}key = \"canary-key\"\n");
    let diff = masked_diff(&config, before, &after);
    assert!(diff.contains("+key"), "{diff}");
    assert!(!diff.contains("canary"), "{diff}");
}
