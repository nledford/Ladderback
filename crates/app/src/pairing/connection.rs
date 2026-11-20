use std::sync::Arc;

use domain::AgentId;
use domain::fleet::{Approval, Hostname, HubPin, PublicKey, Refused, RouteRefused, WelcomeRefused};

use super::agent::ChallengeRefused;
use crate::ports::{
    Clock, HubConnector, Identity, IdentityError, LinkError, Random, RandomUnavailable, Signing,
    SigningError,
};

/// A paired agent connecting to its hub (§7.1, §7.2; §16, decision 120,
/// "The handshake" and "Signed messages after the handshake"): it checks the
/// hub against its pin before it proves anything, proves itself with its
/// agent key and connection counter, says `Hello`, and takes the hub's
/// signed `Welcome`. One connection, one greeting: the link that stays open
/// is #568's.
pub struct ConnectToHub {
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
    connector: Arc<dyn HubConnector>,
    /// The agent's build, as `--version` names it, which `Hello` carries.
    version: String,
}

/// A connection both sides proved, and what the hub's `Welcome` said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    /// Whether a person has approved this agent on the hub.
    pub approval: Approval,
    /// The pin the agent holds, its generation raised to the hub's.
    pub pin: HubPin,
    /// The agent's own key, whose fingerprint the person compares with the
    /// one the hub shows.
    pub agent_key: PublicKey,
}

/// Why the agent didn't connect. None carries a seed (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectFailure {
    /// The agent holds no pin or no agent key: it never paired.
    NotPaired,
    /// The pinned address can't be connected to; nothing was sent.
    Route(RouteRefused),
    /// The connection didn't open or ended.
    Link(LinkError),
    /// The hub sent a message out of turn, or one that doesn't read: the
    /// connection is treated as dropped, and nothing in it is acted on.
    OutOfTurn,
    /// The hub's `Challenge` didn't prove the pinned hub: the agent sent
    /// no proof.
    Challenge(ChallengeRefused),
    /// An answer the hub didn't sign on this connection, or signed out of
    /// sequence: treated as a dropped connection, and nothing in it is
    /// acted on.
    Unsigned,
    /// The hub refused the agent's proof, signed.
    Refused(Refused),
    /// The hub's `Welcome` held, and the agent acts on none of it.
    Welcome(WelcomeRefused),
    Identity(IdentityError),
    Signing(SigningError),
    Random(RandomUnavailable),
}

impl ConnectToHub {
    pub fn new(
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
        connector: Arc<dyn HubConnector>,
        version: String,
    ) -> Self {
        Self {
            identity,
            signing,
            random,
            clock,
            connector,
            version,
        }
    }

    /// Connects this machine, as `agent_id` under `hostname`, to the hub it
    /// pinned at pairing, and says where the hub holds it.
    pub async fn connect(
        &self,
        agent_id: AgentId,
        hostname: Hostname,
    ) -> Result<Connected, ConnectFailure> {
        let _ = (
            agent_id,
            hostname,
            &self.identity,
            &self.signing,
            &self.random,
            &self.clock,
            &self.connector,
            &self.version,
        );
        todo!("connect")
    }
}

#[cfg(test)]
mod tests;
