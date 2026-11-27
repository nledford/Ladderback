//! The daily copy of the database (§9; §16, decision 174), kept by the
//! daemon's loop on the SQLite store with the fake clock: a copy a day that
//! opens and holds the history, none needlessly, a copy cut short that
//! leaves the one before, and a copy no more open than the database.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use app::daily_copy::KeepDailyCopy;
use app::fakes::FakeClock;
use app::ports::{Clock, DailyCopy, DailyCopyError, OperationStore};
use domain::{SignedDuration, Timestamp};
use store::SqliteStore;
use support::*;

const DAY: SignedDuration = SignedDuration::from_hours(24);
const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// The daily copy beside the scratch database.
fn daily(scratch: &Scratch) -> PathBuf {
    scratch.dir().join("ladderback.db.daily")
}

/// Where a copy is written before it takes the daily copy's place.
fn work(scratch: &Scratch) -> PathBuf {
    scratch.dir().join("ladderback.db.daily-new")
}

/// Which of the operations numbered `ids` the copy at `copy` holds, read by
/// the store itself from a copy of that file, opened as a database of its
/// own: the copy must open as one.
async fn held_in(copy: &Path, ids: &[u128]) -> Vec<u128> {
    let elsewhere = Scratch::new();
    fs::copy(copy, elsewhere.path()).expect("the daily copy can be read");
    let store = elsewhere.open().await;
    let mut held = Vec::new();
    for n in ids {
        if store.get(id(*n)).await.expect("the copy reads").is_some() {
            held.push(*n);
        }
    }
    store.close().await;
    held
}

/// The fake clock, a minute ahead of the real time the file system dates
/// a copy by: a copy taken now is a day old by it a day from now.
fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        Timestamp::now() + SignedDuration::from_mins(1),
    ))
}

/// The daemon's loop over `store`, and what it said.
fn keeping(
    clock: &Arc<FakeClock>,
    store: &Arc<SqliteStore>,
) -> (KeepDailyCopy, Arc<Mutex<Vec<DailyCopyError>>>) {
    let said: Arc<Mutex<Vec<DailyCopyError>>> = Arc::default();
    let told = Arc::clone(&said);
    let keep = KeepDailyCopy {
        clock: clock.clone(),
        copies: store.clone(),
        reports: Arc::new(move |error| {
            told.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(error);
        }),
    };
    (keep, said)
}

fn told(said: &Mutex<Vec<DailyCopyError>>) -> Vec<DailyCopyError> {
    said.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[tokio::test]
async fn after_a_day_there_is_a_copy_that_opens_and_holds_the_history() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    save(&store, &queued_backup(1), None).await;
    let clock = clock();
    let (keep, said) = keeping(&clock, &store);

    let started = clock.now();
    assert_eq!(keep.keep().await, started + DAY);
    assert_eq!(held_in(&daily(&scratch), &[1, 2]).await, vec![1]);

    save(&store, &queued_backup(2), None).await;
    clock.advance(SignedDuration::from_hours(23));
    keep.keep().await;
    assert_eq!(
        held_in(&daily(&scratch), &[1, 2]).await,
        vec![1],
        "no second copy within the day"
    );

    clock.advance(HOUR);
    assert_eq!(keep.keep().await, started + DAY + DAY);
    assert_eq!(held_in(&daily(&scratch), &[1, 2]).await, vec![1, 2]);
    assert!(!work(&scratch).exists(), "no work file is left");
    assert_eq!(told(&said), vec![]);
    store.close().await;
}

#[tokio::test]
async fn a_restart_within_the_day_takes_no_copy() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    save(&store, &queued_backup(1), None).await;
    let clock = clock();
    let (keep, _) = keeping(&clock, &store);
    keep.keep().await;
    save(&store, &queued_backup(2), None).await;
    store.close().await;

    let store = Arc::new(scratch.open().await);
    clock.advance(SignedDuration::from_hours(12));
    let (keep, said) = keeping(&clock, &store);
    let next = keep.keep().await;
    assert!(
        next > clock.now() && next <= clock.now() + DAY,
        "the next copy is due within the day: {next}"
    );
    assert_eq!(held_in(&daily(&scratch), &[1, 2]).await, vec![1]);
    assert_eq!(told(&said), vec![]);
    store.close().await;
}

/// A copy dropped at any of its await points before it took the daily
/// copy's place, as the daemon's stop drops it, and the daemon stopped:
/// the copy before is whole and unchanged, and the next start's copy
/// replaces it and leaves no work file. Where a cut lands depends on the
/// threads (`cut_after`), so the test cuts after every number of polls in
/// turn until one lets the copy end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copy_cut_short_before_its_rename_leaves_the_one_before() {
    for polls in 1..MOST_POLLS {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        save(&store, &queued_backup(1), None).await;
        assert_eq!(store.take().await, Ok(()));
        save(&store, &queued_backup(2), None).await;

        let cut = cut_after(store.take(), polls).await;
        store.close().await;
        if let Some(done) = cut {
            assert_eq!(done, Ok(()), "the copy that ended after {polls} polls");
            assert!(polls > 1, "a copy that ends in one poll is never cut");
            return;
        }

        let what = format!("after a copy cut after {polls} polls");
        assert_eq!(
            held_in(&daily(&scratch), &[1, 2]).await,
            vec![1],
            "the copy before stands {what}"
        );
        let store = scratch.open().await;
        assert_eq!(store.take().await, Ok(()), "the next copy {what}");
        assert_eq!(
            held_in(&daily(&scratch), &[1, 2]).await,
            vec![1, 2],
            "{what}"
        );
        assert!(!work(&scratch).exists(), "no work file is left {what}");
        store.close().await;
    }
    panic!("a copy took more than {MOST_POLLS} polls");
}

/// A copy the daemon couldn't finish, left by a crash: the next copy
/// replaces it, and the daily copy is no more open than the database,
/// whatever the files before it were.
#[cfg(unix)]
#[tokio::test]
async fn the_copy_has_the_databases_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let store = scratch.open().await;
    save(&store, &queued_backup(1), None).await;
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o640))
        .expect("the database's mode is set");
    for (file, held) in [
        (daily(&scratch), "an older copy"),
        (work(&scratch), "half a copy"),
    ] {
        fs::write(&file, held).expect("a file beside the database");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o666))
            .expect("the file's mode is set");
    }

    assert_eq!(store.take().await, Ok(()));
    let mode = fs::metadata(daily(&scratch))
        .expect("the daily copy is there")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640, "the daily copy's mode is {mode:o}");
    assert_eq!(held_in(&daily(&scratch), &[1]).await, vec![1]);
    assert!(!work(&scratch).exists(), "no work file is left");
    store.close().await;
}

#[tokio::test]
async fn a_copy_while_the_database_is_closed_is_skipped_told_and_tried_an_hour_later() {
    let scratch = Scratch::new();
    let store = Arc::new(scratch.open().await);
    save(&store, &queued_backup(1), None).await;
    assert_eq!(store.take().await, Ok(()));
    store.close().await;

    let clock = clock();
    clock.advance(DAY + DAY);
    let (keep, said) = keeping(&clock, &store);
    assert_eq!(keep.keep().await, clock.now() + HOUR);
    let said = told(&said);
    assert!(
        matches!(said.as_slice(), [DailyCopyError::Unavailable { .. }]),
        "said {said:?}"
    );
    assert_eq!(held_in(&daily(&scratch), &[1]).await, vec![1]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_operation_saved_while_a_copy_is_taken_is_saved() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    for n in 1..=50 {
        save(&store, &queued_backup(n), None).await;
    }
    let late = queued_backup(51);
    let (taken, saved) = tokio::join!(store.take(), store.save(&late, None));
    assert_eq!(taken, Ok(()));
    assert_eq!(saved, Ok(()));
    let kept = store.get(late.id()).await.expect("the log reads");
    assert!(
        kept.is_some(),
        "the operation saved during the copy is kept"
    );
    assert_eq!(held_in(&daily(&scratch), &[1, 50]).await, vec![1, 50]);
    store.close().await;
}
