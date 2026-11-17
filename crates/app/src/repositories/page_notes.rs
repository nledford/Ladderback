//! What the Repositories page notes above its list, of the machine rather
//! than of one repository (security §10.2): on Linux, a keyring service that
//! runs now but that nothing can start on demand, while a repository keeps
//! its password in it (#923). Asked only when the page asks, never before a
//! run.

use api::{WarningCode, WarningDto};
use domain::planning::AgentConfig;

use crate::ports::{Keyring, KeyringActivation};

/// The page's notes: `keyring-not-activatable` while `keyring` says its
/// service runs but can't be started on demand and a repository of `config`
/// keeps its password in it, and nothing otherwise. With no password in the
/// keyring no run reads it, so `keyring` isn't asked. A keyring started on
/// demand, none at all, or an answer that couldn't be had is noted nothing:
/// a false alarm would send a person to repair what isn't broken.
pub async fn page_notes(keyring: &dyn Keyring, config: &AgentConfig) -> Vec<WarningDto> {
    if config.keyring_passwords() == 0 {
        return Vec::new();
    }
    match keyring.activation().await {
        KeyringActivation::RunningOnly => {
            vec![WarningDto::new(WarningCode::KeyringNotActivatable, [])]
        }
        KeyringActivation::OnDemand | KeyringActivation::Absent | KeyringActivation::Unknown => {
            Vec::new()
        }
    }
}
