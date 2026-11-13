//! The machine's network link (technical design §5, decision 80): the kind
//! of interface its default route leaves by, which a location's rule is
//! evaluated against. The kernel's routing tables and `/sys/class/net` on
//! Linux; `route -n get default` and `networksetup -listallhardwareports` on
//! macOS. No network's name is read here.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{NetworkLink, PlatformError};
use async_trait::async_trait;
use domain::planning::Link;

use crate::source::{self, Program};

/// Reads `/proc/net/route`, `/proc/net/ipv6_route` and `/sys/class/net`,
/// or files and a folder laid out like them.
#[derive(Clone, Debug)]
pub struct KernelLink {
    route: PathBuf,
    ipv6_route: PathBuf,
    interfaces: PathBuf,
    timeout: Duration,
}

impl KernelLink {
    pub fn system() -> Self {
        Self::at("/proc/net/route", "/proc/net/ipv6_route", "/sys/class/net")
    }

    /// Reads the IPv4 routes from `route`, the IPv6 ones from `ipv6_route`,
    /// and the interfaces from `interfaces`.
    pub fn at(
        route: impl Into<PathBuf>,
        ipv6_route: impl Into<PathBuf>,
        interfaces: impl Into<PathBuf>,
    ) -> Self {
        Self {
            route: route.into(),
            ipv6_route: ipv6_route.into(),
            interfaces: interfaces.into(),
            timeout: source::TIMEOUT,
        }
    }
}

#[async_trait]
impl NetworkLink for KernelLink {
    async fn link(&self) -> Result<Link, PlatformError> {
        todo!("{:?} {:?}", self.route, self.timeout)
    }
}

/// Asks `route -n get default` and `networksetup -listallhardwareports` on
/// macOS, or programs that answer as they do.
#[derive(Clone, Debug)]
pub struct RouteLink {
    route: Program,
    ports: Program,
}

impl RouteLink {
    pub fn system() -> Self {
        Self::running(
            ("/sbin/route", ["-n", "get", "default"].as_slice()),
            (
                "/usr/sbin/networksetup",
                ["-listallhardwareports"].as_slice(),
            ),
        )
    }

    /// Runs `route`, then `ports`, each a program and its arguments, with no
    /// environment, and reads what they print as `route -n get default` and
    /// `networksetup -listallhardwareports` print it.
    pub fn running<A: Into<OsString> + Clone>(
        route: (impl Into<PathBuf>, &[A]),
        ports: (impl Into<PathBuf>, &[A]),
    ) -> Self {
        Self {
            route: Program::new(route.0, route.1.iter().cloned()),
            ports: Program::new(ports.0, ports.1.iter().cloned()),
        }
    }
}

#[async_trait]
impl NetworkLink for RouteLink {
    async fn link(&self) -> Result<Link, PlatformError> {
        todo!("{:?} {:?}", self.route, self.ports)
    }
}

/// This machine's network link: `route` and `networksetup` on macOS, the
/// kernel's elsewhere.
pub fn system_link() -> Arc<dyn NetworkLink> {
    if cfg!(target_os = "macos") {
        Arc::new(RouteLink::system())
    } else {
        Arc::new(KernelLink::system())
    }
}
