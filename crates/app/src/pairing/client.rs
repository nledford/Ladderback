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
        Self::Link(error)
    }
}

impl From<AgentPairingError> for PairFailure {
    fn from(error: AgentPairingError) -> Self {
        Self::Pairing(error)
    }
}

impl PairWithHub {
    pub fn new(pairing: AgentPairing, connector: Arc<dyn HubConnector>) -> Self {
        Self { pairing, connector }
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
        let route = HubRoute::of(token.address(), token.tls()).map_err(PairFailure::Route)?;
        let (connect, handshake) = self.pairing.connect(token, agent_id, hostname).await?;
        let mut link = self.connector.connect(&route).await?;
        link.send(&connect_frame(&connect)).await?;
        let HubMessage::Challenge(challenge) = read(link.receive().await?)? else {
            return Err(PairFailure::OutOfTurn);
        };
        let (pair, awaiting) = self
            .pairing
            .answer_challenge(handshake, &challenge, link.exporter())
            .await?;
        link.send(&pair_frame(&pair)).await?;
        let HubMessage::Answer(answer) = read(link.receive().await?)? else {
            return Err(PairFailure::OutOfTurn);
        };
        Ok(self.pairing.take_answer(awaiting, &answer).await?)
    }
}

fn read(frame: api::sync::HubFrame) -> Result<HubMessage, PairFailure> {
    hub_message(&frame).map_err(|_| PairFailure::OutOfTurn)
}

#[cfg(test)]
mod tests;
