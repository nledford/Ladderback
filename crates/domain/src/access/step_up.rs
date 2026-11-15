//! Step-up confirmation (technical design §10.4): what a person gives again,
//! with a session or without one, before a restore or a download starts.

use crate::SnapshotId;

/// How long a snapshot's short ID is: the first 8 hex digits of its ID, as
/// rustic shows it.
pub const SHORT_ID_LEN: usize = 8;

/// What step-up confirmation asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepUpMethod {
    /// The admin password, entered again even with a live session.
    Password,
    /// The snapshot's short ID, typed or pasted: it guards against a
    /// misclick, not against someone at the keyboard.
    ShortId,
}

impl StepUpMethod {
    /// The method while sign-in by the admin password is on, or not. Only
    /// that sign-in has a password to enter again: with sign-in off, or
    /// behind a proxy that signs people in by a header, the short ID.
    pub fn of(_password_sign_in: bool) -> Self {
        todo!("StepUpMethod::of")
    }
}

/// Whether `typed` confirms `snapshot`: its short ID or its whole ID, in
/// either case, with the spaces around it dropped, as a paste brings them.
pub fn confirms_snapshot(_typed: &str, _snapshot: &SnapshotId) -> bool {
    todo!("confirms_snapshot")
}

/// The snapshot's short ID.
pub fn short_id(snapshot: &SnapshotId) -> &str {
    let _ = snapshot;
    todo!("short_id")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> SnapshotId {
        SnapshotId::parse(&format!("3f2a1b4c{}", "d".repeat(56))).expect("a snapshot ID")
    }

    #[test]
    fn only_password_sign_in_asks_for_the_password() {
        assert_eq!(StepUpMethod::of(true), StepUpMethod::Password);
        assert_eq!(StepUpMethod::of(false), StepUpMethod::ShortId);
    }

    #[test]
    fn the_short_id_or_the_whole_id_confirms_in_either_case_with_spaces_around() {
        let snapshot = snapshot();
        for typed in [
            "3f2a1b4c",
            "3F2A1B4C",
            " 3f2a1b4c\n",
            "\t3f2a1b4c ",
            snapshot.as_str(),
            &snapshot.as_str().to_ascii_uppercase(),
        ] {
            assert!(confirms_snapshot(typed, &snapshot), "{typed:?} confirms");
        }
    }

    #[test]
    fn anything_else_doesnt_confirm() {
        let snapshot = snapshot();
        for typed in [
            "",
            "   ",
            "3f2a1b4d",
            "3f2a1b4",
            "3f2a1b4cd",
            "3f2a 1b4c",
            "3f2a1b4c\u{0}",
            "３f2a1b4c",
            &format!("{}e", snapshot.as_str().get(..63).expect("64 digits")),
        ] {
            assert!(!confirms_snapshot(typed, &snapshot), "{typed:?} doesn't");
        }
    }

    #[test]
    fn the_short_id_is_the_first_8_digits() {
        assert_eq!(short_id(&snapshot()), "3f2a1b4c");
    }
}
