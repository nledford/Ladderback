//! The one-off upgrade of a data folder an earlier release left (§16,
//! decision 170): the machine ID and key moved from their earlier names,
//! and the operation log copied into the baseline schema. Only a process
//! that holds the data folder's lock makes either, so no other Ladderback,
//! 0.5.0's daemon among them, has the log or the key open while they
//! change. A deliberate exception to "no shims": removed at 1.0.

use std::path::Path;

use l10n::msg;
use platform::agent_id::{self, MoveError};
use platform::data_dir_lock::{DataDirLock, LockError};
use store::{BaselineCopy, SqliteStore};

use super::Refused;
use super::startup::{lock_refused, log_refused, store_refused};
use crate::text;

/// The operation log's name in the data folder.
const DATABASE: &str = "ladderback.db";

/// Upgrades `data_dir`, whose lock `held` is: moves the machine ID and key
/// from their earlier names, then copies the operation log into the
/// baseline. What the copy did, for the daemon's log; `None` when the
/// folder needed no copy.
pub(crate) async fn under(
    held: &DataDirLock,
    data_dir: &Path,
) -> Result<Option<BaselineCopy>, Refused> {
    agent_id::move_earlier_names(data_dir, held).map_err(|error| match error {
        MoveError::TwoKeys { earlier, own } => Refused {
            message: msg::CLI_UPGRADE_TWO_KEYS,
            args: vec![
                ("earlier", text(earlier.display())),
                ("own", text(own.display())),
            ],
        },
        MoveError::Io(error) => store_refused(data_dir, &error.to_string()),
    })?;
    let log = data_dir.join(DATABASE);
    SqliteStore::copy_into_baseline(&log)
        .await
        .map_err(|error| log_refused(&log, &error))
}

/// Upgrades `data_dir` for a command that runs without its lock, such as
/// `pair` or `hub-status`: a folder that needs no upgrade is only looked
/// at. One that does is upgraded under the lock, taken for the upgrade and
/// let go after it. While another process holds the lock, a Ladderback
/// running on the folder, it is refused and nothing changes.
pub(crate) async fn unheld(data_dir: &Path) -> Result<(), Refused> {
    let log = data_dir.join(DATABASE);
    let due = agent_id::earlier_names_left(data_dir)
        .map_err(|error| store_refused(data_dir, &error.to_string()))?
        || SqliteStore::needs_copy(&log)
            .await
            .map_err(|error| log_refused(&log, &error))?;
    if !due {
        return Ok(());
    }
    let lock = match DataDirLock::acquire(data_dir) {
        Ok(lock) => lock,
        Err(LockError::InUse { .. }) => {
            return Err(Refused {
                message: msg::CLI_UPGRADE_LADDERBACK_RUNNING,
                args: vec![("path", text(data_dir.display()))],
            });
        }
        Err(error) => return Err(lock_refused(data_dir, error)),
    };
    under(&lock, data_dir).await.map(|_| ())
}
