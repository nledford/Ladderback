use std::path::PathBuf;

use async_trait::async_trait;
use domain::service::ServicePath;

/// The user's service manager (operations §12): systemd's user manager on
/// Linux, launchd on macOS. It keeps the service file and runs what the file
/// names in the background, now and from each login on. The service runs
/// `program serve` and nothing else; its file names no secret, and the
/// manager is never handed one (I4).
///
/// Contract (checked by `contract::service`):
/// - after [`install`](Self::install) the service runs, and its file names
///   the program and `serve`;
/// - installing over an installed service replaces its file and starts it
///   again: one service runs, and it runs the new file;
/// - after [`remove`](Self::remove) the service does not run and its file is
///   gone; removing a service that is not installed is
///   [`Removed::NotInstalled`], not an error, and asks the manager nothing;
/// - a manager that refuses is a [`ServiceError`] with its words.
///
/// Neither replaces nor removes a file at the service file's place that
/// `install-service` didn't write ([`ServiceProblem::NotOurs`]).
#[async_trait]
pub trait ServiceManager: Send + Sync {
    /// Writes the service file for `program` and has the manager start the
    /// service.
    async fn install(&self, program: &ServicePath) -> Result<Installed, ServiceError>;

    /// Has the manager stop the service, and removes its file.
    async fn remove(&self) -> Result<Removed, ServiceError>;
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The service file.
    pub file: PathBuf,
    /// Whether a service was installed before, and is replaced.
    pub replaced: bool,
}

/// What a removal did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removed {
    /// The service was stopped and its file removed.
    Removed { file: PathBuf },
    /// There was no service file: nothing was asked of the manager.
    NotInstalled { file: PathBuf },
}

/// Why an install or a removal did not finish. What was done before it
/// stays done: a file written stays written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    /// The service file.
    pub file: PathBuf,
    pub problem: ServiceProblem,
}

/// What went wrong, with the platform's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceProblem {
    /// The service file couldn't be read, written or removed.
    File { detail: String },
    /// The manager's program could not be started. `command` is the command
    /// line, as a person would type it.
    ManagerMissing { command: String, detail: String },
    /// The manager's program ended with an error; `said` is what it printed
    /// on its standard error, perhaps cut short.
    ManagerRefused { command: String, said: String },
    /// The manager's program had not ended after `seconds`, and was stopped.
    ManagerSilent { command: String, seconds: u64 },
    /// The manager was asked to stop the service with `command`, and still
    /// had it after `seconds`. The service file was left as it was, and an
    /// install started no new service.
    StillStopping { command: String, seconds: u64 },
    /// The file at the service file's place was not written by
    /// `install-service`: a unit written by hand, or a link such as
    /// systemd's mask. It is left as it is, and the manager asked nothing.
    NotOurs,
}
