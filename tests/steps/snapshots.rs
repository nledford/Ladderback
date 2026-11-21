//! Steps for the `@e2e` scenarios of `features/snapshots/list.feature` and
//! `features/snapshots/browse.feature`: the `ListSnapshots` and
//! `BrowseSnapshot` use cases run against the real worker and real scratch
//! repositories. The repositories are made and inspected with `rustic_core`
//! itself, never through Ladderback, so the checks don't trust the code
//! under test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use domain::browse::{EntryKind, FolderPage, FolderPath};
use domain::planning::RepositorySettings;
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, SnapshotId, Uuid};
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, KeyOptions, PathList, Repository, RepositoryOptions,
    SnapshotOptions,
};

use app::browse::{BrowseError, BrowseSnapshot};
use app::ports::SnapshotSummary;
use app::secrets::ResolveRepository;
use app::snapshots::{ListSnapshots, SnapshotError};
use platform::SystemClock;
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

/// What the scenarios set up, and what the listing or the browse gave.
#[derive(Default)]
pub struct SnapshotsSetup {
    cold: Option<PathBuf>,
    settings: Option<RepositorySettings>,
    listed: Option<Result<Vec<SnapshotSummary>, SnapshotError>>,
    /// The snapshot a browse opens.
    snapshot: Option<SnapshotId>,
    /// The folder the last browse opened, and the page it gave.
    opened: Option<(FolderPath, Result<FolderPage, BrowseError>)>,
    /// The real path of the folder the snapshot's top stands for, when its
    /// source was backed up at its own path.
    backed_up_from: Option<PathBuf>,
    /// The status and body of the daemon's answer to a listing over HTTP.
    answered: Option<(u16, String)>,
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
    pub(super) fn next_snapshot_op(&mut self) -> OperationId {
        self.runs += 1;
        OperationId::from_uuid(Uuid::from_u128(self.runs))
    }

    /// The snapshot the scenario made.
    pub(super) fn snapshot_made(&self) -> SnapshotId {
        self.snapshots
            .snapshot
            .clone()
            .expect("the scenario made a snapshot")
    }

    /// The real path of the folder the snapshot's top stands for, when its
    /// source was backed up at its own path: the scratch folder.
    pub(super) fn snapshot_backed_up_from(&self) -> Option<&Path> {
        self.snapshots.backed_up_from.as_deref()
    }

    /// The configuration's entry for the scenario's repository, under an
    /// alias of its own, recording no ID: a read opens whatever is at the
    /// location.
    pub(super) fn snapshot_entry(&self) -> domain::planning::RepositoryConfig {
        domain::planning::RepositoryConfig {
            locations: Default::default(),
            settings: self
                .snapshots
                .settings
                .clone()
                .expect("the scenario set a location"),
            id: None,
            maintenance: domain::planning::Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        }
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

/// Initialises a scratch repository and makes one snapshot in it, recorded
/// at `time`: a date, a time of day and the offset of the clock that took
/// it, read as rustic reads the time of a snapshot file.
#[given(expr = "a repository with a snapshot taken at {string}")]
async fn a_repository_with_a_snapshot_taken_at(agent: &mut Agent, time: String) {
    let cold = agent.scratch().join("repo");
    init(&cold).await;
    let source = agent.scratch().join("source");
    fs::create_dir_all(&source).expect("the source folder");
    fs::write(source.join("a.txt"), b"hello").expect("a file to back up");
    let when = rustic_core::repofile::RusticTime::parse_utc(&time).expect("a time");
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
        let options = SnapshotOptions::default()
            .host("demo-host".to_owned())
            .time(Some(when));
        repo.backup(
            &BackupOptions::default(),
            &paths,
            options.to_snapshot().expect("the snapshot options"),
        )
        .expect("a plain rustic backup");
    })
    .await;
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
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

/// A repository's folder that isn't there, as an unplugged drive's isn't.
#[given("a repository folder that isn't there")]
fn a_repository_folder_that_is_not_there(agent: &mut Agent) {
    let cold = agent.scratch().join("unplugged").join("repo");
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
}

#[given("a wrong password is used for the repository")]
fn a_wrong_password(agent: &mut Agent) {
    agent.snapshot_settings().password = SecretRef::Inline(SecretValue::new("not-the-password"));
}

#[when("the agent lists the repository's snapshots")]
async fn lists(agent: &mut Agent) {
    let use_case = ListSnapshots::new(
        agent.engine.clone(),
        Arc::new(SystemClock),
        ResolveRepository::new(Arc::new(SystemSecrets::new())),
        Arc::default(),
    );
    let entry = agent.snapshot_entry();
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

#[then(expr = "the listing shows the snapshot at {string} with the offset {string}")]
fn shows_the_snapshot_with_its_offset(agent: &mut Agent, instant: String, offset: String) {
    let listed = agent
        .snapshots
        .listed
        .take()
        .expect("the listing ran")
        .expect("the listing succeeds");
    let [snapshot] = listed.as_slice() else {
        panic!("one snapshot listed: {listed:?}");
    };
    let instant: domain::Timestamp = instant.parse().expect("an RFC 3339 instant");
    let offset: api::ClockOffset = offset.parse().expect("an RFC 3339 offset");
    assert_eq!(snapshot.time, instant, "the instant it was taken at");
    assert_eq!(snapshot.offset, offset.get(), "the offset of its clock");
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

#[then(
    "the listing fails because no repository was found, in the engine's words naming the folder"
)]
fn fails_as_no_config(agent: &mut Agent) {
    let listed = agent.snapshots.listed.take().expect("the listing ran");
    let cold = text(agent.snapshots.cold.as_ref().expect("the location"));
    let Err(SnapshotError::Failed(domain::execution::FailureReason::RepositoryConfigNotFound {
        detail,
    })) = &listed
    else {
        panic!("the listing was {listed:?}");
    };
    assert!(detail.contains(&cold), "{detail} names no {cold}");
}

/// The daemon on the folders `the daemon's config file holds:` writes the
/// file into.
#[given("Ladderback is serving on loopback on that configuration")]
async fn serving_on_that_configuration(agent: &mut Agent) {
    agent.web.home = Some(agent.scratch().join("daemon"));
    super::request_checks_steps::serve_on_loopback(agent).await;
}

#[when(expr = "the snapshots of the repository {string} are listed over HTTP")]
async fn listed_over_http(agent: &mut Agent, alias: String) {
    let addr = agent.web.server().addr;
    let path = format!("/api/v1/repositories/{alias}/snapshots");
    let answer =
        blocking(move || super::operations_steps::exchange_at(addr, "GET", &path, "")).await;
    agent.snapshots.answered = Some(answer);
}

/// The engine's words name where it looked: an address that held a
/// password, so words without it were masked, not silent (I4).
#[then(
    expr = "the listing is refused as {string} with status {int}, in the engine's own words naming {string}"
)]
fn refused_in_the_engines_words(agent: &mut Agent, code: String, status: u16, named: String) {
    let (got, body) = agent.snapshots.answered.as_ref().expect("the listing ran");
    assert_eq!(*got, status, "{body}");
    let refusal: serde_json::Value = serde_json::from_str(body).expect("a JSON refusal");
    assert_eq!(refusal["code"], code.as_str(), "{refusal}");
    let detail = refusal["detail"].as_str().unwrap_or_default();
    assert!(detail.contains(named.as_str()), "{refusal}");
}

#[then(expr = "neither the refusal nor Ladderback's log shows {string} or {string}")]
fn neither_shows(agent: &mut Agent, one: String, other: String) {
    let (_, body) = agent.snapshots.answered.as_ref().expect("the listing ran");
    let log = fs::read_to_string(agent.web.log()).expect("the daemon's log");
    for (name, text) in [("the refusal", body), ("the daemon's log", &log)] {
        for secret in [&one, &other] {
            assert!(
                !text.contains(secret.as_str()),
                "{secret:?} in {name}: {text}"
            );
        }
    }
}

/// The source the browse scenarios back up, under `source`: `docs` with two
/// files of known size and modification time and a folder, an empty folder,
/// a folder of 1,001 files, one more than a page holds, and `names`, whose
/// entries' names rustic writes in its escaped form: a quote, a backslash,
/// an accent, a line feed, a tab and a right-to-left override, and a folder
/// `say "hi"` holding a folder `inner`. (A byte that isn't UTF-8 is a unit
/// test's: APFS refuses such a name.)
fn source_tree(source: &Path) {
    let docs = source.join("docs");
    fs::create_dir_all(docs.join("drafts")).expect("the docs folder");
    fs::create_dir_all(source.join("empty")).expect("the empty folder");
    let many = source.join("many");
    fs::create_dir_all(&many).expect("the folder of many files");
    for (name, content, modified) in [
        ("notes.md", "hello", "2026-11-01T12:00:00Z"),
        ("report.txt", "hello world", "2026-11-02T08:30:00Z"),
    ] {
        let path = docs.join(name);
        fs::write(&path, content).expect("a file to back up");
        let modified: domain::Timestamp = modified.parse().expect("an RFC 3339 instant");
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("the file")
            .set_modified(SystemTime::from(modified))
            .expect("its modification time");
    }
    for at in 0..1_001 {
        fs::write(many.join(format!("file-{at:04}.txt")), b"x").expect("a file to back up");
    }
    let names = source.join("names");
    fs::create_dir_all(names.join("say \"hi\"").join("inner")).expect("the folder of names");
    for name in [
        "back\\slash.txt",
        "café.txt",
        "new\nline.txt",
        "tab\there.txt",
        "abc\u{202e}txt.exe",
    ] {
        fs::write(names.join(name), b"x").expect("a file to back up");
    }
}

/// Backs up the browse scenarios' source, `source` in the scratch folder,
/// into a new scratch repository: as `/source`, so the snapshot's top folder
/// holds `source`, or, `at_own_path`, at the source's real path with its
/// symbolic links resolved, so its original paths are inside the scratch
/// folder, where a restore in place may write.
pub(super) async fn snapshot_of_source(agent: &mut Agent, at_own_path: bool) {
    let cold = agent.scratch().join("repo");
    init(&cold).await;
    let source = agent.scratch().join("source");
    let at = cold.clone();
    let snapshot = blocking(move || {
        source_tree(&source);
        let source = fs::canonicalize(&source).expect("the source's real path");
        let repo = repository(&at)
            .open(&credentials())
            .expect("the scratch repository opens")
            .to_indexed_ids()
            .expect("the scratch repository indexes");
        let paths = PathList::from_iter([&source])
            .sanitize()
            .expect("the paths");
        let options = SnapshotOptions::default().host("demo-host".to_owned());
        let mut backup = BackupOptions::default();
        if !at_own_path {
            backup = backup.as_path(PathBuf::from("/source"));
        }
        repo.backup(
            &backup,
            &paths,
            options.to_snapshot().expect("the snapshot options"),
        )
        .expect("a plain rustic backup")
    })
    .await;
    agent.snapshots.snapshot =
        Some(SnapshotId::parse(&snapshot.id.to_hex()).expect("a snapshot ID"));
    agent.snapshots.backed_up_from = at_own_path
        .then(|| fs::canonicalize(agent.scratch()).expect("the scratch folder's real path"));
    agent.snapshots.cold = Some(cold.clone());
    agent.snapshot_settings().repository = text(&cold);
}

#[given("a repository with a snapshot of a source folder")]
async fn a_repository_with_a_snapshot_of_a_source_folder(agent: &mut Agent) {
    snapshot_of_source(agent, false).await;
}

#[given("a repository with a snapshot of a source folder at its own path")]
async fn a_repository_with_a_snapshot_of_a_source_folder_at_its_own_path(agent: &mut Agent) {
    snapshot_of_source(agent, true).await;
}

#[given("the snapshot has been forgotten")]
async fn the_snapshot_has_been_forgotten(agent: &mut Agent) {
    let cold = agent.snapshots.cold.clone().expect("the location");
    let id: rustic_core::repofile::SnapshotId = agent
        .snapshots
        .snapshot
        .as_ref()
        .expect("the scenario made a snapshot")
        .as_str()
        .parse()
        .expect("rustic reads its own IDs");
    blocking(move || {
        repository(&cold)
            .open(&credentials())
            .expect("the scratch repository opens")
            .delete_snapshots(&[id])
            .expect("rustic forgets the snapshot");
    })
    .await;
}

/// Opens the page of `folder` of the scenario's snapshot that starts at
/// `from`, through the browse use case and the real worker.
async fn open(agent: &mut Agent, folder: FolderPath, from: u64) {
    let use_case = BrowseSnapshot::new(
        agent.engine.clone(),
        Arc::new(SystemClock),
        ResolveRepository::new(Arc::new(SystemSecrets::new())),
        Arc::default(),
    );
    let entry = agent.snapshot_entry();
    let snapshot = agent
        .snapshots
        .snapshot
        .clone()
        .expect("the scenario made a snapshot");
    let alias = domain::RepositoryAlias::parse("demo").expect("an alias");
    let op = agent.next_snapshot_op();
    let page = use_case
        .folder(op, &alias, &entry, snapshot, folder.clone(), from, 1_000)
        .await;
    agent.snapshots.opened = Some((folder, page));
}

#[when("the agent opens the snapshot")]
async fn opens_the_snapshot(agent: &mut Agent) {
    open(agent, FolderPath::top(), 0).await;
}

#[when(expr = "the agent opens the folder {string} of the snapshot")]
async fn opens_the_folder(agent: &mut Agent, path: String) {
    open(agent, FolderPath::parse(&path).expect("a folder path"), 0).await;
}

#[when("the agent opens the next page of the folder")]
async fn opens_the_next_page(agent: &mut Agent) {
    let (folder, page) = agent.snapshots.opened.take().expect("a folder was opened");
    let next = page
        .expect("the folder opened")
        .next
        .expect("the page says where the next starts");
    open(agent, folder, next).await;
}

#[when(expr = "the agent opens the folder it shows as {string} in {string}")]
async fn opens_the_folder_shown_as(agent: &mut Agent, shown: String, path: String) {
    let parent = FolderPath::parse(&path).expect("a folder path");
    open(agent, parent.clone(), 0).await;
    let page = agent.page();
    let key = page
        .entries
        .iter()
        .find(|entry| entry.display.as_str() == shown)
        .unwrap_or_else(|| panic!("{shown} shown in {page:?}"))
        .name
        .clone();
    open(agent, parent.join(key).expect("a folder path"), 0).await;
}

impl Agent {
    /// The page the last browse gave, which must have opened.
    fn page(&self) -> &FolderPage {
        let (folder, page) = self.snapshots.opened.as_ref().expect("a folder was opened");
        match page {
            Ok(page) => page,
            Err(error) => panic!("the folder {folder:?} didn't open: {error:?}"),
        }
    }
}

#[then(expr = "the folder lists {string} as a folder")]
fn lists_a_folder(agent: &mut Agent, name: String) {
    let page = agent.page();
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name} in {page:?}"));
    assert_eq!(entry.kind, EntryKind::Folder, "{entry:?}");
    assert_eq!(entry.size, None, "a folder has no size of its own");
}

#[then(expr = "the folder lists {string} as a file of {int} bytes modified at {string}")]
fn lists_a_file(agent: &mut Agent, name: String, size: u64, modified: String) {
    let page = agent.page();
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name} in {page:?}"));
    let modified: domain::Timestamp = modified.parse().expect("an RFC 3339 instant");
    assert_eq!(entry.kind, EntryKind::File, "{entry:?}");
    assert_eq!(entry.size, Some(size), "{entry:?}");
    assert_eq!(entry.modified, Some(modified), "{entry:?}");
}

#[then("the folder is read in one page")]
fn is_one_page(agent: &mut Agent) {
    let page = agent.page();
    assert_eq!(page.next, None, "{page:?}");
    assert_eq!(page.total, page.entries.len() as u64, "{page:?}");
}

#[then("the folder is empty")]
fn the_folder_is_empty(agent: &mut Agent) {
    let page = agent.page();
    assert!(page.entries.is_empty(), "{page:?}");
    assert_eq!((page.total, page.next), (0, None), "{page:?}");
}

#[then("the folder shows its entries as:")]
fn shows_entries_as(agent: &mut Agent, step: &Step) {
    let page = agent.page();
    let table = step.table.as_ref().expect("the step has a table");
    let mut shown: Vec<(&str, &str)> = page
        .entries
        .iter()
        .map(|entry| {
            let kind = match entry.kind {
                EntryKind::Folder => "folder",
                EntryKind::File => "file",
                EntryKind::Link => "link",
                EntryKind::Other => "other",
            };
            (entry.display.as_str(), kind)
        })
        .collect();
    let mut expected: Vec<(&str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| (row[0].as_str(), row[1].as_str()))
        .collect();
    shown.sort_unstable();
    expected.sort_unstable();
    assert_eq!(shown, expected, "{page:?}");
}

#[then(expr = "the page lists {int} of the folder's {int} entries and says where the next starts")]
fn lists_a_page_of_more(agent: &mut Agent, listed: usize, total: u64) {
    let page = agent.page();
    assert_eq!(page.entries.len(), listed);
    assert_eq!(page.total, total);
    assert_eq!(
        page.next,
        Some(listed as u64),
        "the next page starts after this one"
    );
    let first = page.entries.first().map(|entry| entry.name.as_str());
    assert_eq!(first, Some("file-0000.txt"), "in the folder's order");
}

#[then(expr = "the page lists {int} of the folder's {int} entries and is the folder's last")]
fn lists_the_last_page(agent: &mut Agent, listed: usize, total: u64) {
    let page = agent.page();
    assert_eq!(page.entries.len(), listed);
    assert_eq!(page.total, total);
    assert_eq!(page.next, None, "{page:?}");
    let last = page.entries.last().map(|entry| entry.name.as_str());
    assert_eq!(last, Some("file-1000.txt"), "the entry the first page left");
}

#[then("the snapshot is said to be unknown")]
fn the_snapshot_is_unknown(agent: &mut Agent) {
    let (_, page) = agent
        .snapshots
        .opened
        .as_ref()
        .expect("a folder was opened");
    assert_eq!(page, &Err(BrowseError::SnapshotUnknown));
}

#[then("the folder is said to be unknown")]
fn the_folder_is_unknown(agent: &mut Agent) {
    let (_, page) = agent
        .snapshots
        .opened
        .as_ref()
        .expect("a folder was opened");
    assert_eq!(page, &Err(BrowseError::FolderUnknown));
}
