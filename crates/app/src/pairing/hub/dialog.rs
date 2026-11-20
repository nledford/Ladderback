//! A pairing token asked for on the hub's web page (§16, decision 121, "The
//! hub's dialog"): a high-impact action, confirmed by a fresh step-up every
//! time and refused beyond loopback while sign-in is off (decision 120, "Who
//! may make a token, approve or re-key").

use std::fmt;
use std::net::IpAddr;

use domain::access::step_up::{StepUpConfirmation, StepUpFor};
use domain::fleet::{HubAddress, KeyId, PairingToken, PublicKey, TokenId, TokenTls, needs_sign_in};
use domain::{SignedDuration, Timestamp};

use super::{HubPairing, HubPairingError};
use crate::access::{StepUpRefused, confirm_step_up};

/// What the page asks for: a token for `address`, living `life` or 15
/// minutes, confirmed by `confirmation`.
pub struct TokenAsk {
    pub address: HubAddress,
    pub life: Option<SignedDuration>,
    /// The connection's address, whose sign-in waits a wrong password
    /// counts against.
    pub peer: IpAddr,
    /// Whether the request came from beyond loopback: its `Host` isn't a
    /// loopback name, or its peer isn't a loopback address.
    pub beyond_loopback: bool,
    /// The admin password with sign-in on, the hub key's key ID otherwise;
    /// `None` when nothing was typed, or more than any password. Never shown
    /// (I4).
    pub confirmation: Option<StepUpConfirmation>,
}

impl fmt::Debug for TokenAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenAsk")
            .field("address", &self.address)
            .field("life", &self.life)
            .field("beyond_loopback", &self.beyond_loopback)
            .finish_non_exhaustive()
    }
}

/// A token made for the page, shown once: its text, its token ID, the hub
/// key it carries, when it lapses, and how much of its life is left as the
/// hub answers.
#[derive(Clone, Debug)]
pub struct MadeToken {
    pub token: PairingToken,
    pub id: TokenId,
    pub hub_key: PublicKey,
    pub made_at: Timestamp,
    pub expires_at: Timestamp,
    pub left: SignedDuration,
}

/// Why no token was made. None carries the confirmation (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenAskRefused {
    /// The request came from beyond loopback while sign-in is off: the
    /// person sets an admin password first (`pairing-needs-sign-in`).
    NeedsSignIn,
    /// The confirmation wasn't the admin password, or the hub key's key ID
    /// with sign-in off.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left: the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own words.
    StepUpUnavailable(String),
    /// The hub couldn't make it.
    Hub(HubPairingError),
}

impl From<HubPairingError> for TokenAskRefused {
    fn from(error: HubPairingError) -> Self {
        Self::Hub(error)
    }
}

impl HubPairing {
    /// The hub key every agent pins, made while no agent is registered.
    pub async fn hub_key(&self) -> Result<PublicKey, HubPairingError> {
        Ok(*self.keys().await?.key())
    }

    /// A token for `ask`, made only once its step-up is confirmed: the
    /// admin password, in the peer's sign-in waits, with sign-in on; with it
    /// off, the hub key's key ID, and only on loopback. Beyond loopback with
    /// sign-in off, no confirmation helps, so none is read. Of the token the
    /// hub keeps only its pairing key's public half.
    pub async fn make_token_asked(&self, ask: TokenAsk) -> Result<MadeToken, TokenAskRefused> {
        if needs_sign_in(self.access.is_some(), ask.beyond_loopback) {
            return Err(TokenAskRefused::NeedsSignIn);
        }
        // Nothing typed, or more than any password, is no confirmation:
        // refused as a wrong one, in no sign-in wait.
        let Some(confirmation) = ask.confirmation else {
            return Err(TokenAskRefused::StepUpFailed);
        };
        let keys = self.keys().await?;
        let key_id = KeyId::of_key(keys.key());
        confirm_step_up(
            self.access.as_deref(),
            ask.peer,
            &confirmation,
            StepUpFor::HubKey(&key_id),
        )
        .await
        .map_err(|refused| match refused {
            StepUpRefused::Failed => TokenAskRefused::StepUpFailed,
            StepUpRefused::Wait { seconds } => TokenAskRefused::StepUpWait { seconds },
            StepUpRefused::Unavailable(text) => TokenAskRefused::StepUpUnavailable(text),
        })?;
        // Built-in TLS isn't served yet, so every token is of an address a
        // certificate authority's chain vouches for, or of loopback.
        let (token, record) = self
            .mint(&keys, ask.address, TokenTls::Ca, ask.life)
            .await?;
        Ok(MadeToken {
            token,
            id: TokenId::of_pairing_key(&record.pairing_key),
            hub_key: record.hub_key,
            made_at: record.made_at,
            expires_at: record.expires_at,
            left: record.made_at.duration_until(record.expires_at),
        })
    }
}
