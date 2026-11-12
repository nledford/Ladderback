//! A descriptor a child started now would inherit, as a start makes one on
//! a platform without `pipe2` (decision 58): how a test shows that what
//! starts a child waits for the spawn gate, on every platform, not only
//! where the standard library leaves such a moment open.

use std::os::fd::AsRawFd;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How long the pipe stays open, and the gate held: a start that does not
/// wait for the gate has long begun when it closes, and one that does wait
/// is not held up for long.
pub const OPEN_FOR: Duration = Duration::from_millis(500);

/// A pipe that is not close-on-exec, open for [`OPEN_FOR`] while the spawn
/// gate is held: a child that starts meanwhile without waiting for the gate
/// inherits both its ends. Dropping it waits until both are closed.
pub struct InheritablePipe {
    numbers: [i32; 2],
    holder: Option<JoinHandle<()>>,
}

impl InheritablePipe {
    /// Opens the pipe on a thread of its own, and returns once it is open.
    pub fn open() -> Self {
        let (opened, open) = mpsc::channel();
        let holder = thread::spawn(move || {
            spawn_gate::hold(|| {
                let (read, write) = nix::unistd::pipe().expect("a pipe");
                let _ = opened.send([read.as_raw_fd(), write.as_raw_fd()]);
                thread::sleep(OPEN_FOR);
            });
        });
        let numbers = open.recv().expect("the inheritable pipe opened");
        Self {
            numbers,
            holder: Some(holder),
        }
    }

    /// The descriptor numbers of its two ends, as a script takes them.
    pub fn numbers(&self) -> [String; 2] {
        self.numbers.map(|number| number.to_string())
    }
}

impl Drop for InheritablePipe {
    fn drop(&mut self) {
        if let Some(holder) = self.holder.take() {
            let _ = holder.join();
        }
    }
}

/// A shell script that prints the number of each of its arguments that is
/// an open descriptor of its own, one a line. It runs no command of its own
/// and opens nothing, so every one it finds it inherited.
pub const INHERITED: &str = "for n; do if [ -e /dev/fd/$n ]; then echo $n; fi; done";

/// Holds the spawn gate on a thread of its own for `held`, as a start in
/// flight does until its child has run `exec`, and returns once it holds
/// it. Joining the handle gives the moment it let go.
pub fn start_in_flight(held: Duration) -> JoinHandle<Instant> {
    let (holding, hold) = mpsc::channel();
    let start = thread::spawn(move || {
        spawn_gate::hold(|| {
            let _ = holding.send(());
            thread::sleep(held);
            Instant::now()
        })
    });
    hold.recv().expect("the gate held");
    start
}
