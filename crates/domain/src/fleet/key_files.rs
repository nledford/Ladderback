//! The key files: where an agent and its hub keep their keys, in the data
//! folder and never in the database, so `db rebuild` can't change them
//! (§7.1; §16, decision 120, "The hub key" and "A copied data folder"). Their
//! names, what each holds, and which ones a backup of the data folder leaves
//! out. Reading and writing them is the `Identity` port's.

use std::fmt;

use zeroize::Zeroizing;

use crate::planning::escape_glob;

/// One key file in the data folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyFile {
    /// The agent key's seed: 64 lower-case hex digits on one line.
    AgentKey,
    /// The hub key's seed, as the agent key's is kept.
    HubKey,
    /// The hub's TLS key in built-in mode: an ECDSA P-256 key, PKCS#8 in a
    /// PEM `PRIVATE KEY` block.
    HubTlsKey,
    /// The recovery key's public half, as 64 hex digits. Public, and kept
    /// as well in the database, so `hub-recover` can check a seed.
    RecoveryKey,
    /// The recovery key's seed, as the hub key's is kept, until the person
    /// confirms they have the Recovery Kit.
    RecoverySeed,
    /// The agent's connection counter: a decimal number on one line.
    ConnectionCounter,
    /// The agent's pin of its hub: `name = value` lines ([`HubPin`]).
    ///
    /// [`HubPin`]: super::HubPin
    HubPin,
}

impl KeyFile {
    pub const ALL: [Self; 7] = [
        Self::AgentKey,
        Self::HubKey,
        Self::HubTlsKey,
        Self::RecoveryKey,
        Self::RecoverySeed,
        Self::ConnectionCounter,
        Self::HubPin,
    ];

    /// The file's name in the data folder.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::AgentKey => "agent-key",
            Self::HubKey => "hub-key",
            Self::HubTlsKey => "hub-tls-key",
            Self::RecoveryKey => "hub-recovery-key",
            Self::RecoverySeed => "hub-recovery-seed",
            Self::ConnectionCounter => "connection-counter",
            Self::HubPin => "hub-pin",
        }
    }
}

impl fmt::Display for KeyFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.file_name())
    }
}

/// The file the connection counter is taken under. It holds nothing: the
/// counter's own file is replaced whole at each write, and a lock on a file
/// that is then replaced would lock nothing.
pub const COUNTER_LOCK_FILE: &str = "connection-counter.lock";

/// The key files a backup of the data folder leaves out by default: the
/// secret ones (§16, decisions 120, "The hub key", and 122, item 3).
pub const LEFT_OUT_OF_BACKUPS: [KeyFile; 4] = [
    KeyFile::HubKey,
    KeyFile::HubTlsKey,
    KeyFile::RecoverySeed,
    KeyFile::AgentKey,
];

/// The globs that leave [`LEFT_OUT_OF_BACKUPS`] out of a backup that walks
/// the data folder at `folder`, an absolute path as the backup walks it. Each
/// also leaves out every name that starts with the file's, which is what a
/// write stages beside it (`hub-key.<id>.tmp`) and a rotation makes
/// (`hub-key.new`). In rustic's globs (gitignore's rules, §6.1) a leading
/// `!` excludes, a leading `/` reads the path from the root of the file
/// system, `*` never crosses a `/`, and the folder's own characters are
/// escaped so none is read as a pattern.
pub fn left_out_globs(folder: &str) -> Vec<String> {
    let folder = escape_glob(folder.trim_end_matches('/'));
    LEFT_OUT_OF_BACKUPS
        .iter()
        .map(|file| format!("!{folder}/{}*", file.file_name()))
        .collect()
}

/// Whether [`left_out_globs`] leave out the file named `name` in the data
/// folder: a name that starts with a left-out file's, in any case, as an
/// `iglob` reads it. The name's bytes, as the file system gives them, so a
/// name that isn't UTF-8 is judged too.
pub fn is_left_out(name: &[u8]) -> bool {
    let _ = name;
    todo!()
}

/// Which seed a seed file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeedFile {
    AgentKey,
    HubKey,
    RecoverySeed,
}

impl SeedFile {
    pub const ALL: [Self; 3] = [Self::AgentKey, Self::HubKey, Self::RecoverySeed];

    pub const fn key_file(self) -> KeyFile {
        match self {
            Self::AgentKey => KeyFile::AgentKey,
            Self::HubKey => KeyFile::HubKey,
            Self::RecoverySeed => KeyFile::RecoverySeed,
        }
    }
}

/// The text of a key file of one line, its line break taken off; `None`
/// when it isn't one line ended by one line break.
pub fn one_line(text: &str) -> Option<&str> {
    let line = text.strip_suffix('\n').unwrap_or(text);
    (!line.contains(['\n', '\r'])).then_some(line)
}

/// An agent's connection counter (§16, decision 120, "A copied data
/// folder"): raised by one and written before each proof, so a copy of the
/// data folder that connects falls behind the original. An agent with no
/// counter file starts at [`ConnectionCounter::NONE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionCounter(u64);

/// Text that isn't a counter file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterRefused;

impl ConnectionCounter {
    pub const NONE: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// The counter one above, or `None` at the most a `u64` holds.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }

    /// The counter from its file: decimal digits on one line, ended by a
    /// line break, at most 20 of them.
    pub fn parse(text: &str) -> Result<Self, CounterRefused> {
        let digits = one_line(text).ok_or(CounterRefused)?;
        if digits.is_empty()
            || digits.len() > MAX_COUNTER_DIGITS
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CounterRefused);
        }
        digits.parse().map(Self).map_err(|_| CounterRefused)
    }

    /// The file's text: the number and a line break.
    pub fn to_file_text(self) -> String {
        format!("{}\n", self.0)
    }
}

/// The digits of `u64::MAX`; a counter file of more holds no counter.
const MAX_COUNTER_DIGITS: usize = 20;

/// The most bytes a TLS key's encoding may have. An ECDSA P-256 key in
/// PKCS#8 has about 140; more is not a key this build made.
pub const MAX_TLS_KEY_BYTES: usize = 4096;

/// The hub's TLS key in built-in mode: an ECDSA P-256 private key in its
/// PKCS#8 encoding (§16, decision 120, "The hub key"). Only its size is
/// checked here; the TLS adapter that makes and loads it checks the rest.
/// Its `Debug` form never shows it, and its bytes are wiped when it is
/// dropped (I4).
#[derive(Clone)]
pub struct TlsKey(Zeroizing<Vec<u8>>);

/// Bytes that can't be a TLS key: none, or more than
/// [`MAX_TLS_KEY_BYTES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsKeyRefused;

impl TlsKey {
    /// The key from its PKCS#8 encoding, which is wiped when it is dropped,
    /// refused or not.
    pub fn from_der(der: Vec<u8>) -> Result<Self, TlsKeyRefused> {
        let der = Zeroizing::new(der);
        if der.is_empty() || der.len() > MAX_TLS_KEY_BYTES {
            return Err(TlsKeyRefused);
        }
        Ok(Self(der))
    }

    /// The key's PKCS#8 encoding, for the TLS adapter and the key file.
    pub fn der(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for TlsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TlsKey(***)")
    }
}

#[cfg(test)]
mod tests;
