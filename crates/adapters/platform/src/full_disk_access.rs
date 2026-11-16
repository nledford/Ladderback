//! The Full Disk Access probe (technical design §3.1, §16 decision 115): on
//! macOS, whether the person has granted the agent Full Disk Access, learned
//! by listing a folder only that grant opens, `~/Library/Safari`; and System
//! Settings' Full Disk Access pane, where the person grants it.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use app::ports::{FullDiskAccessProbe, ServiceProblem};
use async_trait::async_trait;
use domain::execution::full_disk_access::FullDiskAccess;

use crate::service::Environment;
use crate::service::manager::{self, Ended};

/// The folder the probe lists, under the home `env` names (`HOME`), or
/// `None` without one. Only `HOME` decides, so a daemon started with a home
/// of its own, as every test starts one, never lists the person's folder.
pub fn probe_folder_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    crate::dirs::home(&env).map(|home| home.join("Library").join("Safari"))
}

/// This Mac's probe, of [`probe_folder_from`] the process's environment;
/// `None` off macOS, which has no such grant, and without a home.
pub fn system_probe() -> Option<FolderProbe> {
    if cfg!(target_os = "macos") {
        probe_folder_from(|name| std::env::var_os(name)).map(FolderProbe::new)
    } else {
        None
    }
}

/// Lists one folder: a listing is the grant, a refusal with `EPERM` its
/// absence, and any other answer unknown.
pub struct FolderProbe {
    folder: PathBuf,
}

impl FolderProbe {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }
}

/// `EPERM`, as the system refuses a folder only Full Disk Access opens. A
/// folder's own permissions refuse with `EACCES`, which no grant fixes.
const NOT_PERMITTED: i32 = 1;

/// What listing the folder answered says of the grant.
fn found_from(listed: io::Result<()>) -> FullDiskAccess {
    match listed {
        Ok(()) => FullDiskAccess::Granted,
        Err(error) if error.raw_os_error() == Some(NOT_PERMITTED) => FullDiskAccess::Missing,
        Err(_) => FullDiskAccess::Unknown,
    }
}

#[async_trait]
impl FullDiskAccessProbe for FolderProbe {
    async fn probe(&self) -> FullDiskAccess {
        let folder = self.folder.clone();
        // Listing reads the disk: off the runtime's threads. A task that
        // couldn't finish says nothing of the grant.
        let listed = tokio::task::spawn_blocking(move || std::fs::read_dir(folder).map(|_| ()));
        match listed.await {
            Ok(listed) => found_from(listed),
            Err(_) => FullDiskAccess::Unknown,
        }
    }
}

/// The address of System Settings' Full Disk Access pane.
const PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// macOS's own program that opens an address, or shows a file in the Finder.
const OPEN: &str = "/usr/bin/open";

/// The longest one call of `open` is waited for: it hands the address to the
/// system and ends at once.
const OPEN_LIMIT: Duration = Duration::from_secs(10);

/// Opens System Settings at its Full Disk Access pane, then shows the
/// agent's program in the Finder, ready to drag into the pane's list. Every
/// argument is fixed but the program's path, which the agent read of
/// itself: nothing a request says reaches the command.
pub struct SettingsPane {
    open: String,
    program: PathBuf,
    environment: Environment,
}

impl SettingsPane {
    /// With macOS's `/usr/bin/open`, showing `program`, and the variables of
    /// the process's environment a system program needs.
    pub fn new(program: PathBuf) -> Self {
        Self::opening_with(PathBuf::from(OPEN), program, std::env::vars_os())
    }

    /// With `open` in place of macOS's own, and `vars` in place of the
    /// process's environment, for tests.
    pub fn opening_with(
        open: PathBuf,
        program: PathBuf,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            open: open.to_string_lossy().into_owned(),
            program,
            environment: Environment::keeping(vars),
        }
    }

    /// Opens the pane, then shows the program.
    ///
    /// # Errors
    ///
    /// Either call couldn't start, failed or didn't end in time: what it
    /// said, in the platform's own words, for the daemon's log.
    pub async fn open(&self) -> Result<(), String> {
        self.run(&[OsString::from(PANE)]).await?;
        self.run(&[OsString::from("-R"), self.program.clone().into_os_string()])
            .await
    }

    /// Runs `open args`, with only the variables a system program needs
    /// (I4), and waits at most [`OPEN_LIMIT`].
    async fn run(&self, args: &[OsString]) -> Result<(), String> {
        match manager::run(&self.open, args, &self.environment, OPEN_LIMIT).await {
            Ok(Ended::Succeeded { .. }) => Ok(()),
            Ok(Ended::Failed { code, said }) => {
                let shown = manager::shown(&self.open, args);
                Err(match code {
                    Some(code) => format!("`{shown}` ended with exit code {code}: {said}"),
                    None => format!("`{shown}` ended without an exit code: {said}"),
                })
            }
            Err(ServiceProblem::ManagerMissing { command, detail }) => {
                Err(format!("`{command}` could not start: {detail}"))
            }
            Err(ServiceProblem::ManagerSilent { command, seconds }) => {
                Err(format!("`{command}` didn't end within {seconds} seconds"))
            }
            Err(problem) => Err(format!("{problem:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::Arc;

    use app::contract;
    use app::contract::full_disk_access::FullDiskAccessHarness;

    use super::*;

    struct Folders(tempfile::TempDir);

    impl Folders {
        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl FullDiskAccessHarness for Folders {
        fn folder_lists(&self) -> Arc<dyn FullDiskAccessProbe> {
            let folder = self.path().join("Safari");
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("History.db"), b"demo").unwrap();
            Arc::new(FolderProbe::new(folder))
        }

        fn folder_missing(&self) -> Arc<dyn FullDiskAccessProbe> {
            Arc::new(FolderProbe::new(self.path().join("missing")))
        }

        fn not_a_folder(&self) -> Arc<dyn FullDiskAccessProbe> {
            let file = self.path().join("a-file");
            fs::write(&file, b"demo").unwrap();
            Arc::new(FolderProbe::new(file))
        }
    }

    #[tokio::test]
    async fn the_folder_probe_meets_the_full_disk_access_contract() {
        contract::full_disk_access::run(&Folders(tempfile::tempdir().unwrap())).await;
    }

    #[tokio::test]
    async fn a_folder_its_permissions_refuse_says_nothing_of_the_grant() {
        let root = tempfile::tempdir().unwrap();
        let closed = root.path().join("closed");
        fs::create_dir(&closed).unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        let lists_anyway = fs::read_dir(&closed).is_ok();
        let probe = FolderProbe::new(closed.clone());
        if lists_anyway {
            fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
            testkit::skip!("running as root: permissions don't stop listing a folder");
        }
        contract::full_disk_access::a_folder_its_permissions_refuse_is_unknown(&probe).await;
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// A test machine can't make the system refuse a folder for want of the
    /// grant, so the answer it gives then, `EPERM`, is given here.
    #[test]
    fn a_refusal_with_eperm_is_the_grant_missing() {
        assert_eq!(
            found_from(Err(io::Error::from_raw_os_error(1))),
            FullDiskAccess::Missing
        );
    }

    #[test]
    fn a_listing_is_the_grant_and_any_other_refusal_unknown() {
        assert_eq!(found_from(Ok(())), FullDiskAccess::Granted);
        for errno in [2, 13, 20] {
            assert_eq!(
                found_from(Err(io::Error::from_raw_os_error(errno))),
                FullDiskAccess::Unknown,
                "os error {errno}"
            );
        }
    }

    /// The folder is under `HOME` alone: a test's daemon, started with a
    /// scratch home, never lists the person's own.
    #[test]
    fn the_probe_lists_safari_under_the_home_it_runs_with_and_nothing_without_one() {
        let home = |name: &str| (name == "HOME").then(|| OsString::from("/Users/demo"));
        assert_eq!(
            probe_folder_from(home),
            Some(PathBuf::from("/Users/demo/Library/Safari"))
        );
        assert_eq!(probe_folder_from(|_| None), None);
        assert_eq!(probe_folder_from(|_| Some(OsString::new())), None);
    }

    /// A stand-in for `open` in `folder`, running `body`, which the spawn
    /// gate keeps from another start's inheriting it half written.
    fn stand_in(folder: &Path, body: &str) -> PathBuf {
        let open = folder.join("open");
        spawn_gate::hold(|| {
            fs::write(&open, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
        });
        open
    }

    /// A pane opening with `open`, showing a demo program, with only `vars`.
    fn pane_with<const N: usize>(open: PathBuf, vars: [(&str, &str); N]) -> SettingsPane {
        SettingsPane::opening_with(
            open,
            PathBuf::from("/opt/demo/bin/ladderback"),
            vars.map(|(name, value)| (OsString::from(name), OsString::from(value))),
        )
    }

    #[tokio::test]
    async fn the_pane_opens_at_full_disk_access_then_shows_the_program_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let calls = root.path().join("calls");
        let open = stand_in(
            root.path(),
            &format!(
                "printf '%s|' \"$@\" >> '{}'\necho >> '{}'",
                calls.display(),
                calls.display()
            ),
        );
        let pane = pane_with(open, []);
        assert_eq!(pane.open().await, Ok(()));
        assert_eq!(
            fs::read_to_string(&calls).unwrap(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles|\n\
             -R|/opt/demo/bin/ladderback|\n"
        );
    }

    #[tokio::test]
    async fn a_pane_that_doesnt_open_says_why_and_shows_nothing() {
        let root = tempfile::tempdir().unwrap();
        let calls = root.path().join("calls");
        let open = stand_in(
            root.path(),
            &format!(
                "echo \"$@\" >> '{}'\necho 'no such pane' >&2\nexit 1",
                calls.display()
            ),
        );
        let pane = pane_with(open, []);
        let refused = pane.open().await.unwrap_err();
        assert!(refused.contains("no such pane"), "{refused}");
        assert_eq!(fs::read_to_string(&calls).unwrap().lines().count(), 1);
    }

    #[tokio::test]
    async fn an_open_that_cant_start_says_so() {
        let root = tempfile::tempdir().unwrap();
        let pane = pane_with(root.path().join("missing"), []);
        let refused = pane.open().await.unwrap_err();
        assert!(refused.contains("could not start"), "{refused}");
    }

    /// `open` runs with the variables a system program needs, and no other,
    /// so no secret reaches it (I4).
    #[tokio::test]
    async fn open_never_sees_a_secret_of_the_environment() {
        let root = tempfile::tempdir().unwrap();
        let calls = root.path().join("calls");
        let open = stand_in(
            root.path(),
            &format!("/usr/bin/env >> '{}'", calls.display()),
        );
        let pane = pane_with(
            open,
            [
                ("PATH", "/usr/bin:/bin"),
                ("LANG", "en_US.UTF-8"),
                ("RESTIC_PASSWORD", "CANARY-password"),
                ("AWS_SECRET_ACCESS_KEY", "CANARY-key"),
            ],
        );
        assert_eq!(pane.open().await, Ok(()));
        let calls = fs::read_to_string(&calls).unwrap();
        assert_eq!(
            calls.matches("LANG=en_US.UTF-8").count(),
            2,
            "both calls had the kept variables: {calls}"
        );
        assert!(!calls.contains("CANARY"), "{calls}");
    }
}
