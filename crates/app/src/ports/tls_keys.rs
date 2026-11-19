use domain::fleet::{Fingerprint, TlsKey};

/// The hub's TLS key in built-in mode (§16, decision 120, "The hub key"): an
/// ECDSA P-256 key, kept in `hub-tls-key` through the `Identity` port, from
/// which the hub makes its self-signed certificate at each start. Agents pin
/// the key, not the certificate, so a certificate made again from the same
/// key keeps every agent's pin.
pub trait TlsKeys: Send + Sync {
    /// A new key, from the operating system's randomness.
    fn generate(&self) -> Result<TlsKey, TlsKeyError>;

    /// The fingerprint an agent pins: the SHA-256 of the key's public half
    /// as the certificate carries it (its SubjectPublicKeyInfo). The same
    /// key always has the same fingerprint.
    fn fingerprint(&self, key: &TlsKey) -> Result<Fingerprint, TlsKeyError>;
}

/// Why a TLS key couldn't be made or used. Neither carries any of the key
/// (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TlsKeyError {
    /// The bytes aren't an ECDSA P-256 key in its PKCS#8 encoding.
    NotAKey,
    /// The library couldn't make a key; `detail` is its own message.
    Unavailable { detail: String },
}
