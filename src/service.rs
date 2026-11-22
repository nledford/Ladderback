//! `ladderback install-service` and `ladderback remove-service` (operations
//! §12): wires this platform's `UserService` to the program's own path and
//! says what came of it, in the catalog's words. What a command did goes to
//! stdout; why it did nothing, or stopped, goes to stderr, and it ends with 1.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use api::MsgArg;
use app::ports::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};
use domain::service::{ServicePath, ServicePathProblem};
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::data_dir_lock::{DataDirLock, LockError};
use platform::dirs;
use platform::service::{Unavailable, UserService, program_path};

use crate::output::{to_stderr, to_stdout};
use crate::{finish, said, say, text};

/// Installs this program as the user's service and starts it.
pub(crate) fn install_service() -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, install(&localizer))
}

/// Stops the user's service and removes its file.
pub(crate) fn remove_service() -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, remove(&localizer))
}

fn install(localizer: &Localizer) -> io::Result<ExitCode> {
    let Some(service) = this_service(localizer)? else {
        return Ok(ExitCode::FAILURE);
    };
    let Some(program) = this_program(localizer)? else {
        return Ok(ExitCode::FAILURE);
    };
    let Some(runtime) = runtime(localizer)? else {
        return Ok(ExitCode::FAILURE);
    };
    let keyring = keyring_passwords();
    let held = data_folder_held(&runtime, &service);
    match runtime.block_on(service.install(&program)) {
        Ok(Installed { file, replaced }) => {
            let id = if replaced {
                msg::CLI_SERVICE_REPLACED
            } else {
                msg::CLI_SERVICE_INSTALLED
            };
            let mut told = line(localizer, id, &[("file", MsgArg::Text(shown(&file)))]);
            // A user's systemd services stop when the user's last session
            // ends, unless the user lingers.
            if runtime.block_on(service.should_hint_lingering()) {
                told.push_str(&line(localizer, msg::CLI_SERVICE_LINGER, &[]));
            }
            told.push_str(&keyring_said(localizer, &service, keyring));
            told.push_str(&full_disk_access_said(
                localizer,
                service.needs_full_disk_access(),
                &program,
            ));
            told.push_str(&held_said(localizer, held.as_ref()));
            to_stdout(&told)?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            to_stderr(&error_said(localizer, &error))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// The note that says to grant the installed program Full Disk Access in
/// System Settings, for a service that `needed` it (a launchd agent on
/// macOS, §16 decision 115); nothing for any other.
fn full_disk_access_said(localizer: &Localizer, needed: bool, program: &ServicePath) -> String {
    if !needed {
        return String::new();
    }
    line(
        localizer,
        msg::CLI_SERVICE_FULL_DISK_ACCESS,
        &[("program", MsgArg::Text(shown(Path::new(program.as_str()))))],
    )
}

/// A data folder another process holds: the folder, and that process when
/// the lock file names it.
#[derive(Debug, PartialEq, Eq)]
struct Held {
    folder: PathBuf,
    holder: Option<u32>,
}

/// The data folder the service runs on, when a process other than the
/// installed service's holds it, as a `serve` started by hand does: the
/// service's every start is refused until that process stops (#459). It is
/// the platform's folder, not one the shell's `LADDERBACK_DATA_DIR` names,
/// which the service file doesn't carry.
fn data_folder_held(runtime: &tokio::runtime::Runtime, service: &UserService) -> Option<Held> {
    let folder = dirs::service_data_dir()?;
    let checked = DataDirLock::check(&folder);
    held_by_another(folder, checked, || runtime.block_on(service.process()))
}

/// The data folder `folder` as held by another process, from what the
/// check of its lock found: nothing when it is free or couldn't be checked
/// (the service says why itself when it starts), nor when the lock file
/// names the service's own process, which `service_process` gives and is
/// asked for only then: the service being replaced holds the folder at
/// every reinstall, and is not another Ladderback.
fn held_by_another(
    folder: PathBuf,
    checked: Result<(), LockError>,
    service_process: impl FnOnce() -> Option<u32>,
) -> Option<Held> {
    let holder = match checked {
        Err(LockError::InUse { holder }) => holder,
        Ok(()) | Err(LockError::Unavailable { .. }) => return None,
    };
    if let Some(pid) = holder
        && service_process() == Some(pid)
    {
        return None;
    }
    Some(Held { folder, holder })
}

/// The note that says another Ladderback holds the data folder and must stop
/// for the service to take over; nothing when none does.
fn held_said(localizer: &Localizer, held: Option<&Held>) -> String {
    let Some(Held { folder, holder }) = held else {
        return String::new();
    };
    let path = ("path", MsgArg::Text(shown(folder)));
    match holder {
        Some(pid) => line(
            localizer,
            msg::CLI_SERVICE_DATA_DIR_IN_USE,
            &[path, ("pid", MsgArg::Text(pid.to_string()))],
        ),
        None => line(localizer, msg::CLI_SERVICE_DATA_DIR_IN_USE_UNNAMED, &[path]),
    }
}

fn remove(localizer: &Localizer) -> io::Result<ExitCode> {
    let Some(service) = this_service(localizer)? else {
        return Ok(ExitCode::FAILURE);
    };
    let Some(runtime) = runtime(localizer)? else {
        return Ok(ExitCode::FAILURE);
    };
    match runtime.block_on(service.remove()) {
        Ok(Removed::Removed { file }) => {
            to_stdout(&line(
                localizer,
                msg::CLI_SERVICE_REMOVED,
                &[("file", MsgArg::Text(shown(&file)))],
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        Ok(Removed::NotInstalled { file }) => {
            to_stdout(&line(
                localizer,
                msg::CLI_SERVICE_NOT_INSTALLED,
                &[("file", MsgArg::Text(shown(&file)))],
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            to_stderr(&error_said(localizer, &error))?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// How many repositories of the configuration read their password from the
/// keyring: none when there is no configuration, or it can't be read, which
/// the service says itself when it starts. The file is only read, never
/// migrated (`check_keyring::read_config`).
fn keyring_passwords() -> usize {
    let Some(folder) = dirs::config_dir() else {
        return 0;
    };
    crate::check_keyring::read_config(&folder.join("config.toml"))
        .map_or(0, |config| config.keyring_passwords())
}

/// The line that says a lingering service can't read the `keyring`
/// repositories' passwords before login, with a password file instead, or
/// nothing where the service runs only after login or no password is in the
/// keyring (#479).
fn keyring_said(localizer: &Localizer, service: &UserService, keyring: usize) -> String {
    if keyring == 0 || !service.keyring_waits_for_login() {
        return String::new();
    }
    let count = u64::try_from(keyring).unwrap_or(u64::MAX);
    line(
        localizer,
        msg::CLI_SERVICE_KEYRING_BEFORE_LOGIN,
        &[("count", MsgArg::Count(count))],
    )
}

/// This platform's service manager for this user, or `None` once why there
/// is none has been said.
fn this_service(localizer: &Localizer) -> io::Result<Option<UserService>> {
    let env = |name: &str| std::env::var_os(name);
    match UserService::of_this_platform(env, std::env::vars_os()) {
        Ok(service) => Ok(Some(service)),
        Err(Unavailable::AsRoot) => {
            say(localizer, msg::CLI_SERVICE_AS_ROOT, &[])?;
            Ok(None)
        }
        Err(Unavailable::NoHome) => {
            say(localizer, msg::CLI_SERVICE_NO_HOME, &[])?;
            Ok(None)
        }
        Err(Unavailable::Unsupported) => {
            say(localizer, msg::CLI_SERVICE_UNSUPPORTED, &[])?;
            Ok(None)
        }
    }
}

/// The path of this program, as the service file names it, or `None` once
/// why it can't be named has been said.
fn this_program(localizer: &Localizer) -> io::Result<Option<ServicePath>> {
    let found = std::env::current_exe().and_then(|exe| {
        let ran = std::env::args_os().next();
        program_path(&exe, ran.as_deref(), std::env::var_os("PATH").as_deref())
    });
    let program = match found {
        Ok(program) => program,
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_FIND_OWN_PATH,
                &[("detail", text(error))],
            )?;
            return Ok(None);
        }
    };
    match ServicePath::parse(&program) {
        Ok(path) => Ok(Some(path)),
        Err(problem) => {
            say(
                localizer,
                unusable(problem),
                &[("path", MsgArg::Text(shown(&program)))],
            )?;
            Ok(None)
        }
    }
}

/// The message that says why a service file cannot name the program's path.
fn unusable(problem: ServicePathProblem) -> MessageId {
    match problem {
        ServicePathProblem::NotText => msg::CLI_SERVICE_PROGRAM_NOT_TEXT,
        ServicePathProblem::Relative => msg::CLI_SERVICE_PROGRAM_RELATIVE,
        ServicePathProblem::StepsUp => msg::CLI_SERVICE_PROGRAM_STEPS_UP,
        ServicePathProblem::ControlCharacter => msg::CLI_SERVICE_PROGRAM_CONTROL,
        ServicePathProblem::Quote => msg::CLI_SERVICE_PROGRAM_QUOTE,
    }
}

/// A runtime to run the manager's program on, or `None` once why there is
/// none has been said.
fn runtime(localizer: &Localizer) -> io::Result<Option<tokio::runtime::Runtime>> {
    match tokio::runtime::Runtime::new() {
        Ok(runtime) => Ok(Some(runtime)),
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            Ok(None)
        }
    }
}

/// The line, for stderr, that says why an install or a removal stopped: its
/// texts, the service manager's own words among them, escaped.
fn error_said(localizer: &Localizer, error: &ServiceError) -> String {
    match &error.problem {
        ServiceProblem::File { detail } => said(
            localizer,
            msg::CLI_SERVICE_FILE_ERROR,
            &[
                ("file", MsgArg::Text(shown(&error.file))),
                ("detail", outside(detail)),
            ],
        ),
        ServiceProblem::ManagerMissing { command, detail } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_MISSING,
            &[("command", outside(command)), ("detail", outside(detail))],
        ),
        ServiceProblem::ManagerRefused {
            command,
            said: words,
        } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_REFUSED,
            &[("command", outside(command)), ("said", outside(words))],
        ),
        ServiceProblem::ManagerSilent { command, seconds } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_SILENT,
            &[
                ("command", outside(command)),
                ("limit", MsgArg::DurationMs(seconds.saturating_mul(1000))),
            ],
        ),
        ServiceProblem::StillStopping { command, seconds } => said(
            localizer,
            msg::CLI_SERVICE_STILL_STOPPING,
            &[
                ("command", outside(command)),
                ("limit", MsgArg::DurationMs(seconds.saturating_mul(1000))),
            ],
        ),
        ServiceProblem::NotOurs => said(
            localizer,
            msg::CLI_SERVICE_NOT_OURS,
            &[("file", MsgArg::Text(shown(&error.file)))],
        ),
    }
}

/// Text from outside (the service manager's, the operating system's) as a
/// message's argument, escaped.
fn outside(text: &str) -> MsgArg {
    MsgArg::Text(crate::output::escaped(text))
}

/// One of the catalog's messages, as a line of its own.
fn line(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    let mut line = localizer.message(id, args).to_string();
    line.push('\n');
    line
}

/// `path` as a message shows it: a control character as its escape, never
/// sent to the terminal as it is.
fn shown(path: &Path) -> String {
    crate::output::escaped(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use app::ports::{ServiceError, ServiceProblem};
    use l10n::Localizer;

    use domain::service::ServicePath;

    use platform::data_dir_lock::LockError;

    use super::{Held, error_said, full_disk_access_said, held_by_another, held_said, shown};

    /// The manager is never asked for the service's process here.
    fn not_asked() -> Option<u32> {
        panic!("the manager was asked for the service's process")
    }

    /// Only a process other than the service's is named; a folder that is
    /// free, or whose lock can't be checked, is said nothing of, and the
    /// manager is asked for the service's process only when the lock file
    /// names one (#459).
    #[test]
    fn only_a_holder_other_than_the_service_is_named() {
        let folder = || PathBuf::from("/home/demo/.local/share/ladderback");
        let held = |holder| {
            Some(Held {
                folder: folder(),
                holder,
            })
        };
        assert_eq!(held_by_another(folder(), Ok(()), not_asked), None);
        let unavailable = LockError::Unavailable {
            detail: "Operation not supported".to_owned(),
        };
        assert_eq!(held_by_another(folder(), Err(unavailable), not_asked), None);
        let unnamed = LockError::InUse { holder: None };
        assert_eq!(
            held_by_another(folder(), Err(unnamed), not_asked),
            held(None)
        );
        let named = || LockError::InUse { holder: Some(4321) };
        assert_eq!(held_by_another(folder(), Err(named()), || Some(4321)), None);
        assert_eq!(
            held_by_another(folder(), Err(named()), || Some(4400)),
            held(Some(4321))
        );
        assert_eq!(
            held_by_another(folder(), Err(named()), || None),
            held(Some(4321))
        );
    }

    /// After installing a launchd agent, the note names the program to grant
    /// Full Disk Access to, and where; a service that needs no grant gets
    /// no note (§16, decision 115).
    #[test]
    fn the_full_disk_access_note_names_the_program_only_where_it_is_needed() {
        let program = ServicePath::parse(Path::new("/opt/homebrew/bin/ladderback")).unwrap();
        let localizer = Localizer::new(&[]);
        let said = full_disk_access_said(&localizer, true, &program);
        assert!(said.contains("/opt/homebrew/bin/ladderback"), "{said}");
        assert!(said.contains("Full Disk Access"), "{said}");
        assert!(said.contains("System Settings"), "{said}");
        assert_eq!(full_disk_access_said(&localizer, false, &program), "");
    }

    /// Another Ladderback that holds the data folder is named by its process
    /// when the lock file names one, and the folder always (#459); a folder
    /// nobody else holds gets no note.
    #[test]
    fn the_note_on_a_held_data_folder_names_its_holder_when_it_can() {
        let localizer = Localizer::new(&[]);
        let folder = PathBuf::from("/home/demo/.local/share/ladderback");
        let named = held_said(
            &localizer,
            Some(&Held {
                folder: folder.clone(),
                holder: Some(4321),
            }),
        );
        assert!(named.contains("process 4321"), "{named}");
        assert!(
            named.contains("/home/demo/.local/share/ladderback"),
            "{named}"
        );
        let unnamed = held_said(
            &localizer,
            Some(&Held {
                folder,
                holder: None,
            }),
        );
        assert!(!unnamed.contains("process"), "{unnamed}");
        assert!(
            unnamed.contains("/home/demo/.local/share/ladderback"),
            "{unnamed}"
        );
        assert_eq!(held_said(&localizer, None), "");
    }

    #[test]
    fn a_service_still_stopping_is_said_with_the_command_and_how_long_it_was_waited_for() {
        let error = ServiceError {
            file: PathBuf::from(
                "/home/demo/Library/LaunchAgents/io.github.nledford.ladderback.plist",
            ),
            problem: ServiceProblem::StillStopping {
                command: "launchctl bootout gui/501/io.github.nledford.ladderback".to_owned(),
                seconds: 30,
            },
        };
        let said = error_said(&Localizer::new(&[]), &error);
        assert!(
            said.contains("launchctl bootout gui/501/io.github.nledford.ladderback"),
            "{said}"
        );
        assert!(said.contains("30 s"), "{said}");
    }

    #[test]
    fn a_path_is_shown_as_it_is() {
        let path = "/home/demo/My Apps/o'brien \"x\" \\ é/ladderback.service";
        assert_eq!(shown(Path::new(path)), path);
    }

    #[test]
    fn a_control_character_of_a_path_is_shown_as_its_escape() {
        assert_eq!(
            shown(Path::new("/srv/a\nb\u{1b}[31m\u{85}/ladderback.service")),
            r"/srv/a\nb\u{1b}[31m\u{85}/ladderback.service"
        );
    }

    #[test]
    fn the_service_managers_words_are_said_with_each_control_and_bidi_character_escaped() {
        let unsafe_words = "\u{1b}[2J\r\u{7}\u{202e}";
        let escaped = r"\u{1b}[2J\r\u{7}\u{202e}";
        for problem in [
            ServiceProblem::ManagerRefused {
                command: "systemctl --user enable ladderback".to_owned(),
                said: format!("Failed {unsafe_words}"),
            },
            ServiceProblem::ManagerMissing {
                command: "systemctl".to_owned(),
                detail: format!("not found {unsafe_words}"),
            },
            ServiceProblem::File {
                detail: format!("denied {unsafe_words}"),
            },
            ServiceProblem::ManagerSilent {
                command: format!("systemctl --user enable /home/demo/a{unsafe_words}"),
                seconds: 30,
            },
        ] {
            let error = ServiceError {
                file: PathBuf::from("/home/demo/.config/systemd/user/ladderback.service"),
                problem,
            };
            let said = error_said(&Localizer::new(&[]), &error);
            let body = said.strip_suffix('\n').expect("a line ends with a break");
            assert!(!body.chars().any(char::is_control), "{said:?}");
            assert!(!said.contains('\u{202e}'), "{said:?}");
            assert!(said.contains(escaped), "{said:?}");
        }
    }
}
