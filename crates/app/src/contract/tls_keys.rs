//! The `TlsKeys` port's contract (§16, decision 120, "The hub key"): what
//! the hub relies on whichever implementation makes its TLS key. The real
//! adapter's own tests add what only real keys show: the certificate's key
//! is the one pinned, and a key of another kind is refused.

use domain::fleet::{Fingerprint, TlsKey};

use crate::ports::{TlsKeyError, TlsKeys};

pub fn run(keys: &dyn TlsKeys) {
    a_key_has_one_fingerprint(keys);
    a_key_read_back_from_its_bytes_has_the_same_fingerprint(keys);
    two_new_keys_have_different_fingerprints(keys);
    bytes_that_are_no_key_are_refused(keys);
}

/// A new key; `None`, after a failed assertion, if none was made.
fn generate(keys: &dyn TlsKeys) -> Option<TlsKey> {
    let key = keys.generate();
    assert!(key.is_ok(), "no TLS key was made: {key:?}");
    key.ok()
}

/// A new key's fingerprint; `None`, after a failed assertion, if none.
fn fingerprint_of_new(keys: &dyn TlsKeys) -> Option<Fingerprint> {
    let fingerprint = keys.fingerprint(&generate(keys)?);
    assert!(
        fingerprint.is_ok(),
        "a new key has no fingerprint: {fingerprint:?}"
    );
    fingerprint.ok()
}

fn a_key_has_one_fingerprint(keys: &dyn TlsKeys) {
    let Some(key) = generate(keys) else { return };
    let first = keys.fingerprint(&key);
    assert!(first.is_ok(), "a new key has no fingerprint: {first:?}");
    assert_eq!(first, keys.fingerprint(&key), "one key has one fingerprint");
}

/// The hub reads its key back from `hub-tls-key` at each start, and the
/// fingerprint agents pinned must not change.
fn a_key_read_back_from_its_bytes_has_the_same_fingerprint(keys: &dyn TlsKeys) {
    let Some(key) = generate(keys) else { return };
    let read_back = TlsKey::from_der(key.der().to_vec());
    assert!(read_back.is_ok(), "a key's own bytes are refused");
    let Ok(read_back) = read_back else { return };
    let fingerprint = keys.fingerprint(&key);
    assert!(
        fingerprint.is_ok(),
        "a new key has no fingerprint: {fingerprint:?}"
    );
    assert_eq!(
        keys.fingerprint(&read_back),
        fingerprint,
        "the key read back has the fingerprint agents pinned"
    );
}

fn two_new_keys_have_different_fingerprints(keys: &dyn TlsKeys) {
    let (Some(first), Some(second)) = (fingerprint_of_new(keys), fingerprint_of_new(keys)) else {
        return;
    };
    assert_ne!(first, second, "each new key is another key");
}

fn bytes_that_are_no_key_are_refused(keys: &dyn TlsKeys) {
    let not_a_key = TlsKey::from_der(b"not a key".to_vec());
    assert!(not_a_key.is_ok(), "nine bytes are within a TLS key's size");
    if let Ok(not_a_key) = not_a_key {
        assert_eq!(keys.fingerprint(&not_a_key), Err(TlsKeyError::NotAKey));
    }
}
