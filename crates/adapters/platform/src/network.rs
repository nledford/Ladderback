//! Whether the machine is online (technical design §5): the kernel's
//! network interfaces on Linux, `scutil --nwi` on macOS.
//!
//! The rule on Linux: online when an interface other than loopback is
//! switched on (`IFF_UP` in its `flags`) and its `operstate` is `up`, or
//! `unknown` with a carrier (`carrier` is 1), as a tunnel or a
//! point-to-point link reports. `flags` in sysfs holds only what the
//! interface was set to, never `IFF_RUNNING`, which the kernel works out for
//! `ioctl` and netlink alone. On macOS: online when the system's network
//! reachability says reachable without a connection still to make.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use app::ports::{Network, PlatformError};
use async_trait::async_trait;

use crate::source::{self, Program, attribute, entries, unrecognized};

/// `IFF_UP` and `IFF_LOOPBACK` of Linux's `if.h`.
const IFF_UP: u32 = 0x1;
const IFF_LOOPBACK: u32 = 0x8;

/// `kSCNetworkReachabilityFlagsReachable` and `…ConnectionRequired`.
const REACHABLE: u32 = 0x2;
const CONNECTION_REQUIRED: u32 = 0x4;

/// Reads `/sys/class/net`, or a folder laid out like it.
#[derive(Clone, Debug)]
pub struct SysfsNetwork {
    root: PathBuf,
    timeout: Duration,
}

impl SysfsNetwork {
    /// Where the Linux kernel lists the machine's network interfaces.
    pub const ROOT: &str = "/sys/class/net";

    pub fn system() -> Self {
        Self::at(Self::ROOT)
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            timeout: source::TIMEOUT,
        }
    }

    /// How long reading the folder may take before the answer is given up.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }
}

#[async_trait]
impl Network for SysfsNetwork {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        let root = self.root.clone();
        source::blocking(&self.root, self.timeout, move || read_interfaces(&root)).await
    }
}

fn read_interfaces(root: &Path) -> Result<bool, PlatformError> {
    // Every Linux machine lists its loopback interface: no folder is no
    // answer, not "offline".
    let Some(entries) = entries(root)? else {
        return Err(PlatformError::Unreadable {
            from: root.display().to_string(),
            detail: "no such folder".to_owned(),
        });
    };
    for entry in entries {
        if interface_online(&entry)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the interface at `entry` is one this machine is online through.
/// An interface without `flags` or `operstate` is passed over.
fn interface_online(entry: &Path) -> Result<bool, PlatformError> {
    let Some(text) = attribute(entry, "flags")? else {
        return Ok(false);
    };
    let flags = hex(&text).ok_or_else(|| unrecognized(&entry.join("flags"), &text))?;
    if flags & IFF_UP == 0 || flags & IFF_LOOPBACK != 0 {
        return Ok(false);
    }
    let Some(state) = attribute(entry, "operstate")? else {
        return Ok(false);
    };
    // RFC 2863's states, as the kernel's `operstates` table writes them.
    match state.as_str() {
        "up" => Ok(true),
        // Read only here: on an interface switched off the kernel refuses
        // to say.
        "unknown" => match attribute(entry, "carrier")?.as_deref() {
            None | Some("0") => Ok(false),
            Some("1") => Ok(true),
            Some(other) => Err(unrecognized(&entry.join("carrier"), other)),
        },
        "down" | "lowerlayerdown" | "dormant" | "testing" | "notpresent" => Ok(false),
        other => Err(unrecognized(&entry.join("operstate"), other)),
    }
}

/// A `0x`-prefixed hexadecimal number that fits in 32 bits.
fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

/// Asks `scutil --nwi` on macOS, or a program that answers as it does.
#[derive(Clone, Debug)]
pub struct ScutilNetwork {
    program: Program,
}

impl ScutilNetwork {
    pub fn system() -> Self {
        Self::running("/usr/sbin/scutil", ["--nwi"])
    }

    /// Runs `program` with `args`, with no environment, and reads what it
    /// prints as `scutil --nwi` prints it.
    pub fn running<A: Into<OsString>>(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = A>,
    ) -> Self {
        Self {
            program: Program::new(program, args),
        }
    }

    /// How long the program may take before it is stopped.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.program.timeout = timeout;
        self
    }
}

#[async_trait]
impl Network for ScutilNetwork {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        let output = self.program.output().await?;
        let from = self.program.path.as_path();
        let mut seen = false;
        for line in output.lines() {
            let Some(rest) = line.trim().strip_prefix("REACH : flags ") else {
                continue;
            };
            let word = rest.split_whitespace().next().unwrap_or_default();
            let flags = hex(word).ok_or_else(|| unrecognized(from, line.trim()))?;
            seen = true;
            if flags & REACHABLE != 0 && flags & CONNECTION_REQUIRED == 0 {
                return Ok(true);
            }
        }
        if seen {
            Ok(false)
        } else {
            Err(unrecognized(from, "no reachability"))
        }
    }
}

/// This machine's network: `scutil` on macOS, the kernel's elsewhere.
pub fn system_network() -> Arc<dyn Network> {
    if cfg!(target_os = "macos") {
        Arc::new(ScutilNetwork::system())
    } else {
        Arc::new(SysfsNetwork::system())
    }
}
