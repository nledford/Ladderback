//! A cancelled backup (SIGTERM, the daemon's stop) writes no snapshot, and
//! what it had already saved stays in the repository's index (§4.1): a
//! check finds no pack the index doesn't list, and the next backup reuses
//! that data instead of saving it again.

mod support;

use std::fs;
use std::path::Path;
use std::time::Duration;

use app::ports::{Engine, EngineEvent, RunningJob};
use domain::SignedDuration;
use futures::StreamExt;
use rustic_core::{CheckOptions, ConfigOptions};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// How much the source holds: far more than a backup saves before the cancel.
const SOURCE_BYTES: usize = 64 << 20;

/// Data packs of 1 MiB, so whole packs are saved long before the backup ends.
fn small_packs() -> ConfigOptions {
    serde_json::from_value(serde_json::json!({
        "set-datapack-size": "1 MiB",
        "set-datapack-growfactor": 0,
    }))
    .unwrap()
}

/// The packs in a local folder of a repository: the files under `data`, but
/// for the temporary ones the local backend writes before renaming.
fn packs_in(folder: &Path) -> usize {
    let Ok(subfolders) = fs::read_dir(folder.join("data")) else {
        return 0;
    };
    subfolders
        .flatten()
        .filter_map(|subfolder| fs::read_dir(subfolder.path()).ok())
        .flatten()
        .flatten()
        .filter(|file| !file.file_name().to_string_lossy().contains("-tmp-"))
        .count()
}

/// The daemon's grace period in these tests: past the worker's own 30 s
/// limit on a wind-down (`WIND_DOWN_LIMIT` in `src/worker.rs`), so the
/// worker, not the daemon's SIGKILL, ends a backup that winds down. A
/// wind-down cut off by the grace period leaves the packs it wrote with no
/// index (decision 50), which is not what these tests are about: on a busy
/// machine one can outlast the 5 s that `engine()` gives (#285).
const PAST_THE_WIND_DOWN_LIMIT: Duration = Duration::from_secs(35);

/// Starts a backup, stops its worker once `packs` packs are in the cold half,
/// and reads its events to their end. A backup that ends first proves
/// nothing, so it fails the test.
async fn cancel_once_packs_are_saved(scratch: &Scratch, location: &Location, packs: usize) {
    let RunningJob {
        mut events,
        mut control,
    } = engine()
        .with_stop_grace(PAST_THE_WIND_DOWN_LIMIT)
        .run(op(), backup_job(location, &scratch.source(), "home", HOUR))
        .await
        .unwrap();
    let mut poll = tokio::time::interval(Duration::from_millis(5));
    loop {
        tokio::select! {
            event = events.next() => match event {
                Some(EngineEvent::Succeeded(_)) => panic!("the backup ended before the cancel"),
                Some(EngineEvent::Failed(reason)) => panic!("the backup failed: {reason:?}"),
                Some(_) => {}
                None => panic!("the backup's events ended before the cancel"),
            },
            _ = poll.tick() => if packs_in(&scratch.repo_dir()) >= packs {
                break;
            },
        }
    }
    control.stop().await;
    while let Some(event) = events.next().await {
        assert!(
            !matches!(event, EngineEvent::Succeeded(_)),
            "a cancelled backup reported a result: {event:?}"
        );
    }
}

/// Every finding of rustic's check, reading all data.
fn findings(location: &Location) -> Vec<String> {
    off_runtime(|| {
        location
            .open()
            .check(CheckOptions::default().read_data(true))
            .unwrap()
            .0
            .iter()
            .map(|(level, finding)| format!("{level:?}: {finding}"))
            .collect()
    })
}

async fn a_cancelled_backup_leaves_its_packs_indexed_and_no_snapshot(
    scratch: &Scratch,
    location: Location,
) {
    off_runtime(|| location.init_with(&small_packs()));
    let mut rng = Rng(0x5EED);
    for n in 0..SOURCE_BYTES / (4 << 20) {
        write_file(
            scratch.source().join(format!("file{n}.bin")),
            &rng.bytes(4 << 20),
        );
    }

    cancel_once_packs_are_saved(scratch, &location, 2).await;

    // Each half's own folder: a listing through rustic reads only one half.
    for half in [scratch.repo_dir(), scratch.hot_dir()] {
        let snapshots = fs::read_dir(half.join("snapshots")).map_or(0, |files| files.count());
        assert_eq!(
            snapshots,
            0,
            "a cancelled backup wrote a snapshot to {}",
            half.display()
        );
    }
    assert!(
        packs_in(&scratch.repo_dir()) >= 2,
        "the cancel came after packs were saved"
    );
    let findings = findings(&location);
    assert!(findings.is_empty(), "{findings:#?}");

    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "home", HOUR),
    )
    .await;
    let summary = backed_up(&events);
    assert!(
        summary.data_added < SOURCE_BYTES as u64,
        "the next backup saved {} of the source's {SOURCE_BYTES} bytes again",
        summary.data_added
    );
}

#[tokio::test]
async fn a_cancelled_backup_leaves_its_packs_indexed_and_no_snapshot_in_a_local_repository() {
    let scratch = Scratch::new();
    let location = scratch.local();
    a_cancelled_backup_leaves_its_packs_indexed_and_no_snapshot(&scratch, location).await;
}

#[tokio::test]
async fn a_cancelled_backup_leaves_its_packs_indexed_and_no_snapshot_in_either_half_of_a_hot_cold_repository()
 {
    let scratch = Scratch::new();
    let location = scratch.hot_cold();
    a_cancelled_backup_leaves_its_packs_indexed_and_no_snapshot(&scratch, location).await;
}

/// The stop's grace in [`a_cancelled_backup_whose_events_nobody_reads_still_leaves_its_packs_indexed`]:
/// far past the wind-down of a local repository (under a second on a busy
/// machine), and short of the worker's own 30 s limit, so a stalled
/// wind-down ends at this grace's SIGKILL.
#[cfg(unix)]
const PATIENT_GRACE: Duration = Duration::from_secs(20);

/// How many unreadable files follow the data. Each is a warning of more than
/// 400 bytes, its long path, so together they hold several times what the
/// event pipe holds (64 KiB).
#[cfg(unix)]
const UNREADABLE_FILES: usize = 400;

/// A cancel that lands while the worker is blocked writing to a full event
/// pipe, because nobody has read its events for a while, still lets the
/// backup wind down (§4.1): the stop reads what the worker writes meanwhile,
/// so rustic's threads aren't held writing, the index of the packs saved is
/// written, and the worker ends before the grace's SIGKILL.
#[cfg(unix)]
#[tokio::test]
async fn a_cancelled_backup_whose_events_nobody_reads_still_leaves_its_packs_indexed() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let location = scratch.local();
    off_runtime(|| location.init_with(&small_packs()));
    // rustic reads the source in path order: the data first, which fills
    // packs, then a flood of warnings, one per file it can't open.
    let mut rng = Rng(0x5EED);
    for n in 0..SOURCE_BYTES / (4 << 20) {
        write_file(
            scratch.source().join(format!("a/file{n}.bin")),
            &rng.bytes(4 << 20),
        );
    }
    let unreadable = scratch.source().join("b").join("d".repeat(200));
    for n in 0..UNREADABLE_FILES {
        let file = unreadable.join(format!("{}{n:03}", "f".repeat(200)));
        write_file(&file, b"x");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    }
    if fs::File::open(unreadable.join(format!("{}000", "f".repeat(200)))).is_ok() {
        testkit::skip!("running as a user that can read mode 000 files");
    }

    let RunningJob {
        mut events,
        mut control,
    } = engine()
        .with_stop_grace(PATIENT_GRACE)
        .run(op(), backup_job(&location, &scratch.source(), "home", HOUR))
        .await
        .unwrap();
    // The first warning: the data is behind the worker, and the flood has
    // begun. Nothing is read from here on.
    loop {
        match events.next().await {
            Some(EngineEvent::Warning(_)) => break,
            Some(EngineEvent::Succeeded(_)) => panic!("the backup ended before the cancel"),
            Some(EngineEvent::Failed(reason)) => panic!("the backup failed: {reason:?}"),
            Some(_) => {}
            None => panic!("the backup's events ended before the cancel"),
        }
    }
    // rustic's packers trail its reads, but by far less than the data.
    let packed_by = std::time::Instant::now() + Duration::from_secs(10);
    while packs_in(&scratch.repo_dir()) < 2 {
        assert!(
            std::time::Instant::now() < packed_by,
            "no 2 packs were saved before the flood"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The rest of the flood fills the pipe in a few milliseconds, and then
    // the worker's writer blocks for good. A cancel that came before would
    // find the stream free and end it, and the test would prove nothing.
    // Nothing tells the test when the pipe is full without reading it, so
    // this waits: a wait too short could only let a stop that doesn't read
    // pass, never fail one that does, whose timing doesn't matter.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let began = std::time::Instant::now();
    control.stop().await;
    let took = began.elapsed();
    assert!(events.next().await.is_none(), "events followed the stop");
    let snapshots = fs::read_dir(scratch.repo_dir().join("snapshots")).map_or(0, |f| f.count());
    assert_eq!(snapshots, 0, "a cancelled backup wrote a snapshot");
    let findings = findings(&location);
    assert!(findings.is_empty(), "{findings:#?}");
    assert!(
        took < PATIENT_GRACE,
        "the wind-down ran into the grace's SIGKILL: the stop took {took:?}"
    );
}
