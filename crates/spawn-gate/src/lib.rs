//! The spawn gate: one child process starts at a time in this process
//! (technical design §16, decision 58).
//!
//! A child inherits every descriptor of its parent that is not close-on-exec
//! when it starts. Where the platform has no `pipe2` (macOS), the standard
//! library makes a pipe in two steps, `pipe` and then `fcntl` for
//! close-on-exec, so for a moment both ends can be inherited: a child that
//! another thread starts in that moment holds them for as long as it runs.
//! A pipe whose read end a stranger holds never fails its writer, and one
//! whose write end a stranger holds never ends for its reader. Every start
//! makes its pipes (the child's stdin, stdout and stderr) inside the call
//! that starts the child, so starts that go through the gate one at a time
//! cannot inherit each other's: each start's pipes are close-on-exec before
//! the next start begins.
//!
//! Whatever starts a child goes through [`hold`], and so does whatever makes
//! a descriptor that is not close-on-exec from the start.
//!
//! A start can also hold a copy of every descriptor of the process, close-on-
//! exec or not: where the standard library starts a child with `fork` and
//! `exec` (a program named without a path, when the child's environment is
//! cleared or sets its own `PATH`), the forked child has them all until its
//! `exec`, and the start holds the gate until then. So a pipe end whose peer
//! must see it gone once a call returns is dropped inside [`hold`] too: then
//! no start in flight has a copy left.
//!
//! A start whose `exec` stalls holds the gate as long, so async code waits
//! for it on the blocking pool with [`hold_blocking`], never on one of the
//! runtime's threads.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::sync::{Mutex, PoisonError};

/// The gate: one per process, as the descriptor table is.
static GATE: Mutex<()> = Mutex::new(());

/// Runs `start` while no other call of `hold` in this process runs: `start`
/// starts a child, or makes a descriptor that is not close-on-exec from the
/// start and makes it so before it returns. A `start` that panicked leaves
/// the gate open: the gate guards no data, only the moment.
pub fn hold<T>(start: impl FnOnce() -> T) -> T {
    let _held = GATE.lock().unwrap_or_else(PoisonError::into_inner);
    start()
}

/// [`hold`] on tokio's blocking pool: waiting for the gate, and a start whose
/// `exec` stalls (a script on a hung mount, the first scan of a new program),
/// park a thread of the pool, never one of the runtime's, whose other tasks
/// run on meanwhile. `start` runs in the runtime's context there, so it may
/// start a `tokio::process` child, and it runs to its end even if the caller
/// stops waiting. Call it within a tokio runtime.
///
/// # Errors
///
/// The runtime shut down before `start` ran. A `start` that panicked panics
/// here with the same payload.
pub async fn hold_blocking<T: Send + 'static>(
    start: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(move || hold(start))
        .await
        .map_err(|error| match error.try_into_panic() {
            Ok(payload) => std::panic::resume_unwind(payload),
            Err(cancelled) => std::io::Error::other(cancelled),
        })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::thread;
    use std::time::Duration;

    use super::hold;

    /// A second start waits while the first holds the gate, and goes through
    /// as soon as the first lets go.
    #[test]
    fn a_start_waits_while_another_holds_the_gate() {
        let (entered, first_in) = mpsc::channel();
        let (leave, first_leaves) = mpsc::channel::<()>();
        let first = thread::spawn(move || {
            hold(|| {
                entered.send(()).unwrap();
                first_leaves.recv().unwrap();
            });
        });
        first_in.recv_timeout(Duration::from_secs(5)).unwrap();
        let (passed, second_in) = mpsc::channel();
        let second = thread::spawn(move || hold(|| passed.send(()).unwrap()));
        assert_eq!(
            second_in.recv_timeout(Duration::from_millis(200)),
            Err(RecvTimeoutError::Timeout),
            "the second start went through while the first held the gate"
        );
        leave.send(()).unwrap();
        second_in
            .recv_timeout(Duration::from_secs(5))
            .expect("the second start went through once the first let go");
        first.join().unwrap();
        second.join().unwrap();
    }

    /// A start that panicked leaves the gate open for the next: a lost
    /// start must not stop every later one.
    #[test]
    fn a_start_that_panicked_leaves_the_gate_open() {
        let panicked = thread::spawn(|| hold(|| panic!("the start failed"))).join();
        assert!(panicked.is_err());
        assert_eq!(hold(|| 7), 7);
    }

    /// Waiting for the gate on the blocking pool leaves the runtime's own
    /// thread free: here a task of the same one-thread runtime lets the gate
    /// go, which it can only while the wait goes on elsewhere.
    #[tokio::test]
    async fn a_start_waits_for_the_gate_off_the_runtime() {
        let (entered, first_in) = mpsc::channel();
        let (leave, first_leaves) = mpsc::channel::<()>();
        let first = thread::spawn(move || {
            hold(|| {
                entered.send(()).unwrap();
                first_leaves.recv_timeout(Duration::from_secs(2)).is_ok()
            })
        });
        first_in.recv_timeout(Duration::from_secs(5)).unwrap();
        let let_go = async {
            tokio::task::yield_now().await;
            let _ = leave.send(());
        };
        let (started, ()) = tokio::join!(super::hold_blocking(|| 7), let_go);
        assert_eq!(started.unwrap(), 7);
        assert!(
            first.join().unwrap(),
            "the start waited for the gate on the runtime's thread"
        );
    }

    /// A start that panicked on the blocking pool panics where it was
    /// awaited, as it would have had it run there.
    #[tokio::test]
    async fn a_start_that_panicked_off_the_runtime_panics_its_caller() {
        let awaited = tokio::spawn(super::hold_blocking(|| panic!("the start failed"))).await;
        assert!(awaited.is_err_and(|error| error.is_panic()));
        assert_eq!(super::hold_blocking(|| 7).await.unwrap(), 7);
    }
}
