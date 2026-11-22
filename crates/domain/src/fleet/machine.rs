//! The machine identifier and how a start compares it (technical design
//! §7.1; §16, decision NN; #296): the identifier the operating system gives
//! the computer, recorded in the data folder beside the machine ID. A data
//! folder copied to another computer brings that computer's machine ID with
//! it, but not its identifier, so the two disagree there.

use std::fmt;

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
        let _ = text;
        todo!()
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
        let _ = (recorded, current);
        todo!()
    }

    /// Whether the machine maintains the repositories its configuration
    /// asks it to: everywhere but in a copied data folder (I1, I6).
    pub fn maintains(self) -> bool {
        todo!()
    }
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
}
