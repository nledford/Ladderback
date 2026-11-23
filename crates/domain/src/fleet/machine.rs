//! The machine identifier and how a start compares it (technical design
//! §7.1; §16, decision 144; #296): the identifier the operating system gives
//! the computer, recorded in the data folder beside the machine ID. A data
//! folder copied to another computer brings that computer's machine ID with
//! it, but not its identifier, so the two disagree there. The hub tells the
//! two computers apart by the machine fingerprint each one's `Hello`
//! carries, and takes one of them (§7.2; §16, decision NN).

use std::fmt;

use sha2::{Digest, Sha256};

use super::keys::{KEY_BYTES, decode_hex32, lower_hex};
use super::signed::WireName;
use crate::AgentId;

/// The most bytes a machine identifier has: a Linux `machine-id` is 32 hex
/// digits and a macOS `IOPlatformUUID` 36 characters, so anything longer is
/// not one.
pub const MAX_MACHINE_IDENTIFIER_BYTES: usize = 128;

/// What the operating system calls this computer: `/etc/machine-id` on
/// Linux, `IOPlatformUUID` on macOS. 1 to [`MAX_MACHINE_IDENTIFIER_BYTES`]
/// visible ASCII characters, with the whitespace around them dropped. Two
/// are compared exactly.
#[derive(Clone, PartialEq, Eq)]
pub struct MachineIdentifier(String);

/// Why text isn't a machine identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidMachineIdentifier {
    Empty,
    TooLong,
    /// A character that isn't visible ASCII, inside the identifier.
    Unexpected(char),
}

impl MachineIdentifier {
    pub fn parse(text: &str) -> Result<Self, InvalidMachineIdentifier> {
        let text = text.trim();
        if text.is_empty() {
            return Err(InvalidMachineIdentifier::Empty);
        }
        if text.len() > MAX_MACHINE_IDENTIFIER_BYTES {
            return Err(InvalidMachineIdentifier::TooLong);
        }
        if let Some(unexpected) = text.chars().find(|c| !c.is_ascii_graphic()) {
            return Err(InvalidMachineIdentifier::Unexpected(unexpected));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for MachineIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MachineIdentifier({:?})", self.0)
    }
}

/// How the identifier recorded beside the machine ID compares with the one
/// the operating system gives this computer now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierMatch {
    /// They are one: the data folder is on the computer it was made on.
    Same,
    /// None is recorded: a data folder from before the identifier was
    /// kept. This computer's is recorded, as an upgrade, never as a copy.
    Unrecorded,
    /// They differ: the data folder came from another computer, by a copy,
    /// a restored backup or a cloned disk. The machine keeps backing up and
    /// stops maintaining until the person takes a new machine ID.
    Differs,
    /// This computer's identifier couldn't be read, so nothing can be told:
    /// the machine runs as usual, and its log says so.
    Unread,
}

impl IdentifierMatch {
    /// Compares `recorded`, the identifier beside the machine ID, with
    /// `current`, the one the operating system gives now (`None` when it
    /// couldn't be read).
    pub fn of(recorded: Option<&MachineIdentifier>, current: Option<&MachineIdentifier>) -> Self {
        match (recorded, current) {
            (_, None) => Self::Unread,
            (None, Some(_)) => Self::Unrecorded,
            (Some(recorded), Some(current)) if recorded == current => Self::Same,
            (Some(_), Some(_)) => Self::Differs,
        }
    }

    /// Whether the machine maintains the repositories its configuration
    /// asks it to: everywhere but in a copied data folder (I1, I6).
    pub fn maintains(self) -> bool {
        self != Self::Differs
    }
}

/// What a machine's `Hello` tells its hub of the computer it runs on: the
/// SHA-256 digest of a label, the machine ID and the machine identifier,
/// never the identifier itself (decision 144 keeps it on the machine). Two
/// computers' fingerprints differ; one computer's under two machine IDs
/// differ too, so the hub can't match one machine to another by it. On the
/// wire and in the hub's database it is 64 lower-case hex digits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MachineFingerprint([u8; KEY_BYTES]);

/// Text that isn't 64 lower-case hex digits, so no machine fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMachineFingerprint;

/// The label hashed first, so the digest of an identifier under a machine
/// ID is never one another use of SHA-256 makes.
const FINGERPRINT_LABEL: &[u8] = b"ladderback machine fingerprint v1\0";

impl MachineFingerprint {
    /// The fingerprint of the computer whose identifier is `identifier`,
    /// running as `agent_id`.
    pub fn of(agent_id: AgentId, identifier: &MachineIdentifier) -> Self {
        let _ = (agent_id, identifier, FINGERPRINT_LABEL, Sha256::new());
        todo!()
    }

    /// The fingerprint 64 lower-case hex digits spell.
    pub fn from_hex(text: &str) -> Result<Self, InvalidMachineFingerprint> {
        let mut bytes = [0; KEY_BYTES];
        let _ = (text, decode_hex32("", &mut bytes));
        todo!()
    }

    /// The fingerprint as 64 lower-case hex digits.
    pub fn to_hex(&self) -> String {
        let _ = lower_hex(&self.0);
        todo!()
    }
}

impl fmt::Debug for MachineFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MachineFingerprint({})", lower_hex(&self.0))
    }
}

/// What the hub makes of the machine fingerprint a `Hello` carries, beside
/// the one it holds under the machine's ID (decision NN).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineCheck {
    /// The hub held none, and keeps this one: the first machine it hears
    /// under the ID is the one it takes from then on.
    Claimed,
    /// The fingerprint the hub holds: the same computer, connecting again.
    Same,
    /// The `Hello` carried none, from a build before fingerprints or a
    /// computer whose identifier can't be read: nothing can be told, so the
    /// machine is taken and nothing is kept, as decision 144 stops nothing
    /// for an identifier it can't read.
    Untold,
    /// Another fingerprint than the hub holds: a second computer under the
    /// ID, as a copied data folder makes. It is refused, and the hub keeps
    /// when, for the person (I1, I6).
    Second,
}

impl MachineCheck {
    /// The code of the refusal a [`MachineCheck::Second`] gets.
    pub fn refusal() -> WireName {
        todo!()
    }

    /// Compares `presented`, the fingerprint the `Hello` carried, with
    /// `held`, the one the hub holds under the machine's ID.
    pub fn of(held: Option<&MachineFingerprint>, presented: Option<&MachineFingerprint>) -> Self {
        let _ = (held, presented);
        todo!()
    }

    /// Whether the hub takes the machine: every check but a second
    /// computer's.
    pub fn takes(self) -> bool {
        todo!()
    }
}

/// An agent under whose ID the hub refused a second computer, and when it
/// last did, which the hub's list of machines shows (decision NN).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondMachine {
    pub agent_id: AgentId,
    pub at: jiff::Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> MachineIdentifier {
        MachineIdentifier::parse(text).unwrap()
    }

    #[test]
    fn a_linux_machine_id_and_a_macos_platform_uuid_are_identifiers() {
        assert_eq!(
            id("0123456789abcdef0123456789abcdef\n").as_str(),
            "0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            id("  9F1C2B3A-1111-2222-3333-444455556666 ").as_str(),
            "9F1C2B3A-1111-2222-3333-444455556666"
        );
    }

    #[test]
    fn empty_long_or_hidden_text_is_no_identifier() {
        assert_eq!(
            MachineIdentifier::parse(" \n"),
            Err(InvalidMachineIdentifier::Empty)
        );
        let longest = "a".repeat(MAX_MACHINE_IDENTIFIER_BYTES);
        assert_eq!(id(&longest).as_str(), longest);
        assert_eq!(
            MachineIdentifier::parse(&format!("{longest}a")),
            Err(InvalidMachineIdentifier::TooLong)
        );
        assert_eq!(
            MachineIdentifier::parse("ab cd"),
            Err(InvalidMachineIdentifier::Unexpected(' '))
        );
        assert_eq!(
            MachineIdentifier::parse("ab\u{200B}cd"),
            Err(InvalidMachineIdentifier::Unexpected('\u{200B}'))
        );
    }

    #[test]
    fn an_identifier_compares_with_the_recorded_one() {
        let here = id("here");
        let there = id("there");
        assert_eq!(
            IdentifierMatch::of(Some(&here), Some(&here)),
            IdentifierMatch::Same
        );
        assert_eq!(
            IdentifierMatch::of(Some(&there), Some(&here)),
            IdentifierMatch::Differs
        );
        assert_eq!(
            IdentifierMatch::of(None, Some(&here)),
            IdentifierMatch::Unrecorded
        );
        assert_eq!(
            IdentifierMatch::of(Some(&here), None),
            IdentifierMatch::Unread
        );
        assert_eq!(IdentifierMatch::of(None, None), IdentifierMatch::Unread);
    }

    #[test]
    fn only_a_copied_data_folder_stops_maintaining() {
        assert!(IdentifierMatch::Same.maintains());
        assert!(IdentifierMatch::Unrecorded.maintains());
        assert!(IdentifierMatch::Unread.maintains());
        assert!(!IdentifierMatch::Differs.maintains());
    }

    fn agent(n: u128) -> AgentId {
        AgentId::from_uuid(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn a_fingerprint_is_the_digest_of_the_label_the_machine_id_and_the_identifier() {
        let identifier = id("0123456789abcdef0123456789abcdef");
        let mut digest = Sha256::new();
        digest.update(b"ladderback machine fingerprint v1\0");
        digest.update(agent(7).as_uuid().as_bytes());
        digest.update(identifier.as_str().as_bytes());
        let expected: [u8; KEY_BYTES] = digest.finalize().into();
        let fingerprint = MachineFingerprint::of(agent(7), &identifier);
        assert_eq!(fingerprint, MachineFingerprint(expected));
        assert_eq!(fingerprint, MachineFingerprint::of(agent(7), &identifier));
        assert_ne!(fingerprint, MachineFingerprint::of(agent(8), &identifier));
        assert_ne!(
            fingerprint,
            MachineFingerprint::of(agent(7), &id("0123456789abcdef0123456789abcdee"))
        );
    }

    #[test]
    fn a_fingerprint_is_spelt_and_read_as_64_lower_case_hex_digits() {
        let fingerprint = MachineFingerprint([0xa5; KEY_BYTES]);
        let text = fingerprint.to_hex();
        assert_eq!(text, "a5".repeat(KEY_BYTES));
        assert_eq!(MachineFingerprint::from_hex(&text), Ok(fingerprint));
        for wrong in [
            "A5".repeat(KEY_BYTES),
            "a5".repeat(KEY_BYTES - 1),
            format!("{}a", "a5".repeat(KEY_BYTES)),
            String::new(),
        ] {
            assert_eq!(
                MachineFingerprint::from_hex(&wrong),
                Err(InvalidMachineFingerprint),
                "{wrong}"
            );
        }
    }

    #[test]
    fn a_fingerprint_debugs_as_its_hex_digits_never_as_the_identifier() {
        // I4's spirit (decision 150): a log line or a panic that prints a
        // fingerprint never gives the machine identifier back.
        let identifier = id("0123456789abcdef0123456789abcdef");
        let fingerprint = MachineFingerprint::of(agent(7), &identifier);
        let shown = format!("{fingerprint:?}");
        assert_eq!(
            shown,
            format!("MachineFingerprint({})", fingerprint.to_hex())
        );
        assert!(!shown.contains(identifier.as_str()), "{shown}");
        assert_eq!(
            format!("{:?}", MachineFingerprint([0xa5; KEY_BYTES])),
            format!("MachineFingerprint({})", "a5".repeat(KEY_BYTES))
        );
    }

    #[test]
    fn the_hub_takes_the_first_fingerprint_the_same_again_and_none_but_refuses_another() {
        let first = MachineFingerprint([1; KEY_BYTES]);
        let second = MachineFingerprint([2; KEY_BYTES]);
        assert_eq!(MachineCheck::of(None, Some(&first)), MachineCheck::Claimed);
        assert_eq!(
            MachineCheck::of(Some(&first), Some(&first)),
            MachineCheck::Same
        );
        assert_eq!(
            MachineCheck::of(Some(&first), Some(&second)),
            MachineCheck::Second
        );
        assert_eq!(MachineCheck::of(Some(&first), None), MachineCheck::Untold);
        assert_eq!(MachineCheck::of(None, None), MachineCheck::Untold);
        assert!(MachineCheck::Claimed.takes());
        assert!(MachineCheck::Same.takes());
        assert!(MachineCheck::Untold.takes());
        assert!(!MachineCheck::Second.takes());
        assert_eq!(MachineCheck::refusal().as_str(), "identity-conflict");
    }
}
