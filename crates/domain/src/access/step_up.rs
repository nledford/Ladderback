//! Step-up confirmation (technical design §10.4): what a person gives again,
//! with a session or without one, before a restore or a download starts or
//! a proposal is approved.

use crate::SnapshotId;
use crate::access::{Password, PasswordProblem};
use crate::planning::{ProposalId, short_proposal_id};

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
    pub fn of(password_sign_in: bool) -> Self {
        if password_sign_in {
            Self::Password
        } else {
            Self::ShortId
        }
    }
}

/// What a step-up confirmation is for, whose ID confirms it with sign-in
/// off.
#[derive(Clone, Copy, Debug)]
pub enum StepUpFor<'a> {
    /// A restore or a download of an entry of the snapshot.
    Snapshot(&'a SnapshotId),
    /// The approval of the proposal.
    Proposal(&'a ProposalId),
}

/// A step-up confirmation as the person typed it: the admin password with
/// sign-in on, the short or whole ID of what it is for otherwise. It may be
/// the admin password, so it is never shown: its `Debug` hides it (I4), it
/// can't be copied, and it wipes its bytes when it is dropped, as does a
/// text [`Self::parse`] refuses (§10.2).
pub struct StepUpConfirmation(Password);

impl StepUpConfirmation {
    /// A confirmation: anything from one byte to
    /// [`super::PASSWORD_MAX_BYTES`], as an attempt at the password is.
    /// Nothing typed, or more than any password, confirms nothing.
    pub fn parse(text: String) -> Result<Self, PasswordProblem> {
        Password::parse(text).map(Self)
    }

    /// The confirmation as an attempt at the admin password, for the check
    /// that hashes it.
    pub fn as_password(&self) -> &Password {
        &self.0
    }

    /// Whether it names `what`: its short ID or its whole ID, in either
    /// case, with the spaces around it dropped, as a paste brings them.
    pub fn names(&self, what: StepUpFor<'_>) -> bool {
        let (short, whole) = match what {
            StepUpFor::Snapshot(snapshot) => (short_id(snapshot), snapshot.as_str()),
            StepUpFor::Proposal(id) => (short_proposal_id(id), id.as_str()),
        };
        let typed = self.0.expose().trim();
        typed.eq_ignore_ascii_case(short) || typed.eq_ignore_ascii_case(whole)
    }
}

impl std::fmt::Debug for StepUpConfirmation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StepUpConfirmation([hidden])")
    }
}

/// The snapshot's short ID.
pub fn short_id(snapshot: &SnapshotId) -> &str {
    snapshot
        .as_str()
        .get(..SHORT_ID_LEN)
        .unwrap_or(snapshot.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{PASSWORD_MAX_BYTES, TOKEN_BYTES};
    use crate::planning::SHORT_PROPOSAL_ID_CHARS;

    fn snapshot() -> SnapshotId {
        SnapshotId::parse(&format!("3f2a1b4c{}", "d".repeat(56))).expect("a snapshot ID")
    }

    fn proposal() -> ProposalId {
        ProposalId::from_bytes(&[7; TOKEN_BYTES])
    }

    fn typed(text: &str) -> StepUpConfirmation {
        StepUpConfirmation::parse(text.to_owned()).expect("a confirmation")
    }

    #[test]
    fn only_password_sign_in_asks_for_the_password() {
        assert_eq!(StepUpMethod::of(true), StepUpMethod::Password);
        assert_eq!(StepUpMethod::of(false), StepUpMethod::ShortId);
    }

    #[test]
    fn a_confirmation_is_one_byte_to_the_longest_password() {
        assert_eq!(
            StepUpConfirmation::parse(String::new()).map(|_| ()),
            Err(PasswordProblem::Empty)
        );
        assert_eq!(
            StepUpConfirmation::parse("a".repeat(PASSWORD_MAX_BYTES + 1)).map(|_| ()),
            Err(PasswordProblem::TooLong)
        );
        for text in ["a".to_owned(), "a".repeat(PASSWORD_MAX_BYTES)] {
            let confirmation = StepUpConfirmation::parse(text.clone()).expect("a confirmation");
            assert_eq!(confirmation.as_password().expose(), text);
        }
    }

    #[test]
    fn a_confirmation_never_shows_what_was_typed() {
        let confirmation = typed("canary-admin-password");
        assert_eq!(format!("{confirmation:?}"), "StepUpConfirmation([hidden])");
        assert_eq!(format!("{confirmation:#?}"), "StepUpConfirmation([hidden])");
    }

    #[test]
    fn the_short_id_or_the_whole_id_confirms_in_either_case_with_spaces_around() {
        let snapshot = snapshot();
        for text in [
            "3f2a1b4c",
            "3F2A1B4C",
            " 3f2a1b4c\n",
            "\t3f2a1b4c ",
            snapshot.as_str(),
            &snapshot.as_str().to_ascii_uppercase(),
        ] {
            assert!(
                typed(text).names(StepUpFor::Snapshot(&snapshot)),
                "{text:?} confirms"
            );
        }
    }

    #[test]
    fn anything_else_doesnt_confirm() {
        let snapshot = snapshot();
        for text in [
            "   ",
            "3f2a1b4d",
            "3f2a1b4",
            "3f2a1b4cd",
            "3f2a 1b4c",
            "3f2a1b4c\u{0}",
            "３f2a1b4c",
            &format!("{}e", snapshot.as_str().get(..63).expect("64 digits")),
        ] {
            assert!(
                !typed(text).names(StepUpFor::Snapshot(&snapshot)),
                "{text:?} doesn't"
            );
        }
    }

    #[test]
    fn a_proposal_is_confirmed_by_its_short_or_whole_id_not_another_s() {
        let id = proposal();
        let short = short_proposal_id(&id);
        assert_eq!(short.len(), SHORT_PROPOSAL_ID_CHARS);
        assert!(id.as_str().starts_with(short));
        for text in [&format!(" {} ", short.to_uppercase()), id.as_str()] {
            assert!(typed(text).names(StepUpFor::Proposal(&id)), "{text:?}");
        }
        let other = ProposalId::from_bytes(&[9; TOKEN_BYTES]);
        for text in [
            short_proposal_id(&other),
            id.as_str().get(..5).expect("5 characters"),
        ] {
            assert!(!typed(text).names(StepUpFor::Proposal(&id)), "{text:?}");
        }
    }

    #[test]
    fn the_short_id_is_the_first_8_digits() {
        assert_eq!(short_id(&snapshot()), "3f2a1b4c");
    }
}
