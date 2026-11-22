//! This machine's identity as the operating system and the data folder hold
//! it (technical design §7.1; §16, decision 144; #296): the machine
//! identifier the system gives the computer, the one recorded beside the
//! machine ID, and a new machine ID for a copied data folder.

use domain::AgentId;
use domain::fleet::MachineIdentifier;

/// Why the data folder's record couldn't be read or written, or the
/// system's identifier couldn't be read: the system's own words, for the
/// daemon's log only. It names a file or a command, never a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineError {
    pub detail: String,
}

/// Reads the machine identifier the operating system gives this computer,
/// and keeps the one recorded beside the machine ID in the data folder.
pub trait ThisMachine: Send + Sync {
    /// The identifier the operating system gives this computer now:
    /// `/etc/machine-id` on Linux, `IOPlatformUUID` on macOS. Two reads on
    /// one computer agree.
    fn identifier(&self) -> Result<MachineIdentifier, MachineError>;

    /// The identifier recorded beside the machine ID, or `None` when none
    /// is, as in a data folder from before it was kept. A record that holds
    /// no identifier is an error, never `None`.
    fn recorded(&self) -> Result<Option<MachineIdentifier>, MachineError>;

    /// Records `identifier` beside the machine ID, replacing any recorded
    /// before, durably: a crash after it returns can't bring the old one
    /// back.
    fn record(&self, identifier: &MachineIdentifier) -> Result<(), MachineError>;

    /// Replaces the machine ID with a new one and records `identifier`
    /// beside it, answering the new ID. Each call gives an ID never given
    /// before.
    fn renew(&self, identifier: &MachineIdentifier) -> Result<AgentId, MachineError>;
}
