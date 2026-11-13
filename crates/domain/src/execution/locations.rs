//! Which location of a repository a run used, and why each earlier one was
//! skipped (technical design §9, decision 80). Positions count from 1, in
//! the order the entry writes its locations. Nothing here holds a network's
//! name or a credential: an address is kept as the run knew it, masked.

use serde::{Deserialize, Serialize};

use super::{Warning, WarningKind};

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
    /// A setting of this location isn't a valid template. The skip's code is
    /// the refusal's, `template-invalid`, as for a repository of one
    /// location.
    TemplateInvalid,
    /// A path of this location, resolved, is relative
    /// (`repository-path-relative`).
    PathRelative,
    /// A setting of this location expands to more than a setting holds
    /// (`secret-expansion-too-long`).
    ExpansionTooLong,
    /// A setting of this location that becomes a process's argument holds a
    /// reference (`secret-in-process-arguments`).
    ReferenceInProcessArguments,
    /// An rclone connection string of this location holds more credentials
    /// than a job masks (`location-too-many-credentials`).
    TooManyCredentials,
}

impl LocationOutcome {
    /// The outcome's stored code, as `locations_json` and the warning carry
    /// it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Used => "used",
            Self::NotTried => "not-tried",
            Self::RuleNotMet => "rule-not-met",
            Self::RuleUnknown => "rule-unknown",
            Self::Silent => "silent",
            Self::Refused => "refused",
            Self::NotARepository => "not-a-repository",
            Self::OtherRepository => "other-repository",
            Self::PasswordNotAccepted => "password-not-accepted",
            Self::SecretUnavailable => "secret-unavailable",
            Self::TemplateInvalid => "template-invalid",
            Self::PathRelative => "repository-path-relative",
            Self::ExpansionTooLong => "secret-expansion-too-long",
            Self::ReferenceInProcessArguments => "secret-in-process-arguments",
            Self::TooManyCredentials => "location-too-many-credentials",
        }
    }

    /// Whether a skip with this outcome is a warning (`location-skipped`):
    /// always for another repository, an unavailable secret and settings
    /// that can't be used, which are wrong on any network; for any skip of a location whose rule held,
    /// since the person said it should work there; and never otherwise, as
    /// an address with no rule may be someone else's on this network.
    pub fn warns(self, rule_held: bool) -> bool {
        match self {
            Self::OtherRepository
            | Self::SecretUnavailable
            | Self::TemplateInvalid
            | Self::PathRelative
            | Self::ExpansionTooLong
            | Self::ReferenceInProcessArguments
            | Self::TooManyCredentials => true,
            Self::Silent | Self::Refused | Self::NotARepository | Self::PasswordNotAccepted => {
                rule_held
            }
            Self::Used | Self::NotTried | Self::RuleNotMet | Self::RuleUnknown => false,
        }
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
    /// looked for (`env LAN_KEY`, `file /etc/…`), never a value; for a
    /// setting that can't be used, the setting (`repository`,
    /// `options.http-cacert`).
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
        self.locations
            .iter()
            .find(|tried| tried.outcome == LocationOutcome::Used)
            .map(|tried| tried.position)
    }

    /// The location at `position`.
    pub fn at(&self, position: u8) -> Option<&LocationTried> {
        self.locations
            .iter()
            .find(|tried| tried.position == position)
    }

    /// The operation's warnings this choice gives: one `location-skipped`
    /// for each skip that [warns](LocationOutcome::warns), where `rule_held`
    /// says, by position, whether that location's rule held.
    pub fn warnings(&self, rule_held: impl Fn(u8) -> bool) -> Vec<Warning> {
        self.locations
            .iter()
            .filter(|tried| tried.outcome.warns(rule_held(tried.position)))
            .map(|tried| Warning {
                kind: WarningKind::LocationSkipped,
                item: Some(tried.position.to_string()),
                during: "location".to_owned(),
                detail: tried.outcome.code().to_owned(),
            })
            .collect()
    }
}

/// `address`, already masked, cut to at most `max` bytes on a character
/// boundary.
pub fn recorded_address(address: &str, max: usize) -> String {
    let end = address.floor_char_boundary(max);
    address.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tried(position: u8, outcome: LocationOutcome) -> LocationTried {
        LocationTried {
            position,
            address: format!("/srv/{position}"),
            outcome,
            source: None,
        }
    }

    #[test]
    fn every_outcome_has_its_own_code() {
        use LocationOutcome::*;
        let all = [
            Used,
            NotTried,
            RuleNotMet,
            RuleUnknown,
            Silent,
            Refused,
            NotARepository,
            OtherRepository,
            PasswordNotAccepted,
            SecretUnavailable,
            TemplateInvalid,
            PathRelative,
            ExpansionTooLong,
            ReferenceInProcessArguments,
            TooManyCredentials,
        ];
        let codes: std::collections::BTreeSet<&str> = all.iter().map(|o| o.code()).collect();
        assert_eq!(codes.len(), all.len());
        assert_eq!(NotARepository.code(), "not-a-repository");
        // A setting's refusal keeps the code a repository of one location
        // is refused with.
        assert_eq!(PathRelative.code(), "repository-path-relative");
        assert_eq!(TemplateInvalid.code(), "template-invalid");
    }

    #[test]
    fn a_skip_warns_when_it_is_wrong_on_any_network_or_its_rule_held() {
        use LocationOutcome::*;
        for outcome in [
            OtherRepository,
            SecretUnavailable,
            TemplateInvalid,
            PathRelative,
            ExpansionTooLong,
            ReferenceInProcessArguments,
            TooManyCredentials,
        ] {
            assert!(outcome.warns(false), "{outcome:?}");
            assert!(outcome.warns(true), "{outcome:?}");
        }
        for outcome in [Silent, Refused, NotARepository, PasswordNotAccepted] {
            assert!(!outcome.warns(false), "{outcome:?}");
            assert!(outcome.warns(true), "{outcome:?}");
        }
        for outcome in [Used, NotTried, RuleNotMet, RuleUnknown] {
            assert!(!outcome.warns(false), "{outcome:?}");
            assert!(!outcome.warns(true), "{outcome:?}");
        }
    }

    #[test]
    fn a_record_finds_the_location_used_and_warns_by_position() {
        let record = LocationsRecord {
            locations: vec![
                tried(1, LocationOutcome::OtherRepository),
                tried(2, LocationOutcome::Silent),
                tried(3, LocationOutcome::Used),
                tried(4, LocationOutcome::NotTried),
            ],
        };
        assert_eq!(record.used(), Some(3));
        assert_eq!(
            record.at(2).map(|t| t.outcome),
            Some(LocationOutcome::Silent)
        );
        assert_eq!(record.at(5), None);
        let warnings = record.warnings(|position| position == 2);
        let shown: Vec<(Option<&str>, &str)> = warnings
            .iter()
            .map(|w| (w.item.as_deref(), w.detail.as_str()))
            .collect();
        assert_eq!(
            shown,
            vec![(Some("1"), "other-repository"), (Some("2"), "silent")]
        );
        assert!(
            warnings
                .iter()
                .all(|w| w.kind == WarningKind::LocationSkipped)
        );
        assert_eq!(LocationsRecord { locations: vec![] }.used(), None);
    }

    #[test]
    fn an_address_is_cut_on_a_character_boundary() {
        assert_eq!(recorded_address("/srv/nas", 256), "/srv/nas");
        assert_eq!(recorded_address("abcdef", 3), "abc");
        assert_eq!(recorded_address("aé", 2), "a");
        assert_eq!(recorded_address("aé", 3), "aé");
    }
}
