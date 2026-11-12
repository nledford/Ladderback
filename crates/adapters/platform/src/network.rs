//! Whether the machine is online (technical design §5): the kernel's
//! network interfaces on Linux, `scutil --nwi` on macOS.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use app::ports::{Network, PlatformError};
use async_trait::async_trait;

/// Reads `/sys/class/net`, or a folder laid out like it.
#[derive(Clone, Debug)]
pub struct SysfsNetwork {
    root: PathBuf,
}

impl SysfsNetwork {
    /// Where the Linux kernel lists the machine's network interfaces.
    pub const ROOT: &str = "/sys/class/net";

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
impl Network for SysfsNetwork {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        let _ = &self.root;
        todo!()
    }
}

/// Asks `scutil --nwi` on macOS, or a program that answers as it does.
#[derive(Clone, Debug)]
pub struct ScutilNetwork {}

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
impl Network for ScutilNetwork {
    async fn is_online(&self) -> Result<bool, PlatformError> {
        todo!()
    }
}

/// This machine's network: `scutil` on macOS, the kernel's elsewhere.
pub fn system_network() -> Arc<dyn Network> {
    todo!()
}
