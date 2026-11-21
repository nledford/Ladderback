//! The machine zone: the IANA time zone a machine runs in, which it reports
//! to its hub when it pairs and at each connection (technical design §7.1,
//! §7.2; #114). The hub keeps it only to show the machine's times.

use std::fmt;

use jiff::tz::{TimeZone, TimeZoneDatabase};

/// The longest machine zone name, in bytes; the longest in the database has
/// 32.
pub const MAX_ZONE_BYTES: usize = 64;

/// The IANA time-zone name a machine runs in, such as `America/Chicago`:
/// up to [`MAX_ZONE_BYTES`] of `A-Z`, `a-z`, `0-9`, `_`, `+` and `-`, in
/// non-empty segments joined by `/`, the shape every name in the database
/// has. Whether a database holds the name is for whoever looks it up: the
/// hub keeps only a zone its own copy knows ([`MachineZone::known_in`]),
/// and holds any other as unknown rather than guessing one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineZone(String);

/// The text isn't shaped like an IANA time-zone name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidZoneName;

impl fmt::Display for InvalidZoneName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not an IANA time-zone name")
    }
}

impl std::error::Error for InvalidZoneName {}

impl MachineZone {
    /// The name `zone` has in the database it came from; none for a zone
    /// with no such name: a fixed offset, a POSIX TZ string, or the
    /// database's "unknown" when the system's zone couldn't be found.
    pub fn of(zone: &TimeZone) -> Option<Self> {
        zone.iana_name().and_then(|name| Self::parse(name).ok())
    }

    /// `name`, if it has the shape of an IANA time-zone name.
    pub fn parse(name: &str) -> Result<Self, InvalidZoneName> {
        let shaped = !name.is_empty()
            && name.len() <= MAX_ZONE_BYTES
            && name.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
            });
        if shaped {
            Ok(Self(name.to_owned()))
        } else {
            Err(InvalidZoneName)
        }
    }

    /// This zone as `database` spells it, when `database` holds it.
    pub fn known_in(&self, database: &TimeZoneDatabase) -> Option<Self> {
        database.get(&self.0).ok().as_ref().and_then(Self::of)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
