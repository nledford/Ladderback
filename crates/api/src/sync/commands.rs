//! The payloads of the hub's commands to a machine and of the machine's
//! answers (§7.2; §16, decisions 120 and 176). A field a later release adds
//! is read past.

use serde::{Deserialize, Serialize};

use super::{UnreadableFrame, written};

/// The payload of the hub's `RunPlan`: the command's ID (a UUID), the
/// hub's time when it was asked, in seconds since the Unix epoch, the plan
/// to back up, and the tag of the hub session that asked, left out with
/// sign-in off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunPlanPayload {
    pub command_id: String,
    pub requested_at: u64,
    pub plan_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// The payload of the hub's `CancelOp`: as [`RunPlanPayload`], with the
/// operation to cancel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOpPayload {
    pub command_id: String,
    pub requested_at: u64,
    pub op_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// The payload of the machine's `CommandResult`: the command it answers,
/// and `started` with the operation that runs the plan (a new one, or the
/// one already running), `canceled` with the operation canceled, or
/// `refused` with an error code's wire name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandResultPayload {
    pub command_id: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// With `command-rate-limited`: when, in seconds since the Unix epoch
    /// on the machine's clock, the hub may ask again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<u64>,
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

payload_bytes!(RunPlanPayload, CancelOpPayload, CommandResultPayload);
