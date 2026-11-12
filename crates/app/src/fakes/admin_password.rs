use async_trait::async_trait;
use domain::access::{Password, PasswordFingerprint};

use crate::ports::{AdminPassword, AdminPasswordError};

/// The admin password kept in memory.
#[derive(Default)]
pub struct InMemoryAdminPassword;

impl InMemoryAdminPassword {
    pub fn with(_password: &str) -> Self {
        todo!()
    }
}

#[async_trait]
impl AdminPassword for InMemoryAdminPassword {
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
