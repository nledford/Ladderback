//! Processes a test run starts and owns: a daemon above all, with the
//! workers it starts. Each is ended when its [`Owned`] is dropped (a scenario
//! that passes, fails or panics drops its world), and every one still running
//! is ended by [`end_all`], which the scenario watch calls before it stops a
//! run at its limit (#499): `std::process::exit` runs no destructor, and a
//! daemon left running holds the runner's output pipe, so whatever waits for
//! that pipe to close (`just red-first`) waits for its own timeout.
//!
//! Ending one stops the process, kills the process group each of its children
//! leads (a daemon's workers each lead one, with whatever they started), then
//! kills and reaps the process.

use std::io;
use std::process::{ChildStderr, ChildStdout, Command, ExitStatus};

/// A child process the run owns, ended with its children's process groups
/// when dropped.
#[derive(Debug)]
pub struct Owned {
    pid: u32,
}

impl Owned {
    /// Starts `command` through the spawn gate. Call it outside
    /// `spawn_gate::hold`: it takes the gate itself.
    ///
    /// # Errors
    ///
    /// The process could not start.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let _ = command;
        todo!()
    }

    /// The process's ID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Its stdout, if it was piped and not taken yet.
    #[must_use]
    pub fn take_stdout(&self) -> Option<ChildStdout> {
        todo!()
    }

    /// Its stderr, if it was piped and not taken yet.
    #[must_use]
    pub fn take_stderr(&self) -> Option<ChildStderr> {
        todo!()
    }

    /// How it ended, if it has.
    ///
    /// # Errors
    ///
    /// Its status could not be read.
    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        todo!()
    }

    /// Waits until it ends by itself.
    ///
    /// # Errors
    ///
    /// Its status could not be read.
    pub fn wait(&self) -> io::Result<ExitStatus> {
        todo!()
    }
}
