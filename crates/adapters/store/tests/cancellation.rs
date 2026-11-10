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
