//! Whether this machine is paired with a hub, and with which (components
//! §4.6, `pairing_state`): read from its pin, which the machine writes only
//! once the hub has accepted its pairing. It never connects to the hub, so
//! asking it costs the hub nothing and moves no connection counter.

use std::sync::Arc;

use api::{PairedHubDto, PairingDto, TokenCertificateDto};
use domain::fleet::{Fingerprint, TokenTls};

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
    /// otherwise. A pin that can't be read is the port's error. Only what
    /// the pin holds in public is named: the address, the hub key's
    /// fingerprint and how the certificate is checked (I4).
    pub async fn read(&self) -> Result<PairingDto, IdentityError> {
        let Some(pin) = self.identity.pin().await? else {
            return Ok(PairingDto {
                paired: false,
                hub: None,
            });
        };
        let certificate = match pin.tls {
            TokenTls::Ca => TokenCertificateDto::Ca,
            TokenTls::Pinned(_) => TokenCertificateDto::Pinned,
        };
        Ok(PairingDto {
            paired: true,
            hub: Some(PairedHubDto {
                address: pin.address.as_str().to_owned(),
                fingerprint: Fingerprint::of_key(&pin.hub_key).to_string(),
                certificate,
            }),
        })
    }
}

#[cfg(test)]
mod tests;
