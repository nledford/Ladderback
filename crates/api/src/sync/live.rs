//! The payloads of what the agent tells its hub live (§7.2, #1440): the
//! snapshot of its plans, which the hub acknowledges, and the progress of an
//! operation it runs, which it doesn't.

use serde::{Deserialize, Serialize};

use super::{UnreadableFrame, written};

/// The payload of the agent's `PlansSnapshot`: its plans in its
/// configuration's order, at most `domain::fleet::MAX_PLANS_REPORTED`, and
/// how many more it has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlansPayload {
    pub plans: Vec<PlanPayload>,
    pub left_out: u64,
}

/// One plan of a `PlansSnapshot`: its ID, its last run and its next run, in
/// seconds since the Unix epoch, each left out when it has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPayload {
    pub plan: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastRunPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

/// A plan's last run: when it last changed, and its outcome by its wire
/// name (`succeeded`, `failed`, …).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRunPayload {
    pub at: u64,
    pub outcome: String,
}

/// The payload of the hub's `PlansAck`: it keeps the snapshot. Empty; the
/// signed message's place in the session says which snapshot it answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlansAckPayload {}

/// The payload of the agent's `Progress`: the latest progress of the
/// operation `op_id`, its step and unit by their wire names, how far it
/// came of its total, and when the agent heard it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressPayload {
    pub op_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub unit: String,
    pub current: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub at: u64,
}

macro_rules! payload_bytes {
    ($($payload:ty),+) => {$(
        impl $payload {
            /// The payload `bytes` hold.
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, UnreadableFrame> {
                serde_json::from_slice(bytes).map_err(|_| UnreadableFrame)
            }

            /// The payload's bytes, which its sender signs.
            pub fn to_bytes(&self) -> Vec<u8> {
                written(self).into_bytes()
            }
        }
    )+};
}

payload_bytes!(PlansPayload, PlansAckPayload, ProgressPayload);
