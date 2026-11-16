//! `ladderback install-service` and `ladderback remove-service` (operations
//! §12): wires this platform's `UserService` to the program's own path and
//! says what came of it, in the catalog's words. What a command did goes to
//! stdout; why it did nothing, or stopped, goes to stderr, and it ends with 1.

use std::io::{self, Read as _};
use std::path::Path;
use std::process::ExitCode;

use api::MsgArg;
use app::ports::{Installed, Removed, ServiceError, ServiceManager, ServiceProblem};
use config_adapter::{MAX_CONFIG_BYTES, Schemas, parse};
use domain::service::{ServicePath, ServicePathProblem};
use l10n::{Localizer, MessageId, PosixEnv, msg};
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
/// macOS, §16 decision NN); nothing for any other.
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
/// migrated: a daemon may be running on it, and only the one that holds the
/// data folder's lock may rewrite it.
fn keyring_passwords() -> usize {
    let Some(folder) = dirs::config_dir() else {
        return 0;
    };
    let file = folder.join("config.toml");
    let Ok(opened) = std::fs::File::open(&file) else {
        return 0;
    };
    let mut text = String::new();
    if opened
        .take(MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_string(&mut text)
        .is_err()
        || u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BYTES
    {
        return 0;
    }
    parse(&file, &text, &Schemas::production())
        .map_or(0, |parsed| parsed.config.keyring_passwords())
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

/// The line, for stderr, that says why an install or a removal stopped.
fn error_said(localizer: &Localizer, error: &ServiceError) -> String {
    match &error.problem {
        ServiceProblem::File { detail } => said(
            localizer,
            msg::CLI_SERVICE_FILE_ERROR,
            &[
                ("file", MsgArg::Text(shown(&error.file))),
                ("detail", text(detail)),
            ],
        ),
        ServiceProblem::ManagerMissing { command, detail } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_MISSING,
            &[("command", text(command)), ("detail", text(detail))],
        ),
        ServiceProblem::ManagerRefused {
            command,
            said: words,
        } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_REFUSED,
            &[("command", text(command)), ("said", text(words))],
        ),
        ServiceProblem::ManagerSilent { command, seconds } => said(
            localizer,
            msg::CLI_SERVICE_MANAGER_SILENT,
            &[
                ("command", text(command)),
                ("limit", MsgArg::DurationMs(seconds.saturating_mul(1000))),
            ],
        ),
        ServiceProblem::StillStopping { command, seconds } => said(
            localizer,
            msg::CLI_SERVICE_STILL_STOPPING,
            &[
                ("command", text(command)),
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

/// One of the catalog's messages, as a line of its own.
fn line(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    let mut line = localizer.message(id, args).to_string();
    line.push('\n');
    line
}

/// `path` as a message shows it: a control character as its escape, never
/// sent to the terminal as it is.
fn shown(path: &Path) -> String {
    let mut shown = String::new();
    for c in path.to_string_lossy().chars() {
        if c.is_control() {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use app::ports::{ServiceError, ServiceProblem};
    use l10n::Localizer;

    use domain::service::ServicePath;

    use super::{error_said, full_disk_access_said, shown};

    /// After installing a launchd agent, the note names the program to grant
    /// Full Disk Access to, and where; a service that needs no grant gets
    /// no note (§16, decision NN).
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
}
