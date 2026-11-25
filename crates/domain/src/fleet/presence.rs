//! A paired machine's lasting connection to its hub (technical design §7.2;
//! §16, decision 159): an approved machine holds one connection open while
//! it runs and sends a heartbeat on it every [`HEARTBEAT_EVERY`], which the
//! hub answers, so each side learns within a bound that the other is gone.
//! The hub closes a connection that sent nothing for [`SILENCE_LIMIT`], and
//! a machine is online while the hub holds its connection.

use super::WireName;
use crate::SignedDuration;

/// How often an approved machine sends its hub a heartbeat while it holds
/// its connection.
pub const HEARTBEAT_EVERY: SignedDuration = SignedDuration::from_secs(30);

/// How long the hub keeps a machine's connection that sent nothing: three
/// heartbeats missed in a row.
pub const SILENCE_LIMIT: SignedDuration = SignedDuration::from_secs(90);

/// The kind of the machine's heartbeat.
pub fn heartbeat_kind() -> WireName {
    WireName::known("Heartbeat")
}

/// The kind of the hub's answer to a heartbeat.
pub fn heartbeat_ack_kind() -> WireName {
    WireName::known("HeartbeatAck")
}
