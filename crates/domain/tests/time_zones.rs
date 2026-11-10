//! Time zones need no database on the machine (technical design §3.2): the
//! build carries a copy of the IANA time-zone database, so a zone resolves on a
//! host with no `tzdata` (a slim or distroless container image) and in the
//! browser, which has none. `jiff` reads the system copy first, so these tests
//! ask for the bundled one by name: that way a machine that has `tzdata` can't
//! hide a build that left the bundle out.

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZoneDatabase};

fn offset_of(zone: &str, at: &str) -> Offset {
    let database = TimeZoneDatabase::bundled();
    let zone = database
        .get(zone)
        .unwrap_or_else(|error| panic!("{zone} is in the bundled database: {error}"));
    let instant: Timestamp = at.parse().unwrap();
    zone.to_offset(instant)
}

#[test]
fn a_zone_resolves_from_the_bundled_database_with_its_daylight_saving_rules() {
    assert_eq!(
        offset_of("America/New_York", "2026-11-08T13:30:45Z"),
        Offset::constant(-5)
    );
    assert_eq!(
        offset_of("America/New_York", "2026-07-08T13:30:45Z"),
        Offset::constant(-4)
    );
}

#[test]
fn a_zone_without_daylight_saving_keeps_one_offset_all_year() {
    for at in ["2026-01-08T00:00:00Z", "2026-07-08T00:00:00Z"] {
        assert_eq!(offset_of("Asia/Tokyo", at), Offset::constant(9), "{at}");
    }
}
