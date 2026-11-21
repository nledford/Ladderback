//! The hub's own routes under `/api/v1/hub` (§16, decisions 120 and 121):
//! its key, and the pairing tokens its page makes and cancels. Only a build
//! with the hub serves them (decision 122).

use serde::{Deserialize, Serialize};

/// Where the hub's key is read: `GET`, answered 404 by a daemon that isn't
/// a hub.
pub const HUB_PATH: &str = "/api/v1/hub";

/// Where the page makes a pairing token: `POST` a [`MakePairingTokenDto`].
pub const PAIRING_TOKENS_PATH: &str = "/api/v1/hub/pairing-tokens";

/// The hub's key as a person compares it: its fingerprint in full (20
/// Crockford base32 characters in groups of four) and its key ID, the 8 hex
/// digits a step-up with sign-in off asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubDto {
    pub fingerprint: String,
    pub key_id: String,
}

/// A pairing token asked for: the address agents will use, its life in
/// minutes (15 when absent, 5 to 1440), and the step-up confirmation: the
/// admin password with sign-in on, the hub key's key ID otherwise. The
/// confirmation is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MakePairingTokenDto {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life_minutes: Option<u32>,
    pub confirmation: String,
}

impl std::fmt::Debug for MakePairingTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MakePairingTokenDto")
            .field("address", &self.address)
            .field("life_minutes", &self.life_minutes)
            .finish_non_exhaustive()
    }
}

/// A pairing token made, shown once: its text, its token ID, the hub key's
/// fingerprint it carries, its address, when it lapses (RFC 3339) and how
/// many seconds it has left by the hub's clock, which the page counts down
/// on its own monotonic clock. Its `Debug` hides the text (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PairingTokenDto {
    pub token: String,
    pub token_id: String,
    pub fingerprint: String,
    pub address: String,
    pub expires_at: String,
    pub seconds_left: u64,
}

impl std::fmt::Debug for PairingTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingTokenDto")
            .field("token_id", &self.token_id)
            .field("fingerprint", &self.fingerprint)
            .field("address", &self.address)
            .field("expires_at", &self.expires_at)
            .field("seconds_left", &self.seconds_left)
            .finish_non_exhaustive()
    }
}
