use std::cmp::Ordering::{Equal, Greater, Less};

use super::{InvalidVersion, MAX_VERSION_BYTES, MachineVersion};

fn version(text: &str) -> MachineVersion {
    MachineVersion::parse(text).unwrap_or_else(|_| panic!("{text} is shaped like a version"))
}

fn against(machine: &str, hub: &str) -> Option<std::cmp::Ordering> {
    version(machine).against(&version(hub))
}

#[test]
fn a_version_up_to_its_longest_parses_as_written() {
    let longest = "1".repeat(MAX_VERSION_BYTES);
    for text in [
        "0.6.0",
        "0.6.0+3f937cf.dirty",
        "nightly",
        "!~",
        longest.as_str(),
    ] {
        assert_eq!(
            MachineVersion::parse(text).map(|v| v.as_str().to_owned()),
            Ok(text.to_owned())
        );
    }
}

#[test]
fn a_text_not_shaped_like_a_version_is_refused() {
    let too_long = "1".repeat(MAX_VERSION_BYTES + 1);
    for text in [
        "",
        "0.6.0 beta",
        "0.6.0\n",
        "0.6.0\u{7f}",
        "0.6.0é",
        too_long.as_str(),
    ] {
        assert_eq!(MachineVersion::parse(text), Err(InvalidVersion), "{text:?}");
    }
}

#[test]
fn each_part_of_the_release_orders_by_its_number() {
    for (machine, hub, standing) in [
        ("0.6.0", "0.7.0", Less),
        ("0.8.0", "0.7.0", Greater),
        ("0.7.0", "0.7.0", Equal),
        ("1.0.0", "0.9.9", Greater),
        ("0.7.1", "0.7.2", Less),
        ("0.7.3", "0.7.2", Greater),
        ("0.9.0", "0.10.0", Less),
        ("10.0.0", "9.0.0", Greater),
    ] {
        assert_eq!(
            against(machine, hub),
            Some(standing),
            "{machine} against {hub}"
        );
    }
}

#[test]
fn build_metadata_is_ignored_so_two_builds_of_one_release_are_equal() {
    for (machine, hub) in [
        ("0.6.0+3f937cf", "0.6.0"),
        ("0.6.0", "0.6.0+3f937cf.dirty"),
        ("0.6.0+aaaaaaa", "0.6.0+bbbbbbb"),
        ("0.6.0-rc.1+aaaaaaa", "0.6.0-rc.1"),
    ] {
        assert_eq!(
            against(machine, hub),
            Some(Equal),
            "{machine} against {hub}"
        );
    }
    assert_eq!(against("0.6.0+3f937cf", "0.7.0"), Some(Less));
}

#[test]
fn a_pre_release_comes_before_its_release_and_orders_by_semver_precedence() {
    for (machine, hub, standing) in [
        ("0.7.0-rc.1", "0.7.0", Less),
        ("0.7.0", "0.7.0-rc.1", Greater),
        ("0.7.0-rc.1", "0.6.0", Greater),
        ("1.0.0-alpha", "1.0.0-beta", Less),
        ("1.0.0-rc.2", "1.0.0-rc.10", Less),
        ("1.0.0-1", "1.0.0-alpha", Less),
        ("1.0.0-alpha", "1.0.0-1", Greater),
        ("1.0.0-alpha", "1.0.0-alpha.1", Less),
        ("1.0.0-x-y", "1.0.0-x-y", Equal),
        ("1.0.0-0a", "1.0.0-0b", Less),
    ] {
        assert_eq!(
            against(machine, hub),
            Some(standing),
            "{machine} against {hub}"
        );
    }
}

#[test]
fn a_version_that_isnt_semver_stands_nowhere_against_another() {
    for text in [
        "nightly",
        "v0.6.0",
        "0.6",
        "0.6.0.1",
        "06.0.0",
        "0.06.0",
        "0.6.00",
        "0.6.a",
        "0.6.0-",
        "0.6.0-rc..1",
        "0.6.0-01",
        "0.6.0-a_b",
        "0.6.0+",
        "0.6.0+a..b",
        "0.6.0+a_b",
        "18446744073709551616.0.0",
    ] {
        assert_eq!(against(text, "0.6.0"), None, "{text}");
        assert_eq!(against("0.6.0", text), None, "against {text}");
    }
}
