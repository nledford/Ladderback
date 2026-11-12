//! The user's service manager (operations §12): a systemd user unit on
//! Linux, a launchd agent on macOS, behind `app`'s `ServiceManager` port.
//!
//! The service file is written here, from a template with two inputs: the
//! program's path, and on macOS the home folder its log goes in. Both are
//! [`ServicePath`]s, absolute, text, with no `..`, no control character and
//! no quote or backslash, and each is quoted by its file's own rules
//! (`files`). Nothing of the environment goes into the file, so no secret
//! can (I4). A file at the service file's place is replaced or removed only
//! when it is a file with the template's mark line: one written by hand, or
//! systemd's mask, is left as it is.
//!
//! The manager is run as `systemctl --user …` or `launchctl …`, by name from
//! the `PATH`, as a shell would, with an environment of only the variables
//! it needs to find the user's manager and speak the user's language
//! ([`Environment`]), through the spawn gate (decision 58), with argv only.

mod files;
mod manager;
mod program;

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app::ports::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};
use async_trait::async_trait;
use domain::service::ServicePath;

use files::{LAUNCHD_MARK, SYSTEMD_MARK};
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

/// The exit codes of `launchctl print` for an agent that isn't loaded: 113
/// ("Could not find service"), and 3 (`ESRCH`, "No such process") from
/// older releases.
const NOT_LOADED: [i32; 2] = [113, 3];

/// How much of a file at the service file's place is read to find the mark
/// line, which the template puts in its first few lines.
const MARK_WITHIN: u64 = 4096;

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

/// What is at the service file's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Nothing,
    /// A file with the template's mark line: `install-service` wrote it.
    Ours,
    /// Anything else: a file without the mark line, a link, a folder.
    NotOurs,
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
        #[cfg(unix)]
        let as_root = nix::unistd::geteuid().is_root();
        #[cfg(not(unix))]
        let as_root = false;
        Self::of_platform(env, vars, as_root)
    }

    /// [`Self::of_this_platform`], for a program that runs as root when
    /// `as_root`.
    fn of_platform(
        env: impl Fn(&str) -> Option<OsString>,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
        as_root: bool,
    ) -> Result<Self, Unavailable> {
        if as_root {
            return Err(Unavailable::AsRoot);
        }
        let home = env("HOME")
            .and_then(|home| ServicePath::parse(Path::new(&home)).ok())
            .ok_or(Unavailable::NoHome)?;
        let environment = Environment::keeping(vars);
        if cfg!(target_os = "macos") {
            #[cfg(unix)]
            let uid = nix::unistd::getuid().as_raw();
            #[cfg(not(unix))]
            let uid = 0;
            Self::launchd(&home, LAUNCHD_LABEL, uid, environment)
        } else if cfg!(target_os = "linux") {
            // A relative XDG_CONFIG_HOME is ignored, as the specification says.
            let config = env("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute())
                .unwrap_or_else(|| Path::new(home.as_str()).join(".config"));
            Ok(Self::systemd(
                &config.join("systemd/user"),
                SYSTEMD_UNIT,
                environment,
            ))
        } else {
            Err(Unavailable::Unsupported)
        }
    }

    /// A systemd user unit named `name`, whose file goes in `unit_folder`.
    pub fn systemd(unit_folder: &Path, name: &str, environment: Environment) -> Self {
        Self {
            kind: Kind::Systemd,
            name: name.to_owned(),
            file: unit_folder.join(name),
            environment,
            limit: MANAGER_LIMIT,
        }
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
        let home = Path::new(home.as_str());
        let log = ServicePath::parse(&home.join("Library/Logs/ladderback.log"))
            .map_err(|_| Unavailable::NoHome)?;
        Ok(Self {
            kind: Kind::Launchd { uid, log },
            name: label.to_owned(),
            file: home
                .join("Library/LaunchAgents")
                .join(format!("{label}.plist")),
            environment,
            limit: MANAGER_LIMIT,
        })
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
        let replaced = match self.found()? {
            Found::Nothing => false,
            Found::Ours => true,
            Found::NotOurs => return Err(self.error(ServiceProblem::NotOurs)),
        };
        match &self.kind {
            Kind::Systemd => {
                self.write(&systemd_unit(program))?;
                self.call("systemctl", &["--user", "daemon-reload"]).await?;
                let file = self.file.clone().into_os_string();
                self.call_os("systemctl", vec!["--user".into(), "enable".into(), file])
                    .await?;
                self.call("systemctl", &["--user", "restart", &self.name])
                    .await?;
            }
            Kind::Launchd { uid, log } => {
                let agent = format!("gui/{uid}/{}", self.name);
                if self.loaded(&agent).await? {
                    self.call("launchctl", &["bootout", &agent]).await?;
                }
                self.write(&launchd_plist(&self.name, program, log))?;
                self.call("launchctl", &["enable", &agent]).await?;
                let file = self.file.clone().into_os_string();
                self.call_os(
                    "launchctl",
                    vec!["bootstrap".into(), format!("gui/{uid}").into(), file],
                )
                .await?;
            }
        }
        Ok(Installed {
            file: self.file.clone(),
            replaced,
        })
    }

    async fn remove(&self) -> Result<Removed, ServiceError> {
        match self.found()? {
            Found::Nothing => {
                return Ok(Removed::NotInstalled {
                    file: self.file.clone(),
                });
            }
            Found::Ours => {}
            Found::NotOurs => return Err(self.error(ServiceProblem::NotOurs)),
        }
        match &self.kind {
            Kind::Systemd => {
                self.call("systemctl", &["--user", "disable", "--now", &self.name])
                    .await?;
                self.remove_file()?;
                self.call("systemctl", &["--user", "daemon-reload"]).await?;
            }
            Kind::Launchd { uid, .. } => {
                let agent = format!("gui/{uid}/{}", self.name);
                if self.loaded(&agent).await? {
                    self.call("launchctl", &["bootout", &agent]).await?;
                }
                self.remove_file()?;
            }
        }
        // A temporary file an install that stopped part way left beside it.
        match std::fs::remove_file(self.temporary()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(self.file_error(&error)),
        }
        Ok(Removed::Removed {
            file: self.file.clone(),
        })
    }
}

impl UserService {
    fn error(&self, problem: ServiceProblem) -> ServiceError {
        ServiceError {
            file: self.file.clone(),
            problem,
        }
    }

    fn file_error(&self, error: &std::io::Error) -> ServiceError {
        self.error(ServiceProblem::File {
            detail: error.to_string(),
        })
    }

    /// What is at the service file's place. A link is never followed: a
    /// link, systemd's mask among them, is not a file `install-service`
    /// wrote.
    fn found(&self) -> Result<Found, ServiceError> {
        let metadata = match std::fs::symlink_metadata(&self.file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Found::Nothing);
            }
            Err(error) => return Err(self.file_error(&error)),
        };
        if !metadata.is_file() {
            return Ok(Found::NotOurs);
        }
        let mut head = Vec::new();
        std::fs::File::open(&self.file)
            .and_then(|file| file.take(MARK_WITHIN).read_to_end(&mut head))
            .map_err(|error| self.file_error(&error))?;
        let (mark, line) = match self.kind {
            Kind::Systemd => (SYSTEMD_MARK, 0),
            Kind::Launchd { .. } => (LAUNCHD_MARK, 2),
        };
        if String::from_utf8_lossy(&head).lines().nth(line) == Some(mark) {
            Ok(Found::Ours)
        } else {
            Ok(Found::NotOurs)
        }
    }

    /// The file a write goes to before it is renamed over the service file.
    fn temporary(&self) -> PathBuf {
        let name = self.file.file_name().unwrap_or_default().to_string_lossy();
        self.file.with_file_name(format!(".{name}.tmp"))
    }

    /// Writes `text` to a new file beside the service file and renames it
    /// over it: a link at the service file's place is replaced, never
    /// written through. The new file is its user's alone until it is
    /// written, then 0644 whatever the umask: whoever could write it could
    /// change what the service runs as its user. A write that stops part way
    /// takes its new file with it.
    fn write(&self, text: &str) -> Result<(), ServiceError> {
        let folder = self.file.parent().unwrap_or(Path::new("/"));
        // A folder it makes is its user's alone, as the XDG base directory
        // specification asks; one already there keeps its mode.
        let mut folders = std::fs::DirBuilder::new();
        folders.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut folders, 0o700);
        folders
            .create(folder)
            .map_err(|error| self.file_error(&error))?;
        let temporary = self.temporary();
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(self.file_error(&error)),
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        // Opened and closed while no child starts: one forked meanwhile
        // would hold it (decision 58). The outer result is the opening, the
        // inner one what came after it.
        let filled = spawn_gate::hold(|| {
            let mut file = options.open(&temporary)?;
            Ok(
                std::io::Write::write_all(&mut file, text.as_bytes()).and_then(|()| {
                    #[cfg(unix)]
                    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o644))?;
                    Ok(())
                }),
            )
        })
        .map_err(|error: std::io::Error| self.file_error(&error))?;
        if let Err(error) = filled.and_then(|()| std::fs::rename(&temporary, &self.file)) {
            // Best effort: the error said is the write's, not this one's.
            let _ = std::fs::remove_file(&temporary);
            return Err(self.file_error(&error));
        }
        Ok(())
    }

    fn remove_file(&self) -> Result<(), ServiceError> {
        std::fs::remove_file(&self.file).map_err(|error| self.file_error(&error))
    }

    /// Whether launchd has `agent` loaded. `print` says "not loaded" by
    /// ending with [`NOT_LOADED`]; any other failure is a refusal, so a
    /// manager that can't answer never lets the file of a running agent go.
    async fn loaded(&self, agent: &str) -> Result<bool, ServiceError> {
        let args: [OsString; 2] = ["print".into(), agent.into()];
        match manager::run("launchctl", &args, &self.environment, self.limit)
            .await
            .map_err(|problem| self.error(problem))?
        {
            manager::Ended::Succeeded => Ok(true),
            manager::Ended::Failed {
                code: Some(code), ..
            } if NOT_LOADED.contains(&code) => Ok(false),
            manager::Ended::Failed { said, .. } => {
                Err(self.error(ServiceProblem::ManagerRefused {
                    command: manager::shown("launchctl", &args),
                    said,
                }))
            }
        }
    }

    async fn call(&self, program: &str, args: &[&str]) -> Result<(), ServiceError> {
        self.call_os(program, args.iter().map(OsString::from).collect())
            .await
    }

    /// Runs the manager's `program` with `args`; an error is a refusal.
    async fn call_os(&self, program: &str, args: Vec<OsString>) -> Result<(), ServiceError> {
        match manager::run(program, &args, &self.environment, self.limit).await {
            Ok(manager::Ended::Succeeded) => Ok(()),
            Ok(manager::Ended::Failed { said, .. }) => {
                Err(self.error(ServiceProblem::ManagerRefused {
                    command: manager::shown(program, &args),
                    said,
                }))
            }
            Err(problem) => Err(self.error(problem)),
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
