//! The machine's network link (technical design §5, decision 80): the kind
//! of interface its default route leaves by, which a location's rule is
//! evaluated against. The kernel's routing tables and `/sys/class/net` on
//! Linux; `route -n get default` and `networksetup -listallhardwareports` on
//! macOS. No network's name is read here.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use app::ports::{NetworkLink, PlatformError};
use async_trait::async_trait;
use domain::planning::Link;

use crate::source::{self, Program, attribute, unrecognized};

/// `RTF_UP` and `RTF_REJECT` of Linux's `route.h` and `ipv6_route.h`.
const RTF_UP: u32 = 0x1;
const RTF_REJECT: u32 = 0x200;

/// `ARPHRD_ETHER` of Linux's `if_arp.h`.
const ARPHRD_ETHER: &str = "1";

/// The longest interface name the kernel takes (`IFNAMSIZ`, less its end).
const MAX_INTERFACE_NAME: usize = 15;

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
        let reader = self.clone();
        source::blocking(&self.route, self.timeout, move || reader.read()).await
    }
}

impl KernelLink {
    /// The kind of the interface the default route of lowest metric leaves
    /// by, of either family; [`Link::NoRoute`] when there is none.
    fn read(&self) -> Result<Link, PlatformError> {
        let Some(ipv4) = source::table(&self.route)? else {
            return Err(PlatformError::Unreadable {
                from: self.route.display().to_string(),
                detail: "no such file".to_owned(),
            });
        };
        let mut best = ipv4_default(&self.route, &ipv4)?;
        // A machine with IPv6 switched off has no table for it.
        if let Some(ipv6) = source::table(&self.ipv6_route)? {
            if let Some(route) = ipv6_default(&self.ipv6_route, &ipv6)? {
                if best.as_ref().is_none_or(|(metric, _)| route.0 < *metric) {
                    best = Some(route);
                }
            }
        }
        match best {
            Some((_, interface)) => self.kind(&interface),
            None => Ok(Link::NoRoute),
        }
    }

    /// The kind of the interface named `name`.
    fn kind(&self, name: &str) -> Result<Link, PlatformError> {
        let entry = self.interfaces.join(name);
        let Ok(found) = std::fs::symlink_metadata(&entry) else {
            return Err(unrecognized(
                &self.interfaces,
                &format!("no interface {name}"),
            ));
        };
        // cfg80211 links `phy80211` for every wireless interface; the older
        // wireless extensions make a `wireless` folder.
        if entry.join("phy80211").exists() || entry.join("wireless").exists() {
            return Ok(Link::WiFi);
        }
        let Some(kind) = attribute(&entry, "type")? else {
            return Err(unrecognized(&entry, "no type"));
        };
        if kind != ARPHRD_ETHER {
            return Ok(Link::Other);
        }
        // A bridge, a veth pair or a bond is linked from under
        // `devices/virtual/`; a port of the machine's from under its bus.
        let virtual_interface = found.file_type().is_symlink()
            && std::fs::read_link(&entry)
                .map_err(|error| PlatformError::Unreadable {
                    from: entry.display().to_string(),
                    detail: error.to_string(),
                })?
                .to_string_lossy()
                .contains("devices/virtual/");
        Ok(if virtual_interface {
            Link::Other
        } else {
            Link::Wired
        })
    }
}

/// An interface name as a route names it, refused when it could leave the
/// folder of interfaces or is longer than the kernel allows.
fn interface_name(from: &Path, name: &str) -> Result<String, PlatformError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.len() > MAX_INTERFACE_NAME
    {
        return Err(unrecognized(from, &format!("interface {name}")));
    }
    Ok(name.to_owned())
}

/// The IPv4 default route of lowest metric that is up, from
/// `/proc/net/route`: its metric and interface.
fn ipv4_default(from: &Path, table: &str) -> Result<Option<(u32, String)>, PlatformError> {
    let mut best: Option<(u32, String)> = None;
    // The first line names the columns.
    for line in table.lines().skip(1).filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let [
            interface,
            destination,
            _gateway,
            flags,
            _refcnt,
            _use,
            metric,
            mask,
            ..,
        ] = fields.as_slice()
        else {
            return Err(unrecognized(from, line));
        };
        if *destination != "00000000" || *mask != "00000000" {
            continue;
        }
        let flags = u32::from_str_radix(flags, 16).map_err(|_| unrecognized(from, line))?;
        let metric: u32 = metric.parse().map_err(|_| unrecognized(from, line))?;
        if flags & RTF_UP == 0 {
            continue;
        }
        let interface = interface_name(from, interface)?;
        if best.as_ref().is_none_or(|(lowest, _)| metric < *lowest) {
            best = Some((metric, interface));
        }
    }
    Ok(best)
}

/// The IPv6 default route of lowest metric that is up and reaches
/// somewhere, from `/proc/net/ipv6_route`: its metric and interface.
fn ipv6_default(from: &Path, table: &str) -> Result<Option<(u32, String)>, PlatformError> {
    let mut best: Option<(u32, String)> = None;
    for line in table.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [
            destination,
            length,
            _source,
            _source_length,
            _next_hop,
            metric,
            _refcnt,
            _use,
            flags,
            interface,
        ] = fields.as_slice()
        else {
            return Err(unrecognized(from, line));
        };
        if destination.bytes().any(|byte| byte != b'0') || *length != "00" {
            continue;
        }
        let flags = u32::from_str_radix(flags, 16).map_err(|_| unrecognized(from, line))?;
        let metric = u32::from_str_radix(metric, 16).map_err(|_| unrecognized(from, line))?;
        if flags & RTF_UP == 0 || flags & RTF_REJECT != 0 {
            continue;
        }
        let interface = interface_name(from, interface)?;
        if best.as_ref().is_none_or(|(lowest, _)| metric < *lowest) {
            best = Some((metric, interface));
        }
    }
    Ok(best)
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
        let route = self.route.output().await?;
        let from = self.route.path.as_path();
        let interface = route
            .lines()
            .find_map(|line| line.trim().strip_prefix("interface:"))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| unrecognized(from, "no interface"))?;
        let ports = self.ports.output().await?;
        hardware_port(self.ports.path.as_path(), &ports, interface)
    }
}

/// What `networksetup -listallhardwareports` says of `interface`: Wi-Fi for
/// the Wi-Fi port, not Wi-Fi for any other port (no Mac on Ethernet has been
/// seen yet, so a wired port can't be told from a bridge, decision 80), and
/// other for an interface that is no hardware port, as a tunnel is.
fn hardware_port(from: &Path, ports: &str, interface: &str) -> Result<Link, PlatformError> {
    let mut port = None;
    let mut seen = false;
    for line in ports.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Hardware Port:") {
            port = Some(name.trim());
            seen = true;
        } else if let Some(device) = line.strip_prefix("Device:") {
            if device.trim() == interface {
                return Ok(match port {
                    Some("Wi-Fi" | "AirPort") => Link::WiFi,
                    _ => Link::NotWiFi,
                });
            }
        }
    }
    if seen {
        Ok(Link::Other)
    } else {
        Err(unrecognized(from, "no hardware port"))
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
