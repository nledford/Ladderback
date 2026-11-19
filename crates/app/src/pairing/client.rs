use std::sync::Arc;

use domain::AgentId;
use domain::fleet::{Hostname, HubRoute, PairingToken, RouteRefused};

use super::agent::{AgentPairing, AgentPairingError, PairedHub};
use super::wire::{HubMessage, connect_frame, hub_message, pair_frame};
use crate::ports::{HubConnector, LinkError};

/// `ladderback pair`'s use case (§16, decision 121): it checks the token's
/// address, connects, and carries [`AgentPairing`]'s messages to the hub and
/// the hub's back, one connection, one pairing.
pub struct PairWithHub {
    pairing: AgentPairing,
    connector: Arc<dyn HubConnector>,
}

/// Why `pair` didn't pair. None carries anything of the token (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairFailure {
    /// The token's address can't be connected to; nothing was sent.
    Route(RouteRefused),
    /// The connection didn't open or ended.
    Link(LinkError),
    /// The hub sent a message out of turn, or one that doesn't read: the
    /// connection is treated as dropped, and nothing in it is acted on.
    OutOfTurn,
    Pairing(AgentPairingError),
}

impl From<LinkError> for PairFailure {
    fn from(error: LinkError) -> Self {
        todo!()
    }
}

impl From<AgentPairingError> for PairFailure {
    fn from(error: AgentPairingError) -> Self {
        todo!()
    }
}

impl PairWithHub {
    pub fn new(pairing: AgentPairing, connector: Arc<dyn HubConnector>) -> Self {
        todo!()
    }

    /// Pairs this machine, as `agent_id` under `hostname`, with the hub
    /// `token` names. The pin, the route and the hub's proof are checked
    /// before anything of the token leaves the machine.
    pub async fn pair(
        &self,
        token: PairingToken,
        agent_id: AgentId,
        hostname: Hostname,
    ) -> Result<PairedHub, PairFailure> {
        todo!()
    }
}

fn read(frame: api::sync::HubFrame) -> Result<HubMessage, PairFailure> {
        todo!()
    }

#[cfg(test)]
mod tests;
