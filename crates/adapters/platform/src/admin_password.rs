//! The admin password's hash (technical design §10.1, decision 70): an
//! argon2id hash in a file of the data folder that only its owner can read,
//! written by `ladderback set-password`. The password itself is never kept,
//! and `config.toml` never names it.

use std::hash::{BuildHasher, RandomState};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use app::ports::{AdminPassword, AdminPasswordError};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2};
use async_trait::async_trait;
use domain::access::{Password, PasswordFingerprint};
use tokio::sync::Semaphore;

/// The file's name in the data folder.
pub const FILE: &str = "admin-password";

/// The most a hash file may hold: a PHC string of argon2id is about 100
/// bytes, and the file is read whole.
const MAX_FILE_BYTES: u64 = 512;

/// The admin password kept as an argon2id hash in `<data>/admin-password`.
/// One hash is computed at a time: each takes about 19 MiB and some tens of
/// milliseconds, and sign-ins arrive from whoever reaches the daemon.
pub struct FileAdminPassword {
    path: PathBuf,
    hashing: Arc<Semaphore>,
    /// The hash file's fingerprint as last read, beside what its metadata
    /// said then: while the metadata says the same, the file is not read.
    seen: Mutex<Option<(Stamp, PasswordFingerprint)>>,
    /// Keys drawn once per process, so a fingerprint means nothing outside
    /// it and the hash can't be guessed back from one.
    keys: RandomState,
}

/// What the hash file's metadata says of which file it is. `set-password`
/// writes a new file and renames it over the old one, so a new password is a
/// new file (another inode), and its time and length change with it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    file: (u64, u64),
}

impl Stamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        };
        #[cfg(not(unix))]
        let file = (0, 0);
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            file,
        }
    }
}

impl FileAdminPassword {
    /// The admin password of the data folder `data_dir`.
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FILE),
            hashing: Arc::new(Semaphore::new(1)),
            seen: Mutex::new(None),
            keys: RandomState::new(),
        }
    }

    /// Whether sign-in is on: `false` with no hash kept, `true` with one this
    /// build checks. A file that is there but can't be read, isn't such a
    /// hash, or that others can read is an error, never sign-in off.
    pub fn is_set(&self) -> Result<bool, AdminPasswordError> {
        match read_hash(&self.path) {
            Ok(_) => Ok(true),
            Err(AdminPasswordError::NotSet) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// Reads and checks the hash file: present, small, its owner's alone, and an
/// argon2id PHC string.
fn read_hash(path: &Path) -> Result<String, AdminPasswordError> {
    let unreadable = |error: std::io::Error| AdminPasswordError::Unreadable(error.to_string());
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AdminPasswordError::NotSet);
        }
        Err(error) => return Err(unreadable(error)),
    };
    let metadata = file.metadata().map_err(unreadable)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AdminPasswordError::Unreadable(format!(
                "{} can be read by other users than its owner",
                path.display()
            )));
        }
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AdminPasswordError::Unreadable(format!(
            "{} is larger than a password hash",
            path.display()
        )));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(unreadable)?;
    let text = text.trim_end().to_owned();
    let not_a_hash =
        || AdminPasswordError::Unreadable(format!("{} is not an argon2id hash", path.display()));
    let hash = PasswordHash::new(&text).map_err(|_| not_a_hash())?;
    if hash.algorithm != Algorithm::Argon2id.ident() {
        return Err(not_a_hash());
    }
    Ok(text)
}

/// Writes `hash` as the file at `path`, readable by its owner alone, in
/// place of any before it: a file beside it first, then renamed, so a reader
/// finds the old hash or the new one and never half of one.
fn write_hash(path: &Path, hash: &str) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let partial = path.with_extension("partial");
    // A leftover of an earlier run may have other permissions, which opening
    // it would keep.
    match std::fs::remove_file(&partial) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&partial)?;
    file.write_all(hash.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&partial, path)
}

#[async_trait]
impl AdminPassword for FileAdminPassword {
    async fn verify(&self, attempt: &Password) -> Result<bool, AdminPasswordError> {
        let hash = read_hash(&self.path)?;
        let attempt = attempt.clone();
        let _permit = self
            .hashing
            .acquire()
            .await
            .map_err(|error| AdminPasswordError::Unreadable(error.to_string()))?;
        tokio::task::spawn_blocking(move || {
            let parsed = PasswordHash::new(&hash)
                .map_err(|error| AdminPasswordError::Unreadable(error.to_string()))?;
            match Argon2::default().verify_password(attempt.expose().as_bytes(), &parsed) {
                Ok(()) => Ok(true),
                Err(argon2::password_hash::Error::Password) => Ok(false),
                Err(error) => Err(AdminPasswordError::Unreadable(error.to_string())),
            }
        })
        .await
        .map_err(|error| AdminPasswordError::Unreadable(error.to_string()))?
    }

    async fn set(&self, password: &Password) -> Result<(), AdminPasswordError> {
        let unwritable = |text: String| AdminPasswordError::Unwritable(text);
        let mut salt = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut salt)
            .map_err(|error| unwritable(error.to_string()))?;
        let salt = SaltString::encode_b64(&salt).map_err(|error| unwritable(error.to_string()))?;
        let password = password.clone();
        let path = self.path.clone();
        let _permit = self
            .hashing
            .acquire()
            .await
            .map_err(|error| unwritable(error.to_string()))?;
        tokio::task::spawn_blocking(move || {
            let hash = Argon2::default()
                .hash_password(password.expose().as_bytes(), &salt)
                .map_err(|error| unwritable(error.to_string()))?
                .to_string();
            write_hash(&path, &hash).map_err(|error| unwritable(error.to_string()))
        })
        .await
        .map_err(|error| unwritable(error.to_string()))?
    }

    /// One `stat` of the hash file per call; the file is read, checked and
    /// fingerprinted again only when its metadata has changed.
    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        let metadata = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(AdminPasswordError::NotSet);
            }
            Err(error) => return Err(AdminPasswordError::Unreadable(error.to_string())),
        };
        let stamp = Stamp::of(&metadata);
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, fingerprint)) = *seen
            && at == stamp
        {
            return Ok(fingerprint);
        }
        let fingerprint = PasswordFingerprint::new(self.keys.hash_one(read_hash(&self.path)?));
        *seen = Some((stamp, fingerprint));
        Ok(fingerprint)
    }
}

#[cfg(test)]
mod tests;
