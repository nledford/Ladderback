//! Steps for the `@e2e` scenario of `features/maintenance/forget.feature`:
//! the maintenance use case runs against the real worker and a real scratch
//! repository, and the repository is inspected with `rustic_core` itself,
//! never through Ladderback.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use config_adapter::FileConfigStore;
use cucumber::{given, then, when};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, KeepCount, Maintenance, Plan, RepositoryConfig,
    RepositorySettings, RetentionPolicy,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, SnapshotId, Timestamp, Uuid};

use app::maintenance::{MaintenanceKind, RunMaintenance};
use app::ports::ConfigStore;
use secrets::SystemSecrets;

use super::{Agent, blocking};

const PASSWORD: &str = "e2e-scratch-password";

/// What the e2e forget scenario set up, and what the run did.
#[derive(Default)]
pub struct ForgetE2e {
    /// The alias the repository is saved under in the config file.
    alias: Option<RepositoryAlias>,
    /// The ID of the newest of the plan's snapshots, read before the run.
    newest: Option<SnapshotId>,
    /// The ID of the snapshot made without Ladderback, read before the run.
    unmanaged: Option<SnapshotId>,
    /// How many snapshots the plan had, read before the run.
    of_plan: usize,
    /// Each snapshot the scenario wrote at a time of its choosing: the time
    /// as the scenario wrote it, and the snapshot's ID.
    taken: Vec<(String, SnapshotId)>,
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// Opens the scratch repository with rustic itself, the truth the run is
/// checked against, and runs `work` on it. Opening derives the repository's
/// key, which takes a second in a debug build, so both are on a blocking
/// thread: see [`blocking`].
pub async fn with_repository<T: Send + 'static>(
    agent: &Agent,
    work: impl FnOnce(rustic_core::Repository<rustic_core::IndexedIdsStatus>) -> T + Send + 'static,
) -> T {
    let cold = agent.backup.repository.clone().expect("a repository");
    blocking(move || work(open(&cold))).await
}

fn open(cold: &std::path::Path) -> rustic_core::Repository<rustic_core::IndexedIdsStatus> {
    let backends = rustic_backend::BackendOptions::default()
        .repository(cold.to_str().expect("a UTF-8 scratch path"))
        .to_backends()
        .expect("the scratch repository's backend");
    rustic_core::Repository::new(
        &rustic_core::RepositoryOptions::default().no_cache(true),
        &backends,
    )
    .expect("the scratch repository")
    .open(&rustic_core::Credentials::password(PASSWORD))
    .expect("the scratch repository opens")
    .to_indexed_ids()
    .expect("the scratch repository indexes")
}

/// Every snapshot in the repository: its ID, its time and its tags, oldest
/// first.
pub async fn snapshots_of(agent: &Agent) -> Vec<(SnapshotId, Timestamp, Vec<String>)> {
    with_repository(agent, |repository| {
        let mut snapshots: Vec<_> = repository
            .get_all_snapshots()
            .expect("the snapshots load")
            .into_iter()
            .map(|snapshot| {
                (
                    SnapshotId::parse(&snapshot.id.to_hex()).expect("a snapshot ID"),
                    snapshot.time.timestamp(),
                    snapshot
                        .tags
                        .iter()
                        .map(|tag| tag.as_str().to_owned())
                        .collect(),
                )
            })
            .collect();
        snapshots.sort_by_key(|(_, time, _)| *time);
        snapshots
    })
    .await
}

/// Saves the configuration the run loads: the scenario's repository under
/// `demo`, and the plan with its retention policy.
async fn save_config(agent: &mut Agent, retention: RetentionPolicy) {
    let cold = agent.backup.repository.clone().expect("a repository");
    let source = agent.backup.source.clone().expect("a plan source");
    let mut plan = Plan::new(
        agent.backup.plan.clone().expect("a plan"),
        alias(),
        vec![
            domain::planning::Source::parse(source.to_str().expect("a UTF-8 path"))
                .expect("a source"),
        ],
    );
    plan.retention = retention;
    let mut repositories = BTreeMap::new();
    repositories.insert(
        alias(),
        RepositoryConfig {
            locations: Default::default(),
            id: None,
            settings: RepositorySettings {
                repository: cold.to_str().expect("a UTF-8 path").to_owned(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new(PASSWORD)),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::Local,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        },
    );
    let config = AgentConfig::new(
        AgentSettings::default(),
        Hooks::default(),
        repositories,
        vec![plan],
    )
    .expect("the scenario's configuration is valid");
    fs::create_dir_all(agent.scratch().join("config")).expect("the config folder");
    FileConfigStore::new(
        agent.scratch().join("config").join("config.toml"),
        Arc::new(platform::SystemClock),
    )
    .save(&config)
    .await
    .expect("the configuration saves");
    agent.forget.alias = Some(alias());
}

/// The snapshots carrying the plan tag, oldest first.
async fn snapshots_tagged(agent: &Agent, plan: &str) -> Vec<SnapshotId> {
    let tag = format!("ladderback-plan:{plan}");
    snapshots_of(agent)
        .await
        .into_iter()
        .filter(|(_, _, tags)| tags.iter().any(|each| each == &tag))
        .map(|(id, _, _)| id)
        .collect()
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s)")]
async fn plan_keeping_last(agent: &mut Agent, plan: String, keep: i64) {
    crate::backup_steps::source_with_files(agent, &plan, 3);
    let retention = RetentionPolicy {
        keep_last: Some(KeepCount::new(keep).expect("a count")),
        ..RetentionPolicy::default()
    };
    save_config(agent, retention).await;
}

#[given(expr = "a plan {string} keeping {int} daily snapshot(s)")]
async fn plan_keeping_daily(agent: &mut Agent, plan: String, keep: i64) {
    crate::backup_steps::source_with_files(agent, &plan, 1);
    let retention = RetentionPolicy {
        keep_daily: Some(KeepCount::new(keep).expect("a count")),
        ..RetentionPolicy::default()
    };
    save_config(agent, retention).await;
}

/// A backup of the plan's folder with the plan's tags, as the plan's own
/// backup would tag it, recorded at `time`: a date, a time of day and the
/// offset of the clock that took it, read as rustic reads the time of a
/// snapshot file.
#[given(expr = "the plan has a snapshot taken at {string}")]
async fn snapshot_taken_at(agent: &mut Agent, time: String) {
    let source = agent.backup.source.clone().expect("a plan source");
    let plan = agent.backup.plan.clone().expect("a plan");
    let tags = [
        format!("ladderback-agent:{}", agent.agent_id.as_uuid().hyphenated()),
        format!("ladderback-plan:{}", plan.as_str()),
    ];
    // Distinct contents, so each backup is its own snapshot.
    fs::write(source.join("file-1.txt"), format!("taken at {time}\n")).expect("writes");
    let when = rustic_core::repofile::RusticTime::parse_utc(&time).expect("a time");
    let snapshot = with_repository(agent, move |repository| {
        let paths = rustic_core::PathList::from_iter([source])
            .sanitize()
            .expect("the folder's path");
        let mut options = rustic_core::SnapshotOptions::default().time(Some(when));
        for tag in &tags {
            options = options.add_tags(tag).expect("a tag");
        }
        let snapshot = options.to_snapshot().expect("a snapshot");
        repository
            .backup(&rustic_core::BackupOptions::default(), &paths, snapshot)
            .expect("a rustic backup")
    })
    .await;
    let id = SnapshotId::parse(&snapshot.id.to_hex()).expect("a snapshot ID");
    agent.forget.taken.push((time, id));
}

#[then(expr = "the plan keeps only its snapshots taken at {string} and {string}")]
async fn keeps_only_taken_at(agent: &mut Agent, first: String, second: String) {
    let plan = agent.backup.plan.clone().expect("a plan");
    let remaining = snapshots_tagged(agent, plan.as_str()).await;
    // Each remaining snapshot by the time the scenario took it at.
    let mut kept: Vec<&str> = remaining
        .iter()
        .map(|id| {
            agent
                .forget
                .taken
                .iter()
                .find(|(_, each)| each == id)
                .map_or(id.as_str(), |(time, _)| time.as_str())
        })
        .collect();
    kept.sort_unstable();
    let mut expected = [first.as_str(), second.as_str()];
    expected.sort_unstable();
    assert_eq!(
        kept, expected,
        "the plan keeps the snapshots taken at these times"
    );
}

#[given(expr = "the plan has been backed up {int} times")]
async fn backed_up_times(agent: &mut Agent, times: u64) {
    for _ in 0..times {
        agent.run_backup().await;
        let status = agent.operation().status().clone();
        assert_eq!(status, domain::execution::OperationStatus::Succeeded);
        // Distinct contents, so each backup is its own snapshot.
        let source = agent.backup.source.clone().expect("a plan source");
        let files = fs::read_dir(&source).expect("the source folder");
        for file in files {
            let file = file.expect("a file").path();
            let name = file.file_name().expect("a name").to_string_lossy();
            fs::write(&file, format!("{name} {}\n", std::process::id())).expect("writes");
        }
    }
    let plan = agent.backup.plan.clone().expect("a plan");
    let tagged = snapshots_tagged(agent, plan.as_str()).await;
    agent.forget.of_plan = tagged.len();
    agent.forget.newest = tagged.last().cloned();
    let without = snapshots_of(agent)
        .await
        .into_iter()
        .find(|(id, _, _)| !tagged.contains(id))
        .map(|(id, _, _)| id);
    agent.forget.unmanaged = without;
}

#[when("the owner forgets the repository")]
async fn forget(agent: &mut Agent) {
    let config = FileConfigStore::new(
        agent.scratch().join("config").join("config.toml"),
        Arc::new(platform::SystemClock),
    );
    let resolve = app::secrets::ResolveRepository::new(Arc::new(SystemSecrets::new()));
    let use_case = RunMaintenance::new(
        agent.engine.clone(),
        Arc::new(platform::SystemClock),
        agent.store().await,
        resolve,
        Arc::new(engine::RusticRetention),
        Arc::new(config),
        agent.agent_id,
        platform::hostname(),
        Arc::new(engine::RusticSources),
    );
    agent.runs += 1;
    let operation = OperationId::from_uuid(Uuid::from_u128(agent.runs));
    let assessment = use_case
        .assess(operation, &alias(), MaintenanceKind::Forget)
        .await
        .expect("the forget is assessed");
    let prepared = use_case
        .start(assessment)
        .await
        .expect("the repository is free");
    let plan = agent.backup.plan.clone().expect("a plan");
    let tagged = snapshots_tagged(agent, plan.as_str()).await;
    agent.forget.unmanaged = snapshots_of(agent)
        .await
        .into_iter()
        .find(|(id, _, _)| !tagged.contains(id))
        .map(|(id, _, _)| id);
    agent.finished = Some(use_case.execute(prepared, futures::future::pending()).await);
}

#[then(expr = "the remaining snapshot of the plan is the newest of its {int}")]
async fn newest_remains(agent: &mut Agent, had: usize) {
    assert_eq!(
        agent.forget.of_plan, had,
        "the plan had this many snapshots"
    );
    let newest = agent.forget.newest.clone().expect("the newest snapshot");
    let plan = agent.backup.plan.clone().expect("a plan");
    let remaining = snapshots_tagged(agent, plan.as_str()).await;
    assert_eq!(
        remaining,
        vec![newest],
        "only the newest of the plan remains"
    );
}

#[then("the snapshot made without Ladderback is still there")]
async fn unmanaged_remains(agent: &mut Agent) {
    let unmanaged = agent
        .forget
        .unmanaged
        .clone()
        .expect("an unmanaged snapshot");
    let present: Vec<SnapshotId> = snapshots_of(agent)
        .await
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert!(present.contains(&unmanaged), "still there: {present:?}");
}
