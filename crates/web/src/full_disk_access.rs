//! What the Full Disk Access warning, a banner on every page with the steps
//! on the start page, shows and says (§4.4, §16 decision 115), decided on
//! the host; `full_disk_access_warning.rs` draws it.

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

/// The `id` of the Full Disk Access banner, which each page's heading names
/// in `aria-describedby` while it shows.
pub(crate) const BANNER_ID: &str = "full-disk-access-banner";

/// What describes each page's heading, the IDs space-separated: the notices
/// above every page, read after the heading that takes focus when a reader
/// navigates, so neither is passed over (A3). `network_notice` is the ID of
/// the notice that sign-in is off on the network, while it shows; the
/// banner's is there while the agent `found` the grant missing. `None` when
/// neither shows.
pub(crate) fn describing_the_heading(
    network_notice: Option<&'static str>,
    found: Option<&FullDiskAccessDto>,
) -> Option<String> {
    let ids: Vec<&str> = network_notice
        .into_iter()
        .chain(warns(found).then_some(BANNER_ID))
        .collect();
    (!ids.is_empty()).then(|| ids.join(" "))
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

    #[test]
    fn the_heading_is_described_by_each_notice_above_every_page() {
        let missing = found(FullDiskAccessStateDto::Missing, false);
        assert_eq!(
            describing_the_heading(Some("network"), Some(&missing)).as_deref(),
            Some("network full-disk-access-banner")
        );
        assert_eq!(
            describing_the_heading(None, Some(&missing)).as_deref(),
            Some(BANNER_ID)
        );
    }

    #[test]
    fn a_grant_not_found_missing_leaves_the_heading_to_the_network_notice() {
        for state in [
            FullDiskAccessStateDto::Granted,
            FullDiskAccessStateDto::Unknown,
            FullDiskAccessStateDto::NotApplicable,
        ] {
            let found = found(state, true);
            assert_eq!(
                describing_the_heading(Some("network"), Some(&found)).as_deref(),
                Some("network"),
                "{state:?}"
            );
            assert_eq!(
                describing_the_heading(None, Some(&found)),
                None,
                "{state:?}"
            );
        }
        assert_eq!(describing_the_heading(None, None), None);
        assert_eq!(
            describing_the_heading(Some("network"), None).as_deref(),
            Some("network")
        );
    }
}
