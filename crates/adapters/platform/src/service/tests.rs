use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use app::contract;
use app::contract::service::ServiceHarness;
use app::ports::{Removed, ServiceManager, ServiceProblem};
use async_trait::async_trait;
use domain::service::ServicePath;
use testkit::service_manager::FakeServiceManagers;

use super::{Environment, LAUNCHD_LABEL, SYSTEMD_UNIT, Unavailable, UserService};
use super::{launchd_plist, systemd_unit};

fn path(text: &str) -> ServicePath {
    ServicePath::parse(Path::new(text)).unwrap()
}

fn os(text: &str) -> OsString {
    OsString::from(text)
}

#[test]
fn the_systemd_unit_is_the_golden_one() {
    assert_eq!(
        systemd_unit(&path("/home/demo/.local/bin/ladderback")),
        include_str!("../../tests/golden/ladderback.service")
    );
}

#[test]
fn the_launchd_plist_is_the_golden_one() {
    assert_eq!(
        launchd_plist(
            LAUNCHD_LABEL,
            &path("/Users/demo/.local/bin/ladderback"),
            &path("/Users/demo/Library/Logs/ladderback.log"),
        ),
        include_str!("../../tests/golden/io.github.nledford.ladderback.plist")
    );
}

/// The `ExecStart=` line of the unit for `program`.
fn exec_start(program: &str) -> String {
    systemd_unit(&path(program))
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap()
        .to_owned()
}

#[test]
fn the_unit_keeps_what_systemd_would_read_in_the_program_path_as_text() {
    // `%` is a specifier, doubled to stay text. systemd never expands a
    // variable in the path it runs, so `$` is left as it is, and `:` stops
    // it expanding one in the program's argv[0].
    assert_eq!(
        exec_start("/home/demo/My Apps/50% $HOME/ladderback"),
        r#"ExecStart=:"/home/demo/My Apps/50%% $HOME/ladderback" serve"#
    );
}

#[test]
fn a_program_path_can_not_add_a_setting_or_a_command_to_the_unit() {
    let unit = systemd_unit(&path("/home/demo/a ; /bin/sh -c evil/ladderback"));
    assert_eq!(
        unit.lines()
            .filter(|line| line.starts_with("ExecStart"))
            .count(),
        1
    );
    assert!(
        unit.contains(r#"ExecStart=:"/home/demo/a ; /bin/sh -c evil/ladderback" serve"#),
        "{unit}"
    );
}

#[test]
fn the_plist_escapes_what_xml_would_read_in_its_paths() {
    let plist = launchd_plist(
        LAUNCHD_LABEL,
        &path("/Users/demo/R&D <tools>/ladderback"),
        &path("/Users/demo/Library/Logs/ladderback.log"),
    );
    assert!(
        plist.contains("<string>/Users/demo/R&amp;D &lt;tools&gt;/ladderback</string>"),
        "{plist}"
    );
    assert!(!plist.contains("<tools>"));
}

#[test]
fn the_manager_is_run_with_no_variable_it_does_not_need() {
    let kept = Environment::keeping([
        (os("PATH"), os("/usr/bin:/bin")),
        (os("HOME"), os("/home/demo")),
        (os("XDG_RUNTIME_DIR"), os("/run/user/1000")),
        (
            os("DBUS_SESSION_BUS_ADDRESS"),
            os("unix:path=/run/user/1000/bus"),
        ),
        (os("LANG"), os("en_US.UTF-8")),
        (os("RESTIC_PASSWORD"), os("CANARY-password")),
        (os("AWS_SECRET_ACCESS_KEY"), os("CANARY-key")),
        (os("LADDERBACK_CONFIG_DIR"), os("/srv/demo")),
    ]);
    let names: Vec<_> = kept.kept().iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(
        names,
        [
            "PATH",
            "HOME",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "LANG"
        ]
        .map(os)
    );
}

#[test]
fn as_root_there_is_no_user_service() {
    // `sudo` keeps or sets a HOME, and the service would still be root's.
    for home in [Some("/home/demo"), Some("/root"), None] {
        let env = |name: &str| (name == "HOME").then(|| home.map(OsString::from)).flatten();
        assert_eq!(
            UserService::of_platform(env, [], true).err(),
            Some(Unavailable::AsRoot),
            "{home:?}"
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_user_who_is_not_root_has_a_user_service() {
    let env = |name: &str| (name == "HOME").then(|| os("/home/demo"));
    assert!(UserService::of_platform(env, [], false).is_ok());
}

#[test]
fn with_no_home_there_is_no_service() {
    for home in [None, Some(""), Some("relative/home")] {
        let env = |name: &str| (name == "HOME").then(|| home.map(OsString::from)).flatten();
        assert_eq!(
            UserService::of_platform(env, [], false).err(),
            Some(Unavailable::NoHome),
            "{home:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_unit_goes_in_the_users_systemd_folder() {
    let env = |name: &str| match name {
        "HOME" => Some(os("/home/demo")),
        "XDG_CONFIG_HOME" => Some(os("/srv/demo-config")),
        _ => None,
    };
    let service = UserService::of_platform(env, [], false).unwrap();
    assert_eq!(
        service.file(),
        Path::new("/srv/demo-config/systemd/user/ladderback.service")
    );
    // A relative XDG_CONFIG_HOME is ignored, as the specification says.
    let env = |name: &str| match name {
        "HOME" => Some(os("/home/demo")),
        "XDG_CONFIG_HOME" => Some(os("demo-config")),
        _ => None,
    };
    let service = UserService::of_platform(env, [], false).unwrap();
    assert_eq!(
        service.file(),
        Path::new("/home/demo/.config/systemd/user/ladderback.service")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_plist_goes_in_the_users_launch_agents() {
    let env = |name: &str| (name == "HOME").then(|| os("/Users/demo"));
    let service = UserService::of_platform(env, [], false).unwrap();
    assert_eq!(
        service.file(),
        Path::new("/Users/demo/Library/LaunchAgents/io.github.nledford.ladderback.plist")
    );
}

/// Which manager a test runs.
#[derive(Clone, Copy, Debug)]
enum Which {
    Systemd,
    Launchd,
}

/// The real adapter, run against the stand-in managers in a scratch folder.
struct Faked {
    service: UserService,
    fakes: FakeServiceManagers,
    name: &'static str,
    scratch: tempfile::TempDir,
}

impl Faked {
    fn new(which: Which) -> Self {
        Self::with_vars(which, [])
    }

    /// With `vars` in the environment the manager is handed, beside a `PATH`
    /// holding the stand-ins only.
    fn with_vars(which: Which, vars: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let fakes = FakeServiceManagers::install(&scratch.path().join("managers"));
        let mut all: Vec<_> = vars.into_iter().collect();
        all.push((os("PATH"), fakes.bin().into_os_string()));
        let environment = Environment::keeping(all);
        let home = scratch.path().join("home");
        let (service, name) = match which {
            Which::Systemd => (
                UserService::systemd(
                    &home.join(".config/systemd/user"),
                    SYSTEMD_UNIT,
                    environment,
                ),
                SYSTEMD_UNIT,
            ),
            Which::Launchd => (
                UserService::launchd(
                    &ServicePath::parse(&home).unwrap(),
                    LAUNCHD_LABEL,
                    501,
                    environment,
                )
                .unwrap(),
                LAUNCHD_LABEL,
            ),
        };
        Self {
            service,
            fakes,
            name,
            scratch,
        }
    }

    /// A program in the scratch folder, by its folder's name.
    fn program(&self, folder: &str) -> ServicePath {
        ServicePath::parse(&self.scratch.path().join(folder).join("ladderback")).unwrap()
    }
}

#[async_trait]
impl ServiceHarness for Faked {
    fn manager(&self) -> &dyn ServiceManager {
        &self.service
    }

    fn programs(&self) -> [ServicePath; 2] {
        [self.program("first"), self.program("second")]
    }

    async fn runs(&self) -> bool {
        self.fakes.runs(self.name)
    }

    fn file_text(&self) -> Option<String> {
        std::fs::read_to_string(self.service.file()).ok()
    }
}

#[tokio::test]
async fn the_systemd_service_meets_the_contract_with_a_stand_in_manager() {
    contract::service::run(&Faked::new(Which::Systemd)).await;
}

#[tokio::test]
async fn the_launchd_service_meets_the_contract_with_a_stand_in_manager() {
    contract::service::run(&Faked::new(Which::Launchd)).await;
}

#[tokio::test]
async fn systemd_is_asked_to_load_enable_and_restart_the_unit() {
    let faked = Faked::new(Which::Systemd);
    let file = faked.service.file().display().to_string();
    faked.service.install(&faked.program("bin")).await.unwrap();
    assert_eq!(
        faked.fakes.commands(),
        [
            "systemctl --user daemon-reload".to_owned(),
            format!("systemctl --user enable {file}"),
            "systemctl --user restart ladderback.service".to_owned(),
        ]
    );
    faked.service.remove().await.unwrap();
    assert_eq!(
        faked.fakes.commands()[3..],
        [
            "systemctl --user disable --now ladderback.service".to_owned(),
            "systemctl --user daemon-reload".to_owned(),
        ]
    );
}

#[tokio::test]
async fn launchd_boots_the_old_agent_out_before_it_boots_the_new_one_in() {
    let faked = Faked::new(Which::Launchd);
    let file = faked.service.file().display().to_string();
    let agent = format!("gui/501/{LAUNCHD_LABEL}");
    faked.service.install(&faked.program("bin")).await.unwrap();
    faked.service.install(&faked.program("bin")).await.unwrap();
    assert_eq!(
        faked.fakes.commands(),
        [
            format!("launchctl print {agent}"),
            format!("launchctl enable {agent}"),
            format!("launchctl bootstrap gui/501 {file}"),
            format!("launchctl print {agent}"),
            format!("launchctl bootout {agent}"),
            format!("launchctl enable {agent}"),
            format!("launchctl bootstrap gui/501 {file}"),
        ]
    );
}

#[tokio::test]
async fn removing_no_service_asks_the_manager_nothing() {
    for which in [Which::Systemd, Which::Launchd] {
        let faked = Faked::new(which);
        assert!(matches!(
            faked.service.remove().await,
            Ok(Removed::NotInstalled { .. })
        ));
        assert_eq!(faked.fakes.commands(), Vec::<String>::new(), "{which:?}");
    }
}

#[tokio::test]
async fn a_refusal_is_the_managers_command_and_words() {
    let faked = Faked::new(Which::Systemd);
    faked
        .fakes
        .refuse("Failed to connect to bus: No medium found");
    let error = faked
        .service
        .install(&faked.program("bin"))
        .await
        .unwrap_err();
    assert_eq!(error.file, faked.service.file());
    assert_eq!(
        error.problem,
        ServiceProblem::ManagerRefused {
            command: "systemctl --user daemon-reload".to_owned(),
            said: "Failed to connect to bus: No medium found".to_owned(),
        }
    );
}

#[tokio::test]
async fn a_refused_removal_keeps_the_file() {
    for (which, words) in [
        (Which::Systemd, "Failed to connect to bus: No medium found"),
        (
            Which::Launchd,
            "Boot-out failed: 1: Operation not permitted",
        ),
    ] {
        let faked = Faked::new(which);
        faked.service.install(&faked.program("bin")).await.unwrap();
        faked.fakes.refuse(words);
        let error = faked.service.remove().await.unwrap_err();
        assert!(
            matches!(error.problem, ServiceProblem::ManagerRefused { .. }),
            "{which:?}"
        );
        assert!(faked.service.file().exists(), "{which:?}");
    }
}

#[tokio::test]
async fn an_agent_launchd_does_not_know_is_removed_without_a_bootout() {
    // `print` ends 113, "Could not find service", for an agent that isn't
    // loaded; older releases end 3, "No such process".
    for code in [113, 3] {
        let faked = Faked::new(Which::Launchd);
        faked.service.install(&faked.program("bin")).await.unwrap();
        faked.fakes.refuse_ending("Could not find service", code);
        assert!(
            matches!(faked.service.remove().await, Ok(Removed::Removed { .. })),
            "{code}"
        );
        assert!(!faked.service.file().exists(), "{code}");
        assert!(
            !faked
                .fakes
                .commands()
                .iter()
                .any(|call| call.contains("bootout")),
            "{code}"
        );
    }
}

#[tokio::test]
async fn a_manager_that_is_not_on_the_path_is_named() {
    let mut faked = Faked::new(Which::Systemd);
    let empty = faked.scratch.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    faked.service.environment = Environment::keeping([(os("PATH"), empty.into_os_string())]);
    let error = faked
        .service
        .install(&faked.program("bin"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error.problem,
            ServiceProblem::ManagerMissing { command, detail }
                if command == "systemctl --user daemon-reload" && !detail.is_empty()
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_manager_that_does_not_answer_is_stopped_at_the_limit() {
    let faked = Faked::new(Which::Systemd);
    let service = faked.service.clone().with_limit(Duration::from_millis(500));
    faked.fakes.hang();
    let started = std::time::Instant::now();
    let error = service.install(&faked.program("bin")).await.unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "it waited out the hang"
    );
    assert_eq!(
        error.problem,
        ServiceProblem::ManagerSilent {
            command: "systemctl --user daemon-reload".to_owned(),
            seconds: 0,
        }
    );
}

#[tokio::test]
async fn the_manager_never_sees_a_secret_of_the_environment() {
    for which in [Which::Systemd, Which::Launchd] {
        let faked = Faked::with_vars(
            which,
            [
                (os("RESTIC_PASSWORD"), os("CANARY-password")),
                (os("LANG"), os("en_US.UTF-8")),
            ],
        );
        faked.service.install(&faked.program("bin")).await.unwrap();
        faked.service.remove().await.unwrap();
        let calls = faked.fakes.calls();
        assert!(
            calls.contains("LANG"),
            "the environment was recorded: {calls}"
        );
        assert!(!calls.contains("CANARY"), "{which:?}: {calls}");
    }
}

#[tokio::test]
async fn a_file_that_can_not_be_written_is_named_with_the_systems_words() {
    let faked = Faked::new(Which::Systemd);
    // A file where the unit's folder should be.
    let folder = faked.service.file().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, "").unwrap();
    let error = faked
        .service
        .install(&faked.program("bin"))
        .await
        .unwrap_err();
    assert!(
        matches!(&error.problem, ServiceProblem::File { detail } if !detail.is_empty()),
        "{error:?}"
    );
    assert_eq!(faked.fakes.commands(), Vec::<String>::new());
}

/// A file at the service file's place that `install-service` didn't write.
fn not_ours(faked: &Faked, text: &str) -> std::path::PathBuf {
    let file = faked.service.file().to_path_buf();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    file
}

#[tokio::test]
async fn a_service_file_written_by_hand_is_neither_replaced_nor_removed() {
    let by_hand = "[Service]\nExecStart=/usr/local/bin/my-backups\n";
    for which in [Which::Systemd, Which::Launchd] {
        let faked = Faked::new(which);
        let file = not_ours(&faked, by_hand);
        let installed = faked.service.install(&faked.program("bin")).await;
        assert_eq!(
            installed.unwrap_err().problem,
            ServiceProblem::NotOurs,
            "{which:?}"
        );
        let removed = faked.service.remove().await;
        assert_eq!(
            removed.unwrap_err().problem,
            ServiceProblem::NotOurs,
            "{which:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            by_hand,
            "{which:?}"
        );
        assert_eq!(faked.fakes.commands(), Vec::<String>::new(), "{which:?}");
    }
}

#[tokio::test]
async fn a_file_that_only_mentions_install_service_is_not_ours() {
    // The mark is a line of its own, not a word anywhere in the file.
    let faked = Faked::new(Which::Systemd);
    let file = not_ours(
        &faked,
        "[Unit]\nDescription=# Written by `ladderback install-service`; `ladderback remove-service` removes it.\n",
    );
    let removed = faked.service.remove().await;
    assert_eq!(removed.unwrap_err().problem, ServiceProblem::NotOurs);
    assert!(file.exists());
}

#[tokio::test]
async fn a_masked_unit_stays_masked() {
    // `systemctl --user mask` puts a link to /dev/null at the unit's place.
    let faked = Faked::new(Which::Systemd);
    let file = faked.service.file().to_path_buf();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("/dev/null", &file).unwrap();
    let removed = faked.service.remove().await;
    assert_eq!(removed.unwrap_err().problem, ServiceProblem::NotOurs);
    let installed = faked.service.install(&faked.program("bin")).await;
    assert_eq!(installed.unwrap_err().problem, ServiceProblem::NotOurs);
    assert_eq!(std::fs::read_link(&file).unwrap(), Path::new("/dev/null"));
    assert_eq!(faked.fakes.commands(), Vec::<String>::new());
}

#[tokio::test]
async fn an_install_never_writes_through_a_link_at_the_files_place() {
    // Not even to a file `install-service` wrote: a link is not its file.
    let faked = Faked::new(Which::Systemd);
    faked.service.install(&faked.program("bin")).await.unwrap();
    let file = faked.service.file().to_path_buf();
    let elsewhere = faked.scratch.path().join("elsewhere");
    std::fs::rename(&file, &elsewhere).unwrap();
    let written = std::fs::read_to_string(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
    let installed = faked.service.install(&faked.program("other")).await;
    assert_eq!(installed.unwrap_err().problem, ServiceProblem::NotOurs);
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), written);
    assert!(std::fs::symlink_metadata(&file).unwrap().is_symlink());
}

#[tokio::test]
async fn a_removal_takes_a_temporary_file_left_beside_the_service_file() {
    for which in [Which::Systemd, Which::Launchd] {
        let faked = Faked::new(which);
        faked.service.install(&faked.program("bin")).await.unwrap();
        let file = faked.service.file();
        let name = file.file_name().unwrap().to_string_lossy();
        let left = file.with_file_name(format!(".{name}.tmp"));
        std::fs::write(&left, "half a unit").unwrap();
        faked.service.remove().await.unwrap();
        assert!(!left.exists(), "{which:?}");
    }
}

/// On macOS a file can be made immutable without root (`chflags uchg`), so a
/// rename over it fails after the temporary file is written.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_write_that_fails_leaves_no_temporary_file() {
    let flags = |flag: &str, file: &Path| {
        let done = Command::new("chflags")
            .arg(flag)
            .arg(file)
            .status()
            .unwrap();
        assert!(done.success());
    };
    let faked = Faked::new(Which::Systemd);
    faked.service.install(&faked.program("bin")).await.unwrap();
    let file = faked.service.file().to_path_buf();
    flags("uchg", &file);
    let installed = faked.service.install(&faked.program("other")).await;
    flags("nouchg", &file);
    assert!(matches!(
        installed.unwrap_err().problem,
        ServiceProblem::File { .. }
    ));
    let left: Vec<_> = std::fs::read_dir(file.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, vec![file.file_name().unwrap().to_owned()]);
}

#[tokio::test]
async fn the_service_file_can_be_changed_by_its_user_alone() {
    use std::os::unix::fs::PermissionsExt;

    use nix::sys::stat::{Mode, umask};

    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    for which in [Which::Systemd, Which::Launchd] {
        let faked = Faked::new(which);
        // A umask that leaves the group its write bit: whoever shares the
        // group could change what the service runs.
        let before = umask(Mode::from_bits_truncate(0o002));
        let installed = faked.service.install(&faked.program("bin")).await;
        umask(before);
        installed.unwrap();
        let file = faked.service.file();
        assert_eq!(mode(file), 0o644, "{which:?}");
        // The folders it made, as the XDG base directory specification asks.
        assert_eq!(mode(file.parent().unwrap()), 0o700, "{which:?}");
    }
}

// The real manager: only when a person asks for it.

/// Whether the real manager may be asked to run a service: only when a
/// person asks for it, since it runs a service in their session.
fn real_manager_is_safe_here() -> bool {
    std::env::var_os("LADDERBACK_TEST_REAL_SERVICE_MANAGER").is_some_and(|value| !value.is_empty())
}

/// The real adapter and the real manager, on a service of its own name that
/// runs a program from a scratch folder.
struct Real {
    service: UserService,
    name: String,
    scratch: tempfile::TempDir,
}

impl Real {
    fn program(&self, folder: &str) -> ServicePath {
        let program = self.scratch.path().join(folder).join("ladderback");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        spawn_gate::hold(|| {
            std::fs::write(&program, "#!/bin/sh\nexec /bin/sleep 600\n").unwrap();
            std::fs::set_permissions(
                &program,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )
            .unwrap();
        });
        ServicePath::parse(&program).unwrap()
    }
}

#[async_trait]
impl ServiceHarness for Real {
    fn manager(&self) -> &dyn ServiceManager {
        &self.service
    }

    fn programs(&self) -> [ServicePath; 2] {
        [self.program("first"), self.program("second")]
    }

    async fn runs(&self) -> bool {
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || real_runs(&name))
            .await
            .unwrap()
    }

    fn file_text(&self) -> Option<String> {
        std::fs::read_to_string(self.service.file()).ok()
    }
}

#[cfg(target_os = "linux")]
fn real_runs(name: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", name])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn real_runs(name: &str) -> bool {
    let uid = nix::unistd::getuid().as_raw();
    Command::new("launchctl")
        .arg("print")
        .arg(format!("gui/{uid}/{name}"))
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The real adapter, on a service named for this test run whose file goes in
/// `scratch`. Making it asks the manager nothing.
#[cfg(target_os = "linux")]
fn real_service(scratch: &Path) -> (UserService, String) {
    let name = format!("ladderback-contract-{}.service", std::process::id());
    let service = UserService::systemd(
        &scratch.join("units"),
        &name,
        Environment::keeping(std::env::vars_os()),
    );
    (service, name)
}

/// Whether the user's manager answers: there is a user session.
#[cfg(target_os = "linux")]
fn real_manager_answers() -> bool {
    Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The real adapter, on a service labelled for this test run whose plist
/// and log go in `scratch`. Making it asks the manager nothing.
#[cfg(target_os = "macos")]
fn real_service(scratch: &Path) -> (UserService, String) {
    let uid = nix::unistd::getuid().as_raw();
    let name = format!("{LAUNCHD_LABEL}.contract-{}", std::process::id());
    let home = ServicePath::parse(&scratch.join("home")).unwrap();
    let service =
        UserService::launchd(&home, &name, uid, Environment::keeping(std::env::vars_os())).unwrap();
    (service, name)
}

/// Whether the user's manager answers: there is a GUI session.
#[cfg(target_os = "macos")]
fn real_manager_answers() -> bool {
    let uid = nix::unistd::getuid().as_raw();
    Command::new("launchctl")
        .arg("print")
        .arg(format!("gui/{uid}"))
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn the_real_manager_meets_the_contract() {
    let scratch = tempfile::tempdir().unwrap();
    let (service, name) = real_service(scratch.path());
    if !real_manager_is_safe_here() {
        testkit::skip!(
            "asks the real user service manager to run a service; set \
             LADDERBACK_TEST_REAL_SERVICE_MANAGER=1 to run it"
        );
    }
    if !real_manager_answers() {
        testkit::skip!("no user service manager answers here (no user session?)");
    }
    let real = Real {
        service,
        name,
        scratch,
    };
    contract::service::run(&real).await;
}
