//! A machine's version of Ladderback: the build its `Hello` names, as
//! `--version` prints it (technical design §7.2; §16, decision 90), which
//! the hub keeps to show on the machine's card, and how it stands against
//! the hub's own (#1707).

use std::cmp::Ordering;
use std::fmt;

/// The longest machine version, in bytes. A build's version is a SemVer
/// version with a short commit after it, a few dozen bytes at most.
pub const MAX_VERSION_BYTES: usize = 64;

/// The version of Ladderback a machine runs, as its `Hello` wrote it:
/// `0.6.0`, or `0.6.0+3f937cf` between releases. Up to
/// [`MAX_VERSION_BYTES`] of printable ASCII, no spaces; whether it reads as
/// SemVer is for [`MachineVersion::against`] to find, and a version that
/// doesn't is still shown as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineVersion(String);

/// The text isn't shaped like a version: empty, too long, or holding a
/// space or a character that isn't printable ASCII.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidVersion;

impl fmt::Display for InvalidVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not shaped like a version")
    }
}

impl std::error::Error for InvalidVersion {}

impl MachineVersion {
    /// `text`, if it is shaped like a version.
    pub fn parse(text: &str) -> Result<Self, InvalidVersion> {
        let _ = text;
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How this version stands against `other` by SemVer precedence: the
    /// release, then its pre-release, with build metadata ignored, so two
    /// builds of one release are equal. None when either doesn't read as
    /// SemVer.
    pub fn against(&self, other: &Self) -> Option<Ordering> {
        let _ = other;
        todo!()
    }
}

#[cfg(test)]
mod tests;
