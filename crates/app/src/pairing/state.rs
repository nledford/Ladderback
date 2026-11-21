//! Whether this machine is paired with a hub, and with which (components
//! §4.6, `pairing_state`): read from its pin, which the machine writes only
//! once the hub has accepted its pairing. It never connects to the hub, so
//! asking it costs the hub nothing and moves no connection counter.

use std::sync::Arc;

use api::{PairedHubDto, PairingDto};

use crate::ports::{Identity, IdentityError};

/// What a machine knows of its pairing, as `GET /api/v1/pairing` and the
/// in-process MCP backend answer it.
pub struct PairingState {
    identity: Arc<dyn Identity>,
}

impl PairingState {
    pub fn new(identity: Arc<dyn Identity>) -> Self {
        Self { identity }
    }

    /// Paired, and with which hub, when the machine holds a pin; not paired
    /// otherwise. A pin that can't be read is the port's error.
    pub async fn read(&self) -> Result<PairingDto, IdentityError> {
        let _ = (&self.identity, PairedHubDto::clone);
        todo!("read the pin")
    }
}

#[cfg(test)]
mod tests;
