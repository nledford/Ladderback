//! The pairing guide, the resource `ladderback://pairing` (§4.6): the steps
//! a person takes to pair a machine with its hub, where each is taken, and
//! the refusals each most often meets, by code, in the catalog's words. MCP
//! explains pairing; a person pairs and approves.

use api::{ErrorCode, MsgArg};
use l10n::{Localizer, MessageId, msg};
use serde_json::{Value, json};

/// The URI of the pairing guide as an MCP resource.
pub const PAIRING_RESOURCE: &str = "ladderback://pairing";

/// What `resources/list` says the pairing guide is.
pub const PAIRING_RESOURCE_DESCRIPTION: &str = "How a person pairs a machine with its hub: \
    make a pairing token on the hub, paste it into `ladderback pair` on the machine, and \
    approve the machine on the hub. Each step says where it is taken and what its common \
    refusals mean, by code. The pairing_state tool says whether this machine is paired.";

/// One step of pairing: its name, where it is taken, what the catalog says
/// of it, and the refusals it most often meets, in the order a person meets
/// them.
struct Step {
    name: &'static str,
    on: &'static str,
    says: MessageId,
    refusals: &'static [ErrorCode],
}

/// The steps. Making a token is refused from beyond loopback without
/// sign-in, and at the hub's cap of unused tokens. Pairing is refused by
/// the token, by the hub's registry, then by the connection. The machine
/// checks approval with `hub-status`, whose usual refusals follow.
const STEPS: [Step; 3] = [
    Step {
        name: "make-token",
        on: "hub",
        says: msg::CLI_MCP_PAIRING_MAKE_TOKEN,
        refusals: &[ErrorCode::PairingNeedsSignIn, ErrorCode::PairingTokenLimit],
    },
    Step {
        name: "pair",
        on: "machine",
        says: msg::CLI_MCP_PAIRING_PAIR,
        refusals: &[
            ErrorCode::TokenExpired,
            ErrorCode::TokenUsed,
            ErrorCode::TokenRevoked,
            ErrorCode::HostnameTaken,
            ErrorCode::PendingLimit,
            ErrorCode::PlainAddress,
            ErrorCode::HubUnreachable,
            ErrorCode::HubUntrusted,
            ErrorCode::HubPinMismatch,
            ErrorCode::HubConnectionLost,
        ],
    },
    Step {
        name: "approve",
        on: "hub",
        says: msg::CLI_MCP_PAIRING_APPROVE,
        refusals: &[
            ErrorCode::ProofRefused,
            ErrorCode::HubLinkLost,
            ErrorCode::ProtocolOutOfDate,
        ],
    },
];

/// The guide, said in `localizer`'s locale. The names only a real refusal
/// knows are placeholders a person reads as such.
pub fn guide(localizer: &Localizer) -> Value {
    let placeholders = [
        ("address", MsgArg::Text("<address>".to_owned())),
        ("hostname", MsgArg::Text("<hostname>".to_owned())),
        ("last_seen", MsgArg::Text("<when>".to_owned())),
    ];
    let steps: Vec<Value> = STEPS
        .iter()
        .map(|step| {
            let refusals: Vec<Value> = step
                .refusals
                .iter()
                .map(|&code| {
                    json!({
                        "code": code,
                        "says": localizer.error(code, &placeholders, None).into_text(),
                    })
                })
                .collect();
            json!({
                "step": step.name,
                "on": step.on,
                "says": localizer.message(step.says, &[]).into_text(),
                "refusals": refusals,
            })
        })
        .collect();
    json!({ "steps": steps })
}
