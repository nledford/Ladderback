//! The daemon's runs of operations and the orderly stop that freezes them
//! (§4.2, decision 92).

/// What the daemon's orderly stop freezes: the run of each operation, on a
/// task of its own. A run frozen goes no further than where it stands, so it
/// starts no worker, runs no hook, records no end and announces none: its
/// timers, its turn in the queue and the end its worker read all belong to a
/// daemon that is going, and the stop's operation log step records its end
/// (`daemon-shut-down`). Clones share one switch.
#[derive(Clone, Default)]
pub(crate) struct Runs {}

impl Runs {
    /// `run`, until the runs are frozen: from then on it is never polled
    /// again, and one begun after that never starts.
    pub(crate) fn until_frozen(
        &self,
        run: impl Future<Output = ()> + Send + 'static,
    ) -> impl Future<Output = ()> + Send + 'static {
        let _ = run;
        async { todo!() }
    }

    /// Freezes every run, those begun from now on too.
    pub(crate) fn freeze(&self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::Runs;

    /// A run that goes on (or not) once `go` is sent, and says whether it did.
    fn held_run() -> (
        futures::channel::oneshot::Sender<()>,
        Arc<AtomicBool>,
        impl Future<Output = ()> + Send + 'static,
    ) {
        let (go, gone) = futures::channel::oneshot::channel::<()>();
        let went_on = Arc::new(AtomicBool::new(false));
        let run = {
            let went_on = Arc::clone(&went_on);
            async move {
                let _ = gone.await;
                went_on.store(true, Ordering::SeqCst);
            }
        };
        (go, went_on, run)
    }

    #[tokio::test]
    async fn a_run_goes_no_further_once_the_runs_are_frozen() {
        let runs = Runs::default();
        let (go, went_on, run) = held_run();
        let run = tokio::spawn(runs.until_frozen(run));
        // The run waits for `go`.
        tokio::task::yield_now().await;
        runs.freeze();
        let _ = go.send(());
        run.await.unwrap();
        assert!(
            !went_on.load(Ordering::SeqCst),
            "the run went on once frozen"
        );
    }

    #[tokio::test]
    async fn a_run_begun_once_the_runs_are_frozen_never_starts() {
        let runs = Runs::default();
        runs.freeze();
        let started = Arc::new(AtomicBool::new(false));
        runs.until_frozen({
            let started = Arc::clone(&started);
            async move { started.store(true, Ordering::SeqCst) }
        })
        .await;
        assert!(!started.load(Ordering::SeqCst), "the run started");
    }

    #[tokio::test]
    async fn a_run_never_frozen_runs_to_its_end() {
        let runs = Runs::default();
        let (go, went_on, run) = held_run();
        let run = tokio::spawn(runs.until_frozen(run));
        let _ = go.send(());
        run.await.unwrap();
        assert!(went_on.load(Ordering::SeqCst), "the run was cut short");
    }

    /// The runs go when the daemon does; a run still going then is not
    /// frozen by their going.
    #[tokio::test]
    async fn a_run_outlives_the_runs_it_was_begun_under() {
        let runs = Runs::default();
        let (go, went_on, run) = held_run();
        let run = tokio::spawn(runs.until_frozen(run));
        tokio::task::yield_now().await;
        drop(runs);
        let _ = go.send(());
        run.await.unwrap();
        assert!(went_on.load(Ordering::SeqCst), "the run was cut short");
    }
}
