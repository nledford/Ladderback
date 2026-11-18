//! The agent's pin of its hub (§16, decision 120, "The hub key"): what the
//! agent learned at pairing and keeps in `hub-pin`, a text file of
//! `name = value` lines in its data folder, never its database. A reader
//! keeps the names it doesn't know and writes them back, so a newer agent's
//! file survives an older agent that rewrites it.

use super::fingerprint::Fingerprint;
use super::keys::{PublicKey, decode_hex32};
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
    /// More than [`MAX_PIN_BYTES`], as read or as it would be written.
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
        Self {
            hub_key,
            tls,
            recovery_key,
            recovery_number: 0,
            generation: 1,
            address,
            unknown: Vec::new(),
        }
    }

    /// Reads a pin. Blank lines are skipped; a name this build doesn't
    /// know is kept with its value, which [`HubPin::to_text`] writes back.
    /// A pin whose text as written would be past [`MAX_PIN_BYTES`] is
    /// refused too, so every pin read can be written and read again.
    pub fn parse(text: &str) -> Result<Self, PinRefused> {
        if text.len() > MAX_PIN_BYTES {
            return Err(PinRefused::TooLong);
        }
        let mut read = Read::default();
        for (index, line) in text.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() {
                continue;
            }
            let malformed = PinRefused::Malformed { line: index + 1 };
            let (name, value) = line.split_once('=').ok_or(malformed)?;
            let (name, value) = (name.trim(), value.trim());
            if !is_name(name) {
                return Err(malformed);
            }
            read.line(name, value)?;
        }
        let pin = read.finish()?;
        if pin.to_text().len() > MAX_PIN_BYTES {
            return Err(PinRefused::TooLong);
        }
        Ok(pin)
    }

    /// The pin's text: the names this build knows in a fixed order, then
    /// every line it didn't know, as it was read, each ended by a line break.
    pub fn to_text(&self) -> String {
        let tls = match &self.tls {
            TokenTls::Ca => TLS_CA.to_owned(),
            TokenTls::Pinned(fingerprint) => fingerprint.full_hex(),
        };
        let known = [
            (HUB_KEY, self.hub_key.to_hex()),
            (TLS, tls),
            (RECOVERY_KEY, self.recovery_key.to_hex()),
            (RECOVERY_NUMBER, self.recovery_number.to_string()),
            (GENERATION, self.generation.to_string()),
            (ADDRESS, self.address.as_str().to_owned()),
        ];
        let unknown = self
            .unknown
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()));
        known
            .into_iter()
            .chain(unknown)
            .map(|(name, value)| format!("{name} = {value}\n"))
            .collect()
    }
}

/// A name of lower-case ASCII letters, digits and hyphens.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// What a pin's lines have given so far.
#[derive(Default)]
struct Read {
    hub_key: Option<PublicKey>,
    tls: Option<TokenTls>,
    recovery_key: Option<PublicKey>,
    recovery_number: Option<u64>,
    generation: Option<u64>,
    address: Option<HubAddress>,
    unknown: Vec<(String, String)>,
}

impl Read {
    fn line(&mut self, name: &str, value: &str) -> Result<(), PinRefused> {
        let Some(known) = KNOWN.into_iter().find(|known| *known == name) else {
            self.unknown.push((name.to_owned(), value.to_owned()));
            return Ok(());
        };
        let unreadable = PinRefused::Unreadable { name: known };
        match known {
            HUB_KEY => set(&mut self.hub_key, known, key(value).ok_or(unreadable)?),
            TLS => set(&mut self.tls, known, tls(value).ok_or(unreadable)?),
            RECOVERY_KEY => set(&mut self.recovery_key, known, key(value).ok_or(unreadable)?),
            RECOVERY_NUMBER => set(
                &mut self.recovery_number,
                known,
                number(value).ok_or(unreadable)?,
            ),
            GENERATION => set(
                &mut self.generation,
                known,
                number(value).ok_or(unreadable)?,
            ),
            _ => set(
                &mut self.address,
                known,
                HubAddress::parse(value).map_err(|_| unreadable)?,
            ),
        }
    }

    fn finish(self) -> Result<HubPin, PinRefused> {
        let missing = |name| PinRefused::Missing { name };
        Ok(HubPin {
            hub_key: self.hub_key.ok_or(missing(HUB_KEY))?,
            tls: self.tls.ok_or(missing(TLS))?,
            recovery_key: self.recovery_key.ok_or(missing(RECOVERY_KEY))?,
            recovery_number: self.recovery_number.ok_or(missing(RECOVERY_NUMBER))?,
            generation: self.generation.ok_or(missing(GENERATION))?,
            address: self.address.ok_or(missing(ADDRESS))?,
            unknown: self.unknown,
        })
    }
}

/// Sets a name's value, refusing it a second time.
fn set<T>(slot: &mut Option<T>, name: &'static str, value: T) -> Result<(), PinRefused> {
    if slot.is_some() {
        return Err(PinRefused::Repeated { name });
    }
    *slot = Some(value);
    Ok(())
}

fn key(value: &str) -> Option<PublicKey> {
    PublicKey::from_hex(value).ok()
}

fn tls(value: &str) -> Option<TokenTls> {
    if value == TLS_CA {
        return Some(TokenTls::Ca);
    }
    let mut digest = [0; 32];
    decode_hex32(value, &mut digest).then(|| TokenTls::Pinned(Fingerprint::from_digest(digest)))
}

/// Decimal digits alone, as a `u64`.
fn number(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
mod tests;
