//! What powers the machine (technical design §5): the kernel's power
//! supplies on Linux, `pmset` on macOS.
//!
//! The rule on Linux: the machine is on battery only when no supply that
//! brings power in (mains, USB, wireless) is online and one of the machine's
//! own batteries, or a UPS, is discharging. A peripheral's battery (`scope`
//! `Device`, a mouse) doesn't count, and a machine that reports nothing, a
//! desktop, is on AC power.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use app::ports::{PlatformError, PowerSupply};
use async_trait::async_trait;
use domain::execution::conditions::PowerSource;

use crate::source::{self, Program, attribute, entries, unrecognized};

/// Reads `/sys/class/power_supply`, or a folder laid out like it.
#[derive(Clone, Debug)]
pub struct SysfsPower {
    root: PathBuf,
    timeout: Duration,
}

impl SysfsPower {
    /// Where the Linux kernel lists the machine's power supplies.
    pub const ROOT: &str = "/sys/class/power_supply";

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
impl PowerSupply for SysfsPower {
    async fn source(&self) -> Result<PowerSource, PlatformError> {
        let root = self.root.clone();
        source::blocking(&self.root, self.timeout, move || read_supplies(&root)).await
    }
}

fn read_supplies(root: &Path) -> Result<PowerSource, PlatformError> {
    let Some(entries) = entries(root)? else {
        return Ok(PowerSource::Ac);
    };
    let mut discharging = false;
    for entry in entries {
        let Some(kind) = attribute(&entry, "type")? else {
            continue;
        };
        if attribute(&entry, "scope")?.as_deref() == Some("Device") {
            continue;
        }
        if kind == "Battery" || kind == "UPS" {
            match attribute(&entry, "status")?.as_deref() {
                Some("Discharging") => discharging = true,
                None | Some("Charging" | "Full" | "Not charging" | "Unknown") => {}
                Some(other) => return Err(unrecognized(&entry.join("status"), other)),
            }
        } else if let Some(online) = attribute(&entry, "online")? {
            match online.parse::<u32>() {
                Ok(0) => {}
                Ok(_) => return Ok(PowerSource::Ac),
                Err(_) => return Err(unrecognized(&entry.join("online"), &online)),
            }
        }
    }
    Ok(if discharging {
        PowerSource::Battery
    } else {
        PowerSource::Ac
    })
}

/// Asks `pmset -g batt` on macOS, or a program that answers as it does.
#[derive(Clone, Debug)]
pub struct PmsetPower {
    program: Program,
}

impl PmsetPower {
    pub fn system() -> Self {
        Self::running("/usr/bin/pmset", ["-g", "batt"])
    }

    /// Runs `program` with `args`, with no environment, and reads what it
    /// prints as `pmset -g batt` prints it.
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
impl PowerSupply for PmsetPower {
    async fn source(&self) -> Result<PowerSource, PlatformError> {
        let output = self.program.output().await?;
        let from = self.program.path.as_path();
        let drawing = output
            .lines()
            .find_map(|line| line.trim().strip_prefix("Now drawing from '"))
            .and_then(|rest| rest.strip_suffix('\''))
            .ok_or_else(|| unrecognized(from, output.lines().next().unwrap_or_default()))?;
        match drawing {
            "AC Power" => Ok(PowerSource::Ac),
            "Battery Power" | "UPS Power" => Ok(PowerSource::Battery),
            other => Err(unrecognized(from, other)),
        }
    }
}

/// This machine's power supply: `pmset` on macOS, the kernel's elsewhere.
pub fn system_power() -> Arc<dyn PowerSupply> {
    if cfg!(target_os = "macos") {
        Arc::new(PmsetPower::system())
    } else {
        Arc::new(SysfsPower::system())
    }
}
