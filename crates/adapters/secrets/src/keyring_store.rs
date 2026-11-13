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
    /// There is no store to ask: on Linux no Secret Service runs, there is no
    /// session bus to reach one by, or the service says its keyring is locked
    /// or its unlock prompt was dismissed. The platform's own words.
    NoService(String),
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

/// Which OS store an error came from: the same `keyring` error says another
/// thing in each.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, Copy, Debug)]
pub(crate) enum OsStore {
    /// The macOS keychain, which every session has.
    Keychain,
    /// The Secret Service on Linux, over the session's D-Bus, which a server
    /// or a system service often lacks.
    SecretService,
}

/// This machine's store. Both variants are built on every OS, so neither is
/// dead code where the other is the one in use.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const THIS_STORE: OsStore = if cfg!(target_os = "linux") {
    OsStore::SecretService
} else {
    OsStore::Keychain
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Os;

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl KeyringStore for Os {
    fn get(&self, service: &str, name: &str) -> Result<String, Failure> {
        keyring::Entry::new(service, name)
            .and_then(|entry| entry.get_password())
            .map_err(|error| failure(error, THIS_STORE, || None))
    }
}

/// Why the OS store `store` gave no secret. `locked` asks the store whether
/// its keyring is locked, and gives what it said when it is.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn failure(
    error: keyring::Error,
    store: OsStore,
    locked: impl FnOnce() -> Option<String>,
) -> Failure {
    let _ = locked;
    // `keyring` reports a Secret Service it can't reach as `PlatformFailure`
    // and a locked or missing collection as `NoStorageAccess`, but its
    // `PlatformFailure` also carries a failed decryption or an answer it
    // couldn't parse: only the error it boxed tells them apart.
    let no_service = matches!(store, OsStore::SecretService)
        && match &error {
            keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source) => {
                cfg_select! {
                    target_os = "linux" => { secret_service_absent(source.as_ref()) }
                    // The Secret Service is Linux's alone: no error from
                    // elsewhere says it is missing.
                    _ => {
                        let _ = source;
                        false
                    }
                }
            }
            _ => false,
        };
    match error {
        keyring::Error::NoEntry => Failure::NoEntry,
        // The error holds the bytes: a secret. Drop them.
        keyring::Error::BadEncoding(_) => Failure::NotUnicode,
        error if no_service => Failure::NoService(error.to_string()),
        other => Failure::Unreachable(other.to_string()),
    }
}

/// Whether the error `keyring` boxed for a Secret Service failure says there
/// is no service to ask: no session bus, no daemon on it, or a collection
/// that is locked, missing or whose unlock prompt was dismissed. Anything
/// else, or an error of another type, is a read that failed.
#[cfg(target_os = "linux")]
fn secret_service_absent(source: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    use dbus_secret_service::Error;
    match source.downcast_ref::<Error>() {
        Some(Error::Locked | Error::NoResult | Error::Prompt | Error::Unavailable) => true,
        Some(Error::Dbus(error)) => error.name().is_some_and(bus_or_service_missing),
        _ => false,
    }
}

/// Whether a D-Bus error, by its name, says there is no Secret Service to
/// ask: no session bus to connect to (`NoServer`, `FileNotFound`, or
/// `NotSupported` from libdbus's autolaunch without a display), no daemon
/// owning the service's name (`ServiceUnknown`, `NameHasNoOwner`), or the
/// service's own word that its collection is locked (`IsLocked`). A call the
/// service took and failed (no reply, access denied, an object gone) is not
/// one. Only Linux reads D-Bus errors; the tests run it everywhere.
#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
pub(crate) fn bus_or_service_missing(name: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.NoServer"
            | "org.freedesktop.DBus.Error.FileNotFound"
            | "org.freedesktop.DBus.Error.NotSupported"
            | "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.Secret.Error.IsLocked"
    )
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
