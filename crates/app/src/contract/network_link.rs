use std::sync::Arc;

use domain::planning::Link;

use crate::ports::{NetworkLink, PlatformError};

/// Sets up a network link in a known state: a fake told what to say, or a
/// real adapter reading a machine the harness made up.
pub trait NetworkLinkHarness: Send + Sync {
    /// A machine whose default route leaves by an Ethernet port.
    fn wired(&self) -> Arc<dyn NetworkLink>;
    /// What [`wired`](Self::wired) reads as: [`Link::Wired`], or
    /// [`Link::NotWiFi`] on a platform that can't tell an Ethernet port from
    /// the other ports that aren't Wi-Fi.
    fn wired_reads_as(&self) -> Link;
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
}

pub async fn run(harness: &dyn NetworkLinkHarness) {
    a_route_out_of_an_ethernet_port_is_wired_where_the_platform_can_tell(harness).await;
    a_route_out_of_wi_fi_is_wi_fi(harness).await;
    a_route_through_a_tunnel_is_neither_wired_nor_wi_fi(harness).await;
    no_default_route_is_said_as_such(harness).await;
    a_source_that_says_something_unknown_is_an_error(harness).await;
    a_source_that_cannot_be_read_is_an_error(harness).await;
}

async fn a_route_out_of_an_ethernet_port_is_wired_where_the_platform_can_tell(
    harness: &dyn NetworkLinkHarness,
) {
    let reads_as = harness.wired_reads_as();
    assert!(
        matches!(reads_as, Link::Wired | Link::NotWiFi),
        "an Ethernet port is wired, or not Wi-Fi where the platform can't tell: {reads_as:?}"
    );
    assert_eq!(harness.wired().link().await, Ok(reads_as));
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
