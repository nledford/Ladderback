//! `ladderback set-password` (technical design §10.1, decision 70): keeps
//! the admin password's hash in the data folder, which turns sign-in on. The
//! password is typed twice at a terminal, without being shown, or read as one
//! line of standard input; it is never an argument, which other users of the
//! machine could read. With no daemon holding the data folder, it has the
//! daemon's store forget every session it keeps too.

use std::io;
use std::path::Path;
use std::process::ExitCode;

use api::MsgArg;
use app::access::{
    PasswordSet, SetPasswordRefused, set_admin_password, set_admin_password_leaving_sessions,
};
use app::ports::StoreError;
use domain::access::{PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARS};
use l10n::{Localizer, MessageId, msg};
use platform::admin_password::{FILE, FileAdminPassword};
use platform::data_dir_lock::{DataDirLock, LockError};
use platform::{dirs, terminal};
use store::{OpenError, SqliteStore};

use crate::composition::{startup, upgrade};
use crate::output::to_stderr;
use crate::{finish, said, say, text};

pub(crate) fn set_password(plain: bool) -> ExitCode {
    // Core dumps and debugger attach are off before the password is read
    // (decision 98): a Ctrl-\ at the prompt dumps nothing.
    let localizer = crate::start_protected(plain, None);
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
    // Hashing runs on a blocking thread of its own, which a runtime gives;
    // the store's waits need its timers.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
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
    let database = data_dir.join(DATABASE);
    match runtime.block_on(set_forgetting_sessions(
        localizer, &data_dir, &database, &kept, typed,
    )) {
        Ok(left) => {
            say(localizer, msg::CLI_SET_PASSWORD_DONE, &[])?;
            if let Some(detail) = left {
                say(
                    localizer,
                    msg::CLI_SET_PASSWORD_SESSIONS_LEFT,
                    &[("path", text(database.display())), ("detail", text(detail))],
                )?;
            }
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

/// The daemon's database in the data folder, as the daemon names it.
const DATABASE: &str = "ladderback.db";

/// Sets the admin password and, when no daemon holds the data folder, has
/// the store at `database` forget every session it keeps. `Ok(Some(detail))`
/// when the store couldn't. While a daemon holds the folder its store is the
/// daemon's alone (§9: a second process never migrates or writes under it),
/// and the sessions end by the password's fingerprint instead, at their next
/// use or at the next start; with no database there are none to forget.
async fn set_forgetting_sessions(
    localizer: &Localizer,
    data_dir: &Path,
    database: &Path,
    kept: &FileAdminPassword,
    typed: String,
) -> Result<Option<String>, SetPasswordRefused> {
    if !database.exists() {
        return leaving(kept, typed, None).await;
    }
    let lock = match DataDirLock::acquire(data_dir) {
        Ok(lock) => lock,
        Err(LockError::InUse { .. }) => return leaving(kept, typed, None).await,
        Err(LockError::Unavailable { detail }) => return leaving(kept, typed, Some(detail)).await,
    };
    // A folder an earlier release left is upgraded under the lock first
    // (decision 170), and a copy it made is said. A refusal, and a log that
    // can't be opened, is said in the user's words, not the error's own.
    match upgrade::under(&lock, data_dir).await {
        // Best effort: the password is set all the same if stderr is gone.
        Ok(copy) => {
            let _ = upgrade::tell(localizer, copy.as_ref());
        }
        Err(refusal) => {
            let detail = said(localizer, refusal.message, &refusal.args);
            return leaving(kept, typed, Some(detail)).await;
        }
    }
    let store = match SqliteStore::open(database).await {
        Ok(store) => store,
        Err(error @ OpenError::BeforeBaseline { .. }) => {
            let refusal = startup::log_refused(database, &error);
            let detail = said(localizer, refusal.message, &refusal.args);
            return leaving(kept, typed, Some(detail)).await;
        }
        Err(error) => return leaving(kept, typed, Some(error.to_string())).await,
    };
    let set = set_admin_password(kept, &store, typed).await;
    store.close().await;
    Ok(sessions_left(set?))
}

/// What `set` leaves to say: nothing once the store forgot every session,
/// or the database's own message when it couldn't. The message is printed
/// whole, not as the bare code `StoreError` gives log lines whose words may
/// repeat an operation row's contents (I4): forgetting the sessions is a
/// `DELETE` that reads no row, so its message can't hold a kept value.
fn sessions_left(set: PasswordSet) -> Option<String> {
    match set {
        PasswordSet::SessionsForgotten => None,
        PasswordSet::SessionsLeft(
            StoreError::Unavailable { detail } | StoreError::Corrupt { detail },
        ) => Some(detail),
    }
}

/// Sets the admin password without the store, then answers `left`.
async fn leaving(
    kept: &FileAdminPassword,
    typed: String,
    left: Option<String>,
) -> Result<Option<String>, SetPasswordRefused> {
    set_admin_password_leaving_sessions(kept, typed).await?;
    Ok(left)
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
