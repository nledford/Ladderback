//! Where keyring entries are read from: the OS store, or one set by a test.

use std::sync::Arc;

/// Why a store gave no secret.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// There is no such entry.
    NoEntry,
    /// The entry holds something that is not text.
    NotUnicode,
    /// The store can't be reached or refused: the platform's own words.
    Unreachable(String),
}

/// A place secrets are kept under a service and a name. Calls may block
/// (D-Bus, the macOS keychain): callers run them off the async executor.
pub(crate) trait KeyringStore: Send + Sync {
    fn get(&self, service: &str, name: &str) -> Result<String, Failure>;
}

/// The store of the user's session: the macOS keychain, or the Secret
/// Service on Linux. Where there is none the answer is always `Unreachable`,
/// and never the `keyring` crate's in-memory mock, which it falls back to
/// when it is built without a platform feature.
pub(crate) fn os_store() -> Arc<dyn KeyringStore> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return Arc::new(Os);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Arc::new(Unavailable);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Os;

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl KeyringStore for Os {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        todo!("read the entry from the OS keyring")
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
struct Unavailable;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl KeyringStore for Unavailable {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        Err(Failure::Unreachable(
            "this platform has no OS keyring".to_owned(),
        ))
    }
}

/// Entries held in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Memory {
    entries: std::sync::Mutex<std::collections::BTreeMap<(String, String), Vec<u8>>>,
}

#[cfg(test)]
impl Memory {
    pub(crate) fn set(&self, service: &str, name: &str, secret: &[u8]) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((service.to_owned(), name.to_owned()), secret.to_vec());
    }
}

#[cfg(test)]
impl KeyringStore for Memory {
    fn get(&self, service: &str, name: &str) -> Result<String, Failure> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = entries
            .get(&(service.to_owned(), name.to_owned()))
            .ok_or(Failure::NoEntry)?;
        String::from_utf8(bytes.clone()).map_err(|_| Failure::NotUnicode)
    }
}
