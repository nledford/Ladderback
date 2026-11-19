//! The Fleet context's primitives for pairing an agent with its hub and for
//! the sync handshake (technical design §3.1, §7.1; §16, decisions 120 and
//! 121): Ed25519 keys and signatures as the protocol accepts them, the bytes
//! every signature covers, how a key is shown to a person, and the pairing
//! token. Rules only: the signing itself is the `Signing` port's, and every
//! seed and nonce comes from the `Random` port.

mod fingerprint;
mod hostname;
mod keys;
mod signed;
mod token;

pub use fingerprint::{Fingerprint, KeyId, TokenId};
pub use hostname::{Hostname, InvalidHostname, MAX_HOSTNAME_BYTES};
pub use keys::{
    KEY_BYTES, KeyRefused, PublicKey, SIGNATURE_BYTES, SMALL_ORDER_KEYS, Signature,
    SignatureRefused, SigningSeed,
};
pub use signed::{
    Binding, EpochSeconds, Handshakes, InvalidWireName, LABEL_PREFIX, MAX_WIRE_NAME_BYTES, Nonce,
    PayloadDigest, SignedBytes, SignedMessage, WireName,
};
pub use token::{
    HubAddress, InvalidHubAddress, MAX_ADDRESS_BYTES, PairingToken, TOKEN_PREFIX, TOKEN_VERSION,
    TokenRefused, TokenTls,
};
