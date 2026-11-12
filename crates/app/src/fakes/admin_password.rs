use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::access::{Password, PasswordFingerprint};

use crate::ports::{AdminPassword, AdminPasswordError};

/// The admin password kept in memory, as typed: a fake has no hash to keep.
/// Its fingerprint counts how many times a password has been set.
#[derive(Default)]
pub struct InMemoryAdminPassword {
    kept: Mutex<Option<(String, u64)>>,
}

impl InMemoryAdminPassword {
    /// A fake that already keeps `password`.
    pub fn with(password: &str) -> Self {
        Self {
            kept: Mutex::new(Some((password.to_owned(), 1))),
        }
    }
}

#[async_trait]
impl AdminPassword for InMemoryAdminPassword {
    async fn verify(&self, attempt: &Password) -> Result<bool, AdminPasswordError> {
        match &*self.kept.lock().unwrap_or_else(PoisonError::into_inner) {
            Some((kept, _)) => Ok(kept == attempt.expose()),
            None => Err(AdminPasswordError::NotSet),
        }
    }

    async fn set(&self, password: &Password) -> Result<(), AdminPasswordError> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let times = kept.as_ref().map_or(0, |(_, times)| *times);
        *kept = Some((password.expose().to_owned(), times.saturating_add(1)));
        Ok(())
    }

    fn fingerprint(&self) -> Result<PasswordFingerprint, AdminPasswordError> {
        match &*self.kept.lock().unwrap_or_else(PoisonError::into_inner) {
            Some((_, times)) => Ok(PasswordFingerprint::new(*times)),
            None => Err(AdminPasswordError::NotSet),
        }
    }
}
