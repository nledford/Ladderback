use super::*;

#[test]
fn a_loopback_address_is_warned_of_as_one_another_machine_cant_reach() {
    for address in [
        "http://127.0.0.1:9898",
        "https://localhost:9443",
        "HTTP://LOCALHOST:9898/",
        "http://[::1]:8080",
        "https://127.1:9443",
        "https://127.1.2.3:9443/sync",
        " http://127.0.0.1:9898 ",
    ] {
        assert_eq!(
            AddressWarning::of(address),
            Some(AddressWarning::Loopback),
            "{address}"
        );
    }
}

#[test]
fn a_plain_http_address_beyond_loopback_is_warned_of_as_one_an_agent_refuses() {
    for address in ["http://hub.example:8080", "http://192.168.1.10:9898/"] {
        assert_eq!(
            AddressWarning::of(address),
            Some(AddressWarning::Plain),
            "{address}"
        );
    }
}

#[test]
fn an_https_address_beyond_loopback_or_one_the_hub_refuses_is_not_warned_of() {
    for address in [
        "https://hub.example:9443",
        "https://192.168.1.10:9899/",
        "https://localhost.example:9443",
        "https://[2001:db8::1]:9443",
        "ftp://127.0.0.1:21",
        "",
    ] {
        assert_eq!(AddressWarning::of(address), None, "{address}");
    }
}

#[test]
fn a_token_counts_down_whole_seconds_from_the_hub_s_answer() {
    let at = |millis| Duration::from_millis(millis);
    let countdown = Countdown::new(900, at(10_000));
    assert_eq!(countdown.left(at(10_000)), 900);
    assert_eq!(countdown.left(at(10_999)), 900);
    assert_eq!(countdown.left(at(11_000)), 899);
    assert_eq!(countdown.left(at(909_999)), 1);
    assert_eq!(countdown.left(at(910_000)), 0);
    assert_eq!(countdown.left(at(u64::MAX)), 0);
    // A clock read before the answer came leaves the whole life.
    assert_eq!(countdown.left(at(5_000)), 900);
}

#[test]
fn the_minutes_left_are_rounded_up() {
    for (seconds, said) in [(900, 15), (899, 15), (61, 2), (60, 1), (1, 1), (0, 0)] {
        assert_eq!(minutes(seconds), said, "{seconds}");
    }
}

#[test]
fn a_life_is_read_in_whole_minutes() {
    assert_eq!(life("15"), Some(15));
    assert_eq!(life(" 1440 "), Some(1440));
    for typed in ["", "fifteen", "-5", "1.5", "15 minutes"] {
        assert_eq!(life(typed), None, "{typed:?}");
    }
}

/// The same table as `crates/domain/src/fleet/route/tests.rs`, so the page
/// warns of the hosts an agent takes for this computer: keep the two
/// identical.
#[test]
fn loopback_is_localhost_or_a_loopback_address_in_any_form_the_resolver_reads() {
    for host in [
        "localhost",
        "LOCALHOST",
        "127.0.0.1",
        "127.255.0.1",
        "::1",
        "127.1",
        "127.0.1",
        "0x7f.1",
        "0177.0.0.1",
        "2130706433",
    ] {
        assert!(is_loopback(host), "{host}");
    }
    for host in [
        "hub.example",
        "10.0.0.1",
        "::",
        "fe80::1",
        "localhost.",
        "128.1",
        "0x80.1",
        "2130706431",
        "127.256.0.1",
        "127.0.0.0.1",
        "127..1",
        "127.",
        "0x",
        "08.1",
        "+127.1",
    ] {
        assert!(!is_loopback(host), "{host}");
    }
}
