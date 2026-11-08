//! A worker killed at any moment must leave a repository that still checks
//! clean and still takes new backups (technical design §4.1, invariant I2:
//! nothing relies on cleanup). Real workers are killed at a point of their
//! work, not at a time: right after the Nth progress event of the job. A run
//! of each kind of job is left alone first, to count its events, and the
//! kills are spread over that count, one slice of it per iteration. A kill
//! that comes after the job ended proves nothing, so the job is run again
//! on new data with an earlier point until it is really interrupted.
//!
//! The backends are the local one and OpenDAL's `fs` service with
//! `atomic_write_dir`: without it OpenDAL writes in place, and a kill leaves
//! half a file under its final name (§4.1).
//!
//! `STEADING_KILL_SEED` repeats a run; `STEADING_KILL_ITERATIONS` (default 2
//! per backend and job) makes it longer; `STEADING_KILL_IN_PLACE=1` leaves
//! `atomic_write_dir` out. The seed is printed first, so a failure shows it.

mod support;

use std::path::Path;
use std::time::Duration;

use app::ports::{Engine, EngineEvent, EngineResult, Job, RunningJob};
use domain::SignedDuration;
use engine::WorkerEngine;
use futures::StreamExt;
use rustic_core::{BackupOptions, PathList, PruneOptions, SnapshotOptions};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// How often a job that was to be killed is started again, each time with an
/// earlier kill point, before the test gives up on interrupting it.
const ATTEMPTS: usize = 8;

fn seed() -> u64 {
    std::env::var("STEADING_KILL_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |since| since.as_nanos() as u64)
        })
}

fn iterations() -> usize {
    std::env::var("STEADING_KILL_ITERATIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2)
}

/// A stopped worker with no grace period: SIGTERM, then SIGKILL at once.
fn killing_engine() -> WorkerEngine {
    engine().with_stop_grace(Duration::ZERO)
}

/// What became of a job that was to be killed.
enum Outcome {
    /// The worker was killed right after the chosen progress event.
    Killed,
    /// The job ended by itself before that event.
    Ended,
}

/// Runs `job` to its success and counts the progress events it sent.
async fn count_progress(engine: &WorkerEngine, job: Job) -> usize {
    let events = run_to_end(engine, job).await;
    succeeded(&events);
    events
        .iter()
        .filter(|event| matches!(event, EngineEvent::Progress(_)))
        .count()
}

/// Runs `job` and kills its worker right after its `at`th progress event.
async fn kill_at(engine: &WorkerEngine, job: Job, at: usize) -> Outcome {
    let RunningJob {
        mut events,
        mut control,
    } = engine.run(op(), job).await.unwrap();
    let mut seen = 0;
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Progress(_) => {
                seen += 1;
                if seen == at {
                    control.stop().await;
                    return Outcome::Killed;
                }
            }
            EngineEvent::Failed(reason) => panic!("the job failed by itself: {reason:?}"),
            EngineEvent::Succeeded(_) => return Outcome::Ended,
            _ => {}
        }
    }
    Outcome::Ended
}

/// Whether a job that was to be killed was interrupted: `Killed` only says
/// the stop came after the chosen progress event, and the worker may have
/// had its snapshot written and its result sent by then. A backup that added
/// a snapshot (`snapshots_after` more than `snapshots_before`) finished.
fn was_interrupted(outcome: &Outcome, snapshots_before: usize, snapshots_after: usize) -> bool {
    matches!(outcome, Outcome::Killed) && snapshots_after == snapshots_before
}

/// The event iteration `i` of `n` kills at: a random one in the `i`th of `n`
/// equal slices of the job's `total` events, never the last.
fn kill_point(rng: &mut Rng, total: usize, i: usize, n: usize) -> usize {
    let slice = (total / n).max(1);
    let first = i.saturating_mul(slice);
    let point = 1 + first + rng.below(slice as u64) as usize;
    point.min(total.saturating_sub(1)).max(1)
}

async fn assert_checks_clean(engine: &WorkerEngine, location: &Location, context: &str) {
    let events = run_to_end(engine, check_job(location, true)).await;
    match succeeded(&events) {
        EngineResult::Check(summary) => {
            assert_eq!(summary.errors, 0, "{context}: {summary:?}");
        }
        other => panic!("{context}: not a check result: {other:?}"),
    }
}

/// New random data to back up, enough that the backup takes a moment.
fn fill_source(source: &Path, rng: &mut Rng) {
    for n in 0..4 {
        let len = (8 << 20) + rng.below(8 << 20) as usize;
        write_file(source.join(format!("file{n}.bin")), &rng.bytes(len));
    }
}

/// Garbage for a prune that has to repack: a snapshot of a big file and a
/// small one, a second of the small one alone, and the first forgotten. The
/// big file's packs are unused, and the pack that holds the small file's
/// blobs is mostly unused, which rustic repacks.
fn make_garbage(location: &Location, source: &Path, rng: &mut Rng) {
    let keep = source.join("keep.bin");
    write_file(&keep, &rng.bytes(1 << 20));
    write_file(source.join("big.bin"), &rng.bytes(36 << 20));
    let everything = PathList::from_iter([source.to_path_buf()])
        .sanitize()
        .unwrap();
    let only_keep = PathList::from_iter([keep]).sanitize().unwrap();
    off_runtime(|| {
        // A repository opened anew for each snapshot, so that the second
        // finds the small file's blobs in the first and shares them.
        let backup = |paths: &PathList| {
            location
                .open()
                .to_indexed_ids()
                .unwrap()
                .backup(
                    &BackupOptions::default(),
                    paths,
                    SnapshotOptions::default().to_snapshot().unwrap(),
                )
                .unwrap()
        };
        let first = backup(&everything);
        backup(&only_keep);
        location.open().delete_snapshots(&[first.id]).unwrap();
    });
}

/// Makes garbage until a prune has packs to repack, so the test can't go on
/// without the work it means to interrupt.
fn make_garbage_to_repack(location: &Location, source: &Path, rng: &mut Rng) {
    for _ in 0..4 {
        make_garbage(location, source, rng);
        let plan = off_runtime(|| location.open().prune_plan(&PruneOptions::default()));
        if plan.unwrap().stats.packs.repack > 0 {
            return;
        }
    }
    panic!("the garbage never left a pack to repack: the prune has no repacking to interrupt");
}

async fn killed_backups(location: Location, source: &Path, label: &str) {
    let seed = seed();
    eprintln!("{label}: kill test seed {seed}");
    let mut rng = Rng(seed);
    off_runtime(|| location.init());
    let engine = killing_engine();
    fill_source(source, &mut rng);
    let total = count_progress(&engine, backup_job(&location, source, "daily", HOUR)).await;
    let n = iterations();
    for i in 0..n {
        let context = format!("{label}, seed {seed}, iteration {i}");
        let mut at = kill_point(&mut rng, total, i, n);
        let mut killed = false;
        for _ in 0..ATTEMPTS {
            fill_source(source, &mut rng);
            let before = off_runtime(|| location.snapshots().len());
            let outcome = kill_at(&engine, backup_job(&location, source, "daily", HOUR), at).await;
            let after = off_runtime(|| location.snapshots().len());
            if was_interrupted(&outcome, before, after) {
                killed = true;
                break;
            }
            at = (at / 2).max(1);
        }
        assert!(killed, "{context}: the backup ended before its first event");
        assert_checks_clean(&engine, &location, &context).await;
        let again = run_to_end(&engine, backup_job(&location, source, "daily", HOUR)).await;
        backed_up(&again);
    }
}

async fn killed_prunes(location: Location, source: &Path, label: &str) {
    let seed = seed();
    eprintln!("{label}: kill test seed {seed}");
    let mut rng = Rng(seed);
    off_runtime(|| location.init());
    let engine = killing_engine();
    make_garbage_to_repack(&location, source, &mut rng);
    let total = count_progress(&engine, prune_job(&location)).await;
    let n = iterations();
    for i in 0..n {
        let context = format!("{label}, seed {seed}, iteration {i}");
        let mut at = kill_point(&mut rng, total, i, n);
        let mut killed = false;
        for _ in 0..ATTEMPTS {
            make_garbage_to_repack(&location, source, &mut rng);
            match kill_at(&engine, prune_job(&location), at).await {
                Outcome::Killed => {
                    killed = true;
                    break;
                }
                Outcome::Ended => at = (at / 2).max(1),
            }
        }
        assert!(killed, "{context}: the prune ended before its first event");
        assert_checks_clean(&engine, &location, &context).await;
        let again = run_to_end(&engine, prune_job(&location)).await;
        assert!(
            matches!(
                again.last(),
                Some(EngineEvent::Succeeded(EngineResult::Prune(_)))
            ),
            "{context}: {again:?}"
        );
        // The repository still takes a backup, and is still sound after it.
        let backup = run_to_end(&engine, backup_job(&location, source, "daily", HOUR)).await;
        backed_up(&backup);
        assert_checks_clean(&engine, &location, &context).await;
    }
}

#[tokio::test]
async fn killed_backups_leave_a_local_repository_intact() {
    let scratch = Scratch::new();
    killed_backups(scratch.local(), &scratch.source(), "local backup").await;
}

#[tokio::test]
async fn killed_backups_leave_an_opendal_fs_repository_intact() {
    let scratch = Scratch::new();
    killed_backups(scratch.opendal_fs(), &scratch.source(), "opendal:fs backup").await;
}

#[tokio::test]
async fn killed_prunes_leave_a_local_repository_intact() {
    let scratch = Scratch::new();
    killed_prunes(scratch.local(), &scratch.source(), "local prune").await;
}

#[tokio::test]
async fn killed_prunes_leave_an_opendal_fs_repository_intact() {
    let scratch = Scratch::new();
    killed_prunes(scratch.opendal_fs(), &scratch.source(), "opendal:fs prune").await;
}
