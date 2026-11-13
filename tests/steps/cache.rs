//! Steps for `features/execution/cache.feature`: the cache slots the real
//! daemon's workers keep rustic's cache in (§4.1), looked at in the cache
//! folder the daemon was given (`LADDERBACK_CACHE_DIR`, which every daemon a
//! test starts gets from `testkit::daemon::isolate`), and what the daemon
//! removes from that folder when it starts.

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use app::ports::OperationStore;
use cucumber::{given, then};
use domain::{OperationId, Uuid};
use store::SqliteStore;

use super::Agent;
use super::events_steps::{Events, save_daemon_config};
use super::operations_steps::has_ended;

/// How long a slot may take to show what a worker did with it.
const SLOT_TIMEOUT: Duration = Duration::from_secs(120);

/// The files under a folder, by their path under it, with each one's size
/// and when it was last written.
type Files = BTreeMap<PathBuf, (u64, SystemTime)>;

/// What the cache scenarios keep between steps: the files the first backup
/// left in the slot, each with its size and when it was last written.
#[derive(Debug, Default)]
pub struct CacheE2e {
    left: BTreeMap<PathBuf, (u64, SystemTime)>,
    /// What each slot the scenario filled held, by the slot's number.
    filled: BTreeMap<u32, Files>,
    /// The unfinished files the scenario left, by slot and age in hours,
    /// with their sizes.
    unfinished: BTreeMap<(u32, u64), (PathBuf, u64)>,
    /// The size of the cache of the repository no entry records.
    unrecorded_bytes: u64,
    /// What that repository's cache slot 0 held.
    unrecorded_first: Files,
    /// The slot locks the scenario holds as another daemon's workers.
    held: Vec<File>,
    /// The folder outside the cache folder, and what it held.
    outside: Option<(PathBuf, Files)>,
}

/// The ID of a repository no entry of the scenario's configuration records.
const UNRECORDED: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

/// The folder of the repository's cache slot `slot`: the cache folder, the
/// repository's ID, the slot's number.
fn slot_dir(agent: &Agent, slot: u32) -> PathBuf {
    let id = agent
        .backup
        .id
        .as_ref()
        .expect("the configuration records the repository's ID");
    Events::cache_dir(agent)
        .join(id.as_str())
        .join(slot.to_string())
}

/// Whether another process holds the lock on `dir`. Looking takes the lock
/// for a moment when nobody holds it.
fn held(dir: &Path) -> bool {
    let folder = File::open(dir).expect("the slot's folder opens");
    match folder.try_lock() {
        Ok(()) => false,
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(error)) => panic!("the slot's lock can't be looked at: {error}"),
    }
}

/// Every file under `dir`, relative to it, with its size and when it was
/// last written.
fn files(dir: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    fn walk(dir: &Path, base: &Path, found: &mut BTreeMap<PathBuf, (u64, SystemTime)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                walk(&path, base, found);
            } else if let (Ok(relative), Ok(modified)) =
                (path.strip_prefix(base), metadata.modified())
            {
                found.insert(relative.to_owned(), (metadata.len(), modified));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// How many of `files` lie in rustic's cache folder of the file type `kind`
/// (`index`, `snapshots`), which rustic keeps under the repository's ID.
fn of_kind(files: &BTreeMap<PathBuf, (u64, SystemTime)>, kind: &str) -> usize {
    files
        .keys()
        .filter(|path| {
            path.components()
                .nth(1)
                .is_some_and(|part| part.as_os_str() == kind)
        })
        .count()
}

#[given("the configuration records the repository's ID")]
async fn the_configuration_records_the_id(agent: &mut Agent) {
    super::backup_steps::id_known(agent).await;
    // The daemon reads its configuration on every request.
    save_daemon_config(agent, None).await;
}

#[given("the configuration no longer records the repository's ID")]
async fn the_configuration_records_no_id(agent: &mut Agent) {
    // The scenario still knows the ID, to find the repository's slots.
    let id = agent.backup.id.take();
    save_daemon_config(agent, None).await;
    agent.backup.id = id;
}

/// The backup's worker holds its slot's lock for as long as it runs, so the
/// scenario holds the backup running (the plan's source holds a file the
/// backup waits on): a backup that ended before the lock was looked at
/// would have let it go already.
#[then(expr = "the repository's cache slot {int} is held while the backup runs")]
async fn held_while_it_runs(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    let operation = agent.ops.the_operation();
    // rustic tags the folder as a cache once it has opened the repository
    // with it, which the worker does only after it took the slot's lock:
    // looking at the lock any earlier could take it from under the worker.
    let tag = dir.join("CACHEDIR.TAG");
    let started = Instant::now();
    let mut looks = 0_u32;
    while !tag.exists() {
        looks += 1;
        // The history page now and then: a backup that ended without the
        // slot will never use it.
        if looks.is_multiple_of(20) && has_ended(agent, &operation).await && !tag.exists() {
            panic!(
                "the backup ended without using the cache slot {}",
                dir.display()
            );
        }
        assert!(
            started.elapsed() < SLOT_TIMEOUT,
            "no worker used the cache slot {} within {} s",
            dir.display(),
            SLOT_TIMEOUT.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        held(&dir),
        "the running backup's cache slot {} is not locked",
        dir.display()
    );
}

#[then(expr = "the repository's cache slot {int} is free once the backup has ended")]
async fn free_once_ended(agent: &mut Agent, slot: u32) {
    let id = agent.ops.the_operation();
    super::operations_steps::wait_ended(agent, &id).await;
    let dir = slot_dir(agent, slot);
    // The operation ends with the worker's result, a moment before the
    // worker's process is gone and its lock with it.
    let started = Instant::now();
    while held(&dir) {
        assert!(
            started.elapsed() < SLOT_TIMEOUT,
            "the cache slot {} stayed locked after its backup ended",
            dir.display()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[then(
    expr = "the repository's cache slot {int} holds rustic's cache of the index and the snapshot"
)]
fn holds_the_cache(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    let left = files(&dir);
    assert!(
        of_kind(&left, "index") >= 1 && of_kind(&left, "snapshots") == 1,
        "the cache slot {} holds no index and snapshot: {:?}",
        dir.display(),
        left.keys().collect::<Vec<_>>()
    );
    agent.cache.left = left;
}

#[given(
    expr = "the repository already holds {int} snapshots of the plan {string} that another tool made"
)]
async fn snapshots_another_tool_made(agent: &mut Agent, count: usize, plan: String) {
    let tags = super::adopt_steps::plan_tags(agent, &plan);
    for _ in 0..count {
        super::adopt_steps::back_up_with_tags(agent, &tags).await;
    }
}

/// rustic keeps a repository's tree packs in its cache as `data`: those of
/// the parent snapshot the backup read, and those it wrote.
#[then(
    expr = "the repository's cache slot {int} holds rustic's cache of the index, {int} snapshots and the trees"
)]
fn holds_index_snapshots_and_trees(agent: &mut Agent, slot: u32, snapshots: usize) {
    let dir = slot_dir(agent, slot);
    let left = files(&dir);
    assert!(
        of_kind(&left, "index") >= 1
            && of_kind(&left, "snapshots") == snapshots
            && of_kind(&left, "data") >= 1,
        "the cache slot {} holds not the index, {snapshots} snapshots and the trees: {:?}",
        dir.display(),
        left.keys().collect::<Vec<_>>()
    );
}

#[then("the operation log names the repository")]
async fn the_log_names_the_repository(agent: &mut Agent) {
    let operation = agent.ops.the_operation();
    let id = OperationId::from_uuid(Uuid::parse_str(&operation).expect("an operation's ID"));
    let path = Events::data_dir(agent).join("ladderback.db");
    // The daemon keeps the log in WAL mode, so it can be read beside it.
    let store = SqliteStore::open(&path)
        .await
        .expect("the operation log opens beside the daemon");
    let detail = store.get(id).await.expect("the log answers");
    store.close().await;
    let record = detail.expect("the log holds the backup").record;
    assert_eq!(
        record.target.repository.as_ref(),
        Some(
            agent
                .backup
                .id
                .as_ref()
                .expect("the scenario knows the repository's ID")
        ),
        "the backup's operation names no repository, or another"
    );
}

#[then("the second backup kept what the cache slot held and added its snapshot to it")]
fn kept_and_added(agent: &mut Agent) {
    let dir = slot_dir(agent, 0);
    let now = files(&dir);
    // A file rustic had to read from the repository again would have been
    // written again, or would be gone from a cache the backup didn't use.
    for (path, before) in &agent.cache.left {
        assert_eq!(
            now.get(path),
            Some(before),
            "{} in the cache slot was not kept as the first backup left it",
            path.display()
        );
    }
    assert_eq!(
        of_kind(&now, "snapshots"),
        2,
        "the second backup's snapshot is not in the cache slot {}: {:?}",
        dir.display(),
        now.keys().collect::<Vec<_>>()
    );
}

/// Writes `bytes` bytes at `path`, making the folders above it.
fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().expect("a folder above")).expect("the folders are made");
    std::fs::write(path, vec![7_u8; bytes]).expect("the file is written");
}

/// Fills `dir` as rustic fills a slot: its tag, and an index file under the
/// repository's ID. Returns what it holds and the size of it.
fn fill(dir: &Path) -> (Files, u64) {
    write(&dir.join("CACHEDIR.TAG"), 43);
    write(
        &dir.join("ab".repeat(32))
            .join("index")
            .join("cd")
            .join("cd".repeat(32)),
        1000,
    );
    let held = files(dir);
    let bytes = held.values().map(|(size, _)| size).sum();
    (held, bytes)
}

/// The lines of the daemon's start-up log that name `repository` and say
/// `word`.
fn said<'a>(log: &'a str, repository: &str, word: &str) -> Vec<&'a str> {
    log.lines()
        .filter(|line| line.contains(repository) && line.contains(word))
        .collect()
}

fn repository_id(agent: &Agent) -> String {
    agent
        .backup
        .id
        .as_ref()
        .expect("the configuration records the repository's ID")
        .as_str()
        .to_owned()
}

#[given(expr = "the repository's cache slot {int} holds a cache")]
fn slot_holds_a_cache(agent: &mut Agent, slot: u32) {
    let (held, _) = fill(&slot_dir(agent, slot));
    agent.cache.filled.insert(slot, held);
}

#[given("the cache folder holds the cache of a repository no entry records")]
fn unrecorded_cache(agent: &mut Agent) {
    let folder = Events::cache_dir(agent).join(UNRECORDED);
    let (held, first) = fill(&folder.join("0"));
    let (_, second) = fill(&folder.join("1"));
    agent.cache.unrecorded_bytes = first + second;
    agent.cache.unrecorded_first = held;
}

#[given("the cache folder holds a folder of the user's own")]
fn users_own_folder(agent: &mut Agent) {
    write(
        &Events::cache_dir(agent)
            .join("notes")
            .join("0")
            .join("readme"),
        10,
    );
}

#[given(expr = "a worker of another daemon holds the repository's cache slot {int}")]
fn another_worker_holds(agent: &mut Agent, slot: u32) {
    let folder = File::open(slot_dir(agent, slot)).expect("the slot's folder opens");
    folder.try_lock().expect("nobody else holds the slot");
    agent.cache.held.push(folder);
}

#[given(
    expr = "the repository's cache slot {int} holds an unfinished file written {int} hours ago"
)]
fn unfinished_file(agent: &mut Agent, slot: u32, hours: u64) {
    // rustic names it after the file it is writing: `<64 hex digits>-tmp-`.
    let path = slot_dir(agent, slot)
        .join("ab".repeat(32))
        .join("snapshots")
        .join(format!(
            "{}-tmp-",
            format!("{:02x}", hours % 256).repeat(32)
        ));
    let bytes = 300 + hours;
    write(&path, usize::try_from(bytes).expect("a small size"));
    File::options()
        .write(true)
        .open(&path)
        .expect("the file opens")
        .set_modified(SystemTime::now() - Duration::from_secs(hours * 60 * 60))
        .expect("the file's time is set");
    agent.cache.unfinished.insert((slot, hours), (path, bytes));
}

#[given("a folder outside the cache folder holds files")]
fn outside_folder(agent: &mut Agent) {
    let outside = agent.scratch().join("outside");
    write(&outside.join("photo.jpg"), 100);
    write(&outside.join("0").join("CACHEDIR.TAG"), 43);
    let held = files(&outside);
    agent.cache.outside = Some((outside, held));
}

fn outside(agent: &Agent) -> PathBuf {
    agent
        .cache
        .outside
        .as_ref()
        .expect("a folder outside the cache folder")
        .0
        .clone()
}

#[given("the cache folder holds a link to that folder, named as a repository no entry records")]
fn unrecorded_link(agent: &mut Agent) {
    std::os::unix::fs::symlink(outside(agent), Events::cache_dir(agent).join(UNRECORDED))
        .expect("the link is made");
}

#[given(expr = "the repository's cache slot {int} is a link to that folder")]
fn slot_is_a_link(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    std::fs::create_dir_all(dir.parent().expect("the repository's folder"))
        .expect("the repository's folder is made");
    std::os::unix::fs::symlink(outside(agent), dir).expect("the link is made");
}

#[given(expr = "the repository's cache slot {int} holds a cache and a link to that folder")]
fn slot_holds_a_link(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    let (held, _) = fill(&dir);
    agent.cache.filled.insert(slot, held);
    std::os::unix::fs::symlink(outside(agent), dir.join("away")).expect("the link is made");
}

#[then("the cache of the repository no entry records is gone")]
fn unrecorded_gone(agent: &mut Agent) {
    let folder = Events::cache_dir(agent).join(UNRECORDED);
    assert!(!folder.exists(), "{} is still there", folder.display());
}

#[then("the repository no entry records keeps its cache slot 0 and loses its cache slot 1")]
fn unrecorded_keeps_slot_0(agent: &mut Agent) {
    let folder = Events::cache_dir(agent).join(UNRECORDED);
    assert_eq!(
        files(&folder.join("0")),
        agent.cache.unrecorded_first,
        "the cache slot 0 of {UNRECORDED} was not kept"
    );
    let spare = folder.join("1");
    assert!(!spare.exists(), "{} is still there", spare.display());
}

#[then("the daemon's log says it removed that cache, with its size")]
fn log_says_unrecorded(agent: &mut Agent) {
    let log = agent.events.start_log();
    let size = format!(" {} bytes", agent.cache.unrecorded_bytes);
    assert!(
        said(log, UNRECORDED, "removed")
            .iter()
            .any(|line| line.contains(&size)),
        "no line of the log says the cache of {UNRECORDED} was removed, with{size}:\n{log}"
    );
}

#[then("the cache folder still holds the user's own folder")]
fn users_folder_kept(agent: &mut Agent) {
    let readme = Events::cache_dir(agent)
        .join("notes")
        .join("0")
        .join("readme");
    assert!(readme.is_file(), "{} is gone", readme.display());
}

#[then(expr = "the repository's cache slot {int} still holds its cache")]
fn slot_kept(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    let now = files(&dir);
    let filled = agent
        .cache
        .filled
        .get(&slot)
        .expect("the scenario filled the slot");
    for (path, before) in filled {
        assert_eq!(
            now.get(path),
            Some(before),
            "{} in the cache slot {slot} was not kept",
            path.display()
        );
    }
}

#[then(expr = "the repository's cache slot {int} is gone")]
fn slot_gone(agent: &mut Agent, slot: u32) {
    let dir = slot_dir(agent, slot);
    assert!(
        std::fs::symlink_metadata(&dir).is_err(),
        "{} is still there",
        dir.display()
    );
}

fn unfinished_path(agent: &Agent, slot: u32, hours: u64) -> PathBuf {
    agent
        .cache
        .unfinished
        .get(&(slot, hours))
        .expect("the scenario left that unfinished file")
        .0
        .clone()
}

#[then(
    expr = "the unfinished file written {int} hours ago in the repository's cache slot {int} is gone"
)]
fn unfinished_gone(agent: &mut Agent, hours: u64, slot: u32) {
    let path = unfinished_path(agent, slot, hours);
    assert!(!path.exists(), "{} is still there", path.display());
}

#[then(
    expr = "the unfinished file written {int} hours ago in the repository's cache slot {int} is kept"
)]
fn unfinished_kept(agent: &mut Agent, hours: u64, slot: u32) {
    let path = unfinished_path(agent, slot, hours);
    assert!(path.is_file(), "{} is gone", path.display());
}

#[then(
    "the daemon's log says it removed the spare cache slot and the unfinished file, with their sizes"
)]
fn log_says_spare_and_unfinished(agent: &mut Agent) {
    let id = repository_id(agent);
    let spare: u64 = agent
        .cache
        .filled
        .iter()
        .filter(|(slot, _)| !slot_dir(agent, **slot).exists())
        .flat_map(|(_, held)| held.values().map(|(size, _)| size))
        .sum();
    let unfinished: u64 = agent
        .cache
        .unfinished
        .values()
        .filter(|(path, _)| !path.exists())
        .map(|(_, bytes)| bytes)
        .sum();
    let log = agent.events.start_log();
    for (word, bytes) in [("spare", spare), ("unfinished", unfinished)] {
        let size = format!(" {bytes} bytes");
        assert!(
            said(log, &id, word).iter().any(|line| line.contains(&size)),
            "no line of the log names the {word} files of {id} removed, with{size}:\n{log}"
        );
    }
}

#[then("the folder outside the cache folder still holds its files")]
fn outside_kept(agent: &mut Agent) {
    let (outside, before) = agent
        .cache
        .outside
        .as_ref()
        .expect("a folder outside the cache folder");
    assert_eq!(&files(outside), before, "the folder outside was changed");
}
