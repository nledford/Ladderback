//! Steps for `features/execution/cache.feature`: the cache slots the real
//! daemon's workers keep rustic's cache in (§4.1), looked at in the cache
//! folder the daemon was given (`LADDERBACK_CACHE_DIR`, which every daemon a
//! test starts gets from `testkit::daemon::isolate`).

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use cucumber::{given, then};

use super::Agent;
use super::events_steps::{Events, save_daemon_config};
use super::operations_steps::has_ended;

/// How long a slot may take to show what a worker did with it.
const SLOT_TIMEOUT: Duration = Duration::from_secs(120);

/// What the cache scenarios keep between steps: the files the first backup
/// left in the slot, each with its size and when it was last written.
#[derive(Debug, Default)]
pub struct CacheE2e {
    left: BTreeMap<PathBuf, (u64, SystemTime)>,
}

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
