//! The key the hub serves its built-in TLS with (§16, decision 120, "The hub
//! key"). The hub makes it on its first start and keeps it in `hub-tls-key`;
//! every agent pins its fingerprint at pairing. The hub never makes a
//! replacement by itself: a new key would lock out every agent that pinned
//! the old one, so a later start that finds the key gone, or another key in
//! its place, is refused and changes nothing.

use std::sync::Arc;

use domain::fleet::{Fingerprint, TlsKey};

use crate::ports::{Identity, IdentityError, TlsKeyError, TlsKeys};

/// Takes the hub's TLS key at its start.
pub struct HubTlsKey {
    identity: Arc<dyn Identity>,
    keys: Arc<dyn TlsKeys>,
}

/// The key the hub serves its built-in TLS with, and the fingerprint agents
/// pin. Its `Debug` form shows the fingerprint and none of the key (I4).
#[derive(Clone, Debug)]
pub struct ServedTlsKey {
    pub key: TlsKey,
    pub fingerprint: Fingerprint,
}

/// Why the hub can't serve its built-in TLS. None carries any of the key
/// (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsKeyRefusal {
    /// The hub recorded its key at its first start, and `hub-tls-key` is
    /// gone. It makes none in its place.
    Missing,
    /// `hub-tls-key` holds another key than the one the hub recorded. It is
    /// left as it is.
    AnotherKey,
    /// `hub-tls-key` holds no key the hub can serve. It is left as it is.
    NotAKey,
    /// The library couldn't make a key; `detail` is its own message.
    Unavailable { detail: String },
    /// The key file couldn't be read or written.
    Identity(IdentityError),
}

impl HubTlsKey {
    pub fn new(identity: Arc<dyn Identity>, keys: Arc<dyn TlsKeys>) -> Self {
        Self { identity, keys }
    }

    /// The key to serve. `recorded` is the fingerprint the hub recorded
    /// when it made its key, `None` before its first start: then a missing
    /// key is made and kept, two first starts at once keep one, and the
    /// caller records the fingerprint answered.
    pub async fn take(&self, recorded: Option<Fingerprint>) -> Result<ServedTlsKey, TlsKeyRefusal> {
        let key = match (self.identity.tls_key().await?, recorded) {
            (Some(key), _) => key,
            (None, Some(_)) => return Err(TlsKeyRefusal::Missing),
            (None, None) => {
                let made = self.keys.generate()?;
                self.identity.keep_tls_key(made).await?
            }
        };
        let fingerprint = self.keys.fingerprint(&key)?;
        match recorded {
            Some(recorded) if recorded != fingerprint => Err(TlsKeyRefusal::AnotherKey),
            _ => Ok(ServedTlsKey { key, fingerprint }),
        }
    }
}

impl From<TlsKeyError> for TlsKeyRefusal {
    fn from(error: TlsKeyError) -> Self {
        match error {
            TlsKeyError::NotAKey => Self::NotAKey,
            TlsKeyError::Unavailable { detail } => Self::Unavailable { detail },
        }
    }
}

impl From<IdentityError> for TlsKeyRefusal {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}
