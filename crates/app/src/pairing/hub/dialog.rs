//! A pairing token asked for, and a waiting agent approved, on the hub's
//! web page (§16, decision 121, "The hub's dialog", "Waiting for
//! approval"): high-impact actions, each confirmed by a fresh step-up every
//! time and refused beyond loopback while sign-in is off (decision 120,
//! "Who may make a token, approve or re-key").

use std::fmt;
use std::net::IpAddr;

use domain::access::step_up::{StepUpConfirmation, StepUpFor};
use domain::fleet::{
    AgentRecord, ApprovalRefused, Fingerprint, HubAddress, ListedAgent, PairingToken, PublicKey,
    TokenId, TokenTls, needs_sign_in,
};
use domain::{AgentId, SignedDuration, Timestamp};

use super::{HubKeys, HubPairing, HubPairingError};
use crate::access::{StepUpRefused, confirm_step_up};

/// What the page asks for: a token for `address`, of `tls`'s kind, living
/// `life` or 15 minutes, confirmed by `confirmation`.
pub struct TokenAsk {
    pub address: HubAddress,
    /// The certificate the address shows: the hub's own, pinned, or one a
    /// certificate authority signed, as a proxy's is.
    pub tls: TokenTls,
    pub life: Option<SignedDuration>,
    /// The connection's address, whose sign-in waits a wrong password
    /// counts against.
    pub peer: IpAddr,
    /// Whether the request came from beyond loopback: its `Host` isn't a
    /// loopback name, or its peer isn't a loopback address.
    pub beyond_loopback: bool,
    /// The admin password with sign-in on, the hub key's fingerprint otherwise;
    /// `None` when nothing was typed, or more than any password. Never shown
    /// (I4).
    pub confirmation: Option<StepUpConfirmation>,
}

impl fmt::Debug for TokenAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenAsk")
            .field("address", &self.address)
            .field("tls", &self.tls)
            .field("life", &self.life)
            .field("beyond_loopback", &self.beyond_loopback)
            .finish_non_exhaustive()
    }
}

/// A token made for the page, shown once: its text, its token ID, the hub
/// key it carries, when it lapses, and how much of its life is left once
/// the hub has kept it, which the page counts down from.
#[derive(Clone, Debug)]
pub struct MadeToken {
    pub token: PairingToken,
    pub id: TokenId,
    pub hub_key: PublicKey,
    pub expires_at: Timestamp,
    pub left: SignedDuration,
}

/// Why no token was made. None carries the confirmation (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenAskRefused {
    /// The request came from beyond loopback while sign-in is off: the
    /// person sets an admin password first (`pairing-needs-sign-in`).
    NeedsSignIn,
    /// The confirmation wasn't the admin password, or the hub key's fingerprint
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

/// What the page asks for: the agent `agent_id`, under the key
/// `agent_key` the person compared with the machine's own screen, approved,
/// confirmed by `confirmation`.
pub struct ApprovalAsk {
    pub agent_id: AgentId,
    pub agent_key: PublicKey,
    /// The connection's address, whose sign-in waits a wrong password
    /// counts against.
    pub peer: IpAddr,
    /// Whether the request came from beyond loopback.
    pub beyond_loopback: bool,
    /// As [`TokenAsk::confirmation`]. Never shown (I4).
    pub confirmation: Option<StepUpConfirmation>,
}

impl fmt::Debug for ApprovalAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApprovalAsk")
            .field("agent_id", &self.agent_id)
            .field("agent_key", &self.agent_key)
            .field("peer", &self.peer)
            .field("beyond_loopback", &self.beyond_loopback)
            .finish_non_exhaustive()
    }
}

/// Every agent the hub has registered, as [`HubPairing::agents`] lists them,
/// and when they were read: how long each has waited is counted from `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentsListed {
    pub agents: Vec<ListedAgent>,
    pub at: Timestamp,
}

impl HubPairing {
    /// The hub key every agent pins, made while no agent is registered.
    pub async fn hub_key(&self) -> Result<PublicKey, HubPairingError> {
        Ok(*self.keys().await?.key())
    }

    /// A token for `ask`, made only once its step-up is confirmed: the
    /// admin password, in the peer's sign-in waits, with sign-in on; with it
    /// off, the hub key's fingerprint, and only on loopback. Beyond loopback with
    /// sign-in off, no confirmation helps, so none is read. Of the token the
    /// hub keeps only its pairing key's public half.
    pub async fn make_token_asked(&self, ask: TokenAsk) -> Result<MadeToken, TokenAskRefused> {
        let keys = self
            .stepped_up(ask.beyond_loopback, ask.peer, ask.confirmation)
            .await?;
        let (token, record) = self.mint(&keys, ask.address, ask.tls, ask.life).await?;
        // Counted once the token is kept, so the time keeping it took is
        // never shown as life it has left.
        let left = self
            .clock
            .now()
            .duration_until(record.expires_at)
            .max(SignedDuration::ZERO);
        Ok(MadeToken {
            token,
            id: TokenId::of_pairing_key(&record.pairing_key),
            hub_key: record.hub_key,
            expires_at: record.expires_at,
            left,
        })
    }

    /// The agent `ask` names approved, once its step-up is confirmed as a
    /// token's is: only that agent ID with that key, never another key the
    /// agent holds now (decision 120, "Pending approval"). Nothing about the
    /// agent is read before the step-up holds, so a wrong one learns nothing.
    pub async fn approve_asked(
        &self,
        ask: ApprovalAsk,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, TokenAskRefused> {
        self.stepped_up(ask.beyond_loopback, ask.peer, ask.confirmation)
            .await?;
        Ok(self.approve_now(ask.agent_id, ask.agent_key).await?)
    }

    /// [`HubPairing::agents`], with the time they were read.
    pub async fn agents_listed(&self) -> Result<AgentsListed, HubPairingError> {
        let agents = self.agents().await?;
        Ok(AgentsListed {
            agents,
            at: self.clock.now(),
        })
    }

    /// The hub's keys, once the step-up `confirmation` holds: the admin
    /// password, in the peer's sign-in waits, with sign-in on; with it off,
    /// the hub key's fingerprint, and only on loopback. Beyond loopback with
    /// sign-in off, no confirmation helps, so none is read.
    async fn stepped_up(
        &self,
        beyond_loopback: bool,
        peer: IpAddr,
        confirmation: Option<StepUpConfirmation>,
    ) -> Result<HubKeys, TokenAskRefused> {
        if needs_sign_in(self.access.is_some(), beyond_loopback) {
            return Err(TokenAskRefused::NeedsSignIn);
        }
        // Nothing typed, or more than any password, is no confirmation:
        // refused as a wrong one, in no sign-in wait.
        let Some(confirmation) = confirmation else {
            return Err(TokenAskRefused::StepUpFailed);
        };
        let keys = self.keys().await?;
        let fingerprint = Fingerprint::of_key(keys.key());
        confirm_step_up(
            self.access.as_deref(),
            peer,
            &confirmation,
            StepUpFor::HubKey(&fingerprint),
        )
        .await
        .map_err(|refused| match refused {
            StepUpRefused::Failed => TokenAskRefused::StepUpFailed,
            StepUpRefused::Wait { seconds } => TokenAskRefused::StepUpWait { seconds },
            StepUpRefused::Unavailable(text) => TokenAskRefused::StepUpUnavailable(text),
        })?;
        Ok(keys)
    }
}
