//! The admin password's hash (technical design §10.1, decision 70): an
//! argon2id hash in a file of the data folder that only its owner can read,
//! written by `ladderback set-password`.

use std::path::Path;

use app::ports::{AdminPassword, AdminPasswordError};
use async_trait::async_trait;
use domain::access::{Password, PasswordFingerprint};

/// The file's name in the data folder.
pub const FILE: &str = "admin-password";

/// The admin password kept as an argon2id hash in `<data>/admin-password`.
pub struct FileAdminPassword;

impl FileAdminPassword {
    pub fn in_data_dir(_data_dir: &Path) -> Self {
        todo!()
    }

    pub fn is_set(&self) -> Result<bool, AdminPasswordError> {
        todo!()
    }
}

fn write_hash(_path: &Path, _hash: &str) -> std::io::Result<()> {
    todo!()
}

#[async_trait]
impl AdminPassword for FileAdminPassword {
    async fn verify(&self, _attempt: &Password) -> Result<bool, AdminPasswordError> {
        todo!()
    }

    async fn set(&self, _password: &Password) -> Result<(), AdminPasswordError> {
        todo!()
    }

    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
