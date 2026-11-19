use std::sync::atomic::{AtomicU64, Ordering};

use domain::fleet::{Fingerprint, TlsKey};

use crate::ports::{TlsKeyError, TlsKeys};

/// What every key the fake makes starts with; other bytes aren't a key.
const MADE: &[u8] = b"fake TLS key ";

/// A stand-in for ECDSA P-256 keys: the key it makes after `n` others is
/// the text `fake TLS key n`, and a key's fingerprint is the SHA-256 of that
/// text, so two keys differ and one key keeps one fingerprint. It counts the
/// keys it made, so a test can tell that none was.
#[derive(Debug, Default)]
pub struct FakeTlsKeys {
    made: AtomicU64,
}

impl FakeTlsKeys {
    /// How many keys [`TlsKeys::generate`] has made.
    pub fn made(&self) -> u64 {
        self.made.load(Ordering::SeqCst)
    }
}

impl TlsKeys for FakeTlsKeys {
    fn generate(&self) -> Result<TlsKey, TlsKeyError> {
        let n = self.made.fetch_add(1, Ordering::SeqCst);
        let mut der = MADE.to_vec();
        der.extend_from_slice(n.to_string().as_bytes());
        TlsKey::from_der(der).map_err(|refused| TlsKeyError::Unavailable {
            detail: format!("{refused:?}"),
        })
    }

    fn fingerprint(&self, key: &TlsKey) -> Result<Fingerprint, TlsKeyError> {
        if key.der().starts_with(MADE) {
            Ok(Fingerprint::of_bytes(key.der()))
        } else {
            Err(TlsKeyError::NotAKey)
        }
    }
}
