//! A call dropped part way, as a run drops the write that would hold up a
//! cancel or its deadline (§9): the log is as usable as it was, and the next
//! write is not left waiting on what the dropped one began.

mod support;

use app::ports::{OperationStore, Start};
use domain::execution::{FailureReason, OperationKind, OperationStatus};
use support::*;

/// More polls than any call here needs: a loop that reaches it fails, rather
/// than runs for ever.
const MOST_POLLS: usize = 200;

/// A call that ends after fewer polls than this was not cut at the points
/// the tests are about: the connection's checkout, the transaction's start,
/// its statements and its commit.
const FEWEST_AWAIT_POINTS: usize = 5;

/// The next write after a cut one must land at once, not after the write
/// lock's five-second wait.
const NEXT_WRITE_WITHIN: u64 = 3;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_save_cut_short_at_any_await_point_leaves_the_log_writable() {
    for polls in 0..MOST_POLLS {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_backup(1);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        if cut_after(store.save(&running, None), polls).await.is_some() {
            assert!(
                polls >= FEWEST_AWAIT_POINTS,
                "the save ended in {polls} polls: nothing was cut"
            );
            return;
        }

        let ended = failed(queued, FailureReason::DeadlinePassed);
        let what = format!("the save after one cut after {polls} polls");
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
    for polls in 0..MOST_POLLS {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_on_repository(1, OperationKind::Prune);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        if cut_after(store.try_start(&running), polls).await.is_some() {
            assert!(
                polls >= FEWEST_AWAIT_POINTS,
                "the start ended in {polls} polls: nothing was cut"
            );
            return;
        }

        // Whether or not the cut start stored anything, the operation starts
        // again: it holds its repository, and may start again.
        let what = format!("the start after one cut after {polls} polls");
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
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_cut_short_at_any_await_point_holds_the_repository_exactly_when_it_stored_the_start()
 {
    let (mut held, mut free) = (0, 0);
    for polls in 0..MOST_POLLS {
        let scratch = Scratch::new();
        let store = scratch.open().await;
        let queued = queued_on_repository(1, OperationKind::Prune);
        save(&store, &queued, None).await;

        let running = started(queued.clone());
        if cut_after(store.try_start(&running), polls).await.is_some() {
            assert!(
                polls >= FEWEST_AWAIT_POINTS,
                "the start ended in {polls} polls: nothing was cut"
            );
            assert!(free > 0, "no cut came before the start was stored");
            assert!(held > 0, "no cut came after the start was stored");
            return;
        }

        // Another maintenance operation asks for the repository. Its start
        // waits for the write lock the cut one may still hold, so its answer
        // is what the cut left: the repository held by the cut operation, or
        // free.
        let what = format!("after a start cut after {polls} polls");
        let rival = started(queued_on_repository(2, OperationKind::Check));
        let answer = within(NEXT_WRITE_WITHIN, &what, store.try_start(&rival)).await;
        let kept = store
            .get(running.id())
            .await
            .expect("reads")
            .expect("present");
        match answer {
            Ok(Start::Refused { running: holder }) => {
                held += 1;
                assert_eq!(holder, running.id(), "{what}");
                assert_eq!(kept, detail_of(&running, None), "{what}: stored whole");
            }
            Ok(Start::Started) => {
                free += 1;
                assert_eq!(kept, detail_of(&queued, None), "{what}: stored not at all");
            }
            Err(error) => panic!("{what}: the log refused the rival's start: {error}"),
        }
    }
    panic!("a start took more than {MOST_POLLS} polls");
}
