//! Steps for the `@e2e` scenarios of `features/snapshots/list.feature`: the
//! `ListSnapshots` use case runs against the real worker and real scratch
//! repositories. The repositories are made and inspected with `rustic_core`
//! itself, never through Ladderback, so the checks don't trust the code
//! under test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cucumber::{given, then, when};
use domain::planning::RepositorySettings;
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, Uuid};
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, KeyOptions, PathList, Repository, RepositoryOptions,
    SnapshotOptions,
};

use app::ports::SnapshotSummary;
use app::secrets::ResolveRepository;
use app::snapshots::{ListSnapshots, SnapshotError};
use secrets::SystemSecrets;

use super::{Agent, blocking};

/// Not a real secret: the scratch repositories' password.
const PASSWORD: &str = "demo-password";

fn credentials() -> Credentials {
    Credentials::password(PASSWORD)
}

/// The scratch repository, without a cache: the tests never write into the
/// developer's own rustic cache.
fn repository(cold: &Path) -> Repository<()> {
    let backends = BackendOptions::default()
        .repository(cold.to_str().expect("a UTF-8 scratch path"))
        .to_backends()
        .expect("the scratch repository's backend");
    Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .expect("the scratch repository")
}

/// Every snapshot in the scratch repository, oldest first, as rustic knows
/// them: the truth the listing is checked against. Opening a repository
/// derives its key, which takes a second in a debug build, so this and the
/// steps below run it on a blocking thread: see [`blocking`].
async fn snapshots(cold: &Path) -> Vec<SnapshotFile> {
    let cold = cold.to_owned();
    blocking(move || {
        let mut snapshots = repository(&cold)
            .open(&credentials())
            .expect("the scratch repository opens")
            .get_all_snapshots()
            .expect("the snapshots load");
        snapshots.sort_by(|a, b| a.time.cmp(&b.time));
        snapshots
    })
    .await
}

/// Initialises a scratch repository at `cold`.
async fn init(cold: &Path) {
    let cold = cold.to_owned();
    blocking(move || {
        repository(&cold)
            .init(
                &credentials(),
                &KeyOptions::default(),
                &ConfigOptions::default(),
            )
            .expect("the scratch repository initialises");
    })
    .await;
}

/// What the scenarios set up, and what the listing gave.
#[derive(Default)]
pub struct SnapshotsSetup {
    cold: Option<PathBuf>,
    settings: Option<RepositorySettings>,
    listed: Option<Result<Vec<SnapshotSummary>, SnapshotError>>,
}

impl Agent {
    fn snapshot_settings(&mut self) -> &mut RepositorySettings {
        self.snapshots
            .settings
            .get_or_insert_with(|| RepositorySettings {
                repository: String::new(),
                repo_hot: None,
                password: SecretRef::Inline(SecretValue::new(PASSWORD)),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            })
    }

    /// A fresh operation ID for each use-case call.
    fn next_snapshot_op(&mut self) -> OperationId {
        self.runs += 1;
        OperationId::from_uuid(Uuid::from_u128(self.runs))
    }
}

fn text(path: &Path) -> String {
    path.to_str().expect("a UTF-8 scratch path").to_owned()
}

/// Initialises a scratch repository and makes three snapshots in it, with
/// the labels "first", "second" and "third" in that order of age.
async fn three_snapshots(agent: &mut Agent) {
    let cold = agent.scratch().join("repo");
    init(&cold).await;
    let source = agent.scratch().join("source");
    fs::create_dir_all(&source).expect("the source folder");
    fs::write(source.join("a.txt"), b"hello").expect("a file to back up");
    let at = cold.clone();
    blocking(move || {
        let repo = repository(&at)
            .open(&credentials())
            .expect("the scratch repository opens")
            .to_indexed_ids()
            .expect("the scratch repository indexes");
        let paths = PathList::from_iter([&source])
            .sanitize()
            .expect("the paths");
        for label in ["first", "second", "third"] {
            let options = SnapshotOptions::default()
                .label(label.to_owned())
                .host("demo-host".to_owned());
            repo.backup(
                &BackupOptions::default(),
                &paths,
                options.to_snapshot().expect("the snapshot options"),
            )
            .expect("a plain rustic backup");
        }
    })
    .await;
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
}

#[given("a repository with three snapshots")]
async fn a_repository_with_three_snapshots(agent: &mut Agent) {
    three_snapshots(agent).await;
}

#[given("a repository with no snapshots")]
async fn a_repository_with_no_snapshots(agent: &mut Agent) {
    let cold = agent.scratch().join("repo");
    init(&cold).await;
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
}

#[given("a location with no repository")]
fn a_location_with_no_repository(agent: &mut Agent) {
    let cold = agent.scratch().join("repo");
    fs::create_dir_all(&cold).expect("the empty location");
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
}

#[given("a wrong password is used for the repository")]
fn a_wrong_password(agent: &mut Agent) {
    agent.snapshot_settings().password = SecretRef::Inline(SecretValue::new("not-the-password"));
}

#[when("the agent lists the repository's snapshots")]
async fn lists(agent: &mut Agent) {
    let settings = agent
        .snapshots
        .settings
        .clone()
        .expect("the scenario set a location");
    let use_case = ListSnapshots::new(
        agent.engine.clone(),
        ResolveRepository::new(Arc::new(SystemSecrets::new())),
    );
    // A repository the scenario gave, under an alias of its own, recording
    // no ID: the listing opens whatever is at the location.
    let entry = domain::planning::RepositoryConfig {
        settings,
        id: None,
        maintenance: domain::planning::Maintenance::None,
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let alias = domain::RepositoryAlias::parse("demo").expect("an alias");
    agent.snapshots.listed = Some(
        use_case
            .list(agent.next_snapshot_op(), &alias, &entry)
            .await,
    );
}

#[then("the listing shows the three snapshots newest first with id, time, host and label")]
async fn shows_three_newest_first(agent: &mut Agent) {
    let listed = agent
        .snapshots
        .listed
        .take()
        .expect("the listing ran")
        .expect("the listing succeeds");
    let cold = agent.snapshots.cold.clone().expect("the location");
    let truth = snapshots(&cold).await;
    assert_eq!(listed.len(), 3, "{listed:?}");
    // rustic's oldest-first order reversed is the expected newest-first one.
    for (summary, snapshot) in listed.iter().zip(truth.iter().rev()) {
        assert_eq!(summary.id.as_str(), snapshot.id.to_hex().as_str());
        assert_eq!(summary.time, snapshot.time.timestamp());
        assert_eq!(summary.hostname, snapshot.hostname);
        assert_eq!(summary.label, snapshot.label);
    }
    let labels: Vec<&str> = listed.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["third", "second", "first"]);
}

#[then("the listing is empty")]
fn is_empty(agent: &mut Agent) {
    let listed = agent
        .snapshots
        .listed
        .take()
        .expect("the listing ran")
        .expect("the listing succeeds");
    assert!(listed.is_empty(), "{listed:?}");
}

#[then("the listing fails")]
fn fails(agent: &mut Agent) {
    let listed = agent.snapshots.listed.take().expect("the listing ran");
    assert!(listed.is_err(), "the listing was {listed:?}");
}
