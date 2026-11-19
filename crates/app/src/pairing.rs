//! Pairing an agent with its hub (§7.1; §16, decision 120, "The handshake",
//! "Pairing" and "Pending approval"): the hub's side and the agent's, one
//! step per message, so the transport that carries the messages holds no
//! rule of its own.

mod agent;
mod hub;

pub use agent::{AgentHandshake, AgentPairing, AgentPairingError, AwaitingPaired, PairedHub};
pub use hub::{HubHandshake, HubKeys, HubPairing, HubPairingError};

#[cfg(test)]
mod tests;
