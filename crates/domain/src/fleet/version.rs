//! A machine's version of Ladderback: the build its `Hello` names, as
//! `--version` prints it (technical design §7.2; §16, decision 90), which
//! the hub keeps to show on the machine's card, and how it stands against
//! the hub's own (#1707).

use std::cmp::Ordering;

pub use crate::updates::MAX_VERSION_BYTES;
use crate::updates::Version;

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

impl MachineVersion {
    /// `text`, if it is shaped like a version.
    pub fn parse(text: &str) -> Result<Self, InvalidVersion> {
        let shaped = !text.is_empty()
            && text.len() <= MAX_VERSION_BYTES
            && text.bytes().all(|byte| byte.is_ascii_graphic());
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidVersion)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How this version stands against `other` by SemVer precedence, as
    /// the update notice orders releases: the release, then its
    /// pre-release, with build metadata ignored, so two builds of one
    /// release are equal. None when either doesn't read as SemVer.
    pub fn against(&self, other: &Self) -> Option<Ordering> {
        let mine = Version::parse(&self.0).ok()?;
        let theirs = Version::parse(&other.0).ok()?;
        Some(mine.cmp(&theirs))
    }
}

#[cfg(test)]
mod tests;
