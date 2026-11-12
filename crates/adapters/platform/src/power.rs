//! What powers the machine (technical design §5): the kernel's power
//! supplies on Linux, `pmset` on macOS.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{PlatformError, PowerSupply};
use async_trait::async_trait;
use domain::execution::conditions::PowerSource;

/// Reads `/sys/class/power_supply`, or a folder laid out like it.
#[derive(Clone, Debug)]
pub struct SysfsPower {
    root: PathBuf,
}

impl SysfsPower {
    /// Where the Linux kernel lists the machine's power supplies.
    pub const ROOT: &str = "/sys/class/power_supply";

    pub fn system() -> Self {
        Self::at(Self::ROOT)
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let _ = root.into();
        todo!()
    }

    /// How long reading the folder may take before the answer is given up.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        let _ = timeout;
        todo!()
    }
}

#[async_trait]
impl PowerSupply for SysfsPower {
    async fn source(&self) -> Result<PowerSource, PlatformError> {
        let _ = &self.root;
        todo!()
    }
}

/// Asks `pmset -g batt` on macOS, or a program that answers as it does.
#[derive(Clone, Debug)]
pub struct PmsetPower {}

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
        let _ = (program.into(), args.into_iter().map(Into::into).count());
        todo!()
    }

    /// How long the program may take before it is stopped.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        let _ = timeout;
        todo!()
    }
}

#[async_trait]
impl PowerSupply for PmsetPower {
    async fn source(&self) -> Result<PowerSource, PlatformError> {
        todo!()
    }
}

/// This machine's power supply: `pmset` on macOS, the kernel's elsewhere.
pub fn system_power() -> Arc<dyn PowerSupply> {
    todo!()
}
