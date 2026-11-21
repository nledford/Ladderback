use jiff::tz::{Offset, TimeZone, TimeZoneDatabase};

use super::{InvalidZoneName, MAX_ZONE_BYTES, MachineZone};

fn named(name: &str) -> MachineZone {
    MachineZone::parse(name).unwrap_or_else(|_| panic!("{name} is shaped like a zone name"))
}

#[test]
fn a_zone_from_the_database_is_named_as_the_database_spells_it() {
    let chicago = TimeZoneDatabase::bundled().get("america/chicago").unwrap();
    assert_eq!(MachineZone::of(&chicago), Some(named("America/Chicago")));
    assert_eq!(MachineZone::of(&TimeZone::UTC), Some(named("UTC")));
}

#[test]
fn a_zone_with_no_name_in_the_database_is_no_machine_zone() {
    for zone in [
        TimeZone::fixed(Offset::constant(5)),
        TimeZone::posix("<+0330>-3:30").unwrap(),
        TimeZone::unknown(),
    ] {
        assert_eq!(MachineZone::of(&zone), None, "{zone:?}");
    }
}

#[test]
fn a_name_shaped_like_a_zone_name_parses() {
    let longest = "a".repeat(MAX_ZONE_BYTES);
    for name in [
        "UTC",
        "America/Argentina/ComodRivadavia",
        "Etc/GMT+5",
        "America/Port-au-Prince",
        longest.as_str(),
    ] {
        assert_eq!(named(name).as_str(), name);
    }
}

#[test]
fn a_name_not_shaped_like_a_zone_name_is_refused() {
    let too_long = "a".repeat(MAX_ZONE_BYTES + 1);
    for name in [
        "",
        "/Europe/Berlin",
        "Europe/",
        "Europe//Berlin",
        "../etc/passwd",
        "Europe\\Berlin",
        "Europe/Berlin\n",
        "Europe/Berlín",
        "Europe Berlin",
        too_long.as_str(),
    ] {
        assert_eq!(MachineZone::parse(name), Err(InvalidZoneName), "{name:?}");
    }
}

#[test]
fn a_zone_the_database_holds_is_known_as_it_spells_it() {
    let database = TimeZoneDatabase::bundled();
    assert_eq!(
        named("europe/berlin").known_in(&database),
        Some(named("Europe/Berlin"))
    );
    assert_eq!(named("UTC").known_in(&database), Some(named("UTC")));
}

#[test]
fn a_zone_the_database_doesnt_hold_is_unknown() {
    let database = TimeZoneDatabase::bundled();
    for name in ["Mars/Olympus_Mons", "Etc/Unknown", "Europe"] {
        assert_eq!(named(name).known_in(&database), None, "{name}");
    }
}
