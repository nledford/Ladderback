//! Which location of a repository a run used, and why each earlier one was
//! skipped (technical design §9, decision 80). Positions count from 1, in
//! the order the entry writes its locations. Nothing here holds a network's
//! name or a credential: an address is kept as the run knew it, masked.

use serde::{Deserialize, Serialize};

use super::Warning;

/// What became of one location in a run's choice: one code each, never a
/// sentence (§8.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationOutcome {
    /// The run used it.
    Used,
    /// A location after the one used: the choice never reached it.
    NotTried,
    /// Its rule doesn't hold on the network the machine is on.
    RuleNotMet,
    /// Its rule couldn't be evaluated, so it doesn't hold.
    RuleUnknown,
    /// No answer by the probe's bound, a refused connection, no route, or a
    /// name that doesn't resolve.
    Silent,
    /// It answered and refused the credentials.
    Refused,
    /// It answers, and no repository's `config` is there.
    NotARepository,
    /// It holds a repository whose ID isn't the entry's.
    OtherRepository,
    /// The repository's password opens nothing there.
    PasswordNotAccepted,
    /// A reference of this location didn't resolve.
    SecretUnavailable,
}

impl LocationOutcome {
    /// The outcome's stored code, as `locations_json` and the warning carry
    /// it.
    pub fn code(self) -> &'static str {
        todo!()
    }

    /// Whether a skip with this outcome is a warning (`location-skipped`):
    /// always for another repository and an unavailable secret, which are
    /// wrong on any network; for any skip of a location whose rule held,
    /// since the person said it should work there; and never otherwise, as
    /// an address with no rule may be someone else's on this network.
    pub fn warns(self, rule_held: bool) -> bool {
        let _ = rule_held;
        todo!()
    }
}

/// One location of a run's choice, as the run saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationTried {
    /// From 1, in the order the entry writes its locations.
    pub position: u8,
    /// The address as the entry writes it, credentials masked, cut at
    /// [`MAX_RECORDED_ADDRESS_BYTES`](crate::planning::MAX_RECORDED_ADDRESS_BYTES).
    pub address: String,
    pub outcome: LocationOutcome,
    /// For [`LocationOutcome::SecretUnavailable`], where the secret was
    /// looked for (`env LAN_KEY`, `file /etc/…`), never a value.
    pub source: Option<String>,
}

/// Every location of the entry as a run saw it, in order, the ones the
/// choice never reached included.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationsRecord {
    pub locations: Vec<LocationTried>,
}

impl LocationsRecord {
    /// The position of the location the run used, if it used one.
    pub fn used(&self) -> Option<u8> {
        todo!()
    }

    /// The location at `position`.
    pub fn at(&self, position: u8) -> Option<&LocationTried> {
        let _ = position;
        todo!()
    }

    /// The operation's warnings this choice gives: one `location-skipped`
    /// for each skip that [warns](LocationOutcome::warns), where `rule_held`
    /// says, by position, whether that location's rule held.
    pub fn warnings(&self, rule_held: impl Fn(u8) -> bool) -> Vec<Warning> {
        let _ = rule_held;
        todo!()
    }
}

/// `address`, already masked, cut to at most `max` bytes on a character
/// boundary.
pub fn recorded_address(address: &str, max: usize) -> String {
    let _ = (address, max);
    todo!()
}
