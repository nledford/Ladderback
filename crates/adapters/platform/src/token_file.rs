//! The file `ladderback mcp --connect` reads its API token from (§4.6,
//! decision 93): `--token-file`, or `mcp-token` in the configuration folder.
//! It must be its owner's alone, as the admin password's hash is, since the
//! token opens the daemon's API to whoever reads it.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The token file's name in the configuration folder, read when
/// `--token-file` names none.
pub const DEFAULT_NAME: &str = "mcp-token";

/// The most bytes read: a token is under a hundred, with room for spaces
/// and line ends around it.
const MAX_FILE_BYTES: u64 = 4096;

/// Why a token file that is there can't be used. Each says the file's path
/// and what is wrong, never what it holds.
#[derive(Debug, PartialEq, Eq)]
pub struct TokenFileError {
    pub path: PathBuf,
    pub detail: String,
}

/// The text of the token file at `path`, or `None` when nothing is there.
///
/// # Errors
///
/// The file is there but can't be read, other users than its owner can read
/// it, it is larger than a token, or it isn't UTF-8.
pub fn read(path: &Path) -> Result<Option<String>, TokenFileError> {
    let refused = |detail: String| TokenFileError {
        path: path.to_owned(),
        detail,
    };
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(refused(error.to_string())),
    };
    let metadata = file
        .metadata()
        .map_err(|error| refused(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refused(
                "it can be read by other users than its owner".to_owned(),
            ));
        }
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(refused("it is larger than an API token".to_owned()));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| refused(error.to_string()))?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `text` as `name` in `folder`, with the permissions `mode`.
    fn file(folder: &Path, name: &str, text: &str, mode: u32) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = mode;
        path
    }

    #[test]
    fn a_file_its_owner_alone_can_read_gives_its_text() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(folder.path(), DEFAULT_NAME, "ladderback_x_y\n", 0o600);
        assert_eq!(read(&path), Ok(Some("ladderback_x_y\n".to_owned())));
    }

    #[test]
    fn no_file_is_no_token() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(read(&folder.path().join(DEFAULT_NAME)), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_other_users_can_read_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        for mode in [0o640, 0o604] {
            let path = file(folder.path(), DEFAULT_NAME, "ladderback_x_y", mode);
            let refused = read(&path).expect_err("a file others can read");
            assert_eq!(refused.path, path, "{mode:o}");
            assert!(
                refused.detail.contains("other users"),
                "{mode:o}: {}",
                refused.detail
            );
        }
    }

    #[test]
    fn a_file_larger_than_a_token_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(folder.path(), DEFAULT_NAME, &"x".repeat(4097), 0o600);
        let refused = read(&path).expect_err("too large a file");
        assert!(refused.detail.contains("larger"), "{}", refused.detail);
        let path = file(folder.path(), "just-fits", &"x".repeat(4096), 0o600);
        assert_eq!(
            read(&path).map(|text| text.map(|text| text.len())),
            Ok(Some(4096))
        );
    }
}
