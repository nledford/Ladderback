//! A schedule's `timezone` is looked up here, in the adapter: the time zone
//! database is read from the system (§6.1, decision 72), and `domain` reads
//! no files.

use domain::TimeZoneDatabase;
use domain::planning::{InvalidValue, ScheduleZone, ValueKind};

/// The zone `database` holds under `name`, in any case (`europe/berlin` is
/// `Europe/Berlin`), kept under the name the database spells it with. A name
/// it doesn't hold is refused as [`ValueKind::TimeZone`], and so is one that
/// names no zone of it (a fixed offset, a POSIX TZ string, its "unknown"):
/// [`ScheduleZone::new`].
pub(crate) fn schedule_zone(
    name: &str,
    database: &TimeZoneDatabase,
) -> Result<ScheduleZone, InvalidValue> {
    todo!("{name} {database:?}")
}

fn refused(name: &str) -> InvalidValue {
    InvalidValue {
        kind: ValueKind::TimeZone,
        value: name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Result<ScheduleZone, InvalidValue> {
        schedule_zone(name, &TimeZoneDatabase::bundled())
    }

    #[test]
    fn a_schedule_zone_is_utc_or_a_name_in_the_time_zone_database() {
        for (written, name) in [
            ("UTC", "UTC"),
            ("utc", "UTC"),
            ("Europe/Berlin", "Europe/Berlin"),
            ("europe/BERLIN", "Europe/Berlin"),
            (
                "America/Argentina/Buenos_Aires",
                "America/Argentina/Buenos_Aires",
            ),
            ("Etc/GMT+5", "Etc/GMT+5"),
        ] {
            let found = named(written).expect("a zone");
            assert_eq!(found.name(), name, "{written:?}");
            assert_eq!(found.zone().iana_name(), Some(name), "{written:?}");
        }
    }

    #[test]
    fn a_schedule_zone_that_is_not_a_name_of_a_zone_is_refused() {
        for text in [
            "",
            "Mars/Olympus",
            "Berlin",
            "+02:00",
            "EST5EDT,M3.2.0,M11.1.0",
            // The database answers this name with a zone that has no rules.
            "Etc/Unknown",
            "../../etc/passwd",
        ] {
            assert_eq!(named(text), Err(refused(text)), "{text:?}");
        }
    }
}
