//! Steps for the `@e2e` scenario of `features/maintenance/forget.feature`:
//! the maintenance use case runs against the real worker and a real scratch
//! repository, and the repository is inspected with `rustic_core` itself,
//! never through Ladderback.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use async_trait::async_trait;
use config_adapter::FileConfigStore;
use cucumber::{given, then, when};
use domain::planning::{
    AgentConfig, AgentSettings, GroupBy, Hooks, Maintenance, Plan, RepositoryConfig,
    RepositorySettings,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, RepositoryAlias, SnapshotId, Timestamp, Uuid};

use app::fakes::FakeRetention;
use app::maintenance::{MaintenanceKind, RunMaintenance};
use app::ports::{ConfigStore, RepositoryAccess, Retention, RetentionError};
use domain::maintenance::Snapshot;
use domain::planning::RetentionPolicy;
use secrets::SystemSecrets;

use super::Agent;

const PASSWORD: &str = "demo-password";

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
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

/// Opens the scratch repository with rustic itself: the truth the run is
/// checked against.
fn open(agent: &Agent) -> rustic_core::Repository<rustic_core::IndexedIdsStatus> {
    let cold = agent.backup.repository.clone().expect("a repository");
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
fn snapshots_of(agent: &Agent) -> Vec<(SnapshotId, Timestamp, Vec<String>)> {
    let mut snapshots: Vec<_> = open(agent)
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
}

/// Saves the configuration the run loads: the scenario's repository under
/// `demo`, and the plan with its retention policy.
async fn save_config(agent: &mut Agent, keep_last: Option<i64>) {
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
    if let Some(keep) = keep_last {
        plan.retention.keep_last = Some(FakeRetention::count(keep));
    }
    let mut repositories = BTreeMap::new();
    repositories.insert(
        alias(),
        RepositoryConfig {
            settings: RepositorySettings {
                repository: cold.to_str().expect("a UTF-8 path").to_owned(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new(PASSWORD)),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            maintenance: Maintenance::default(),
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

/// `keep-last`, as the run evaluates it: newest first, the rest goes.
/// The shared contract suite checks this against rustic's own retention;
/// the run's own port is swapped in where it differs.
struct KeepLast;

#[async_trait]
impl Retention for KeepLast {
    async fn removes(
        &self,
        _repository: &RepositoryAccess,
        policy: &RetentionPolicy,
        group: &[Snapshot],
    ) -> Result<Vec<SnapshotId>, RetentionError> {
        let keep = policy.keep_last.map(|count| {
            if count.is_all() {
                usize::MAX
            } else {
                usize::try_from(count.get()).unwrap_or(0)
            }
        });
        Ok(match keep {
            None => Vec::new(),
            Some(keep) => group.iter().skip(keep).map(|s| s.id.clone()).collect(),
        })
    }
}

/// The snapshots carrying the plan tag, oldest first.
fn snapshots_tagged(agent: &Agent, plan: &str) -> Vec<SnapshotId> {
    let tag = format!("ladderback-plan:{plan}");
    snapshots_of(agent)
        .into_iter()
        .filter(|(_, _, tags)| tags.iter().any(|each| each == &tag))
        .map(|(id, _, _)| id)
        .collect()
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s)")]
async fn plan_keeping_last(agent: &mut Agent, plan: String, keep: i64) {
    crate::backup_steps::source_with_files(agent, &plan, 3);
    save_config(agent, Some(keep)).await;
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
            let name = file
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .to_owned();
            fs::write(&file, format!("{name} {}\n", std::process::id())).expect("writes");
        }
    }
    let plan = agent.backup.plan.clone().expect("a plan");
    let tagged = snapshots_tagged(agent, plan.as_str());
    agent.forget.of_plan = tagged.len();
    agent.forget.newest = tagged.last().cloned();
    let without = snapshots_of(agent)
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
        Arc::new(KeepLast),
        Arc::new(config),
        agent.agent_id,
    );
    agent.runs += 1;
    let operation = OperationId::from_uuid(Uuid::from_u128(agent.runs));
    let prepared = use_case
        .prepare(operation, &alias(), MaintenanceKind::Forget)
        .await
        .expect("the repository is free");
    agent.finished = Some(use_case.execute(prepared, futures::future::pending()).await);
}

#[then(expr = "the remaining snapshot of the plan is the newest of its {int}")]
fn newest_remains(agent: &mut Agent, had: usize) {
    assert_eq!(
        agent.forget.of_plan, had,
        "the plan had this many snapshots"
    );
    let newest = agent.forget.newest.clone().expect("the newest snapshot");
    let plan = agent.backup.plan.clone().expect("a plan");
    let remaining = snapshots_tagged(agent, plan.as_str());
    assert_eq!(
        remaining,
        vec![newest],
        "only the newest of the plan remains"
    );
}

#[then("the snapshot made without Ladderback is still there")]
fn unmanaged_remains(agent: &mut Agent) {
    let unmanaged = agent
        .forget
        .unmanaged
        .clone()
        .expect("an unmanaged snapshot");
    let present: Vec<SnapshotId> = snapshots_of(agent)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert!(present.contains(&unmanaged), "still there: {present:?}");
}
