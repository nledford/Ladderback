//! `ladderback set-password` (technical design §10.1, decision 70): keeps
//! the admin password's hash in the data folder, which turns sign-in on. The
//! password is typed twice at a terminal, without being shown, or read as one
//! line of standard input; it is never an argument, which other users of the
//! machine could read.

use std::io;
use std::process::ExitCode;

use api::MsgArg;
use app::ports::AdminPassword;
use domain::access::{PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARS, Password, PasswordProblem};
use l10n::{Localizer, MessageId, PosixEnv, msg};
use platform::admin_password::{FILE, FileAdminPassword};
use platform::{dirs, terminal};

use crate::output::to_stderr;
use crate::{finish, say, text};

pub(crate) fn set_password() -> ExitCode {
    let localizer = Localizer::new(&PosixEnv::from_process().preferences());
    finish(&localizer, run(&localizer))
}

fn run(localizer: &Localizer) -> io::Result<ExitCode> {
    let Some(data_dir) = dirs::data_dir() else {
        say(localizer, msg::CLI_NO_DATA_DIR, &[])?;
        return Ok(ExitCode::FAILURE);
    };
    let typed = match read_new_password(localizer)? {
        Ok(typed) => typed,
        Err(code) => return Ok(code),
    };
    let password = match Password::new_admin(typed) {
        Ok(password) => password,
        Err(PasswordProblem::Empty | PasswordProblem::TooShort) => {
            let min = u64::try_from(PASSWORD_MIN_CHARS).unwrap_or(u64::MAX);
            say(
                localizer,
                msg::CLI_SET_PASSWORD_TOO_SHORT,
                &[("min", MsgArg::Count(min))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
        Err(PasswordProblem::TooLong) => return too_long(localizer),
    };
    // Hashing runs on a blocking thread of its own, which a runtime gives.
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(
                localizer,
                msg::CLI_CANNOT_START_RUNTIME,
                &[("detail", text(error))],
            )?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let kept = FileAdminPassword::in_data_dir(&data_dir);
    match runtime.block_on(kept.set(&password)) {
        Ok(()) => {
            say(localizer, msg::CLI_SET_PASSWORD_DONE, &[])?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            say(
                localizer,
                msg::CLI_SET_PASSWORD_UNWRITABLE,
                &[
                    ("path", text(data_dir.join(FILE).display())),
                    ("detail", text(format!("{error:?}"))),
                ],
            )?;
            Ok(ExitCode::FAILURE)
        }
    }
}

fn too_long(localizer: &Localizer) -> io::Result<ExitCode> {
    let max = u64::try_from(PASSWORD_MAX_BYTES).unwrap_or(u64::MAX);
    say(
        localizer,
        msg::CLI_SET_PASSWORD_TOO_LONG,
        &[("max", MsgArg::Count(max))],
    )?;
    Ok(ExitCode::FAILURE)
}

/// The new password as typed: twice at a terminal, which must agree, or one
/// line of standard input. `Err` is the status to end with, once said.
fn read_new_password(localizer: &Localizer) -> io::Result<Result<String, ExitCode>> {
    if !terminal::stdin_is_terminal() {
        return read_line(localizer);
    }
    let first = match prompted(localizer, msg::CLI_SET_PASSWORD_PROMPT)? {
        Ok(first) => first,
        Err(code) => return Ok(Err(code)),
    };
    let again = match prompted(localizer, msg::CLI_SET_PASSWORD_PROMPT_AGAIN)? {
        Ok(again) => again,
        Err(code) => return Ok(Err(code)),
    };
    if first != again {
        say(localizer, msg::CLI_SET_PASSWORD_MISMATCH, &[])?;
        return Ok(Err(ExitCode::FAILURE));
    }
    Ok(Ok(first))
}

/// Asks with `prompt` and reads the answer, which the terminal doesn't show;
/// the line the person ended with Enter then ends on stderr too.
fn prompted(localizer: &Localizer, prompt: MessageId) -> io::Result<Result<String, ExitCode>> {
    to_stderr(&format!("{} ", localizer.message(prompt, &[])))?;
    let answer = read_line(localizer);
    to_stderr("\n")?;
    answer
}

fn read_line(localizer: &Localizer) -> io::Result<Result<String, ExitCode>> {
    match terminal::read_secret_line(PASSWORD_MAX_BYTES) {
        Ok(line) => Ok(Ok(line)),
        Err(error) => {
            say(
                localizer,
                msg::CLI_SET_PASSWORD_UNREADABLE,
                &[("detail", text(error))],
            )?;
            Ok(Err(ExitCode::FAILURE))
        }
    }
}
