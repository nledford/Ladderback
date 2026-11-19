//! The key files of an agent and its hub, in the data folder (§7.1; §16,
//! decision 120, "The hub key" and "A copied data folder"), behind the
//! `Identity` port. Each is readable and writable by its owner only (mode
//! 0600). A key file is written whole under a private name and linked into
//! place, so concurrent first starts agree on one key and nothing ever reads
//! half a file, as the agent ID file is; the pin and the counter are
//! replaced whole by a rename. Each write is synced, with its folder, before
//! it answers. Every read is capped and lands in a buffer wiped when it is
//! dropped (I4), and no error carries a file's text.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use app::ports::{Attempt, Identity, IdentityError};
use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::fleet::{
    COUNTER_LOCK_FILE, ConnectionCounter, HubPin, KeyFile, MAX_PIN_BYTES, MAX_TLS_KEY_BYTES,
    PublicKey, SeedFile, SigningSeed, TlsKey, one_line,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::agent_id::sync_dir;

/// The most bytes a key file of one line may have: 64 hex digits and a line
/// break, with room to spare.
const MAX_LINE_BYTES: usize = 256;

/// The most bytes the TLS key's PEM file may have: its base64 is a third
/// longer than the key, with a line break each 64 characters and the two
/// boundary lines.
const MAX_PEM_BYTES: usize = MAX_TLS_KEY_BYTES * 2;

const PEM_BEGIN: &str = "-----BEGIN PRIVATE KEY-----";
const PEM_END: &str = "-----END PRIVATE KEY-----";

/// The bytes of key a PEM line holds: 48, as 64 base64 characters.
const PEM_LINE_BYTES: usize = 48;

/// The key files in one data folder.
#[derive(Debug)]
pub struct FileIdentity {
    folder: PathBuf,
    /// This process's lock on the counter, taken before the file's.
    attempts: Arc<tokio::sync::Mutex<()>>,
}

impl FileIdentity {
    /// The key files in `data_dir`, which is made when a file is first
    /// written.
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self {
            folder: data_dir.to_owned(),
            attempts: Arc::default(),
        }
    }

    /// Reads `file` and makes what it holds of it with `parse`, off the
    /// async runtime: `None` when there is no such file.
    async fn read<T: Send + 'static>(
        &self,
        file: KeyFile,
        parse: fn(&str) -> Option<T>,
    ) -> Result<Option<T>, IdentityError> {
        let path = self.folder.join(file.file_name());
        blocking(file, move || read_parsed(&path, file, parse)).await
    }

    /// Keeps `value`, whose file holds `text`, in `file` unless the file is
    /// there, and answers what the file holds afterwards.
    async fn keep<T: Send + 'static>(
        &self,
        file: KeyFile,
        value: T,
        text: Zeroizing<String>,
        parse: fn(&str) -> Option<T>,
    ) -> Result<T, IdentityError> {
        let folder = self.folder.clone();
        blocking(file, move || {
            let path = folder.join(file.file_name());
            if let Some(found) = read_parsed(&path, file, parse)? {
                return Ok(found);
            }
            match create(&folder, file, text.as_bytes()) {
                Ok(()) => Ok(value),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    read_parsed(&path, file, parse)?.ok_or_else(|| unavailable(file, &error))
                }
                Err(error) => Err(unavailable(file, &error)),
            }
        })
        .await
    }
}

#[async_trait]
impl Identity for FileIdentity {
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
        self.read(file.key_file(), parse_seed).await
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError> {
        let text = line(seed.to_hex());
        self.keep(file.key_file(), seed, text, parse_seed).await
    }

    /// Deletes the seed's file and every staged copy of it a write cut short
    /// left (`hub-recovery-seed.<id>.tmp`), since each holds the seed too.
    async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
        let file = KeyFile::RecoverySeed;
        let folder = self.folder.clone();
        blocking(file, move || {
            forget(&folder, file).map_err(|error| unavailable(file, &error))
        })
        .await
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
        self.read(KeyFile::RecoveryKey, parse_key).await
    }

    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
        let text = line(Zeroizing::new(key.to_hex()));
        self.keep(KeyFile::RecoveryKey, key, text, parse_key).await
    }

    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
        self.read(KeyFile::HubTlsKey, parse_pem).await
    }

    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
        let text = pem(&key);
        self.keep(KeyFile::HubTlsKey, key, text, parse_pem).await
    }

    async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
        self.read(KeyFile::HubPin, |text| HubPin::parse(text).ok())
            .await
    }

    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
        let file = KeyFile::HubPin;
        let folder = self.folder.clone();
        let text = pin.to_text();
        blocking(file, move || {
            replace(&folder, file, text.as_bytes()).map_err(|error| unavailable(file, &error))
        })
        .await
    }

    async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
        let in_process = Arc::clone(&self.attempts)
            .try_lock_owned()
            .map_err(|_| IdentityError::AttemptInFlight)?;
        let file = KeyFile::ConnectionCounter;
        let folder = self.folder.clone();
        let (counter, lock) = blocking(file, move || next_counter(&folder)).await?;
        // The file lock first, then the process's: let go in this order, an
        // attempt in this process that takes the process's lock finds the
        // file's free, never an `AttemptInFlight` of its own.
        Ok(Attempt::new(counter, Box::new((lock, in_process))))
    }
}

/// Takes the counter file's lock, raises the counter and writes it. The
/// lock is the open lock file, let go when it is closed.
fn next_counter(folder: &Path) -> Result<(ConnectionCounter, File), IdentityError> {
    let file = KeyFile::ConnectionCounter;
    let fail = |error: io::Error| unavailable(file, &error);
    fs::create_dir_all(folder).map_err(fail)?;
    let lock = private()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(COUNTER_LOCK_FILE))
        .map_err(fail)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Err(IdentityError::AttemptInFlight),
        Err(fs::TryLockError::Error(error)) => return Err(fail(error)),
    }
    let parse = |text: &str| ConnectionCounter::parse(text).ok();
    let counter = read_parsed(&folder.join(file.file_name()), file, parse)?.unwrap_or_default();
    let next = counter.next().ok_or(IdentityError::CounterExhausted)?;
    replace(folder, file, next.to_file_text().as_bytes()).map_err(fail)?;
    Ok((next, lock))
}

fn parse_seed(text: &str) -> Option<SigningSeed> {
    SigningSeed::from_hex(one_line(text)?)
}

fn parse_key(text: &str) -> Option<PublicKey> {
    PublicKey::from_hex(one_line(text)?).ok()
}

/// The key from a PEM `PRIVATE KEY` block, and nothing else but line
/// breaks around it.
fn parse_pem(text: &str) -> Option<TlsKey> {
    let mut lines = text
        .trim_end()
        .lines()
        .map(|line| line.trim_end_matches('\r'));
    if lines.next()? != PEM_BEGIN || lines.next_back()? != PEM_END {
        return None;
    }
    let mut base64 = Zeroizing::new(String::with_capacity(text.len()));
    for line in lines {
        base64.push_str(line);
    }
    let mut der = Zeroizing::new(Vec::with_capacity(base64.len() / 4 * 3 + 3));
    STANDARD.decode_vec(base64.as_bytes(), &mut der).ok()?;
    TlsKey::from_der(std::mem::take(&mut *der)).ok()
}

/// The key as a PEM `PRIVATE KEY` block, 64 base64 characters a line.
fn pem(key: &TlsKey) -> Zeroizing<String> {
    let der = key.der();
    let lines = der.len().div_ceil(PEM_LINE_BYTES);
    let capacity = PEM_BEGIN.len() + PEM_END.len() + 2 + lines * 65;
    let mut text = Zeroizing::new(String::with_capacity(capacity));
    text.push_str(PEM_BEGIN);
    text.push('\n');
    for chunk in der.chunks(PEM_LINE_BYTES) {
        STANDARD.encode_string(chunk, &mut text);
        text.push('\n');
    }
    text.push_str(PEM_END);
    text.push('\n');
    text
}

/// `text` and a line break, in a buffer wiped when dropped.
fn line(text: Zeroizing<String>) -> Zeroizing<String> {
    let mut line = Zeroizing::new(String::with_capacity(text.len() + 1));
    line.push_str(&text);
    line.push('\n');
    line
}

/// The most bytes `file` may have.
const fn cap(file: KeyFile) -> usize {
    match file {
        KeyFile::HubTlsKey => MAX_PEM_BYTES,
        KeyFile::HubPin => MAX_PIN_BYTES,
        KeyFile::AgentKey
        | KeyFile::HubKey
        | KeyFile::RecoveryKey
        | KeyFile::RecoverySeed
        | KeyFile::ConnectionCounter => MAX_LINE_BYTES,
    }
}

/// What `file` at `path` holds, or `None` when there is none. More than its
/// cap, text that isn't UTF-8, or text `parse` refuses is damage.
fn read_parsed<T>(
    path: &Path,
    file: KeyFile,
    parse: fn(&str) -> Option<T>,
) -> Result<Option<T>, IdentityError> {
    let cap = cap(file);
    let opened = match File::open(path) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unavailable(file, &error)),
    };
    // One byte past the cap tells a file at the cap from a longer one; the
    // buffer is made at that size, so it never grows and leaves a copy.
    let mut bytes = Zeroizing::new(Vec::with_capacity(cap + 1));
    opened
        .take(u64::try_from(cap + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| unavailable(file, &error))?;
    let damaged = IdentityError::Damaged { file };
    if bytes.len() > cap {
        return Err(damaged);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| damaged.clone())?;
    parse(text).map(Some).ok_or(damaged)
}

/// Options that make a file its owner's alone.
fn private() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// Writes `bytes` to a new private file named after `file`, synced, and
/// answers its path. The name starts with the key file's, so the backup
/// exclusion leaves out one a crash left behind.
fn stage(folder: &Path, file: KeyFile, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let staged = folder.join(format!("{file}.{}.tmp", Uuid::now_v7().simple()));
    let mut written = private().write(true).create_new(true).open(&staged)?;
    let synced = written.write_all(bytes).and_then(|()| written.sync_all());
    if let Err(error) = synced {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    Ok(staged)
}

/// Makes `file` with `bytes` unless it is there: `AlreadyExists` when it is.
fn create(folder: &Path, file: KeyFile, bytes: &[u8]) -> io::Result<()> {
    let staged = stage(folder, file, bytes)?;
    let linked = fs::hard_link(&staged, folder.join(file.file_name()));
    let _ = fs::remove_file(&staged);
    linked?;
    sync_dir(folder)
}

/// Replaces `file` with `bytes` whole.
fn replace(folder: &Path, file: KeyFile, bytes: &[u8]) -> io::Result<()> {
    let staged = stage(folder, file, bytes)?;
    if let Err(error) = fs::rename(&staged, folder.join(file.file_name())) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    sync_dir(folder)
}

/// Deletes `file` and every staged copy of it ([`stage`]), then syncs the
/// folder when it deleted any. A folder or file that isn't there is
/// nothing to delete.
fn forget(folder: &Path, file: KeyFile) -> io::Result<()> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let name = file.file_name();
    let staged_prefix = format!("{name}.");
    let mut deleted = false;
    for entry in entries {
        let entry = entry?;
        let entry_name = entry.file_name();
        let Some(entry_name) = entry_name.to_str() else {
            continue;
        };
        let staged = entry_name.starts_with(&staged_prefix) && entry_name.ends_with(".tmp");
        if entry_name != name && !staged {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => deleted = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if deleted { sync_dir(folder) } else { Ok(()) }
}

fn unavailable(file: KeyFile, error: &io::Error) -> IdentityError {
    IdentityError::Unavailable {
        file,
        detail: error.to_string(),
    }
}

/// Runs `work` where blocking is allowed.
async fn blocking<T: Send + 'static>(
    file: KeyFile,
    work: impl FnOnce() -> Result<T, IdentityError> + Send + 'static,
) -> Result<T, IdentityError> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(unavailable(file, &io::Error::other(error.to_string()))))
}

#[cfg(test)]
mod tests;
