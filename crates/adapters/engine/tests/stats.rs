//! A repository's stats through the real worker (§16, decision 104): the
//! snapshots counted and their sizes added up, and the index's blobs added
//! up as rustic's own `repoinfo` adds them.

mod support;

use app::ports::{EngineResult, Job};
use domain::SignedDuration;
use domain::maintenance::RepositoryStats;
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

async fn stats(location: &Location) -> RepositoryStats {
    let events = run_to_end(&engine(), Job::Stats(location.access(PASSWORD))).await;
    let EngineResult::Stats(stats) = succeeded(&events) else {
        panic!("not a stats result: {events:?}");
    };
    *stats
}

/// What rustic's `repoinfo` reads from the index: the sizes in the packs and
/// raw of every blob outside the packs marked for deletion, and the raw size
/// of those inside them.
fn repoinfo(location: &Location) -> (u64, u64, u64) {
    let infos = location.open().infos_index().unwrap();
    let stored = infos.blobs.iter().map(|blob| blob.size).sum();
    let raw = infos.blobs.iter().map(|blob| blob.data_size).sum();
    let raw_marked = infos.blobs_delete.iter().map(|blob| blob.data_size).sum();
    (stored, raw, raw_marked)
}

async fn back_up(location: &Location, scratch: &Scratch) {
    let events = run_to_end(
        &engine(),
        backup_job(location, &scratch.source(), "daily", HOUR),
    )
    .await;
    backed_up(&events);
}

#[tokio::test]
async fn the_stats_add_up_the_snapshots_and_the_index_as_rustic_repoinfo_does() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    // Random bytes don't compress; repeated text compresses to almost
    // nothing, so the repository stores far less than the raw data.
    write_file(scratch.source().join("random.bin"), &Rng(7).bytes(300_000));
    let text = "calm and exact ".repeat(140_000);
    write_file(scratch.source().join("text.txt"), text.as_bytes());
    back_up(&location, &scratch).await;
    write_file(
        scratch.source().join("text.txt"),
        format!("{text}more\n").as_bytes(),
    );
    back_up(&location, &scratch).await;

    let read = stats(&location).await;

    let sources = 300_000 + 2_100_000;
    assert_eq!(read.snapshots, 2);
    assert_eq!(read.total_size, sources + sources + 5);
    let (stored, raw, _) = repoinfo(&location);
    assert_eq!((read.stored_size, read.raw_size), (stored, raw));
    assert!(read.raw_size > sources, "{read:?}");
    assert!(read.stored_size < read.raw_size / 2, "{read:?}");
}

#[tokio::test]
async fn an_empty_repository_has_nothing_to_add_up() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();

    let read = stats(&location).await;

    assert_eq!(
        read,
        RepositoryStats {
            snapshots: 0,
            total_size: 0,
            stored_size: 0,
            raw_size: 0,
        }
    );
}

#[tokio::test]
async fn the_packs_a_prune_marked_for_deletion_are_left_out() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("kept.bin"), &Rng(1).bytes(200_000));
    back_up(&location, &scratch).await;
    write_file(scratch.source().join("dropped.bin"), &Rng(2).bytes(200_000));
    back_up(&location, &scratch).await;
    let snapshots = location.snapshots();
    let newest = snapshots.iter().max_by_key(|s| s.time.clone()).unwrap();
    location.open().delete_snapshots(&[newest.id]).unwrap();
    // A prune never deletes at once (I2): it marks the dropped file's pack
    // for deletion and leaves it in the repository.
    succeeded(&run_to_end(&engine(), prune_job(&location)).await);

    let read = stats(&location).await;

    let (stored, raw, raw_marked) = repoinfo(&location);
    assert!(raw_marked >= 200_000, "nothing marked: {raw_marked}");
    assert_eq!((read.stored_size, read.raw_size), (stored, raw));
    assert_eq!(read.snapshots, 1);
    assert!(read.raw_size < 200_000 + 10_000, "{read:?}");
}

#[tokio::test]
async fn a_marked_pack_a_snapshot_uses_again_is_only_marked_for_recovery_by_the_next_prune() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("kept.bin"), &Rng(1).bytes(200_000));
    back_up(&location, &scratch).await;
    write_file(scratch.source().join("reused.bin"), &Rng(2).bytes(200_000));
    back_up(&location, &scratch).await;
    let snapshots = location.snapshots();
    let newest = snapshots.iter().max_by_key(|s| s.time.clone()).unwrap();
    location.open().delete_snapshots(&[newest.id]).unwrap();
    succeeded(&run_to_end(&engine(), prune_job(&location)).await);
    // The snapshot is back, as when a backup that read the index before
    // that prune marked the pack saves its snapshot after it (§7.3).
    location
        .open()
        .save_snapshots(vec![newest.clone()])
        .unwrap();

    let events = run_to_end(&engine(), prune_job(&location)).await;

    let EngineResult::Prune(pruned) = succeeded(&events) else {
        panic!("not a prune result: {events:?}");
    };
    assert!(pruned.packs_recovered >= 1, "{pruned:?}");
    // This prune marks the pack for recovery; only a prune at least
    // `keep-delete` later brings it back into the index. Until then the
    // stats leave its data out, as a restore can't read it.
    let read = stats(&location).await;
    assert_eq!(read.snapshots, 2);
    assert!(read.raw_size < 200_000 + 10_000, "{read:?}");
}

#[tokio::test]
async fn an_index_entry_listed_twice_counts_twice_as_rustic_repoinfo_counts_it() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("data.bin"), &Rng(3).bytes(200_000));
    back_up(&location, &scratch).await;
    let once = stats(&location).await;
    // A second index file listing the same blobs, as a host that rewrote
    // the index while this one read it can leave behind.
    let index = scratch.repo_dir().join("index");
    let file = std::fs::read_dir(&index)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let name = file.file_name().unwrap().to_str().unwrap();
    let other = if name.starts_with('0') { "1" } else { "0" };
    let rest = name.get(1..).unwrap();
    std::fs::copy(&file, index.join(format!("{other}{rest}"))).unwrap();

    let twice = stats(&location).await;

    assert!(once.stored_size > 0, "{once:?}");
    assert_eq!(
        (twice.stored_size, twice.raw_size),
        (2 * once.stored_size, 2 * once.raw_size)
    );
    let (stored, raw, _) = repoinfo(&location);
    assert_eq!((twice.stored_size, twice.raw_size), (stored, raw));
    assert_eq!(twice.snapshots, once.snapshots);
}
