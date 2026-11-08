//! Steps for the `@e2e` backup scenarios: real scratch repositories, real
//! workers. Repositories are set up and inspected with `rustic_core`
//! directly, never through Steading, so the checks don't trust the code
//! under test.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app::ports::{BackupJob, Job};
use cucumber::{given, then, when};
use domain::execution::deadline::backup_budget;
use domain::execution::plan_tags::PlanTags;
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

/// The scratch repository, without a cache: the tests never write into the
/// developer's own rustic cache.
fn repository(path: &Path) -> Repository<()> {
    let backends = BackendOptions::default()
        .repository(path.to_str().expect("a UTF-8 scratch path"))
        .to_backends()
        .expect("the scratch repository's backend");
    Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .expect("the scratch repository")
}

/// Every snapshot in the scratch repository, oldest first.
pub fn snapshots(repo: &Path) -> Vec<SnapshotFile> {
    let mut snapshots = repository(repo)
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

#[given("a new repository")]
fn new_repository(agent: &mut Agent) {
    let path = agent.scratch().join("repo");
    repository(&path)
        .init(
            &credentials(),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .expect("the scratch repository initializes");
    agent.backup.repository = Some(path);
}

fn new_source(agent: &mut Agent, plan: &str) -> PathBuf {
    agent.backup.plan = Some(domain::PlanId::try_from(plan.to_owned()).expect("a valid plan ID"));
    let source = agent.scratch().join("source");
    fs::create_dir_all(&source).unwrap();
    agent.backup.source = Some(source.clone());
    source
}

#[given(expr = "a plan {string} backing up a folder with {int} files")]
fn plan_with_files(agent: &mut Agent, plan: String, count: usize) {
    let source = new_source(agent, &plan);
    for n in 1..=count {
        fs::write(
            source.join(format!("file-{n}.txt")),
            format!("demo file {n}\n"),
        )
        .unwrap();
    }
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

#[given("the folder was backed up without Steading")]
fn backed_up_without_steading(agent: &mut Agent) {
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

#[then("the new snapshot's parent is the snapshot made without Steading")]
fn parent_is_plain(agent: &mut Agent) {
    assert_eq!(new_snapshot_parent(agent), agent.backup.earlier);
}

/// What the backup scenarios set up.
#[derive(Debug, Default)]
pub struct BackupSetup {
    pub repository: Option<PathBuf>,
    pub plan: Option<domain::PlanId>,
    pub source: Option<PathBuf>,
    /// A snapshot made before the one under test.
    pub earlier: Option<SnapshotId>,
    pub deadline_budget: Option<SignedDuration>,
}

impl Agent {
    fn repository(&self) -> PathBuf {
        self.backup.repository.clone().expect("a repository")
    }

    fn source(&self) -> PathBuf {
        self.backup.source.clone().expect("a plan's folder")
    }

    fn plan_tags(&self) -> PlanTags {
        PlanTags::new(self.backup.plan.clone().expect("a plan"), self.agent_id)
    }

    /// The backup job the scenario's setup describes.
    pub fn backup_job(&self) -> Job {
        Job::Backup(BackupJob {
            repository: app::ports::RepositoryAccess {
                repository: self.repository().to_str().unwrap().to_owned(),
                password: domain::secrets::SecretValue::new(PASSWORD),
                options: Default::default(),
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
