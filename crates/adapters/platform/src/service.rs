//! The user's service manager (operations §12): a systemd user unit on
//! Linux, a launchd agent on macOS, behind `app`'s `ServiceManager` port.
//!
//! The service file is written here, from a template with two inputs: the
//! program's path, and on macOS the home folder its log goes in. Both are
//! [`ServicePath`]s, absolute, text, with no `..` and no control character,
//! and each is quoted by its file's own rules (`files`). Nothing of the
//! environment goes into the file, so no secret can (I4).
//!
//! The manager is run as `systemctl --user …` or `launchctl …`, by name from
//! the `PATH`, as a shell would, with an environment of only the variables
//! it needs to find the user's manager and speak the user's language
//! ([`Environment`]), through the spawn gate (decision 58), with argv only.

mod files;
mod manager;
mod program;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app::ports::{Installed, Removed, ServiceError, ServiceManager};
use async_trait::async_trait;
use domain::service::ServicePath;

pub use files::{launchd_plist, systemd_unit};
pub use manager::Environment;
pub use program::program_path;

/// The unit's name under systemd.
pub const SYSTEMD_UNIT: &str = "ladderback.service";

/// The agent's label under launchd, and its plist's name without `.plist`.
pub const LAUNCHD_LABEL: &str = "io.github.nledford.ladderback";

/// How long the manager's program may take to answer one call. `systemctl`
/// waits 25 s for an answer from the manager itself.
const MANAGER_LIMIT: Duration = Duration::from_secs(30);

/// Why there is no service manager to install a service with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// The program runs as root (its effective user ID is 0), as under
    /// `sudo`: the service would be root's, with root's files, not the
    /// user's it was meant for.
    AsRoot,
    /// No home folder: `HOME` is unset, empty, or not a path a service file
    /// may name.
    NoHome,
    /// This platform has no manager Ladderback knows.
    Unsupported,
}

/// Which manager, and what it needs.
#[derive(Clone, Debug)]
enum Kind {
    Systemd,
    Launchd {
        /// The user's ID: the agent lives in the `gui/<uid>` domain.
        uid: u32,
        /// Where the agent's log goes.
        log: ServicePath,
    },
}

/// The service of one user's service manager.
#[derive(Clone, Debug)]
pub struct UserService {
    kind: Kind,
    /// The unit's name or the agent's label.
    name: String,
    /// The service file.
    file: PathBuf,
    environment: Environment,
    limit: Duration,
}

impl UserService {
    /// This platform's manager for the user whose environment `env` reads,
    /// and the variables `vars` the manager is run with, before
    /// [`Environment`] keeps the ones it needs.
    ///
    /// # Errors
    ///
    /// The program runs as root, there is no home folder, or the platform
    /// has no manager Ladderback knows.
    pub fn of_this_platform(
        env: impl Fn(&str) -> Option<OsString>,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, Unavailable> {
        Self::of_platform(env, vars, false)
    }

    /// [`Self::of_this_platform`], for a program that runs as root when
    /// `as_root`.
    fn of_platform(
        env: impl Fn(&str) -> Option<OsString>,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
        as_root: bool,
    ) -> Result<Self, Unavailable> {
        let _ = (env, vars.into_iter().count(), as_root);
        todo!()
    }

    /// A systemd user unit named `name`, whose file goes in `unit_folder`.
    pub fn systemd(unit_folder: &Path, name: &str, environment: Environment) -> Self {
        let _ = (unit_folder, name, environment, Kind::Systemd);
        todo!()
    }

    /// A launchd agent labelled `label` in `gui/<uid>`, whose plist goes in
    /// `home/Library/LaunchAgents` and log in `home/Library/Logs`.
    ///
    /// # Errors
    ///
    /// The home folder's log path can't be named by a service file.
    pub fn launchd(
        home: &ServicePath,
        label: &str,
        uid: u32,
        environment: Environment,
    ) -> Result<Self, Unavailable> {
        let _ = (home, label, uid, environment);
        todo!()
    }

    /// The same service, with `limit` for each call of the manager.
    #[must_use]
    pub fn with_limit(self, limit: Duration) -> Self {
        Self { limit, ..self }
    }

    /// The service file.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

#[async_trait]
impl ServiceManager for UserService {
    async fn install(&self, program: &ServicePath) -> Result<Installed, ServiceError> {
        let _ = (program, MANAGER_LIMIT, &self.kind, &self.name, self.limit);
        todo!()
    }

    async fn remove(&self) -> Result<Removed, ServiceError> {
        todo!()
    }
}

#[cfg(all(test, unix))]
mod tests;
