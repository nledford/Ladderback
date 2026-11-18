//! The key files of an agent and its hub, in the data folder (§7.1; §16,
//! decision 120, "The hub key" and "A copied data folder"), behind the
//! `Identity` port. Each is readable and writable by its owner only (mode
//! 0600). A key file is written whole under a private name and linked into
//! place, so concurrent first starts agree on one key and nothing ever reads
//! half a file, as the agent ID file is; the pin and the counter are
//! replaced whole by a rename. Each write is synced, with its folder, before
//! it answers. Every read is capped and lands in a buffer wiped when it is
//! dropped (I4), and no error carries a file's text.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use app::ports::{Attempt, Identity, IdentityError};
use async_trait::async_trait;
use domain::fleet::{HubPin, PublicKey, SeedFile, SigningSeed, TlsKey};

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
}

#[async_trait]
impl Identity for FileIdentity {
    async fn seed(&self, file: SeedFile) -> Result<Option<SigningSeed>, IdentityError> {
        let _ = (file, &self.folder);
        todo!()
    }

    async fn keep_seed(
        &self,
        file: SeedFile,
        seed: SigningSeed,
    ) -> Result<SigningSeed, IdentityError> {
        let _ = (file, seed);
        todo!()
    }

    async fn forget_recovery_seed(&self) -> Result<(), IdentityError> {
        todo!()
    }

    async fn recovery_key(&self) -> Result<Option<PublicKey>, IdentityError> {
        todo!()
    }

    async fn keep_recovery_key(&self, key: PublicKey) -> Result<PublicKey, IdentityError> {
        let _ = key;
        todo!()
    }

    async fn tls_key(&self) -> Result<Option<TlsKey>, IdentityError> {
        todo!()
    }

    async fn keep_tls_key(&self, key: TlsKey) -> Result<TlsKey, IdentityError> {
        let _ = key;
        todo!()
    }

    async fn pin(&self) -> Result<Option<HubPin>, IdentityError> {
        todo!()
    }

    async fn write_pin(&self, pin: &HubPin) -> Result<(), IdentityError> {
        let _ = pin;
        todo!()
    }

    async fn begin_attempt(&self) -> Result<Attempt, IdentityError> {
        let _ = (&self.attempts, Attempt::new);
        todo!()
    }
}

#[cfg(test)]
mod tests;
