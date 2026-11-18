//! The agent's pin of its hub (§16, decision 120, "The hub key"): what the
//! agent learned at pairing and keeps in `hub-pin`, a text file of
//! `name = value` lines in its data folder, never its database. A reader
//! keeps the names it doesn't know and writes them back, so a newer agent's
//! file survives an older agent that rewrites it.

use super::fingerprint::Fingerprint;
use super::keys::PublicKey;
use super::token::{HubAddress, TokenTls};

/// The most bytes a pin's text may have. A pin holds a few keys and at most
/// a few dozen addresses; more isn't one.
pub const MAX_PIN_BYTES: usize = 64 * 1024;

const HUB_KEY: &str = "hub-key";
const TLS: &str = "tls";
const RECOVERY_KEY: &str = "recovery-key";
const RECOVERY_NUMBER: &str = "recovery-number";
const GENERATION: &str = "generation";
const ADDRESS: &str = "address";

/// The names this build reads, in the order it writes them.
const KNOWN: [&str; 6] = [
    HUB_KEY,
    TLS,
    RECOVERY_KEY,
    RECOVERY_NUMBER,
    GENERATION,
    ADDRESS,
];

/// The value of `tls` for a hub behind a certificate authority's
/// certificate; a pinned certificate's is its fingerprint in full hex.
const TLS_CA: &str = "ca";

/// What an agent pins of its hub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubPin {
    /// The hub key every message of the hub is checked against.
    pub hub_key: PublicKey,
    /// How the hub's TLS certificate is checked at the token's address.
    pub tls: TokenTls,
    /// The recovery key, which only a rotation it signs itself replaces.
    pub recovery_key: PublicKey,
    /// The highest recovery number followed; 0 before any.
    pub recovery_number: u64,
    /// The hub's generation floor for the pinned key: a lower one is
    /// refused (`generation-lower`).
    pub generation: u64,
    /// The address in the pairing token.
    pub address: HubAddress,
    /// The lines whose names this build doesn't know, in their order.
    unknown: Vec<(String, String)>,
}

/// Why text isn't a pin this build can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinRefused {
    /// More than [`MAX_PIN_BYTES`].
    TooLong,
    /// The line, counted from 1, isn't blank and isn't `name = value` with
    /// a name of lower-case letters, digits and hyphens.
    Malformed { line: usize },
    /// A name this build reads is given twice.
    Repeated { name: &'static str },
    /// A name this build needs is missing.
    Missing { name: &'static str },
    /// A name this build reads has a value it can't read.
    Unreadable { name: &'static str },
}

impl HubPin {
    /// A pin made at pairing: no recovery followed yet, and the hub's first
    /// generation, 1 (§16, decision 120, "Scope").
    pub fn new(
        hub_key: PublicKey,
        tls: TokenTls,
        recovery_key: PublicKey,
        address: HubAddress,
    ) -> Self {
        let _ = (hub_key, tls, recovery_key, address);
        todo!()
    }

    /// Reads a pin. Blank lines are skipped; a name this build doesn't
    /// know is kept with its value, which [`HubPin::to_text`] writes back.
    pub fn parse(text: &str) -> Result<Self, PinRefused> {
        let _ = (text, KNOWN, TLS_CA, Fingerprint::from_digest);
        todo!()
    }

    /// The pin's text: the names this build knows in a fixed order, then
    /// every line it didn't know, as it was read, each ended by a line break.
    pub fn to_text(&self) -> String {
        todo!()
    }
}

#[cfg(test)]
mod tests;
