//! A run's choice of location as `operations.locations_json` keeps it
//! (decision 80): spelled out here, like `summary_json`, because this is the
//! persistence format and the domain's types may change. An outcome is its
//! code, the one the warning and the API carry.
//!
//! Reading ignores fields it doesn't know, so a database a newer version
//! wrote with one more can still be read; a missing field, an unknown
//! outcome or more locations than an entry may have is an error.

use domain::execution::locations::{LocationOutcome, LocationTried, LocationsRecord};
use domain::planning::MAX_LOCATIONS;
use serde::{Deserialize, Serialize};

/// One location, as the column keeps it.
#[derive(Serialize, Deserialize)]
struct Stored {
    position: u8,
    address: String,
    outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

/// Every outcome, so a code reads back as the outcome that wrote it.
const OUTCOMES: [LocationOutcome; 15] = [
    LocationOutcome::Used,
    LocationOutcome::NotTried,
    LocationOutcome::RuleNotMet,
    LocationOutcome::RuleUnknown,
    LocationOutcome::Silent,
    LocationOutcome::Refused,
    LocationOutcome::NotARepository,
    LocationOutcome::OtherRepository,
    LocationOutcome::PasswordNotAccepted,
    LocationOutcome::SecretUnavailable,
    LocationOutcome::TemplateInvalid,
    LocationOutcome::PathRelative,
    LocationOutcome::ExpansionTooLong,
    LocationOutcome::ReferenceInProcessArguments,
    LocationOutcome::TooManyCredentials,
];

/// The column's text for `record`.
pub(crate) fn encode(record: &LocationsRecord) -> Result<String, serde_json::Error> {
    let stored: Vec<Stored> = record
        .locations
        .iter()
        .map(|tried| Stored {
            position: tried.position,
            address: tried.address.clone(),
            outcome: tried.outcome.code().to_owned(),
            source: tried.source.clone(),
        })
        .collect();
    serde_json::to_string(&stored)
}

/// The record the column's `text` keeps, or why it can't be one.
pub(crate) fn decode(text: &str) -> Result<LocationsRecord, String> {
    let stored: Vec<Stored> = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if stored.len() > MAX_LOCATIONS {
        return Err(format!(
            "holds {} locations, more than {MAX_LOCATIONS}",
            stored.len()
        ));
    }
    let locations = stored
        .into_iter()
        .map(|location| {
            let outcome = OUTCOMES
                .into_iter()
                .find(|outcome| outcome.code() == location.outcome)
                .ok_or_else(|| format!("{:?} is not an outcome", location.outcome))?;
            Ok(LocationTried {
                position: location.position,
                address: location.address,
                outcome,
                source: location.source,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(LocationsRecord { locations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_reads_back_as_itself_and_no_two_share_a_code() {
        for outcome in OUTCOMES {
            let matching = OUTCOMES
                .iter()
                .filter(|other| other.code() == outcome.code())
                .count();
            assert_eq!(matching, 1, "{outcome:?}");
            let record = LocationsRecord {
                locations: vec![LocationTried {
                    position: 1,
                    address: "/srv/lan".to_owned(),
                    outcome,
                    source: None,
                }],
            };
            let text = encode(&record).expect("a record encodes");
            assert_eq!(decode(&text), Ok(record));
        }
    }

    #[test]
    fn an_unknown_outcome_and_too_many_locations_are_refused() {
        let unknown = r#"[{"position":1,"address":"/a","outcome":"teleported"}]"#;
        assert!(decode(unknown).is_err());
        let one = r#"{"position":1,"address":"/a","outcome":"silent"}"#;
        let too_many = format!("[{}]", [one; MAX_LOCATIONS + 1].join(","));
        assert!(decode(&too_many).is_err());
        let enough = format!("[{}]", [one; MAX_LOCATIONS].join(","));
        assert!(decode(&enough).is_ok());
    }

    #[test]
    fn a_field_a_newer_build_added_is_ignored() {
        let newer = r#"[{"position":2,"address":"/a","outcome":"used","rule":"wired"}]"#;
        assert_eq!(decode(newer).map(|record| record.used()), Ok(Some(2)));
    }
}
