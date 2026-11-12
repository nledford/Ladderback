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
//! `exec` (a program looked up on a `PATH` the child's environment sets), the
//! forked child has them all until its `exec`, and the start holds the gate
//! until then. So a pipe end whose peer must see it gone once a call returns
//! is dropped inside [`hold`] too: then no start in flight has a copy left.

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
}
