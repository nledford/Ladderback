//! Connections a hub's front door hands the server: each one plain, from
//! loopback, or decrypted from the hub's built-in TLS, with that session's
//! binding (decision 120, "Binding to the TLS session").

use std::fmt;

/// A request that came over the hub's built-in TLS: the session's
/// `tls-exporter` value when it negotiated the hybrid key exchange, which a
/// kind-1 `Connect` binds to, and `None` otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BuiltInTls {
    pub exporter: Option<[u8; 32]>,
}

impl fmt::Debug for BuiltInTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltInTls")
            .field("post_quantum", &self.exporter.is_some())
            .finish_non_exhaustive()
    }
}
