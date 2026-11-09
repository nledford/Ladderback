//! Steps for the `@e2e` backup scenarios: real scratch repositories, real
//! workers. Repositories are set up and inspected with `rustic_core`
//! directly, never through Ladderback, so the checks don't trust the code
//! under test.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app::history::GetOperation;
use app::ports::{BackupJob, EngineResult, Job};
use cucumber::{given, then, when};
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
use domain::execution::{OperationKind, Trigger};
use domain::{SignedDuration, SnapshotId};
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, KeyOptions, PathList, Repository, RepositoryOptions,
    SnapshotOptions,
};

use super::Agent;

/// Not a real secret: the scratch repositories' password.
pub const PASSWORD: &str = "e2e-scratch-password";

/// How long a cancelled backup runs before the user cancels it: long enough
/// that the worker is reading the source, far shorter than the backup.
const CANCEL_AFTER: Duration = Duration::from_millis(500);

fn credentials() -> Credentials {
    Credentials::password(PASSWORD)
}

/// Where a scratch repository is: all in `cold`, or with a hot half beside it.
#[derive(Clone, Debug)]
pub struct Folders {
    pub cold: PathBuf,
    pub hot: Option<PathBuf>,
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 scratch path")
}

/// The scratch repository, without a cache: the tests never write into the
/// developer's own rustic cache.
fn repository(folders: &Folders) -> Repository<()> {
    let options = BackendOptions::default().repository(text(&folders.cold));
    let options = match &folders.hot {
        Some(hot) => options.repo_hot(text(hot)),
        None => options,
    };
    let backends = options
        .to_backends()
        .expect("the scratch repository's backend");
    Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .expect("the scratch repository")
}

/// Every snapshot in the scratch repository, oldest first.
pub fn snapshots(folders: &Folders) -> Vec<SnapshotFile> {
    let mut snapshots = repository(folders)
        .open(&credentials())
        .expect("the scratch repository opens")
        .get_all_snapshots()
        .expect("the snapshots load");
    snapshots.sort_by(|a, b| a.time.cmp(&b.time));
    snapshots
}

fn snapshot_id(snapshot: &SnapshotFile) -> SnapshotId {
    SnapshotId::parse(snapshot.id.to_hex().as_str()).expect("rustic's IDs are 64 hex digits")
}

fn init(folders: &Folders) {
    repository(folders)
        .init(
            &credentials(),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .expect("the scratch repository initializes");
}

#[given("a new repository")]
fn a_new_repository(agent: &mut Agent) {
    new_repository(agent);
}

/// A scratch repository the scenario's plan backs up into: what "a new
/// repository" sets up, for the steps that build a configuration around one.
pub(super) fn new_repository(agent: &mut Agent) -> PathBuf {
    let folders = Folders {
        cold: agent.scratch().join("repo"),
        hot: None,
    };
    init(&folders);
    agent.backup.repository = Some(folders.cold.clone());
    folders.cold
}

#[given("a new hot/cold repository")]
fn new_hot_cold_repository(agent: &mut Agent) {
    let folders = Folders {
        cold: agent.scratch().join("repo"),
        hot: Some(agent.scratch().join("hot")),
    };
    init(&folders);
    agent.backup.repository = Some(folders.cold);
    agent.backup.hot = folders.hot;
}

fn new_source(agent: &mut Agent, plan: &str) -> PathBuf {
    agent.backup.plan = Some(domain::PlanId::try_from(plan.to_owned()).expect("a valid plan ID"));
    let source = agent.scratch().join("source");
    fs::create_dir_all(&source).unwrap();
    agent.backup.source = Some(source.clone());
    source
}

/// A folder of `count` demo files the plan named `plan` backs up: what "a
/// plan backing up a folder with N files" sets up, for the steps that build
/// a configuration around one.
pub(super) fn source_with_files(agent: &mut Agent, plan: &str, count: usize) -> PathBuf {
    let source = new_source(agent, plan);
    for n in 1..=count {
        fs::write(
            source.join(format!("file-{n}.txt")),
            format!("demo file {n}\n"),
        )
        .unwrap();
    }
    source
}

#[given(expr = "a plan {string} backing up a folder with {int} files")]
fn plan_with_files(agent: &mut Agent, plan: String, count: usize) {
    source_with_files(agent, &plan, count);
}

#[given(expr = "a plan {string} backing up a folder with a 4 GiB file")]
fn plan_with_large_file(agent: &mut Agent, plan: String) {
    let source = new_source(agent, &plan);
    // Sparse: costs no disk, yet rustic must read and hash all of it.
    fs::File::create(source.join("large.bin"))
        .unwrap()
        .set_len(4 << 30)
        .unwrap();
}

/// Only runs where file permissions work: `tests/e2e.rs` skips scenarios
/// tagged `@needs-file-permissions` elsewhere, visibly.
#[given(expr = "{int} of the files can't be read")]
fn unreadable(agent: &mut Agent, count: usize) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let source = agent.source();
        let mut files: Vec<PathBuf> = fs::read_dir(&source)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        for file in files.into_iter().take(count) {
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
            assert!(
                fs::read(&file).is_err(),
                "{} is still readable",
                file.display()
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (agent, count);
        panic!("@needs-file-permissions scenarios are skipped off Unix");
    }
}

#[given("the plan has been backed up")]
async fn backed_up(agent: &mut Agent) {
    agent.run_backup().await;
    let status = agent.operation().status().clone();
    assert_eq!(status, domain::execution::OperationStatus::Succeeded);
    let latest = snapshots(&agent.repository()).pop().expect("a snapshot");
    agent.backup.earlier = Some(snapshot_id(&latest));
}

#[given("the plan's folder has moved")]
fn folder_moved(agent: &mut Agent) {
    let from = agent.source();
    let to = from.with_file_name("moved-source");
    fs::rename(&from, &to).unwrap();
    agent.backup.source = Some(to);
}

#[given("the folder was backed up without Ladderback")]
fn backed_up_without_ladderback(agent: &mut Agent) {
    let repo = repository(&agent.repository())
        .open(&credentials())
        .unwrap()
        .to_indexed_ids()
        .unwrap();
    let source = PathList::from_iter([agent.source()]).sanitize().unwrap();
    let snapshot = SnapshotOptions::default().to_snapshot().unwrap();
    let snapshot = repo
        .backup(&BackupOptions::default(), &source, snapshot)
        .expect("a plain rustic backup");
    agent.backup.earlier = Some(snapshot_id(&snapshot));
}

#[given("backups have no time left before a prune")]
fn no_time_left(agent: &mut Agent) {
    agent.backup.deadline_budget = Some(SignedDuration::ZERO);
}

#[when("the agent backs up the plan")]
async fn backs_up(agent: &mut Agent) {
    agent.run_backup().await;
}

#[when("the agent backs up the plan and the user cancels it once it has started")]
async fn backs_up_and_cancels(agent: &mut Agent) {
    agent.run_backup_cancelled_after(CANCEL_AFTER).await;
}

#[then(expr = "the repository has {int} snapshot(s)")]
fn has_snapshots(agent: &mut Agent, count: usize) {
    assert_eq!(snapshots(&agent.repository()).len(), count);
}

#[then("the repository has no snapshots")]
fn has_no_snapshots(agent: &mut Agent) {
    let snapshots = snapshots(&agent.repository());
    assert!(snapshots.is_empty(), "{snapshots:?}");
}

#[then("the snapshot is tagged with the plan and the agent")]
fn tagged(agent: &mut Agent) {
    let snapshot = snapshots(&agent.repository()).pop().expect("a snapshot");
    let tags: Vec<&String> = snapshot.tags.iter().collect();
    for tag in agent.plan_tags().tags() {
        assert!(tags.contains(&&tag), "{tag} missing from {tags:?}");
    }
}

/// The backup's snapshot has a file in `half`, one of the folders of the
/// repository (its `snapshots` folder holds a file for each snapshot). The
/// snapshot is the one the operation reported: reading it back through
/// rustic would need the very file that may be missing.
fn assert_half_holds_the_snapshot(agent: &Agent, half: &Path, name: &str) {
    let Some(EngineResult::Backup(summary)) = &agent.finished().result else {
        panic!("the operation made no backup: {:?}", agent.operation());
    };
    let file = half.join("snapshots").join(summary.snapshot.as_str());
    assert!(file.is_file(), "the {name} half has no {}", file.display());
}

#[then("the snapshot is in the hot half of the repository")]
fn snapshot_in_the_hot_half(agent: &mut Agent) {
    let hot = agent.backup.hot.clone().expect("a hot half");
    assert_half_holds_the_snapshot(agent, &hot, "hot");
}

#[then("the snapshot is in the cold half of the repository")]
fn snapshot_in_the_cold_half(agent: &mut Agent) {
    let cold = agent.repository().cold;
    assert_half_holds_the_snapshot(agent, &cold, "cold");
}

#[then("the history shows the backup's snapshot")]
async fn history_shows_the_snapshot(agent: &mut Agent) {
    let store = agent.store().await;
    let detail = GetOperation::new(store)
        .get(agent.operation().id())
        .await
        .expect("the history answers")
        .expect("the operation is in the history");
    let latest = snapshots(&agent.repository()).pop().expect("a snapshot");
    let Some(EngineResult::Backup(summary)) = &detail.record.result else {
        panic!("the history holds no backup result: {:?}", detail.record);
    };
    assert_eq!(summary.snapshot, snapshot_id(&latest));
    assert_eq!(detail.record.kind, OperationKind::Backup);
    assert_eq!(detail.record.trigger, Trigger::Manual);
    assert_eq!(detail.record.target.plan, agent.backup.plan);
}

fn new_snapshot_parent(agent: &Agent) -> Option<SnapshotId> {
    let latest = snapshots(&agent.repository()).pop().expect("a snapshot");
    latest
        .parent
        .map(|p| SnapshotId::parse(p.to_hex().as_str()).unwrap())
}

#[then("the new snapshot's parent is the plan's previous snapshot")]
fn parent_is_previous(agent: &mut Agent) {
    assert_eq!(new_snapshot_parent(agent), agent.backup.earlier);
}

#[then("the new snapshot's parent is the snapshot made without Ladderback")]
fn parent_is_plain(agent: &mut Agent) {
    assert_eq!(new_snapshot_parent(agent), agent.backup.earlier);
}

/// What the backup scenarios set up.
#[derive(Debug, Default)]
pub struct BackupSetup {
    /// The repository's folder; next to a `hot`, its cold half.
    pub repository: Option<PathBuf>,
    pub hot: Option<PathBuf>,
    pub plan: Option<domain::PlanId>,
    pub source: Option<PathBuf>,
    /// A snapshot made before the one under test.
    pub earlier: Option<SnapshotId>,
    pub deadline_budget: Option<SignedDuration>,
}

impl Agent {
    fn repository(&self) -> Folders {
        Folders {
            cold: self.backup.repository.clone().expect("a repository"),
            hot: self.backup.hot.clone(),
        }
    }

    fn source(&self) -> PathBuf {
        self.backup.source.clone().expect("a plan's folder")
    }

    fn plan_tags(&self) -> PlanTags {
        PlanTags::new(self.backup.plan.clone().expect("a plan"), self.agent_id)
    }

    /// The backup job the scenario's setup describes.
    pub fn backup_job(&self) -> Job {
        let folders = self.repository();
        Job::Backup(BackupJob {
            repository: app::ports::RepositoryAccess {
                repository: text(&folders.cold).to_owned(),
                repo_hot: folders.hot.as_deref().map(|hot| text(hot).to_owned()),
                password: domain::secrets::SecretValue::new(PASSWORD),
                options: Default::default(),
                options_hot: Default::default(),
                options_cold: Default::default(),
                secrets: Vec::new(),
                id: None,
            },
            sources: vec![self.source().to_str().unwrap().to_owned()],
            tags: self.plan_tags(),
            extra_tags: Vec::new(),
            label: None,
            deadline_budget: self
                .backup
                .deadline_budget
                .unwrap_or_else(|| backup_budget(None, None)),
        })
    }
}
