//! A call dropped part way, as a run drops the write that would hold up a
//! cancel or its deadline (§9): the log is as usable as it was, and the next
//! write is not left waiting on what the dropped one began.

mod support;

use app::ports::{OperationStore, Start};
use domain::execution::{FailureReason, OperationKind, OperationStatus};
use support::*;

/// The next write after a cut one must land at once, not after the write
/// lock's five-second wait.
const NEXT_WRITE_WITHIN: u64 = 3;

// Each test cuts the call at the start of its transaction, the one await
// point every call reaches whatever the connection threads do, then after
// every number of polls in turn until one lets the call end. Where those
// later cuts land depends on the threads (see `cut_after`), so no test
// counts them: the first cut is the one that proves the call was cut inside
// its transaction.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_save_cut_short_at_any_await_point_leaves_the_log_writable() {
    for cut in Cut::every() {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_backup(1);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        let done = cut.made(store.save(&running, None), &scratch).await;
        if done.is_some() {
            return;
        }

        let ended = failed(queued, FailureReason::DeadlinePassed);
        let what = format!("the save after one cut {cut}");
        let next = within(NEXT_WRITE_WITHIN, &what, store.save(&ended, None)).await;
        assert_eq!(next, Ok(()), "{what}");
        let kept = store
            .get(ended.id())
            .await
            .expect("reads")
            .expect("present");
        assert_eq!(kept, detail_of(&ended, None), "{what}");
    }
    panic!("a save took more than {MOST_POLLS} polls");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_cut_short_at_any_await_point_leaves_the_log_writable() {
    for cut in Cut::every() {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_on_repository(1, OperationKind::Prune);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        let done = cut.made(store.try_start(&running), &scratch).await;
        if done.is_some() {
            return;
        }

        // Whether or not the cut start stored anything, the operation starts
        // again: it holds its repository, and may start again.
        let what = format!("the start after one cut {cut}");
        let again = within(NEXT_WRITE_WITHIN, &what, store.try_start(&running)).await;
        assert_eq!(again, Ok(Start::Started), "{what}");
        let kept = store
            .get(running.id())
            .await
            .expect("reads")
            .expect("present");
        assert_eq!(kept.record.status, OperationStatus::Running, "{what}");
    }
    panic!("a start took more than {MOST_POLLS} polls");
}

/// A start that was dropped leaves one of two things, and its caller is not
/// told which: the operation still queued and the repository free, or the
/// operation stored as running, which holds its repository (I7) with no one to
/// run it. A caller that has no one to end what a start stored must not drop
/// one, and the daemon's route doesn't (§16, decision 42).
///
/// The cut at the start of the transaction comes before the start is stored,
/// so it must leave the repository free. A cut after the commit has run, but
/// before the call has seen it, comes only when the connection thread is
/// slower than the poll that sent the commit: no lock holds a commit back
/// in write-ahead logging, so no test can force one. Each such cut is checked
/// when it comes, and the start that ends, stored in full, is checked to hold
/// the repository as one cut after its commit must.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_cut_short_at_any_await_point_holds_the_repository_exactly_when_it_stored_the_start()
 {
    for cut in Cut::every() {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_on_repository(1, OperationKind::Prune);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        let ended = cut.made(store.try_start(&running), &scratch).await;
        let what = match &ended {
            Some(started) => {
                assert_eq!(started, &Ok(Start::Started), "a start that ended {cut}");
                format!("after a start that ended before a cut {cut}")
            }
            None => format!("after a start cut {cut}"),
        };

        // Another maintenance operation asks for the repository. Its start
        // waits for the write lock the cut one may still hold, so its answer
        // is what the cut left: the repository held by the cut operation, or
        // free.
        let rival = started(queued_on_repository(2, OperationKind::Check));
        let answer = within(NEXT_WRITE_WITHIN, &what, store.try_start(&rival)).await;
        let kept = store
            .get(running.id())
            .await
            .expect("reads")
            .expect("present");
        match answer {
            Ok(Start::Refused { running: holder }) => {
                assert_eq!(holder, running.id(), "{what}");
                assert_eq!(kept, detail_of(&running, None), "{what}: stored whole");
                assert!(
                    !matches!(cut, Cut::AtTransactionStart),
                    "{what}: the repository is held, but the start was cut before its commit"
                );
            }
            Ok(Start::Started) => {
                assert_eq!(kept, detail_of(&queued, None), "{what}: stored not at all");
                assert!(ended.is_none(), "{what}: a start that ended left it free");
            }
            Err(error) => panic!("{what}: the log refused the rival's start: {error}"),
        }
        if ended.is_some() {
            return;
        }
    }
    panic!("a start took more than {MOST_POLLS} polls");
}
