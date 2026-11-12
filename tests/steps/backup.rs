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
use domain::{RepositoryId, SignedDuration, SnapshotId};
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, KeyOptions, PathList, Repository, RepositoryOptions,
    SnapshotOptions,
};

use super::{Agent, blocking};

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

/// Every snapshot in the scratch repository, oldest first. Opening it derives
/// its key, which takes a second in a debug build, so the work is on a
/// blocking thread (see [`blocking`]).
pub async fn snapshots(folders: &Folders) -> Vec<SnapshotFile> {
    let folders = folders.clone();
    blocking(move || {
        let mut snapshots = repository(&folders)
            .open(&credentials())
            .expect("the scratch repository opens")
            .get_all_snapshots()
            .expect("the snapshots load");
        snapshots.sort_by(|a, b| a.time.cmp(&b.time));
        snapshots
    })
    .await
}

/// The newest snapshot in the scenario's repository: the backup's.
pub(crate) async fn newest_snapshot(agent: &Agent) -> SnapshotFile {
    snapshots(&agent.repository())
        .await
        .pop()
        .expect("the backup's snapshot")
}

/// The name of every file and folder the newest snapshot holds, at any
/// depth, as rustic itself lists them.
pub(crate) async fn names_in_newest(agent: &Agent) -> Vec<String> {
    let snapshot = newest_snapshot(agent).await;
    let folders = agent.repository();
    blocking(move || {
        let repository = repository(&folders)
            .open(&credentials())
            .expect("the scratch repository opens")
            .to_indexed()
            .expect("the scratch repository indexes");
        let root = repository
            .node_from_snapshot_and_path(&snapshot, "")
            .expect("the snapshot's root");
        repository
            .ls(&root, &rustic_core::LsOptions::default())
            .expect("the snapshot lists")
            .map(|entry| {
                let (_, node) = entry.expect("a listed node");
                node.name().to_string_lossy().into_owned()
            })
            .collect()
    })
    .await
}

fn snapshot_id(snapshot: &SnapshotFile) -> SnapshotId {
    SnapshotId::parse(snapshot.id.to_hex().as_str()).expect("rustic's IDs are 64 hex digits")
}

async fn init(folders: &Folders) {
    init_with(folders, ConfigOptions::default()).await;
}

async fn init_with(folders: &Folders, config: ConfigOptions) {
    let folders = folders.clone();
    blocking(move || {
        repository(&folders)
            .init(&credentials(), &KeyOptions::default(), &config)
            .expect("the scratch repository initializes");
    })
    .await;
}

#[given("a new repository")]
async fn a_new_repository(agent: &mut Agent) {
    new_repository(agent).await;
}

/// Small packs, so that a backup of a few megabytes has written whole packs
/// long before it ends.
#[given(expr = "a new repository that saves data in packs of {int} MiB")]
async fn a_new_repository_with_small_packs(agent: &mut Agent, mib: u64) {
    let config: ConfigOptions = toml::from_str(&format!(
        "set-datapack-size = \"{mib} MiB\"\nset-datapack-growfactor = 0"
    ))
    .expect("rustic's config options");
    let folders = Folders {
        cold: agent.scratch().join("repo"),
        hot: None,
    };
    init_with(&folders, config).await;
    agent.backup.repository = Some(folders.cold);
}

/// A scratch repository the scenario's plan backs up into: what "a new
/// repository" sets up, for the steps that build a configuration around one.
pub(super) async fn new_repository(agent: &mut Agent) -> PathBuf {
    let folders = Folders {
        cold: agent.scratch().join("repo"),
        hot: None,
    };
    init(&folders).await;
    agent.backup.repository = Some(folders.cold.clone());
    folders.cold
}

#[given("a new hot/cold repository")]
async fn new_hot_cold_repository(agent: &mut Agent) {
    let folders = Folders {
        cold: agent.scratch().join("repo"),
        hot: Some(agent.scratch().join("hot")),
    };
    init(&folders).await;
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
pub(crate) fn source_with_files(agent: &mut Agent, plan: &str, count: usize) -> PathBuf {
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

/// Sorts after the `data-{n}.bin` files of "N MiB of new data": rustic reads
/// a folder in path order, so it saves the new data, and writes its packs,
/// before it reaches this file. A scenario that stops the backup once the
/// repository holds some packs relies on that; a name that sorted first
/// could keep a backup on this file until the scenario's time limit.
const LARGE_FILE: &str = "large.bin";

/// A file of `gib` GiB in `source`. Sparse: costs no disk, yet rustic must
/// read and hash all of it before it writes a snapshot, which keeps a
/// backup of the folder from ending before a cancel or a deadline lands.
/// The step that sets them off runs on the runner's thread, which may go
/// unpolled for up to 8 s (`MAX_STALL` in `tests/e2e.rs`; 3 s seen in a
/// full run, #252), and rustic's release build backs up 16 GiB of it in 6 s
/// on a ten-core Mac: 128 GiB outlasts any stall the runner allows. The
/// backup is abandoned at the cancel or deadline, so a bigger file costs no
/// test time.
fn add_large_file(source: &Path, gib: u64) {
    // `set_len` leaves the file sparse on CI's filesystems (ext4, APFS); one
    // that wrote the length out would fill the disk.
    fs::File::create(source.join(LARGE_FILE))
        .unwrap()
        .set_len(gib << 30)
        .unwrap();
}

#[given(expr = "a plan {string} backing up a folder with a {int} GiB file")]
fn plan_with_large_file(agent: &mut Agent, plan: String, gib: u64) {
    let source = new_source(agent, &plan);
    add_large_file(&source, gib);
}

/// Keeps a backup that saves the folder's other data from ending before a
/// cancel or a deadline that waits for its first packs, however soon it
/// has saved the rest.
#[given(expr = "the folder also holds a {int} GiB file")]
fn also_a_large_file(agent: &mut Agent, gib: u64) {
    add_large_file(&agent.source(), gib);
}

#[when(expr = "the {int} GiB file is removed from the folder")]
fn large_file_removed(agent: &mut Agent, gib: u64) {
    let file = agent.source().join(LARGE_FILE);
    let len = fs::metadata(&file)
        .expect("the folder holds the file")
        .len();
    assert_eq!(len, gib << 30, "the file is {len} bytes");
    fs::remove_file(file).unwrap();
}

/// Data no chunk of which repeats, so a backup must save every byte of it:
/// files of 4 MiB from a seeded generator (SplitMix64).
#[given(expr = "a plan {string} backing up a folder with {int} MiB of new data")]
fn plan_with_new_data(agent: &mut Agent, plan: String, mib: usize) {
    let source = new_source(agent, &plan);
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    for n in 0..mib.div_ceil(4) {
        let mut file = Vec::with_capacity(4 << 20);
        while file.len() < 4 << 20 {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            file.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
        }
        fs::write(source.join(format!("data-{n}.bin")), file).unwrap();
    }
    agent.backup.source_bytes = Some((mib as u64) << 20);
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
    let latest = snapshots(&agent.repository())
        .await
        .pop()
        .expect("a snapshot");
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
async fn backed_up_without_ladderback(agent: &mut Agent) {
    back_up_without_ladderback(agent, None).await;
}

/// A plain rustic backup of the plan's folder, with no tags or label, under
/// `host` or, when it is `None`, the machine's own name, as rustic chooses.
pub(crate) async fn back_up_without_ladderback(agent: &mut Agent, host: Option<String>) {
    let (folders, source) = (agent.repository(), agent.source());
    let snapshot = blocking(move || {
        let repo = repository(&folders)
            .open(&credentials())
            .unwrap()
            .to_indexed_ids()
            .unwrap();
        let source = PathList::from_iter([source]).sanitize().unwrap();
        let snapshot = SnapshotOptions::default().host(host).to_snapshot().unwrap();
        repo.backup(&BackupOptions::default(), &source, snapshot)
            .expect("a plain rustic backup")
    })
    .await;
    agent.backup.earlier = Some(snapshot_id(&snapshot));
}

#[given("backups have no time left before a prune")]
fn no_time_left(agent: &mut Agent) {
    agent.backup.deadline_budget = Some(SignedDuration::ZERO);
}

#[given("the repository's ID is known")]
async fn id_known(agent: &mut Agent) {
    let folders = agent.repository();
    let id = blocking(move || {
        let opened = repository(&folders)
            .open(&credentials())
            .expect("the scratch repository opens");
        RepositoryId::parse(opened.config().id.to_hex().as_str())
            .expect("rustic's IDs are 64 hex digits")
    })
    .await;
    agent.backup.id = Some(id);
}

/// The folder is emptied and a new repository is created in it: the same
/// location, with the same password, holds a repository with another ID.
#[given("another repository has replaced it at its location")]
async fn replaced(agent: &mut Agent) {
    let folders = agent.repository();
    fs::remove_dir_all(&folders.cold).expect("the old repository goes");
    init(&folders).await;
}

#[then("the operation ends as failed because its location holds another repository")]
fn failed_as_replaced(agent: &mut Agent) {
    let status = agent.operation().status().clone();
    assert_eq!(
        status,
        domain::execution::OperationStatus::Failed {
            reason: domain::execution::FailureReason::RepositoryReplaced,
        }
    );
}

#[when("the agent backs up the plan")]
async fn backs_up(agent: &mut Agent) {
    agent.run_backup().await;
}

#[when("the agent backs up the plan and the user cancels it once it has started")]
async fn backs_up_and_cancels(agent: &mut Agent) {
    agent.run_backup_cancelled_after(CANCEL_AFTER).await;
}

#[when(
    expr = "the agent backs up the plan and the user cancels it once the repository holds {int} packs"
)]
async fn backs_up_and_cancels_once_packs_saved(agent: &mut Agent, packs: usize) {
    let data = agent.repository().cold.join("data");
    agent
        .run_backup_cancelled_once(async move {
            while packs_in(&data) < packs {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
}

#[when(
    expr = "the agent backs up the plan and its deadline passes once the repository holds {int} packs"
)]
async fn backs_up_past_deadline_once_packs_saved(agent: &mut Agent, packs: usize) {
    let data = agent.repository().cold.join("data");
    agent
        .run_backup_past_its_deadline_once(async move {
            while packs_in(&data) < packs {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
}

/// The packs in a local repository's `data` folder: its files, but for the
/// temporary ones the local backend writes a pack to before renaming it.
fn packs_in(data: &Path) -> usize {
    let Ok(folders) = fs::read_dir(data) else {
        return 0;
    };
    folders
        .flatten()
        .filter_map(|folder| fs::read_dir(folder.path()).ok())
        .flatten()
        .flatten()
        .filter(|file| !file.file_name().to_string_lossy().contains("-tmp-"))
        .count()
}

#[then(expr = "the repository has {int} snapshot(s)")]
async fn has_snapshots(agent: &mut Agent, count: usize) {
    assert_eq!(snapshots(&agent.repository()).await.len(), count);
}

#[then("the repository has no snapshots")]
async fn has_no_snapshots(agent: &mut Agent) {
    let snapshots = snapshots(&agent.repository()).await;
    assert!(snapshots.is_empty(), "{snapshots:?}");
}

/// rustic's own check, reading every pack: a pack no index lists is one of
/// its warnings.
#[then("a check of the repository finds nothing to warn about")]
async fn check_finds_nothing(agent: &mut Agent) {
    let folders = agent.repository();
    let findings = blocking(move || {
        let results = repository(&folders)
            .open(&credentials())
            .expect("the scratch repository opens")
            .check(rustic_core::CheckOptions::default().read_data(true))
            .expect("the check runs");
        results
            .0
            .iter()
            .map(|(level, finding)| format!("{level:?}: {finding}"))
            .collect::<Vec<_>>()
    })
    .await;
    assert!(findings.is_empty(), "{findings:#?}");
}

#[then("the backup adds less data than the folder holds")]
fn adds_less_than_the_folder(agent: &mut Agent) {
    let Some(EngineResult::Backup(summary)) = &agent.finished().result else {
        panic!("the operation made no backup: {:?}", agent.operation());
    };
    let folder = agent.backup.source_bytes.expect("a folder of known size");
    assert!(
        summary.data_added < folder,
        "the backup added {} of the folder's {folder} bytes",
        summary.data_added
    );
}

#[then("the snapshot is tagged with the plan and the agent")]
async fn tagged(agent: &mut Agent) {
    let snapshot = snapshots(&agent.repository())
        .await
        .pop()
        .expect("a snapshot");
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
    let latest = snapshots(&agent.repository())
        .await
        .pop()
        .expect("a snapshot");
    let Some(EngineResult::Backup(summary)) = &detail.record.result else {
        panic!("the history holds no backup result: {:?}", detail.record);
    };
    assert_eq!(summary.snapshot, snapshot_id(&latest));
    assert_eq!(detail.record.kind, OperationKind::Backup);
    assert_eq!(detail.record.trigger, Trigger::Manual);
    assert_eq!(detail.record.target.plan, agent.backup.plan);
}

async fn new_snapshot_parent(agent: &Agent) -> Option<SnapshotId> {
    let latest = snapshots(&agent.repository())
        .await
        .pop()
        .expect("a snapshot");
    latest
        .parent
        .map(|p| SnapshotId::parse(p.to_hex().as_str()).unwrap())
}

#[then("the new snapshot's parent is the plan's previous snapshot")]
async fn parent_is_previous(agent: &mut Agent) {
    assert_eq!(new_snapshot_parent(agent).await, agent.backup.earlier);
}

#[then("the new snapshot's parent is the snapshot made without Ladderback")]
async fn parent_is_plain(agent: &mut Agent) {
    assert_eq!(new_snapshot_parent(agent).await, agent.backup.earlier);
}

/// What the backup scenarios set up.
#[derive(Debug, Default)]
pub struct BackupSetup {
    /// The repository's folder; next to a `hot`, its cold half.
    pub repository: Option<PathBuf>,
    pub hot: Option<PathBuf>,
    pub plan: Option<domain::PlanId>,
    pub source: Option<PathBuf>,
    /// How many bytes the plan's folder holds, where a step made it so.
    pub source_bytes: Option<u64>,
    /// A snapshot made before the one under test.
    pub earlier: Option<SnapshotId>,
    pub deadline_budget: Option<SignedDuration>,
    /// The plan's own host, written to the daemon's configuration.
    pub plan_host: Option<domain::planning::Hostname>,
    /// The agent's hostname, written to the daemon's configuration.
    pub agent_hostname: Option<domain::planning::Hostname>,
    /// The plan's source filter, written to the daemon's configuration.
    pub filter: domain::planning::SourceFilter,
    /// The repository ID the job names as the one it must open (§7.3):
    /// what the configuration recorded when the repository was connected.
    pub id: Option<RepositoryId>,
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
                id: self.backup.id.clone(),
            },
            sources: vec![self.source().to_str().unwrap().to_owned()],
            tags: self.plan_tags(),
            extra_tags: Vec::new(),
            label: None,
            host: None,
            filter: domain::planning::SourceFilter::default(),
            deadline_budget: self
                .backup
                .deadline_budget
                .unwrap_or_else(|| backup_budget(None, None)),
        })
    }
}
