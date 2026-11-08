//! A secret from a file. Blocking: callers run it off the async executor.

use app::ports::Resolved;
use domain::secrets::{FileRead, SecretError};

/// Reads the secret in the file at `path`. `credentials_directory` is the
/// value of `$CREDENTIALS_DIRECTORY`, if it is set.
pub(crate) fn read(
    _path: &str,
    _read: FileRead,
    _credentials_directory: Option<&str>,
) -> Result<Resolved, SecretError> {
    todo!("read the secret in the file")
}
