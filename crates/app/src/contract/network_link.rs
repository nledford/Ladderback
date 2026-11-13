use std::sync::Arc;

use domain::planning::{CurrentNetworkName, Link};

use crate::ports::{NetworkLink, PlatformError};

/// Sets up a network link in a known state: a fake told what to say, or a
/// real adapter reading a machine the harness made up.
pub trait NetworkLinkHarness: Send + Sync {
    /// A machine whose default route leaves by an Ethernet port.
    fn wired(&self) -> Arc<dyn NetworkLink>;
    /// A machine whose default route leaves by its Wi-Fi interface.
    fn wifi(&self) -> Arc<dyn NetworkLink>;
    /// A machine whose default route leaves by a tunnel.
    fn tunnel(&self) -> Arc<dyn NetworkLink>;
    /// A machine with no default route, as just after a wake.
    fn no_route(&self) -> Arc<dyn NetworkLink>;
    /// A source the adapter can read but doesn't understand.
    fn unrecognized(&self) -> Arc<dyn NetworkLink>;
    /// A source the adapter can't read.
    fn unreadable(&self) -> Arc<dyn NetworkLink>;
    /// A machine on the Wi-Fi network named `name`.
    fn on_network(&self, name: &str) -> Arc<dyn NetworkLink>;
    /// A machine whose Wi-Fi has joined no network.
    fn on_no_network(&self) -> Arc<dyn NetworkLink>;
    /// A machine on a Wi-Fi network whose name the platform hides, as a
    /// macOS release that redacts it does.
    fn name_hidden(&self) -> Arc<dyn NetworkLink>;
    /// A machine with no reader for the name, or one that fails.
    fn no_name_reader(&self) -> Arc<dyn NetworkLink>;
    /// A source of the name this adapter doesn't understand, which holds
    /// `name` somewhere in what it says.
    fn name_unrecognized(&self, name: &str) -> Arc<dyn NetworkLink>;
}

pub async fn run(harness: &dyn NetworkLinkHarness) {
    a_route_out_of_an_ethernet_port_is_wired(harness).await;
    a_route_out_of_wi_fi_is_wi_fi(harness).await;
    a_route_through_a_tunnel_is_neither_wired_nor_wi_fi(harness).await;
    no_default_route_is_said_as_such(harness).await;
    a_source_that_says_something_unknown_is_an_error(harness).await;
    a_source_that_cannot_be_read_is_an_error(harness).await;
    the_name_of_the_network_joined_is_read_as_it_is(harness).await;
    no_network_joined_is_no_name(harness).await;
    a_name_the_platform_hides_or_cannot_read_is_an_error(harness).await;
    a_name_is_never_in_an_error(harness).await;
}

async fn a_route_out_of_an_ethernet_port_is_wired(harness: &dyn NetworkLinkHarness) {
    assert_eq!(harness.wired().link().await, Ok(Link::Wired));
}

async fn a_route_out_of_wi_fi_is_wi_fi(harness: &dyn NetworkLinkHarness) {
    assert_eq!(harness.wifi().link().await, Ok(Link::WiFi));
}

async fn a_route_through_a_tunnel_is_neither_wired_nor_wi_fi(harness: &dyn NetworkLinkHarness) {
    assert_eq!(harness.tunnel().link().await, Ok(Link::Other));
}

async fn no_default_route_is_said_as_such(harness: &dyn NetworkLinkHarness) {
    let answer = harness.no_route().link().await;
    assert!(
        matches!(
            answer,
            Ok(Link::NoRoute) | Err(PlatformError::Unreadable { .. })
        ),
        "no default route is no route, or no answer where the platform says it as a failure: {answer:?}"
    );
}

async fn a_source_that_says_something_unknown_is_an_error(harness: &dyn NetworkLinkHarness) {
    let answer = harness.unrecognized().link().await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "a source this adapter doesn't understand is no answer, not a guess: {answer:?}"
    );
}

async fn a_source_that_cannot_be_read_is_an_error(harness: &dyn NetworkLinkHarness) {
    let answer = harness.unreadable().link().await;
    assert!(
        matches!(answer, Err(PlatformError::Unreadable { .. })),
        "a source that can't be read is no answer, not a guess: {answer:?}"
    );
}

async fn the_name_of_the_network_joined_is_read_as_it_is(harness: &dyn NetworkLinkHarness) {
    for name in ["Office", "Café 5 GHz", "office"] {
        assert_eq!(
            harness.on_network(name).network_name().await,
            Ok(CurrentNetworkName::new(name.as_bytes())),
            "{name:?}"
        );
    }
}

async fn no_network_joined_is_no_name(harness: &dyn NetworkLinkHarness) {
    assert_eq!(harness.on_no_network().network_name().await, Ok(None));
}

async fn a_name_the_platform_hides_or_cannot_read_is_an_error(harness: &dyn NetworkLinkHarness) {
    for link in [harness.name_hidden(), harness.no_name_reader()] {
        let answer = link.network_name().await;
        assert!(
            matches!(
                answer,
                Err(PlatformError::Unreadable { .. } | PlatformError::Unrecognized { .. })
            ),
            "a name that can't be read is no answer, not a guess: {answer:?}"
        );
    }
}

/// The current network's name says where the machine is: an error, which
/// may reach a log, never holds it (decision 80).
async fn a_name_is_never_in_an_error(harness: &dyn NetworkLinkHarness) {
    let answer = harness
        .name_unrecognized("canary-network")
        .network_name()
        .await;
    assert!(answer.is_err(), "{answer:?}");
    assert!(!format!("{answer:?}").contains("canary"), "{answer:?}");
}
