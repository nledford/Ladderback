use super::*;

fn route(address: &str, tls: TokenTls) -> Result<HubRoute, RouteRefused> {
    HubRoute::of(&HubAddress::parse(address).unwrap(), tls)
}

fn pin() -> Fingerprint {
    Fingerprint::from_digest([7; 32])
}

#[test]
fn an_https_address_is_reached_by_its_host_and_port() {
    assert_eq!(
        route("https://hub.example.lan:9899", TokenTls::Ca),
        Ok(HubRoute {
            host: "hub.example.lan".into(),
            port: 9899,
            session: RouteSession::Ca,
        })
    );
    assert_eq!(
        route("https://hub.example.lan:9899/", TokenTls::Pinned(pin())),
        Ok(HubRoute {
            host: "hub.example.lan".into(),
            port: 9899,
            session: RouteSession::Pinned(pin()),
        })
    );
}

#[test]
fn an_address_without_a_port_takes_its_schemes() {
    assert_eq!(
        route("https://hub.example", TokenTls::Ca).unwrap().port,
        443
    );
    assert_eq!(route("http://localhost", TokenTls::Ca).unwrap().port, 80);
}

#[test]
fn a_plain_address_on_this_computer_is_reached_without_tls() {
    for (address, host) in [
        ("http://127.0.0.1:9898", "127.0.0.1"),
        ("http://127.1.2.3:9898", "127.1.2.3"),
        ("http://localhost:9898", "localhost"),
        ("http://LocalHost:9898", "LocalHost"),
        ("http://[::1]:9898", "::1"),
    ] {
        assert_eq!(
            route(address, TokenTls::Ca),
            Ok(HubRoute {
                host: host.into(),
                port: 9898,
                session: RouteSession::Plain,
            }),
            "{address}"
        );
    }
}

#[test]
fn a_plain_address_off_this_computer_is_refused() {
    for address in [
        "http://hub.example:9899",
        "http://192.168.1.10:9899",
        "http://[fe80::1]:9899",
        "http://localhost.example:9899",
        "http://0.0.0.0:9899",
    ] {
        assert_eq!(
            route(address, TokenTls::Ca),
            Err(RouteRefused::PlainOffLoopback),
            "{address}"
        );
    }
}

#[test]
fn an_ipv6_host_is_reached_without_its_brackets() {
    assert_eq!(
        route("https://[2001:db8::7]:9899", TokenTls::Ca)
            .unwrap()
            .host,
        "2001:db8::7"
    );
}

#[test]
fn an_address_that_is_more_or_less_than_a_host_and_port_is_refused() {
    for address in [
        "https://user@hub.example:9899",
        "https://hub.example:9899/sync",
        "https://hub.example:0",
        "https://hub.example:65536",
        "https://hub.example:port",
        "https://hub.example:",
        "https://:9899",
        "https://[::1:9899",
        "https://[not-an-address]:9899",
        "https://hub.example:9899?x",
        "https://hub.example:9899#x",
    ] {
        assert_eq!(
            route(address, TokenTls::Ca),
            Err(RouteRefused::Malformed),
            "{address}"
        );
    }
}

/// A pinned certificate is the hub's own TLS: a plain address can't carry
/// it, even on this computer.
#[test]
fn a_plain_address_with_a_pinned_certificate_is_refused() {
    assert_eq!(
        route("http://127.0.0.1:9898", TokenTls::Pinned(pin())),
        Err(RouteRefused::Malformed)
    );
}

#[test]
fn loopback_is_localhost_or_a_loopback_address() {
    for host in ["localhost", "LOCALHOST", "127.0.0.1", "127.255.0.1", "::1"] {
        assert!(is_loopback(host), "{host}");
    }
    for host in ["hub.example", "10.0.0.1", "::", "fe80::1", "localhost."] {
        assert!(!is_loopback(host), "{host}");
    }
}
