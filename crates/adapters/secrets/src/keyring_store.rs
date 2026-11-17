//! Where keyring entries are read from: the OS store, or one set by a test.

use std::sync::Arc;

use app::ports::KeyringActivation;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use domain::secrets::KeyringCause;

/// Why a store gave no secret.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// There is no such entry.
    NoEntry,
    /// The entry holds something that is not text.
    NotUnicode,
    /// The store can't be reached or refused: the platform's own words.
    Unreachable(String),
    /// There is no store to ask: on Linux no Secret Service owns its name on
    /// the session bus, or there is no session bus to reach one by. The
    /// platform's own words.
    NoService(String),
    /// The Secret Service answered and refused: it says its keyring is
    /// locked, or its unlock prompt was dismissed. The platform's own words.
    Refused(String),
    /// The Secret Service found no entry but couldn't say whether its
    /// keyring is locked, where an entry goes unseen: the entry may be
    /// there. The platform's words for why it couldn't say.
    Unconfirmed(String),
}

/// What the Secret Service said when asked whether its keyring is locked.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Debug)]
#[cfg_attr(
    all(not(target_os = "linux"), not(test)),
    expect(dead_code, reason = "only the Secret Service, on Linux, is asked")
)]
pub(crate) enum Lock {
    Unlocked,
    /// Locked: the words `keyring` gives a locked collection.
    Locked(String),
    /// It couldn't be asked: the platform's words why.
    Unknown(String),
}

/// A place secrets are kept under a service and a name. Calls may block
/// (D-Bus, the macOS keychain): callers run them off the async executor.
pub(crate) trait KeyringStore: Send + Sync {
    fn get(&self, service: &str, name: &str) -> Result<String, Failure>;

    /// Writes `secret` to the entry, adding it or replacing what it held:
    /// whoever calls this has made sure it may.
    fn set(&self, service: &str, name: &str, secret: &str) -> Result<(), Failure>;

    /// Deletes the entry; [`Failure::NoEntry`] when it isn't there.
    fn delete(&self, service: &str, name: &str) -> Result<(), Failure>;
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

/// How this machine's keyring service is started: the macOS keychain is
/// always there, the Secret Service as the session bus says, and where
/// there is no OS keyring there is nothing to start.
pub(crate) fn os_activation() -> KeyringActivation {
    #[cfg(target_os = "linux")]
    return match dbus::blocking::Connection::new_session() {
        Ok(connection) => activation_on(&connection),
        // No session bus to ask.
        Err(_) => KeyringActivation::Unknown,
    };
    #[cfg(target_os = "macos")]
    return KeyringActivation::OnDemand;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return KeyringActivation::Absent;
}

/// The name the Secret Service owns on the session bus.
#[cfg(target_os = "linux")]
const SECRET_SERVICE: &str = "org.freedesktop.secrets";

/// How long each question to the session bus may take: the bus itself
/// answers both, from what it holds, so a healthy one answers at once. Both
/// together leave time to connect within the whole question's limit.
#[cfg(target_os = "linux")]
const BUS_CALL_LIMIT: std::time::Duration = std::time::Duration::from_millis(500);

/// How the Secret Service is started, as the bus `connection` reaches says:
/// on demand when an activation file names it (`ListActivatableNames`),
/// whether it runs or not; running only when it owns its name
/// (`NameHasOwner`) and no file names it; absent when neither. A question
/// the bus refuses, or doesn't answer within [`BUS_CALL_LIMIT`], leaves it
/// unknown. Neither question touches an entry, so neither can raise an
/// unlock prompt, and neither carries a secret.
#[cfg(target_os = "linux")]
pub(crate) fn activation_on(connection: &dbus::blocking::Connection) -> KeyringActivation {
    let bus = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        BUS_CALL_LIMIT,
    );
    let activatable: Result<(Vec<String>,), dbus::Error> =
        bus.method_call("org.freedesktop.DBus", "ListActivatableNames", ());
    match activatable {
        Ok((names,)) if names.iter().any(|name| name == SECRET_SERVICE) => {
            KeyringActivation::OnDemand
        }
        Ok(_) => {
            let running: Result<(bool,), dbus::Error> =
                bus.method_call("org.freedesktop.DBus", "NameHasOwner", (SECRET_SERVICE,));
            match running {
                Ok((true,)) => KeyringActivation::RunningOnly,
                Ok((false,)) => KeyringActivation::Absent,
                Err(_) => KeyringActivation::Unknown,
            }
        }
        Err(_) => KeyringActivation::Unknown,
    }
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
            .map_err(os_failure)
    }

    fn set(&self, service: &str, name: &str, secret: &str) -> Result<(), Failure> {
        keyring::Entry::new(service, name)
            .and_then(|entry| entry.set_password(secret))
            .map_err(os_failure)
    }

    fn delete(&self, service: &str, name: &str) -> Result<(), Failure> {
        keyring::Entry::new(service, name)
            .and_then(|entry| entry.delete_credential())
            .map_err(os_failure)
    }
}

/// Why this machine's store refused a call.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn os_failure(error: keyring::Error) -> Failure {
    failure(error, THIS_STORE, || {
        cfg_select! {
            target_os = "linux" => { default_collection_lock() }
            // The keychain says when it is locked: never asked.
            _ => { Lock::Unlocked }
        }
    })
}

/// Whether the collection under the alias `default`, where `keyring` keeps
/// and looks for an entry, is locked, as the Secret Service says.
#[cfg(target_os = "linux")]
fn default_collection_lock() -> Lock {
    use dbus_secret_service::{EncryptionType, Error, SecretService};
    let locked = SecretService::connect(EncryptionType::Plain).and_then(|service| {
        service
            .get_default_collection()
            .and_then(|collection| collection.is_locked())
    });
    match locked {
        Ok(false) => Lock::Unlocked,
        Ok(true) => {
            Lock::Locked(keyring::Error::NoStorageAccess(Box::new(Error::Locked)).to_string())
        }
        Err(error) => Lock::Unknown(error.to_string()),
    }
}

/// Why the OS store `store` gave no secret. `locked` asks the store whether
/// its keyring is locked.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn failure(
    error: keyring::Error,
    store: OsStore,
    locked: impl FnOnce() -> Lock,
) -> Failure {
    // `keyring` reports a Secret Service it can't reach as `PlatformFailure`
    // and a locked or missing collection as `NoStorageAccess`, but its
    // `PlatformFailure` also carries a failed decryption or an answer it
    // couldn't parse: only the error it boxed tells them apart.
    let unavailable = match (store, &error) {
        (
            OsStore::SecretService,
            keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source),
        ) => {
            cfg_select! {
                target_os = "linux" => { secret_service_unavailable(source.as_ref()) }
                // The Secret Service is Linux's alone: no error from
                // elsewhere says it is missing.
                _ => {
                    let _ = source;
                    None
                }
            }
        }
        _ => None,
    };
    match error {
        // oo7 finds no items in a locked collection, so `keyring` says there
        // is no entry where the entry may well be (#483): the service, asked
        // whether its collection is locked, tells the two apart, and one that
        // can't be asked leaves it unconfirmed.
        keyring::Error::NoEntry => match store {
            OsStore::SecretService => match locked() {
                Lock::Unlocked => Failure::NoEntry,
                Lock::Locked(said) => Failure::Refused(said),
                Lock::Unknown(why) => Failure::Unconfirmed(why),
            },
            OsStore::Keychain => Failure::NoEntry,
        },
        // The error holds the bytes: a secret. Wipe them.
        keyring::Error::BadEncoding(mut bytes) => {
            zeroize::Zeroize::zeroize(&mut bytes);
            Failure::NotUnicode
        }
        error => match unavailable {
            Some(KeyringCause::NoService) => Failure::NoService(error.to_string()),
            Some(KeyringCause::Refused) => Failure::Refused(error.to_string()),
            None => Failure::Unreachable(error.to_string()),
        },
    }
}

/// What the error `keyring` boxed for a Secret Service failure says of the
/// service: none to ask (no session bus, no daemon on it, a provider that
/// isn't there), or one that refused (a locked or missing collection, a
/// dismissed unlock prompt). Anything else, or an error of another type, is
/// a read that failed: `None`.
#[cfg(target_os = "linux")]
fn secret_service_unavailable(
    source: &(dyn std::error::Error + Send + Sync + 'static),
) -> Option<KeyringCause> {
    use dbus_secret_service::Error;
    match source.downcast_ref::<Error>() {
        Some(Error::Unavailable) => Some(KeyringCause::NoService),
        Some(Error::Locked | Error::NoResult | Error::Prompt) => Some(KeyringCause::Refused),
        Some(Error::Dbus(error)) => error.name().and_then(bus_error_cause),
        _ => None,
    }
}

/// What a D-Bus error, by its name, says of the Secret Service: none to ask
/// when there is no session bus to connect to (`NoServer`, `FileNotFound`,
/// or `NotSupported` from libdbus's autolaunch without a display) or no
/// daemon owns the service's name (`ServiceUnknown`, `NameHasNoOwner`); one
/// that refused when it says its collection is locked (`IsLocked`). A call
/// the service took and failed (no reply, access denied, an object gone) is
/// neither. Only Linux reads D-Bus errors; the tests run it everywhere.
#[cfg(any(target_os = "linux", all(test, target_os = "macos")))]
pub(crate) fn bus_error_cause(name: &str) -> Option<KeyringCause> {
    match name {
        "org.freedesktop.DBus.Error.NoServer"
        | "org.freedesktop.DBus.Error.FileNotFound"
        | "org.freedesktop.DBus.Error.NotSupported"
        | "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => Some(KeyringCause::NoService),
        "org.freedesktop.Secret.Error.IsLocked" => Some(KeyringCause::Refused),
        _ => None,
    }
}

/// The store where there is none. Tests build it on every OS, so what it
/// answers is checked on the hosts that run them.
#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
pub(crate) struct Unavailable;

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
impl KeyringStore for Unavailable {
    fn get(&self, _service: &str, _name: &str) -> Result<String, Failure> {
        Err(no_os_keyring())
    }

    fn set(&self, _service: &str, _name: &str, _secret: &str) -> Result<(), Failure> {
        Err(no_os_keyring())
    }

    fn delete(&self, _service: &str, _name: &str) -> Result<(), Failure> {
        Err(no_os_keyring())
    }
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
fn no_os_keyring() -> Failure {
    Failure::Unreachable("this platform has no OS keyring".to_owned())
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

    fn set(&self, service: &str, name: &str, secret: &str) -> Result<(), Failure> {
        Memory::set(self, service, name, secret.as_bytes());
        Ok(())
    }

    fn delete(&self, service: &str, name: &str) -> Result<(), Failure> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(service.to_owned(), name.to_owned()))
            .map(|_| ())
            .ok_or(Failure::NoEntry)
    }
}
