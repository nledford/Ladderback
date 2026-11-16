//! What the start page's Full Disk Access warning shows and says (§4.4, §16
//! decision NN), decided on the host; `full_disk_access_warning.rs` draws it.

use api::{FullDiskAccessDto, FullDiskAccessStateDto};
use l10n::{MessageId, msg};

/// The steps that turn Full Disk Access on, in order: shown to every
/// browser, with the button that opens System Settings or without it.
pub(crate) const STEPS: [MessageId; 4] = [
    msg::UI_FULL_DISK_ACCESS_STEP_OPEN,
    msg::UI_FULL_DISK_ACCESS_STEP_PANE,
    msg::UI_FULL_DISK_ACCESS_STEP_TURN_ON,
    msg::UI_FULL_DISK_ACCESS_STEP_CHECK,
];

/// Whether the page warns, given what the agent `found`: only while it found
/// the grant missing. Granted, unknown, an agent not on macOS and no answer
/// yet show nothing: a false alarm would send a person to System Settings
/// for nothing.
pub(crate) fn warns(found: Option<&FullDiskAccessDto>) -> bool {
    found.is_some_and(|found| found.state == FullDiskAccessStateDto::Missing)
}

/// Whether the warning offers the button that opens System Settings: only
/// when the agent says this browser is on its own Mac.
pub(crate) fn opens_settings(found: Option<&FullDiskAccessDto>) -> bool {
    found.is_some_and(|found| found.opens_settings)
}

/// What the page says once Check again is answered with `state`: the
/// warning stays, or why it went.
pub(crate) fn said_after_check(state: FullDiskAccessStateDto) -> MessageId {
    match state {
        FullDiskAccessStateDto::Missing => msg::UI_FULL_DISK_ACCESS_STILL_MISSING,
        FullDiskAccessStateDto::Granted => msg::UI_FULL_DISK_ACCESS_GRANTED,
        FullDiskAccessStateDto::Unknown | FullDiskAccessStateDto::NotApplicable => {
            msg::UI_FULL_DISK_ACCESS_UNKNOWN
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(state: FullDiskAccessStateDto, opens_settings: bool) -> FullDiskAccessDto {
        FullDiskAccessDto {
            state,
            opens_settings,
        }
    }

    #[test]
    fn only_a_grant_found_missing_is_a_warning() {
        assert!(warns(Some(&found(FullDiskAccessStateDto::Missing, false))));
        for state in [
            FullDiskAccessStateDto::Granted,
            FullDiskAccessStateDto::Unknown,
            FullDiskAccessStateDto::NotApplicable,
        ] {
            assert!(!warns(Some(&found(state, true))), "{state:?}");
        }
        assert!(!warns(None));
    }

    #[test]
    fn the_settings_button_is_offered_only_where_the_agent_says_it_opens() {
        let missing = FullDiskAccessStateDto::Missing;
        assert!(opens_settings(Some(&found(missing, true))));
        assert!(!opens_settings(Some(&found(missing, false))));
        assert!(!opens_settings(None));
    }

    #[test]
    fn check_again_says_whether_the_warning_stays_and_why_it_went() {
        assert_eq!(
            said_after_check(FullDiskAccessStateDto::Missing),
            msg::UI_FULL_DISK_ACCESS_STILL_MISSING
        );
        assert_eq!(
            said_after_check(FullDiskAccessStateDto::Granted),
            msg::UI_FULL_DISK_ACCESS_GRANTED
        );
        for state in [
            FullDiskAccessStateDto::Unknown,
            FullDiskAccessStateDto::NotApplicable,
        ] {
            assert_eq!(said_after_check(state), msg::UI_FULL_DISK_ACCESS_UNKNOWN);
        }
    }
}
