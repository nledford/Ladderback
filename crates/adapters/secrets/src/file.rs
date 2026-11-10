//! A secret from a file. Blocking: callers run it off the async executor.

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use app::ports::Resolved;
use domain::secrets::{
    FileRead, MAX_SECRET_BYTES, Problem, SecretError, SourceName, decode, file_warning,
};
use nix::fcntl::OFlag;

/// Reads the secret in the file at `path`. `credentials_directory` is the
/// value of `$CREDENTIALS_DIRECTORY`, if it is set.
pub(crate) fn read(
    path: &str,
    read: FileRead,
    credentials_directory: Option<&str>,
) -> Result<Resolved, SecretError> {
    let source = SourceName::File {
        path: path.to_owned(),
    };
    let fail = |problem| SecretError {
        source: source.clone(),
        problem,
    };
    let unreachable = |detail: String| fail(Problem::Unreachable { detail });

    // Without O_NONBLOCK, opening a FIFO waits for someone to write to it.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NONBLOCK | OFlag::O_NOCTTY).bits())
        .open(path)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => {
                SecretError::missing(source.clone())
            }
            _ => unreachable(error.to_string()),
        })?;
    // The open file's own metadata: the path may have changed meanwhile.
    let metadata = file
        .metadata()
        .map_err(|error| unreachable(error.to_string()))?;
    if !metadata.is_file() {
        // A FIFO or a device would block, or never end.
        return Err(unreachable("not a regular file".to_owned()));
    }

    // One byte more than the limit: that is already too much.
    let mut bytes = Vec::new();
    file.take(MAX_SECRET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreachable(error.to_string()))?;
    let value = decode(&source, &bytes, |text| read.extract(text))?;
    Ok(Resolved {
        value,
        warnings: file_warning(
            path,
            metadata.permissions().mode(),
            credentials_directory,
            |path| Some(path.to_owned()),
        )
        .into_iter()
        .collect(),
    })
}
