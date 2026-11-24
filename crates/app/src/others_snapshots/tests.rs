use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::maintenance::RepositoryReport;
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, RepositoryConfig, RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{
    AgentId, OperationId, RepositoryAlias, RepositoryId, SignedDuration, SnapshotId, Timestamp,
    Uuid,
};
use futures::StreamExt;

use super::{ListingsDue, NoteListings, OthersSnapshots};
use crate::fakes::{
    FakeClock, FakeSecrets, InMemoryConfigStore, InMemoryOperationStore, ListingEngine,
    SeededRandom,
};
use crate::ports::{
    Clock as _, ConfigStore as _, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    OperationStore as _, RepositoryAccess, RunningJob, SnapshotSummary, Start,
};
use crate::secrets::ResolveRepository;
use crate::snapshots::ListSnapshots;

fn machine() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(1))
}

fn another_machine() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(2))
}

fn photos() -> RepositoryId {
    RepositoryId::parse(&"a".repeat(64)).unwrap()
}

fn music() -> RepositoryId {
    RepositoryId::parse(&"b".repeat(64)).unwrap()
}

fn videos() -> RepositoryId {
    RepositoryId::parse(&"c".repeat(64)).unwrap()
}

fn letters() -> RepositoryId {
    RepositoryId::parse(&"d".repeat(64)).unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn days(count: i64) -> SignedDuration {
    SignedDuration::from_hours(count * 24)
}

const NANOSECOND: SignedDuration = SignedDuration::from_nanos(1);

/// A snapshot `agent`'s plan `home` made at `time`.
fn made_by(agent: &AgentId, time: Timestamp) -> SnapshotSummary {
    SnapshotSummary {
        id: SnapshotId::parse(&format!("{:064x}", time.as_second())).unwrap(),
        time,
        offset: domain::Offset::UTC,
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: vec![
            format!("{PLAN_TAG_PREFIX}home"),
            format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
        ],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
        tags_truncated: false,
    }
}

/// Whether [`machine`] reports `repository` as holding another agent's
/// snapshots at `at`.
fn holds(seen: &OthersSnapshots, repository: RepositoryId, at: Timestamp) -> bool {
    seen.report(repository, Maintenance::Local, &machine(), at)
        .holds_other_agents_snapshots
}

#[test]
fn i1_a_repository_never_listed_is_reported_as_holding_another_agents_snapshots() {
    let seen = OthersSnapshots::default();
    assert_eq!(
        seen.report(photos(), Maintenance::Local, &machine(), now()),
        RepositoryReport {
            repository: photos(),
            request: Maintenance::Local,
            holds_other_agents_snapshots: true,
        }
    );
    assert!(seen.due(&photos(), &machine(), now()));
}

#[test]
fn a_listed_repository_is_reported_with_its_request_and_what_its_listing_showed() {
    let seen = OthersSnapshots::default();
    seen.listed(photos(), machine(), now(), &[made_by(&machine(), now())]);
    for request in [Maintenance::Local, Maintenance::Hub, Maintenance::None] {
        assert_eq!(
            seen.report(photos(), request, &machine(), now()),
            RepositoryReport {
                repository: photos(),
                request,
                holds_other_agents_snapshots: false,
            }
        );
    }
    assert!(!seen.due(&photos(), &machine(), now()));
}

#[test]
fn i1_another_agents_snapshot_counts_until_it_is_30_days_old_without_a_new_listing() {
    let seen = OthersSnapshots::default();
    let half_a_day = SignedDuration::from_hours(12);
    seen.listed(
        photos(),
        machine(),
        now(),
        &[
            made_by(&machine(), now()),
            made_by(&another_machine(), now() - days(30) + half_a_day),
        ],
    );
    assert!(holds(&seen, photos(), now()));
    assert!(holds(&seen, photos(), now() + half_a_day));
    assert!(!holds(&seen, photos(), now() + half_a_day + NANOSECOND));
}

#[test]
fn i1_a_listing_stands_for_a_day_and_is_then_reported_as_holding_another_agents_snapshots() {
    let seen = OthersSnapshots::default();
    seen.listed(photos(), machine(), now(), &[made_by(&machine(), now())]);
    let day = days(1);
    for (at, stands) in [
        (now() + day, true),
        (now() + day + NANOSECOND, false),
        (now() - day, true),
        (now() - day - NANOSECOND, false),
    ] {
        assert_eq!(holds(&seen, photos(), at), !stands, "{at}");
        assert_eq!(seen.due(&photos(), &machine(), at), !stands, "{at}");
    }
}

#[test]
fn i1_a_listing_judged_for_another_machine_id_counts_as_none() {
    let seen = OthersSnapshots::default();
    seen.listed(
        photos(),
        another_machine(),
        now(),
        &[made_by(&machine(), now())],
    );
    assert!(holds(&seen, photos(), now()));
    assert!(seen.due(&photos(), &machine(), now()));
}

#[test]
fn a_later_listing_replaces_an_earlier_one_as_after_an_adoption() {
    let seen = OthersSnapshots::default();
    seen.listed(
        photos(),
        machine(),
        now(),
        &[made_by(&another_machine(), now())],
    );
    assert!(holds(&seen, photos(), now()));
    // The adoption rewrote the earlier installation's history under this
    // machine's tags (decision 62).
    seen.listed(photos(), machine(), now(), &[made_by(&machine(), now())]);
    assert!(!holds(&seen, photos(), now()));
}

#[test]
fn each_repository_keeps_its_own_listing() {
    let seen = OthersSnapshots::default();
    seen.listed(photos(), machine(), now(), &[]);
    assert!(!holds(&seen, photos(), now()));
    assert!(holds(&seen, music(), now()));
}

/// An engine that answers every job with `events`.
struct Answers(Vec<EngineEvent>);

struct Stop;

#[async_trait]
impl JobControl for Stop {
    async fn stop(&mut self) {}
}

#[async_trait]
impl Engine for Answers {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        Ok(RunningJob {
            events: futures::stream::iter(self.0.clone()).boxed(),
            control: Box::new(Stop),
        })
    }
}

fn access() -> RepositoryAccess {
    RepositoryAccess {
        repository: "/srv/photos".to_owned(),
        repo_hot: None,
        password: SecretValue::new("demo-password"),
        options: BTreeMap::new(),
        options_hot: BTreeMap::new(),
        options_cold: BTreeMap::new(),
        secrets: Vec::new(),
        id: None,
        entry: None,
    }
}

fn started(repository: Option<RepositoryId>) -> EngineEvent {
    EngineEvent::Started {
        index_read_at: now(),
        repository,
    }
}

/// Runs `job` on an engine answering `events` through [`NoteListings`]
/// into `seen`, on a clock at [`now`], and returns the events that passed.
async fn run_noted(
    seen: &Arc<OthersSnapshots>,
    job: Job,
    events: Vec<EngineEvent>,
) -> Vec<EngineEvent> {
    let noting = NoteListings::new(
        Arc::new(Answers(events)),
        Arc::clone(seen),
        machine(),
        Arc::new(FakeClock::new(now())),
    );
    let running = noting
        .run(OperationId::from_uuid(Uuid::from_u128(7)), job)
        .await
        .unwrap();
    running.events.collect().await
}

#[tokio::test]
async fn a_listing_is_noted_for_the_repository_it_opened_as_of_its_start() {
    let seen = Arc::new(OthersSnapshots::default());
    let events = vec![
        started(Some(photos())),
        EngineEvent::Succeeded(EngineResult::Snapshots(vec![made_by(&machine(), now())])),
    ];
    let passed = run_noted(&seen, Job::Ls(access()), events.clone()).await;
    assert_eq!(passed, events);
    assert!(!holds(&seen, photos(), now()));
    assert!(holds(&seen, music(), now()));
    // Noted at the clock's now when it started: it stands a day from then.
    assert!(!holds(&seen, photos(), now() + days(1)));
    assert!(holds(&seen, photos(), now() + days(1) + NANOSECOND));
}

#[tokio::test]
async fn i1_a_listing_that_finds_another_agents_recent_snapshot_is_noted() {
    let seen = Arc::new(OthersSnapshots::default());
    seen.listed(photos(), machine(), now(), &[]);
    run_noted(
        &seen,
        Job::Ls(access()),
        vec![
            started(Some(photos())),
            EngineEvent::Succeeded(EngineResult::Snapshots(vec![made_by(
                &another_machine(),
                now() - days(3),
            )])),
        ],
    )
    .await;
    assert!(holds(&seen, photos(), now()));
}

#[tokio::test]
async fn a_failed_listing_one_that_opened_no_repository_and_any_other_job_note_nothing() {
    let others = || {
        EngineEvent::Succeeded(EngineResult::Snapshots(vec![made_by(
            &another_machine(),
            now(),
        )]))
    };
    for (job, events) in [
        (
            Job::Ls(access()),
            vec![
                started(Some(photos())),
                EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "the listing failed".to_owned(),
                }),
            ],
        ),
        (Job::Ls(access()), vec![started(None), others()]),
        (
            Job::Identify(access()),
            vec![started(Some(photos())), others()],
        ),
    ] {
        let seen = Arc::new(OthersSnapshots::default());
        seen.listed(photos(), machine(), now(), &[]);
        run_noted(&seen, job, events).await;
        assert!(!holds(&seen, photos(), now()));
    }
}

/// An entry of the repository `id`, under the alias `alias`, asking for
/// `maintenance`.
fn entry(alias: &str, id: Option<RepositoryId>, maintenance: Maintenance) -> RepositoryConfig {
    RepositoryConfig {
        locations: Default::default(),
        id,
        settings: RepositorySettings {
            repository: format!("/srv/{alias}"),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        },
        maintenance,
        group_by: GroupBy::default(),
        hooks: Hooks::default(),
    }
}

/// A machine's listings before its syncs, of a configuration that asks to
/// maintain `photos` and `music` here, `videos` by the hub, and `letters`
/// nowhere, and holds an entry asking to maintain here whose repository ID
/// isn't known yet.
struct Due {
    listings: ListingsDue,
    engine: Arc<ListingEngine>,
    seen: Arc<OthersSnapshots>,
    clock: Arc<FakeClock>,
    operations: Arc<InMemoryOperationStore>,
}

async fn due() -> Due {
    let repositories = [
        ("photos", Some(photos()), Maintenance::Local),
        ("music", Some(music()), Maintenance::Local),
        ("videos", Some(videos()), Maintenance::Hub),
        ("letters", Some(letters()), Maintenance::None),
        ("unopened", None, Maintenance::Local),
    ]
    .into_iter()
    .map(|(alias, id, maintenance)| {
        (
            RepositoryAlias::parse(alias).unwrap(),
            entry(alias, id, maintenance),
        )
    })
    .collect();
    let config = Arc::new(InMemoryConfigStore::new());
    config
        .save(
            &AgentConfig::new(
                AgentSettings::default(),
                Hooks::default(),
                repositories,
                Vec::new(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let engine = Arc::new(ListingEngine::default());
    let seen = Arc::new(OthersSnapshots::default());
    let clock = Arc::new(FakeClock::new(now()));
    let operations = Arc::new(InMemoryOperationStore::new());
    let listing = ListSnapshots::new(
        Arc::new(NoteListings::new(
            engine.clone(),
            Arc::clone(&seen),
            machine(),
            clock.clone(),
        )),
        clock.clone(),
        ResolveRepository::new(Arc::new(FakeSecrets::default())),
        Arc::default(),
    );
    let listings = ListingsDue::new(
        config,
        operations.clone(),
        Arc::clone(&seen),
        listing,
        Arc::new(SeededRandom::new(1)),
        machine(),
        clock.clone(),
    );
    Due {
        listings,
        engine,
        seen,
        clock,
        operations,
    }
}

impl Due {
    fn listed(&self) -> [usize; 4] {
        [photos(), music(), videos(), letters()].map(|id| self.engine.listings(&id))
    }

    fn holds(&self, repository: RepositoryId) -> bool {
        holds(&self.seen, repository, self.clock.now())
    }
}

/// Before a sync, each repository the machine asks to maintain here is
/// listed once, and only once while its listing stands, so the report says
/// what it holds (§7.3, decision 156, I1).
#[tokio::test]
async fn i1_each_local_repository_is_listed_once_before_a_sync_and_reported_as_it_holds() {
    let due = due().await;
    due.engine.holds(photos(), vec![made_by(&machine(), now())]);
    due.engine
        .holds(music(), vec![made_by(&another_machine(), now() - days(3))]);
    assert!(due.holds(photos()));
    due.listings.list().await;
    assert_eq!(due.listed(), [1, 1, 0, 0]);
    assert!(!due.holds(photos()));
    assert!(due.holds(music()));
    due.listings.list().await;
    assert_eq!(due.listed(), [1, 1, 0, 0]);
}

/// A listing older than a day is run again at the next sync, which then
/// sees what was backed up to the repository since (decision 156, I1).
#[tokio::test]
async fn i1_a_listing_older_than_a_day_is_run_again_at_the_next_sync() {
    let due = due().await;
    due.listings.list().await;
    assert!(!due.holds(photos()));
    due.engine
        .holds(photos(), vec![made_by(&another_machine(), now())]);
    due.clock.advance(days(1));
    due.listings.list().await;
    assert_eq!(due.listed(), [1, 1, 0, 0]);
    assert!(!due.holds(photos()));
    due.clock.advance(NANOSECOND);
    assert!(due.holds(photos()));
    due.listings.list().await;
    assert_eq!(due.listed(), [2, 2, 0, 0]);
    assert!(due.holds(photos()));
}

/// A listing that fails leaves the repository reported as holding another
/// agent's snapshots, is warned of once, and is tried again at each sync
/// until one succeeds (decision 156).
#[tokio::test]
async fn i1_a_failed_listing_is_tried_again_at_the_next_sync_and_warned_of_once() {
    crate::test_log::capture_the_log();
    let due = due().await;
    due.engine.fails(photos(), true);
    due.listings.list().await;
    due.listings.list().await;
    assert_eq!(due.listed(), [2, 1, 0, 0]);
    assert!(due.holds(photos()));
    let warned = |module| {
        crate::test_log::logged_by(module)
            .into_iter()
            .filter(|(level, _)| *level == log::Level::Warn)
            .count()
    };
    assert_eq!(warned("app::others_snapshots"), 1);
    due.engine.fails(photos(), false);
    due.listings.list().await;
    assert_eq!(due.listed(), [3, 1, 0, 0]);
    assert!(!due.holds(photos()));
    // A later failure is a new streak, warned of again.
    due.engine.fails(photos(), true);
    due.clock.advance(days(2));
    due.listings.list().await;
    assert_eq!(warned("app::others_snapshots"), 1);
}

/// No repository is listed while a maintenance of it runs here; the others
/// are (decision 156).
#[tokio::test]
async fn no_repository_is_listed_while_a_maintenance_of_it_runs_here() {
    use domain::execution::{Operation, OperationKind, Target, Trigger};

    let due = due().await;
    let target = Target {
        plan: None,
        repository: Some(photos()),
    };
    let id = OperationId::from_uuid(Uuid::from_u128(0x0619));
    let mut prune = Operation::queue(id, OperationKind::Prune, Trigger::Manual, target, now());
    prune.start(now()).unwrap();
    assert_eq!(due.operations.try_start(&prune).await, Ok(Start::Started));
    due.listings.list().await;
    assert_eq!(due.listed(), [0, 1, 0, 0]);
    assert!(due.holds(photos()));
}
