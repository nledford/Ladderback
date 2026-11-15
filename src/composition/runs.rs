//! The daemon's runs of operations and the orderly stop that freezes them
//! (§4.2, decision 92).

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;

/// What the daemon's orderly stop freezes: the run of each operation, on a
/// task of its own. A run frozen goes no further than where it stands, so it
/// starts no worker, runs no hook, records no end and announces none: its
/// timers, its turn in the queue and the end its worker read all belong to a
/// daemon that is going, and the stop's operation log step records its end
/// (`daemon-shut-down`). A frozen run is dropped where it stands, so a hook
/// it was running is killed at once, as the runtime's end once killed it.
/// Clones share one switch, and one count of the maintenance runs going,
/// which a restart that waits for the running operations counts with the
/// queue's running backups and holds, so none starts meanwhile (decision NN).
#[derive(Clone)]
pub(crate) struct Runs {
    frozen: Arc<watch::Sender<bool>>,
    maintenance: Arc<Mutex<Maintenance>>,
}

/// The maintenance runs going, and whether a restart holds new ones back.
/// One lock for both, so a run either counts before the hold or is refused
/// after it.
#[derive(Default)]
struct Maintenance {
    running: usize,
    held: bool,
}

impl Default for Runs {
    fn default() -> Self {
        Self {
            frozen: Arc::new(watch::channel(false).0),
            maintenance: Arc::new(Mutex::new(Maintenance::default())),
        }
    }
}

impl Runs {
    /// `run`, until the runs are frozen: from then on it is never polled
    /// again, and one begun after that never starts.
    pub(crate) fn until_frozen(
        &self,
        run: impl Future<Output = ()> + Send + 'static,
    ) -> impl Future<Output = ()> + Send + 'static {
        let mut frozen = self.frozen.subscribe();
        async move {
            tokio::select! {
                // Frozen first: a run that could go on in the same poll
                // doesn't.
                biased;
                () = frozen_at(&mut frozen) => {}
                () = run => {}
            }
        }
    }

    /// Freezes every run, those begun from now on too.
    pub(crate) fn freeze(&self) {
        self.frozen.send_replace(true);
    }

    /// Counts a maintenance run as running until what this returns is
    /// dropped, however the run ends; `None`, and nothing counted, while a
    /// restart holds new runs back.
    pub(crate) fn maintaining(&self) -> Option<Maintaining> {
        let mut maintenance = self.lock();
        if maintenance.held {
            return None;
        }
        maintenance.running = maintenance.running.saturating_add(1);
        Some(Maintaining(Arc::clone(&self.maintenance)))
    }

    /// How many maintenance runs are running.
    pub(crate) fn maintenance_running(&self) -> usize {
        todo!()
    }

    /// Starts no maintenance run from now on, until [`Self::release`]: a
    /// restart waits for the running ones.
    fn hold(&self) {
        todo!()
    }

    /// Starts maintenance runs again: the restart was cancelled.
    fn release(&self) {
        todo!()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Maintenance> {
        self.maintenance
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// A maintenance run counted as running ([`Runs::maintaining`]).
pub(crate) struct Maintaining(Arc<Mutex<Maintenance>>);

impl Drop for Maintaining {
    fn drop(&mut self) {
        let mut maintenance = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        maintenance.running = maintenance.running.saturating_sub(1);
    }
}

/// What a restart that waits for the running operations counts and holds
/// (decision NN): the queue's running backups, whose waiting ones it starts
/// no more, and the maintenance runs going, of which it starts no new one.
pub(crate) struct Work {
    pub(crate) queue: Arc<app::queue::RunQueue>,
    pub(crate) runs: Runs,
}

impl http_adapter::Work for Work {
    fn hold(&self) {
        self.queue.hold();
        self.runs.hold();
    }

    fn release(&self) {
        self.runs.release();
        self.queue.release();
    }

    fn running(&self) -> usize {
        self.queue
            .running()
            .saturating_add(self.runs.maintenance_running())
    }
}

/// Returns once the runs are frozen; never once nothing can freeze them.
async fn frozen_at(frozen: &mut watch::Receiver<bool>) {
    if frozen.wait_for(|frozen| *frozen).await.is_err() {
        std::future::pending::<()>().await;
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

    #[test]
    fn a_maintenance_run_counts_as_running_until_it_ends() {
        let runs = Runs::default();
        let prune = runs.maintaining();
        let check = runs.clone().maintaining();
        assert!(prune.is_some() && check.is_some());
        assert_eq!(runs.maintenance_running(), 2);
        drop(prune);
        assert_eq!(runs.maintenance_running(), 1);
        drop(check);
        assert_eq!(runs.maintenance_running(), 0);
    }

    /// A restart that waits holds the work: it counts the maintenance run
    /// and the backup already running, starts no new maintenance run and no
    /// queued backup, and its cancel lets both go.
    #[test]
    fn a_held_restart_starts_no_new_operation_until_it_is_released() {
        use futures::FutureExt;
        use http_adapter::Work as _;

        let runs = Runs::default();
        let work = super::Work {
            queue: Arc::new(app::queue::RunQueue::new()),
            runs: runs.clone(),
        };
        let limit = domain::planning::Concurrency::default();
        let home = domain::PlanId::parse("home").unwrap();
        let docs = domain::PlanId::parse("docs").unwrap();
        let running_backup = work.queue.push(&home, limit);
        let prune = runs.maintaining();
        work.hold();
        assert_eq!(work.running(), 2, "the running operations aren't counted");
        assert!(runs.maintaining().is_none(), "a maintenance run started");
        assert_eq!(work.running(), 2, "a refused run was counted");
        let mut queued = work.queue.push(&docs, limit);
        drop(running_backup);
        drop(prune);
        assert_eq!(work.running(), 0);
        assert!(
            queued.turn().now_or_never().is_none(),
            "a queued backup started"
        );
        work.release();
        assert!(runs.maintaining().is_some(), "maintenance stays held");
        assert!(
            queued.turn().now_or_never().is_some(),
            "the queued backup didn't start"
        );
    }
}
