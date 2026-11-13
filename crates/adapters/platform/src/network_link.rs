//! The machine's network link (technical design §5, decision 80): the kind
//! of interface its default route leaves by, which a location's rule is
//! evaluated against. The kernel's routing tables and `/sys/class/net` on
//! Linux; `route -n get default` and `networksetup -listallhardwareports` on
//! macOS. The name of the Wi-Fi network that interface has joined, asked only
//! when a rule lists names: NetworkManager over D-Bus on Linux, and
//! `system_profiler -json SPAirPortDataType` on macOS. No error repeats a
//! name: it says where the machine is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use app::ports::{NetworkLink, PlatformError};
use async_trait::async_trait;
use domain::planning::{CurrentNetworkName, Link};

use crate::source::{self, Program, attribute, unrecognized};

/// `RTF_UP` and `RTF_REJECT` of Linux's `route.h` and `ipv6_route.h`.
const RTF_UP: u32 = 0x1;
const RTF_REJECT: u32 = 0x200;

/// `ARPHRD_ETHER` of Linux's `if_arp.h`.
const ARPHRD_ETHER: &str = "1";

/// The longest interface name the kernel takes (`IFNAMSIZ`, less its end).
const MAX_INTERFACE_NAME: usize = 15;

/// How long `system_profiler` may take: 4 to 9 s were seen (decision 80).
const PROFILE_TIMEOUT: Duration = Duration::from_secs(15);

/// The most bytes of `system_profiler`'s report read: it lists the networks
/// around too, so it runs to tens of KiB.
const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

/// What macOS writes for a network's name it withholds.
const REDACTED: &str = "<redacted>";

/// Where NetworkManager is asked for the name: the system bus, or a bus at
/// an address (a stand-in's, in the tests).
#[derive(Clone, Debug)]
enum Bus {
    System,
    At(String),
}

impl Bus {
    /// Where an error says the name was read from.
    fn from(&self) -> String {
        match self {
            Self::System => "NetworkManager on the system bus".to_owned(),
            Self::At(address) => format!("NetworkManager on {address}"),
        }
    }
}

/// Reads `/proc/net/route`, `/proc/net/ipv6_route` and `/sys/class/net`,
/// or files and a folder laid out like them.
#[derive(Clone, Debug)]
pub struct KernelLink {
    route: PathBuf,
    ipv6_route: PathBuf,
    interfaces: PathBuf,
    timeout: Duration,
    bus: Bus,
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
            bus: Bus::System,
        }
    }

    /// Asks the NetworkManager on the bus at `address` for the network's
    /// name, in place of the one on the system bus.
    pub fn with_bus(self, address: impl Into<String>) -> Self {
        Self {
            bus: Bus::At(address.into()),
            ..self
        }
    }
}

#[async_trait]
impl NetworkLink for KernelLink {
    async fn link(&self) -> Result<Link, PlatformError> {
        let reader = self.clone();
        source::blocking(&self.route, self.timeout, move || reader.read()).await
    }

    async fn network_name(&self) -> Result<Option<CurrentNetworkName>, PlatformError> {
        let reader = self.clone();
        source::blocking(&self.route, self.timeout, move || reader.read_name()).await
    }
}

impl KernelLink {
    /// The kind of the interface the default route of lowest metric leaves
    /// by, of either family; [`Link::NoRoute`] when there is none.
    fn read(&self) -> Result<Link, PlatformError> {
        match self.default_interface()? {
            Some(interface) => self.kind(&interface),
            None => Ok(Link::NoRoute),
        }
    }

    /// The name of the Wi-Fi network the default route's interface has
    /// joined; `None` when that interface isn't Wi-Fi. The kernel says what
    /// the interface is, and NetworkManager is asked only for a Wi-Fi one, so
    /// a wired machine without NetworkManager has no network, as on macOS.
    /// No default route is an error, not "no network": the link was read as
    /// Wi-Fi a moment ago, and a rule's `except` must not hold on a route
    /// that has just gone.
    fn read_name(&self) -> Result<Option<CurrentNetworkName>, PlatformError> {
        let Some(interface) = self.default_interface()? else {
            return Err(PlatformError::Unreadable {
                from: self.route.display().to_string(),
                detail: "no default route".to_owned(),
            });
        };
        if self.kind(&interface)? != Link::WiFi {
            return Ok(None);
        }
        network_manager::current_network(&self.bus, &interface)
    }

    /// The interface the default route of lowest metric leaves by, of either
    /// family; `None` when there is none.
    fn default_interface(&self) -> Result<Option<String>, PlatformError> {
        let Some(ipv4) = source::table(&self.route)? else {
            return Err(PlatformError::Unreadable {
                from: self.route.display().to_string(),
                detail: "no such file".to_owned(),
            });
        };
        let mut best = ipv4_default(&self.route, &ipv4)?;
        // A machine with IPv6 switched off has no table for it.
        if let Some(ipv6) = source::table(&self.ipv6_route)?
            && let Some(route) = ipv6_default(&self.ipv6_route, &ipv6)?
            && best.as_ref().is_none_or(|(metric, _)| route.0 < *metric)
        {
            best = Some(route);
        }
        Ok(best.map(|(_, interface)| interface))
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

/// Asks `route -n get default`, `networksetup -listallhardwareports` and
/// `system_profiler -json SPAirPortDataType` on macOS, or programs that
/// answer as they do.
#[derive(Clone, Debug)]
pub struct RouteLink {
    route: Program,
    ports: Program,
    profile: Program,
}

impl RouteLink {
    pub fn system() -> Self {
        Self::running(
            ("/sbin/route", ["-n", "get", "default"].as_slice()),
            (
                "/usr/sbin/networksetup",
                ["-listallhardwareports"].as_slice(),
            ),
            (
                "/usr/sbin/system_profiler",
                ["-json", "SPAirPortDataType"].as_slice(),
            ),
        )
    }

    /// Runs `route`, then `ports` for the link, or `profile` for the
    /// network's name, each a program and its arguments, with no
    /// environment, and reads what they print as `route -n get default`,
    /// `networksetup -listallhardwareports` and `system_profiler -json
    /// SPAirPortDataType` print it. `profile` has 15 s and 1 MiB.
    pub fn running<A: Into<OsString> + Clone>(
        route: (impl Into<PathBuf>, &[A]),
        ports: (impl Into<PathBuf>, &[A]),
        profile: (impl Into<PathBuf>, &[A]),
    ) -> Self {
        Self {
            route: Program::new(route.0, route.1.iter().cloned()),
            ports: Program::new(ports.0, ports.1.iter().cloned()),
            profile: Program::new(profile.0, profile.1.iter().cloned())
                .with_limits(PROFILE_TIMEOUT, MAX_PROFILE_BYTES),
        }
    }

    /// The interface the default route leaves by.
    async fn interface(&self) -> Result<String, PlatformError> {
        let route = self.route.output().await?;
        route
            .lines()
            .find_map(|line| line.trim().strip_prefix("interface:"))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| unrecognized(self.route.path.as_path(), "no interface"))
    }
}

#[async_trait]
impl NetworkLink for RouteLink {
    async fn link(&self) -> Result<Link, PlatformError> {
        let interface = self.interface().await?;
        let ports = self.ports.output().await?;
        hardware_port(self.ports.path.as_path(), &ports, &interface)
    }

    /// `None` when the default route's interface isn't the Wi-Fi port.
    async fn network_name(&self) -> Result<Option<CurrentNetworkName>, PlatformError> {
        let interface = self.interface().await?;
        let ports = self.ports.output().await?;
        if hardware_port(self.ports.path.as_path(), &ports, &interface)? != Link::WiFi {
            return Ok(None);
        }
        let profile = self.profile.output().await?;
        current_network(self.profile.path.as_path(), &profile, &interface)
    }
}

/// The name of the network the Wi-Fi port `interface` has joined, from what
/// `system_profiler -json SPAirPortDataType` prints: `None` when the port's
/// block has no current network, and an error when there is no block for it. A name absent from that block, empty, longer than 32
/// bytes, `<redacted>` or not text is withheld: an error, never `None`, so a
/// rule's `except` can't hold on a name the platform kept back (decision 80).
/// No error repeats what was read: the report names the networks around.
fn current_network(
    from: &Path,
    profile: &str,
    interface: &str,
) -> Result<Option<CurrentNetworkName>, PlatformError> {
    use serde_json::Value;
    let report: Value =
        serde_json::from_str(profile).map_err(|_| unrecognized(from, "a report not JSON"))?;
    let blocks = report
        .get("SPAirPortDataType")
        .and_then(Value::as_array)
        .ok_or_else(|| unrecognized(from, "no SPAirPortDataType"))?;
    let mut matching = blocks
        .iter()
        .filter_map(|block| block.get("spairport_airport_interfaces"))
        .filter_map(Value::as_array)
        .flatten()
        .filter(|entry| entry.get("_name").and_then(Value::as_str) == Some(interface));
    let (Some(entry), None) = (matching.next(), matching.next()) else {
        return Err(unrecognized(
            from,
            &format!("not one block for the interface {interface}"),
        ));
    };
    let Some(current) = entry.get("spairport_current_network_information") else {
        return Ok(None);
    };
    match current.get("_name").and_then(Value::as_str) {
        Some(REDACTED) => Err(unrecognized(from, "the network's name redacted")),
        Some(name) => CurrentNetworkName::new(name.as_bytes())
            .map(Some)
            .ok_or_else(|| unrecognized(from, "a network's name empty or over 32 bytes")),
        None => Err(unrecognized(from, "no network's name as text")),
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
        } else if let Some(device) = line.strip_prefix("Device:")
            && device.trim() == interface
        {
            return Ok(match port {
                Some("Wi-Fi" | "AirPort") => Link::WiFi,
                _ => Link::NotWiFi,
            });
        }
    }
    if seen {
        Ok(Link::Other)
    } else {
        Err(unrecognized(from, "no hardware port"))
    }
}

/// Whether this machine's reader tells a wired link from other links: not
/// on macOS, until a run on Ethernet has shown how a Mac names its wired
/// ports (decision 80), so a rule's `wired` never holds there.
pub fn tells_wired() -> bool {
    !cfg!(target_os = "macos")
}

/// NetworkManager's answer, over D-Bus: its read-only properties only
/// (`DBus.Properties.Get`), which its bus policy lets an unprivileged process
/// read (decision 80). No error repeats a property's value or the service's
/// own message, only the D-Bus error's name.
#[cfg(target_os = "linux")]
mod network_manager {
    use std::time::Duration;

    use app::ports::PlatformError;
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::{Connection, Proxy};
    use dbus::channel::{BusType, Channel};
    use domain::planning::CurrentNetworkName;

    use super::Bus;

    const SERVICE: &str = "org.freedesktop.NetworkManager";
    const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
    const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
    const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";

    /// `NM_DEVICE_TYPE_WIFI` of NetworkManager's D-Bus API.
    const DEVICE_TYPE_WIFI: u32 = 2;

    /// The most devices looked through: a machine has a handful.
    const MAX_DEVICES: usize = 256;

    /// How long one call may take; the whole read is bounded by the
    /// reader's timeout as well.
    const CALL_TIMEOUT: Duration = Duration::from_secs(2);

    /// The name of the Wi-Fi network the interface `interface` has joined,
    /// `None` when its access point is none (`/`).
    pub(super) fn current_network(
        bus: &Bus,
        interface: &str,
    ) -> Result<Option<CurrentNetworkName>, PlatformError> {
        let from = bus.from();
        let failed = |error: dbus::Error| PlatformError::Unreadable {
            from: from.clone(),
            detail: crate::source::detail(error.name().unwrap_or("a D-Bus error with no name")),
        };
        let unknown = |what: &str| PlatformError::Unrecognized {
            from: from.clone(),
            detail: what.to_owned(),
        };
        let connection = connect(bus).map_err(failed)?;
        let object = |path: dbus::Path<'static>| -> Proxy<'_, &Connection> {
            connection.with_proxy(SERVICE, path, CALL_TIMEOUT)
        };
        let devices: Vec<dbus::Path<'static>> = object("/org/freedesktop/NetworkManager".into())
            .get(SERVICE, "Devices")
            .map_err(failed)?;
        if devices.len() > MAX_DEVICES {
            return Err(unknown(&format!("more than {MAX_DEVICES} devices")));
        }
        let mut found = None;
        for device in devices {
            let name: String = object(device.clone())
                .get(DEVICE, "Interface")
                .map_err(failed)?;
            if name == interface {
                if found.is_some() {
                    return Err(unknown(&format!(
                        "two devices for the interface {interface}"
                    )));
                }
                found = Some(device);
            }
        }
        let Some(device) = found else {
            return Err(unknown(&format!("no device for the interface {interface}")));
        };
        let kind: u32 = object(device.clone())
            .get(DEVICE, "DeviceType")
            .map_err(failed)?;
        if kind != DEVICE_TYPE_WIFI {
            return Err(unknown(&format!("the interface {interface} is not Wi-Fi")));
        }
        let access_point: dbus::Path<'static> = object(device)
            .get(WIRELESS, "ActiveAccessPoint")
            .map_err(failed)?;
        if &*access_point == "/" {
            return Ok(None);
        }
        let ssid: Vec<u8> = object(access_point)
            .get(ACCESS_POINT, "Ssid")
            .map_err(failed)?;
        CurrentNetworkName::new(&ssid)
            .map(Some)
            .ok_or_else(|| unknown("a network's name empty or over 32 bytes"))
    }

    /// A connection of its own to `bus`, closed when dropped.
    fn connect(bus: &Bus) -> Result<Connection, dbus::Error> {
        let channel = match bus {
            Bus::System => Channel::get_private(BusType::System)?,
            Bus::At(address) => {
                let mut channel = Channel::open_private(address)?;
                channel.register()?;
                channel
            }
        };
        Ok(Connection::from(channel))
    }
}

/// NetworkManager is a Linux service: elsewhere the kernel's reader has no
/// name to give.
#[cfg(not(target_os = "linux"))]
mod network_manager {
    use app::ports::PlatformError;
    use domain::planning::CurrentNetworkName;

    use super::Bus;

    pub(super) fn current_network(
        bus: &Bus,
        _interface: &str,
    ) -> Result<Option<CurrentNetworkName>, PlatformError> {
        Err(PlatformError::Unreadable {
            from: bus.from(),
            detail: "NetworkManager is read only on Linux".to_owned(),
        })
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
