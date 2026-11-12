//! A reader's visit to a live page, and the work the page does in the
//! background for as long as it lasts (technical design §4.4, #300): its
//! events stream, the waits to reconnect it and the reads of the history they
//! make, and the settle-poll after a start or a cancel. The page begins a
//! visit as it is built and ends it as it goes (Leptos's `on_cleanup`), and
//! every task the visit runs ends there, at its next await: dropped, with
//! what it holds, so an open events stream closes with it.

use std::pin::pin;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{Either, Shared, select};

/// A reader's visit to a page. Every clone is the same visit, and ends with
/// it.
#[derive(Clone)]
pub(crate) struct Visit {
    gone: Shared<oneshot::Receiver<()>>,
}

/// The end of a visit: dropping it ends the visit, and every task the visit
/// runs.
pub(crate) struct Departure {
    _sender: oneshot::Sender<()>,
}

impl Visit {
    /// A visit that lasts until its departure is dropped.
    pub(crate) fn begin() -> (Self, Departure) {
        let (sender, gone) = oneshot::channel();
        (
            Self {
                gone: gone.shared(),
            },
            Departure { _sender: sender },
        )
    }

    /// Runs `task` while the visit lasts: its output, or `None` once the
    /// visit has ended. A task the end finds waiting is dropped there; one it
    /// finds ready to go on goes no further, since the page it would change is
    /// gone; and one that begins after the end never starts.
    pub(crate) async fn during<F: Future>(self, task: F) -> Option<F::Output> {
        // The end is polled first, so a task and an end ready at once is
        // an end.
        match select(self.gone, pin!(task)).await {
            Either::Left(_) => None,
            Either::Right((output, _)) => Some(output),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use futures::FutureExt;
    use futures::executor::block_on;
    use futures::future::pending;

    use super::*;

    #[test]
    fn a_task_ends_with_its_output_while_the_visit_lasts() {
        let (visit, _departure) = Visit::begin();
        assert_eq!(block_on(visit.during(async { 7 })), Some(7));
    }

    #[test]
    fn a_task_waiting_when_the_reader_leaves_ends_there_and_lets_go_of_what_it_holds() {
        let (visit, departure) = Visit::begin();
        let held = Rc::new(());
        let mut task = Box::pin(visit.during({
            let held = Rc::clone(&held);
            async move {
                let _held = held;
                pending::<()>().await;
            }
        }));
        assert_eq!(
            (&mut task).now_or_never(),
            None,
            "the task waits while the visit lasts"
        );
        drop(departure);
        // Ended at the first poll after the reader left: a `during` that
        // missed the end fails here, rather than waiting forever.
        assert_eq!(task.now_or_never(), Some(None));
        assert_eq!(Rc::strong_count(&held), 1, "what the task held is let go");
    }

    #[test]
    fn every_task_of_a_visit_ends_with_it() {
        let (visit, departure) = Visit::begin();
        let first = visit.clone().during(pending::<()>());
        let second = visit.during(pending::<()>());
        drop(departure);
        assert_eq!(first.now_or_never(), Some(None));
        assert_eq!(second.now_or_never(), Some(None));
    }

    #[test]
    fn a_task_goes_no_further_once_the_reader_has_left() {
        let (visit, departure) = Visit::begin();
        drop(departure);
        let ran = Cell::new(false);
        assert_eq!(block_on(visit.during(async { ran.set(true) })), None);
        assert!(!ran.get(), "a task ran for a page that is gone");
    }
}
