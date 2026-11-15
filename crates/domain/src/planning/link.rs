//! The machine's network link (technical design §3.1, decision 80): the
//! interface each IP family's default route leaves by, which a location's
//! rule is evaluated against. Choosing it from the default routes the
//! platform lists is pure; listing them is the `NetworkLink` port's.

use super::rule::CurrentNetworkName;

/// The network link the machine reaches others by: the interface its default
/// routes leave by, as the platform read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// An Ethernet interface, not a virtual one.
    Wired,
    /// A Wi-Fi interface.
    WiFi,
    /// A tunnel, a bridge, a cellular modem, a phone's tethering.
    Other,
    /// No default route in either family, as just after a wake: nothing says
    /// where the next one will leave by.
    NoRoute,
    /// The IPv4 and IPv6 default routes leave by different interfaces, of
    /// these kinds: a location may be reached by either, and nothing says
    /// which before it is tried.
    Mixed {
        ipv4: InterfaceKind,
        ipv6: InterfaceKind,
    },
}

impl From<InterfaceKind> for Link {
    fn from(kind: InterfaceKind) -> Self {
        match kind {
            InterfaceKind::Wired => Self::Wired,
            InterfaceKind::WiFi => Self::WiFi,
            InterfaceKind::Other => Self::Other,
        }
    }
}

/// An IP family, of which each has a default route of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Ipv4,
    Ipv6,
}

/// What a network interface is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceKind {
    /// An Ethernet interface, not a virtual one.
    Wired,
    /// A Wi-Fi interface.
    WiFi,
    /// A tunnel, a bridge, a cellular modem, a phone's tethering.
    Other,
}

/// Whether a default route the platform lists carries traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteState {
    /// It leaves by an interface that is up and has a carrier, of this kind.
    Carries(InterfaceKind),
    /// It leads nowhere: an `unreachable`, `blackhole` or `prohibit` route.
    LeadsNowhere,
    /// Its interface is down or has no carrier.
    InterfaceDown,
}

/// A default route as the platform lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultRoute {
    pub family: Family,
    /// Its metric, compared only with the same family's.
    pub metric: u32,
    /// The interface it leaves by, as the platform names it.
    pub interface: String,
    pub state: RouteState,
}

/// The interface one family's traffic leaves by: its name and kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub interface: String,
    pub kind: InterfaceKind,
}

/// Each family's default route that carries traffic, the one of lowest
/// metric within the family: the link the machine is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultRoutes {
    ipv4: Option<Outgoing>,
    ipv6: Option<Outgoing>,
}

impl DefaultRoutes {
    /// Chooses, for each family, the default route of `routes` that carries
    /// traffic and has the lowest metric, the first listed of equal ones.
    pub fn choose(routes: impl IntoIterator<Item = DefaultRoute>) -> Self {
        let _ = routes.into_iter();
        todo!()
    }

    /// The link the routes make: the one interface when only one family has
    /// a route or both leave by the same interface, mixed when they leave by
    /// different ones, and no route when neither has one.
    pub fn link(&self) -> Link {
        todo!()
    }

    /// The interface whose Wi-Fi network's name is asked: `family`'s, or for
    /// `None` the link's one interface. `None` when that family has no route,
    /// or, for the link's, when the link is mixed or has no route.
    pub fn interface(&self, family: Option<Family>) -> Option<&Outgoing> {
        let _ = family;
        todo!()
    }
}

/// The names of the Wi-Fi networks the link's interfaces have joined, as far
/// as they were read: the link's own on a link of one interface, each
/// family's on a mixed one. A name not read, or not asked, is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WiFiNames<'a> {
    pub link: Option<&'a CurrentNetworkName>,
    pub ipv4: Option<&'a CurrentNetworkName>,
    pub ipv6: Option<&'a CurrentNetworkName>,
}

impl<'a> WiFiNames<'a> {
    /// The name of the network of a link of one interface.
    pub fn of_link(name: Option<&'a CurrentNetworkName>) -> Self {
        Self {
            link: name,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(family: Family, metric: u32, interface: &str, state: RouteState) -> DefaultRoute {
        DefaultRoute {
            family,
            metric,
            interface: interface.to_owned(),
            state,
        }
    }

    fn carries(family: Family, metric: u32, interface: &str, kind: InterfaceKind) -> DefaultRoute {
        route(family, metric, interface, RouteState::Carries(kind))
    }

    #[test]
    fn a_link_of_one_family_is_never_mixed() {
        for family in [Family::Ipv4, Family::Ipv6] {
            for kind in [
                InterfaceKind::Wired,
                InterfaceKind::WiFi,
                InterfaceKind::Other,
            ] {
                let routes = DefaultRoutes::choose([
                    carries(family, 600, "wlan0", kind),
                    carries(family, 100, "eth0", kind),
                ]);
                assert_eq!(routes.link(), Link::from(kind), "{family:?} {kind:?}");
                assert_eq!(
                    routes.interface(None).map(|out| out.interface.as_str()),
                    Some("eth0"),
                    "{family:?}"
                );
            }
        }
    }

    #[test]
    fn both_families_out_of_one_interface_are_one_link() {
        let routes = DefaultRoutes::choose([
            carries(Family::Ipv6, 100, "wlan0", InterfaceKind::WiFi),
            carries(Family::Ipv4, 600, "wlan0", InterfaceKind::WiFi),
        ]);
        assert_eq!(routes.link(), Link::WiFi);
        for family in [None, Some(Family::Ipv4), Some(Family::Ipv6)] {
            assert_eq!(
                routes.interface(family).map(|out| out.interface.as_str()),
                Some("wlan0"),
                "{family:?}"
            );
        }
    }

    /// #492: Ethernet's IPv6 default at 100 never outranks Wi-Fi's IPv4 one
    /// at 600; the link is mixed, and names the interface of each family.
    #[test]
    fn families_out_of_different_interfaces_are_mixed_whatever_their_metrics() {
        let routes = DefaultRoutes::choose([
            carries(Family::Ipv4, 600, "wlan0", InterfaceKind::WiFi),
            carries(Family::Ipv4, 10000, "eth0", InterfaceKind::Wired),
            carries(Family::Ipv6, 100, "eth0", InterfaceKind::Wired),
        ]);
        assert_eq!(
            routes.link(),
            Link::Mixed {
                ipv4: InterfaceKind::WiFi,
                ipv6: InterfaceKind::Wired,
            }
        );
        assert_eq!(routes.interface(None), None);
        assert_eq!(
            routes.interface(Some(Family::Ipv4)),
            Some(&Outgoing {
                interface: "wlan0".to_owned(),
                kind: InterfaceKind::WiFi,
            })
        );
        assert_eq!(
            routes.interface(Some(Family::Ipv6)),
            Some(&Outgoing {
                interface: "eth0".to_owned(),
                kind: InterfaceKind::Wired,
            })
        );
    }

    #[test]
    fn two_interfaces_of_one_kind_are_still_mixed() {
        let routes = DefaultRoutes::choose([
            carries(Family::Ipv4, 100, "eth0", InterfaceKind::Wired),
            carries(Family::Ipv6, 100, "eth1", InterfaceKind::Wired),
        ]);
        assert_eq!(
            routes.link(),
            Link::Mixed {
                ipv4: InterfaceKind::Wired,
                ipv6: InterfaceKind::Wired,
            }
        );
    }

    #[test]
    fn a_default_route_that_carries_nothing_is_passed_over() {
        for state in [RouteState::LeadsNowhere, RouteState::InterfaceDown] {
            let routes = DefaultRoutes::choose([
                route(Family::Ipv6, 1, "eth0", state),
                carries(Family::Ipv4, 600, "wlan0", InterfaceKind::WiFi),
                route(Family::Ipv4, 1, "eth0", state),
            ]);
            assert_eq!(routes.link(), Link::WiFi, "{state:?}");
            assert_eq!(routes.interface(Some(Family::Ipv6)), None, "{state:?}");
        }
    }

    #[test]
    fn no_route_that_carries_traffic_in_either_family_is_no_route() {
        let routes = DefaultRoutes::choose([
            route(Family::Ipv4, 100, "*", RouteState::LeadsNowhere),
            route(Family::Ipv6, 100, "eth0", RouteState::InterfaceDown),
        ]);
        assert_eq!(routes.link(), Link::NoRoute);
        for family in [None, Some(Family::Ipv4), Some(Family::Ipv6)] {
            assert_eq!(routes.interface(family), None, "{family:?}");
        }
        assert_eq!(DefaultRoutes::choose([]).link(), Link::NoRoute);
    }

    #[test]
    fn of_equal_metrics_the_first_listed_is_chosen() {
        let routes = DefaultRoutes::choose([
            carries(Family::Ipv4, 100, "eth0", InterfaceKind::Wired),
            carries(Family::Ipv4, 100, "wlan0", InterfaceKind::WiFi),
        ]);
        assert_eq!(routes.link(), Link::Wired);
    }
}
