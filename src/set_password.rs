//! `ladderback set-password` (technical design §10.1, decision 70): keeps
//! the admin password's hash in the data folder, which turns sign-in on. The
//! password is typed twice at a terminal, without being shown, or read as one
//! line of standard input; it is never an argument, which other users of the
//! machine could read.

use std::io;
use std::process::ExitCode;

use api::MsgArg;
use app::access::{PasswordSet, SetPasswordRefused, set_admin_password_leaving_sessions};
use domain::access::{PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARS};
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
    match runtime.block_on(set_admin_password_leaving_sessions(&kept, typed)) {
        Ok(()) => {
            say(localizer, msg::CLI_SET_PASSWORD_DONE, &[])?;
            Ok(ExitCode::SUCCESS)
        }
        Err(SetPasswordRefused::TooShort) => {
            let min = u64::try_from(PASSWORD_MIN_CHARS).unwrap_or(u64::MAX);
            say(
                localizer,
                msg::CLI_SET_PASSWORD_TOO_SHORT,
                &[("min", MsgArg::Count(min))],
            )?;
            Ok(ExitCode::FAILURE)
        }
        Err(SetPasswordRefused::TooLong) => too_long(localizer),
        Err(SetPasswordRefused::Unwritable(detail)) => {
            say(
                localizer,
                msg::CLI_SET_PASSWORD_UNWRITABLE,
                &[
                    ("path", text(data_dir.join(FILE).display())),
                    ("detail", text(detail)),
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

/// What `set` leaves to say: nothing once the store forgot every session,
/// or the database's own message when it couldn't.
fn sessions_left(_set: PasswordSet) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use app::access::PasswordSet;
    use app::ports::StoreError;

    use super::sessions_left;

    #[test]
    fn sessions_the_store_couldnt_forget_are_said_in_the_database_s_own_words() {
        let detail = "/demo/ladderback.db: disk I/O error";
        for error in [
            StoreError::Unavailable {
                detail: detail.to_owned(),
            },
            StoreError::Corrupt {
                detail: detail.to_owned(),
            },
        ] {
            assert_eq!(
                sessions_left(PasswordSet::SessionsLeft(error)),
                Some(detail.to_owned())
            );
        }
        assert_eq!(sessions_left(PasswordSet::SessionsForgotten), None);
    }
}
