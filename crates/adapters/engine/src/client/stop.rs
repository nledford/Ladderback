//! The worker process from its start to its exit: what supervises it, how a
//! job and the daemon's workers are stopped, and the signals and waits that
//! end a worker and everything it started.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use app::ports::JobControl;
use async_trait::async_trait;
use tokio::io::{AsyncRead, ReadBuf};
use tokio::process::{Child, ChildStdout};
use tokio::sync::{mpsc, watch};

use super::cache_slots::CacheSlot;

/// How long a worker may keep running after it reported its result, for
/// cleanup, before it is killed.
const AFTER_RESULT_GRACE: Duration = Duration::from_secs(10);

/// What the daemon asks of the task that owns the worker process.
pub(super) enum Order {
    /// SIGTERM, then SIGKILL after the grace period.
    Stop,
    /// SIGKILL at once: the worker closed its output and lives on, or sent
    /// nothing in the time it had to start.
    Kill,
    /// The worker reported its result and may finish its cleanup.
    ResultReported,
}

/// The exit code of a worker that ended: `None` when a signal ended it.
pub(super) type Exited = Option<Option<i32>>;

/// What a job needs to be stopped with the daemon's workers: whether that
/// was asked for, which it holds until its worker has exited, the worker's
/// stdout, and what tells the job's events that the daemon stopped it.
pub(super) struct DaemonStop {
    pub(super) asked: watch::Receiver<bool>,
    pub(super) pipe: ForStop,
    pub(super) stopped: Arc<AtomicBool>,
}

/// Owns the worker process, and the cache slot it uses, until it exits.
pub(super) async fn supervise(
    mut child: Child,
    commands: mpsc::UnboundedReceiver<Order>,
    exited: watch::Sender<Exited>,
    grace: Duration,
    mut daemon_stop: DaemonStop,
    _slot: Option<CacheSlot>,
) {
    let mut commands = Some(commands);
    let mut after_result = None;
    let leader = child.id();
    // The exit is seen without reaping the worker where that can be done:
    // see `WorkerExit`. Elsewhere the worker is reaped first.
    let gone = exit_unreaped(leader);
    tokio::pin!(gone);
    let mut unreaped_exit = true;
    let status = loop {
        let cleanup_over = async {
            match after_result {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            // In this order: a stop of the daemon's workers wins over the job
            // let go of in the same instant, as the daemon's stop lets go of
            // every run once it has asked, so the worker still gets its
            // SIGTERM and its grace rather than being killed.
            biased;
            exit = &mut gone, if unreaped_exit => match exit {
                WorkerExit::Unreaped => break reap(&mut child, leader, true).await,
                WorkerExit::Reaped => break reap(&mut child, leader, false).await,
                WorkerExit::Unobservable => unreaped_exit = false,
            },
            status = child.wait(), if !unreaped_exit => {
                signal_group(leader, Signal::Kill);
                break status;
            }
            () = stop_asked(&mut daemon_stop.asked) => {
                // Before the events end, so they see who ended them.
                daemon_stop.stopped.store(true, Ordering::SeqCst);
                break terminate_draining(&mut child, grace, &mut daemon_stop.pipe).await;
            }
            command = next_command(&mut commands) => match command {
                Some(Order::Stop) => break terminate(&mut child, grace).await,
                Some(Order::Kill) => break kill(&mut child).await,
                Some(Order::ResultReported) => {
                    after_result = Some(deadline_after(tokio::time::Instant::now(), AFTER_RESULT_GRACE));
                }
                // The daemon let go of the job. Before a result that means
                // kill; after it, the worker finishes its cleanup first.
                None if after_result.is_some() => commands = None,
                None => break kill(&mut child).await,
            },
            () = cleanup_over => break kill(&mut child).await,
        }
    };
    let _ = exited.send(Some(status.ok().and_then(|status| status.code())));
}

async fn next_command(commands: &mut Option<mpsc::UnboundedReceiver<Order>>) -> Option<Order> {
    match commands {
        Some(commands) => commands.recv().await,
        None => std::future::pending().await,
    }
}

/// Returns once the daemon is stopping; never when no one can stop it.
async fn stop_asked(stopping: &mut watch::Receiver<bool>) {
    if stopping.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Stops the worker as [`Control::stop`] does, for the daemon's stop of its
/// workers: the job's events end here, and what the worker writes until it
/// exits is read and dropped while [`terminate`] waits, so a backup winding
/// down is never held by a full pipe.
async fn terminate_draining(
    child: &mut Child,
    grace: Duration,
    pipe: &mut ForStop,
) -> std::io::Result<std::process::ExitStatus> {
    pipe.0.stopped.store(true, Ordering::SeqCst);
    let ended = terminate(child, grace);
    tokio::pin!(ended);
    let mut dropped = tokio::io::sink();
    tokio::select! {
        biased;
        status = &mut ended => status,
        // A pipe that ends or fails first leaves the wait for the exit,
        // which the grace bounds.
        _ = tokio::io::copy(pipe, &mut dropped) => ended.await,
    }
}

/// When a worker that reported its result must be done with its cleanup:
/// `grace` after `now`, or `now` when that time can't be represented.
pub(super) fn deadline_after(now: tokio::time::Instant, grace: Duration) -> tokio::time::Instant {
    now.checked_add(grace).unwrap_or(now)
}

/// Kills the worker and everything it started, and waits for the worker to
/// exit.
async fn kill(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    kill_now(child);
    child.wait().await
}

/// SIGKILL to the worker and everything it started, without waiting for the
/// worker to exit.
pub(super) fn kill_now(child: &mut Child) {
    if !signal_group(child.id(), Signal::Kill) {
        // No process group to signal: the worker alone.
        let _ = child.start_kill();
    }
}

/// SIGTERM to the worker, then SIGKILL to it and everything it started once
/// `grace` has passed without the worker exiting. The SIGTERM goes to the
/// worker alone: a backup winding down on it (§4.1) still saves through the
/// rclone or ssh its backend started. What of the group is left when the
/// worker has gone is killed then.
async fn terminate(
    child: &mut Child,
    grace: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let leader = child.id();
    if !signal_worker(leader, Signal::Terminate) {
        return kill(child).await;
    }
    let over = deadline_after(tokio::time::Instant::now(), grace);
    match tokio::time::timeout_at(over, exit_unreaped(leader)).await {
        Ok(WorkerExit::Unreaped) => return reap(child, leader, true).await,
        Ok(WorkerExit::Reaped) => return reap(child, leader, false).await,
        Ok(WorkerExit::Unobservable) => {}
        Err(_) => return kill(child).await,
    }
    match tokio::time::timeout_at(over, child.wait()).await {
        Ok(status) => {
            // Reaped already: see `WorkerExit::Unobservable`.
            signal_group(leader, Signal::Kill);
            status
        }
        Err(_) => kill(child).await,
    }
}

/// Reaps a worker that has exited, after SIGKILL to what it started and left
/// running: rustic's rclone is not stopped by the worker's `process::exit`.
/// `unreaped` tells whether the worker is still a zombie. A zombie holds its
/// process id, so the id is its group's still and nobody else's, and the
/// group may be signaled by it. Once the worker is reaped the id is free to
/// be given to another process, which a signal would reach.
async fn reap(
    child: &mut Child,
    leader: Option<u32>,
    unreaped: bool,
) -> std::io::Result<std::process::ExitStatus> {
    if unreaped {
        signal_group(leader, Signal::Kill);
    }
    child.wait().await
}

/// What a wait for the worker's exit, without reaping it, found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(all(target_os = "linux", not(target_env = "uclibc"))),
    expect(dead_code, reason = "only Linux can see an exit without reaping")
)]
enum WorkerExit {
    /// The worker exited and is a zombie still, holding its process id.
    Unreaped,
    /// Somebody reaped the worker first.
    Reaped,
    /// The exit can't be seen without reaping here. nix 0.31.3 has `waitid`
    /// on Linux only; on macOS the safe alternative, a kqueue `NOTE_EXIT`, is
    /// not known to report a process that exited before it was registered.
    /// A sandbox may refuse `waitid` too.
    ///
    /// The worker is then waited for by `Child::wait`, which reaps it, and
    /// its group is signaled by the id it had. POSIX keeps a process id from
    /// being reused while a process group of that id has a live member, so
    /// when the worker left a process running, which is what the signal is
    /// for, it can only reach that group. The residual window is this: the
    /// group has no live member, and in the instants between the reaping and
    /// the signal a new process takes the id and makes itself a group
    /// leader. The signal then reaches that stranger.
    Unobservable,
}

/// What one `waitid(WEXITED | WNOWAIT)` found; `None` when interrupted.
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
fn worker_exit(waited: nix::Result<nix::sys::wait::WaitStatus>) -> Option<WorkerExit> {
    use nix::errno::Errno;

    match waited {
        // The arguments are fixed and valid, so EINVAL comes from nix after
        // libc's `waitid` succeeded: a status it can't convert, such as a
        // real-time signal. The worker exited all the same.
        Ok(_) | Err(Errno::EINVAL) => Some(WorkerExit::Unreaped),
        Err(Errno::ECHILD) => Some(WorkerExit::Reaped),
        Err(Errno::EINTR) => None,
        Err(_) => Some(WorkerExit::Unobservable),
    }
}

/// Resolves once the worker has exited, *without reaping it*: `waitid` with
/// `WNOWAIT` leaves the worker a zombie that holds its process id, which
/// `Child::wait` would free. The wait is a thread of its own, one for each
/// running worker, and ends with the worker.
#[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
async fn exit_unreaped(leader: Option<u32>) -> WorkerExit {
    use nix::sys::wait::{Id, WaitPidFlag, waitid};
    use nix::unistd::Pid;

    let Some(pid) = leader.and_then(|pid| i32::try_from(pid).ok()) else {
        return WorkerExit::Unobservable;
    };
    let waiting = tokio::task::spawn_blocking(move || {
        loop {
            let flags = WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT;
            if let Some(exit) = worker_exit(waitid(Id::Pid(Pid::from_raw(pid)), flags)) {
                return exit;
            }
        }
    });
    waiting.await.unwrap_or(WorkerExit::Unobservable)
}

/// See [`WorkerExit::Unobservable`].
#[cfg(not(all(target_os = "linux", not(target_env = "uclibc"))))]
async fn exit_unreaped(_leader: Option<u32>) -> WorkerExit {
    WorkerExit::Unobservable
}

#[derive(Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

#[cfg(unix)]
impl Signal {
    fn nix(self) -> nix::sys::signal::Signal {
        match self {
            Self::Terminate => nix::sys::signal::Signal::SIGTERM,
            Self::Kill => nix::sys::signal::Signal::SIGKILL,
        }
    }
}

/// Signals the process group the worker leads: the worker and every process
/// it started that didn't leave the group. `leader` is the worker's process
/// id, `None` once the worker was reaped, and nothing is signaled then: the
/// number may belong to somebody else by now. Whether the signal was sent;
/// off Unix there are no process groups.
#[cfg(unix)]
fn signal_group(leader: Option<u32>, signal: Signal) -> bool {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;

    leader
        .and_then(|pid| i32::try_from(pid).ok())
        .is_some_and(|pid| killpg(Pid::from_raw(pid), signal.nix()).is_ok())
}

#[cfg(not(unix))]
fn signal_group(_leader: Option<u32>, _signal: Signal) -> bool {
    false
}

/// Signals the worker alone, not what it started, on the terms of
/// [`signal_group`].
#[cfg(unix)]
fn signal_worker(leader: Option<u32>, signal: Signal) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    leader
        .and_then(|pid| i32::try_from(pid).ok())
        .is_some_and(|pid| kill(Pid::from_raw(pid), signal.nix()).is_ok())
}

#[cfg(not(unix))]
fn signal_worker(_leader: Option<u32>, _signal: Signal) -> bool {
    false
}

/// The worker's stdout, shared by the job's events and its stop. A pipe
/// keeps the waker of one reader only, so the two never read it at once:
/// the events read it until the job is stopped and find it ended from then
/// on ([`ForEvents`]), and the stop reads and drops what the worker still
/// writes until it exits ([`ForStop`]). Each read takes the pipe for that
/// one poll only, so a read of the events left pending, as a `select!` that
/// took another branch leaves it, never keeps the stop from reading.
#[derive(Clone)]
pub(super) struct EventPipe {
    pub(super) stdout: Arc<Mutex<ChildStdout>>,
    pub(super) stopped: Arc<AtomicBool>,
}

/// [`EventPipe`] as the job's events read it.
pub(super) struct ForEvents(pub(super) EventPipe);

impl AsyncRead for ForEvents {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut stdout = self.0.stdout.lock().unwrap_or_else(PoisonError::into_inner);
        // Looked at under the lock, which the stop takes for its first read
        // only once it has set the flag: no read of the events comes after
        // that one, to take the waker it left.
        if self.0.stopped.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut *stdout).poll_read(cx, buf)
    }
}

/// [`EventPipe`] as the stop reads it.
pub(super) struct ForStop(pub(super) EventPipe);

impl AsyncRead for ForStop {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut stdout = self.0.stdout.lock().unwrap_or_else(PoisonError::into_inner);
        Pin::new(&mut *stdout).poll_read(cx, buf)
    }
}

/// Stops one job.
pub(super) struct Control {
    pub(super) commands: mpsc::UnboundedSender<Order>,
    pub(super) exited: watch::Receiver<Exited>,
    pub(super) stopped: Arc<AtomicBool>,
    pub(super) pipe: ForStop,
}

#[async_trait]
impl JobControl for Control {
    async fn stop(&mut self) {
        // The job's events end here, whatever the worker still writes.
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Order::Stop);
        // What the worker writes until it exits is read and dropped: a
        // backup winding down (§4.1) whose writer is blocked on a full pipe
        // would otherwise never save its index, and the grace's SIGKILL
        // would leave its packs unindexed. Nothing read here is delivered or
        // logged. A pipe that ends or fails first leaves the wait for the
        // exit, which the grace bounds.
        let pipe = &mut self.pipe;
        let drained = async {
            let _ = tokio::io::copy(pipe, &mut tokio::io::sink()).await;
            std::future::pending::<()>().await;
        };
        tokio::select! {
            biased;
            _ = self.exited.wait_for(Option::is_some) => {}
            () = drained => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    #[cfg(unix)]
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use app::ports::{Engine, EngineEvent, RunningJob};
    use domain::{OperationId, Uuid};
    use futures::stream::{self, StreamExt};

    use super::*;
    use crate::client::WorkerEngine;
    use crate::client::test_support::*;
    use crate::job::{OpResult, WorkerEvent};

    /// A worker may still be cleaning up after it reports its result; the
    /// daemon dropping the job must not kill it mid-way.
    #[tokio::test]
    async fn a_worker_may_finish_after_reporting_its_result() {
        let scratch = std::env::temp_dir().join(format!("ladderback-reap-{}", std::process::id()));
        let _ = std::fs::remove_file(&scratch);
        let result =
            serde_json::to_string(&WorkerEvent::Result(OpResult::Fake { steps_completed: 0 }))
                .unwrap();
        let script = format!(
            "cat >/dev/null; echo '{result}'; sleep 0.3; touch '{}'",
            scratch.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob {
            mut events,
            control,
        } = run(&engine).await;
        assert_matches!(events.next().await, Some(EngineEvent::Succeeded(_)));
        drop((events, control));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(scratch.exists(), "the worker was killed after its result");
        let _ = std::fs::remove_file(&scratch);
    }

    #[tokio::test]
    async fn stopping_a_worker_ends_it_with_sigterm_and_ends_its_events_silently() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; sleep 5"]);
        let RunningJob {
            mut events,
            mut control,
        } = run(&engine).await;
        let began = Instant::now();
        control.stop().await;
        assert!(began.elapsed() < Duration::from_secs(5));
        assert!(events.next().await.is_none(), "events followed a stop");
        control.stop().await; // idempotent
    }

    #[tokio::test]
    async fn a_worker_that_ignores_sigterm_is_killed_after_the_grace_period() {
        let engine = WorkerEngine::new("sh", ["-c", "trap '' TERM; cat >/dev/null; sleep 5"])
            .with_stop_grace(Duration::from_millis(300));
        let RunningJob {
            mut events,
            mut control,
        } = run(&engine).await;
        // Let the shell install its trap before the signal arrives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let began = Instant::now();
        control.stop().await;
        let took = began.elapsed();
        assert!(
            took >= Duration::from_millis(250),
            "killed too early: {took:?}"
        );
        assert!(
            took < Duration::from_secs(4),
            "waited for the worker: {took:?}"
        );
        assert!(events.next().await.is_none());
    }

    /// Asserts that the events of a job the daemon's stop of its workers
    /// stopped, called once that stop has returned and so once the worker
    /// has exited, never end: what runs the job then neither records nor
    /// announces an end, nor runs a hook for one, and the daemon's stop
    /// records the operation as ended `daemon-shut-down` (decision 92).
    async fn assert_events_never_end(events: &mut stream::BoxStream<'static, EngineEvent>) {
        let next = tokio::time::timeout(Duration::from_millis(300), events.next()).await;
        assert!(
            next.is_err(),
            "the events of a job the daemon stopped went on: {next:?}"
        );
    }

    /// When the daemon stops, every worker it runs is stopped as a cancel
    /// stops one, SIGTERM first, and the daemon waits until each has
    /// exited. Each worker here ends 0.3 s after its SIGTERM, leaving a mark.
    #[tokio::test]
    async fn stopping_the_workers_sends_each_sigterm_and_waits_until_it_has_exited() {
        let scratch = tempfile::tempdir().unwrap();
        let marks = [scratch.path().join("first"), scratch.path().join("second")];
        let script = "trap 'sleep 0.3; touch \"$1\"; exit 0' TERM; cat >/dev/null; sleep 5 & wait";
        let mut jobs = Vec::new();
        let mut workers = Vec::new();
        for mark in &marks {
            let engine = WorkerEngine::new("sh", ["-c", script, "sh", mark.to_str().unwrap()]);
            jobs.push(run(&engine).await);
            workers.push(engine.workers());
        }
        // Let the shells install their traps before the signal arrives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let began = Instant::now();
        for workers in workers {
            workers.stop().await;
        }
        let took = began.elapsed();
        assert!(
            took < Duration::from_secs(4),
            "waited for the sleep: {took:?}"
        );
        for mark in &marks {
            assert!(
                mark.exists(),
                "the stop returned before {} ended on its SIGTERM",
                mark.display()
            );
        }
        for job in &mut jobs {
            assert_events_never_end(&mut job.events).await;
        }
    }

    /// The daemon's stop asks the workers to stop at once and lets go of
    /// their jobs straight after, as it stops the runs that read them
    /// (decision 92): a worker let go of so still gets its SIGTERM and its
    /// grace, as a backup's wind-down needs (decision 50), and is not killed
    /// as one let go of before its result. Each worker here ends 0.3 s
    /// after its SIGTERM, leaving a mark of its own.
    #[tokio::test]
    async fn a_job_let_go_of_once_the_workers_are_asked_to_stop_still_ends_on_its_sigterm() {
        let scratch = tempfile::tempdir().unwrap();
        let script =
            "trap 'sleep 0.3; touch \"$1/$$\"; exit 0' TERM; cat >/dev/null; sleep 5 & wait";
        let engine =
            WorkerEngine::new("sh", ["-c", script, "sh", scratch.path().to_str().unwrap()]);
        let mut jobs = Vec::new();
        for _ in 0..8 {
            jobs.push(run(&engine).await);
        }
        // Let the shells install their traps before the signal arrives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let workers = engine.workers();
        let stopped = workers.stop();
        drop(jobs);
        tokio::time::timeout(Duration::from_secs(4), stopped)
            .await
            .expect("the workers ended on their SIGTERM");
        let marks = std::fs::read_dir(scratch.path()).unwrap().count();
        assert_eq!(marks, 8, "a worker let go of was killed, not stopped");
    }

    /// Several jobs of one engine are stopped by one stop of its workers.
    #[tokio::test]
    async fn one_stop_of_the_workers_stops_every_job_of_the_engine() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; sleep 5"]);
        let mut jobs = vec![run(&engine).await, run(&engine).await];
        let began = Instant::now();
        engine.workers().stop().await;
        assert!(
            began.elapsed() < Duration::from_secs(4),
            "a worker was left to run"
        );
        for job in &mut jobs {
            assert_events_never_end(&mut job.events).await;
        }
    }

    #[tokio::test]
    async fn a_worker_that_ignores_sigterm_is_killed_after_the_grace_when_the_workers_are_stopped()
    {
        let engine = WorkerEngine::new("sh", ["-c", "trap '' TERM; cat >/dev/null; sleep 5"])
            .with_stop_grace(Duration::from_millis(300));
        let RunningJob { mut events, .. } = run(&engine).await;
        // Let the shell install its trap before the signal arrives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let began = Instant::now();
        engine.workers().stop().await;
        let took = began.elapsed();
        assert!(
            took >= Duration::from_millis(250),
            "killed too early: {took:?}"
        );
        assert!(
            took < Duration::from_secs(4),
            "waited for the worker: {took:?}"
        );
        assert_events_never_end(&mut events).await;
    }

    #[tokio::test]
    async fn stopping_the_workers_when_none_runs_returns_at_once() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; sleep 5"]);
        tokio::time::timeout(Duration::from_secs(4), engine.workers().stop())
            .await
            .expect("the stop waited for no worker");
    }

    /// A run that asks for a job once the daemon is stopping gets no worker,
    /// which nothing would be left to stop, and no answer either: an answer
    /// would end its operation, announce the end and run its hooks, where an
    /// operation an orderly stop interrupts is recorded by the stop and
    /// announced by no one (decision 92). It waits for the daemon to end, and
    /// keeps no later stop of the workers waiting.
    #[tokio::test]
    async fn a_job_asked_for_once_the_workers_are_stopped_never_starts() {
        let engine = WorkerEngine::new("sh", ["-c", "cat >/dev/null; sleep 5"]);
        engine.workers().stop().await;
        let engine = Arc::new(engine);
        let asked = tokio::spawn({
            let engine = Arc::clone(&engine);
            async move {
                engine
                    .run(OperationId::from_uuid(Uuid::from_u128(1)), fake_job())
                    .await
                    .map(|_| ())
            }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!asked.is_finished(), "the job was answered: {asked:?}");
        tokio::time::timeout(Duration::from_secs(4), engine.workers().stop())
            .await
            .expect("the job asked for keeps no stop waiting");
        asked.abort();
    }

    #[tokio::test]
    async fn dropping_a_running_job_kills_the_worker_at_once() {
        let scratch = std::env::temp_dir().join(format!("ladderback-drop-{}", std::process::id()));
        let _ = std::fs::remove_file(&scratch);
        let script = format!("cat >/dev/null; sleep 1; touch '{}'", scratch.display());
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        drop(run(&engine).await);
        tokio::time::sleep(Duration::from_millis(1600)).await;
        assert!(!scratch.exists(), "the worker outlived its dropped job");
    }

    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_worker_that_exited_is_still_unreaped_even_when_its_status_cannot_be_read() {
        use nix::errno::Errno;
        use nix::sys::wait::WaitStatus;
        use nix::unistd::Pid;

        let exited = Ok(WaitStatus::Exited(Pid::from_raw(42), 3));
        assert_eq!(worker_exit(exited), Some(WorkerExit::Unreaped));
        // A real-time signal: libc's waitid succeeded, nix can't name it.
        assert_eq!(worker_exit(Err(Errno::EINVAL)), Some(WorkerExit::Unreaped));
    }

    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_worker_reaped_elsewhere_is_only_reaped_and_an_interrupted_wait_is_retried() {
        use nix::errno::Errno;

        assert_eq!(worker_exit(Err(Errno::ECHILD)), Some(WorkerExit::Reaped));
        assert_eq!(worker_exit(Err(Errno::EINTR)), None);
    }

    /// A sandbox that refuses `waitid` must not leave the worker unwaited:
    /// it is waited for by `Child::wait` instead.
    #[cfg(all(target_os = "linux", not(target_env = "uclibc")))]
    #[test]
    fn a_wait_the_system_refuses_falls_back_to_reaping_the_worker() {
        use nix::errno::Errno;

        for refused in [Errno::ENOSYS, Errno::EPERM, Errno::EFAULT] {
            assert_eq!(
                worker_exit(Err(refused)),
                Some(WorkerExit::Unobservable),
                "{refused}"
            );
        }
    }

    /// A worker script that leaves a `sleep` of its own running, as rustic's
    /// rclone backend does, and the file that holds that process's id.
    #[cfg(unix)]
    fn worker_with_a_child(trap: &str, dir: &Path) -> (WorkerEngine, PathBuf) {
        let pid_file = dir.join("child.pid");
        let script = format!(
            "{trap} cat >/dev/null; sleep 30 & echo $! >'{}'; wait",
            pid_file.display()
        );
        (WorkerEngine::new("sh", ["-c", script.as_str()]), pid_file)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stopping_a_worker_stops_the_processes_it_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("", dir.path());
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        let child = pid_from(&pid_file).await;
        control.stop().await;
        assert!(
            gone_soon(child).await,
            "the worker's child outlived the stop"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_child_that_ignores_sigterm_is_killed_with_the_worker_after_the_grace_period() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("trap '' TERM;", dir.path());
        let engine = engine.with_stop_grace(Duration::from_millis(300));
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        let child = pid_from(&pid_file).await;
        control.stop().await;
        assert!(
            gone_soon(child).await,
            "the worker's child survived the kill"
        );
    }

    /// A stop's SIGTERM goes to the worker alone: a cancelled backup winds
    /// down through the rclone or ssh its backend started (§4.1), which the
    /// signal must not end first. What the worker started is killed once the
    /// worker has exited.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_leaves_what_the_worker_started_running_until_the_worker_has_wound_down() {
        let dir = tempfile::tempdir().unwrap();
        let terminated = dir.path().join("child-terminated");
        let child_script = dir.path().join("child.sh");
        // The child writes its id once a SIGTERM would leave a mark.
        std::fs::write(
            &child_script,
            "trap 'touch \"$1\"; exit' TERM; echo $$ >\"$2.new\"; mv \"$2.new\" \"$2\"; \
             while :; do sleep 0.05; done",
        )
        .unwrap();
        let pid_file = dir.path().join("child.pid");
        // The worker takes a while to end, as a backup winding down does.
        let script = format!(
            "trap 'sleep 0.3; exit 143' TERM; cat >/dev/null; sh '{}' '{}' '{}' & wait",
            child_script.display(),
            terminated.display(),
            pid_file.display()
        );
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        let child = pid_from(&pid_file).await;
        control.stop().await;
        assert!(
            !terminated.exists(),
            "the stop's SIGTERM reached a process the worker started"
        );
        assert!(
            gone_soon(child).await,
            "the worker's child outlived the worker"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_a_running_job_kills_the_processes_the_worker_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, pid_file) = worker_with_a_child("", dir.path());
        let job = run(&engine).await;
        let child = pid_from(&pid_file).await;
        drop(job);
        assert!(
            gone_soon(child).await,
            "the worker's child outlived the dropped job"
        );
    }

    /// rustic's rclone is not stopped by the worker's `process::exit`, so
    /// however the worker ends, at its deadline, with a stream it cannot
    /// write to, on a SIGTERM, or killed, what it started ends with it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_worker_that_ends_by_itself_takes_the_processes_it_started_with_it() {
        for (how, ending) in [
            ("at its deadline", "exit 75"),
            ("with a stream it cannot write to", "exit 74"),
            ("on a SIGTERM", "exit 143"),
            ("by being killed", "kill -KILL $$"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let pid_file = dir.path().join("child.pid");
            let script = format!(
                "cat >/dev/null; sleep 30 & echo $! >'{}'; {ending}",
                pid_file.display()
            );
            let engine = WorkerEngine::new("sh", ["-c", script.as_str()]);
            let events = run(&engine).await.events;
            let child = pid_from(&pid_file).await;
            let _ = events.collect::<Vec<_>>().await;
            assert!(
                gone_soon(child).await,
                "the process a worker started outlived it when it ended {how}"
            );
        }
    }

    #[tokio::test]
    async fn a_cleanup_deadline_that_cannot_be_represented_ends_the_cleanup_at_once() {
        let now = tokio::time::Instant::now();
        assert_eq!(deadline_after(now, Duration::MAX), now);
        assert_eq!(
            deadline_after(now, Duration::from_secs(10)),
            now + Duration::from_secs(10)
        );
    }

    /// A stop returns once the job has stopped, not when the worker's output
    /// ends: one that closes its output and ignores SIGTERM is waited for
    /// until the grace's SIGKILL.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_waits_for_the_exit_of_a_worker_whose_output_ended_first() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("worker.pid");
        let script = format!(
            "trap '' TERM; cat >/dev/null; exec >&-; echo $$ >'{}'; exec sleep 30",
            pid_file.display()
        );
        let grace = Duration::from_secs(1);
        let engine = WorkerEngine::new("sh", ["-c", script.as_str()]).with_stop_grace(grace);
        let RunningJob {
            events: _events,
            mut control,
        } = run(&engine).await;
        // The process id is written once SIGTERM is ignored and the output
        // is closed.
        let worker = pid_from(&pid_file).await;
        let began = Instant::now();
        control.stop().await;
        let took = began.elapsed();
        assert!(
            took >= grace,
            "the stop returned after {took:?}, before the grace's SIGKILL"
        );
        assert!(gone_soon(worker).await, "the worker outlived its stop");
    }
}
