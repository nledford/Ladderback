//! Pairing an agent with its hub (§7.1; §16, decision 120, "The handshake",
//! "Pairing" and "Pending approval"): the hub's side and the agent's, one
//! step per message, so the transport that carries the messages holds no
//! rule of its own.

mod agent;
mod client;
mod connection;
mod hub;
mod session;
mod wire;

pub use agent::{
    AgentHandshake, AgentPairing, AgentPairingError, AwaitingPaired, ChallengeRefused, PairedHub,
};
pub use client::{PairFailure, PairWithHub};
pub use connection::{ConnectFailure, ConnectToHub, Connected};
pub use hub::{
    HubHandshake, HubKeys, HubPairing, HubPairingError, MadeToken, MessageRefused, ProofAnswer,
    TokenAsk, TokenAskRefused,
};
pub use session::{BINDING_UNAVAILABLE, HubSession, HubStep, PROTOCOL_ERROR, SEQUENCE_ERROR};

#[cfg(test)]
mod tests;
