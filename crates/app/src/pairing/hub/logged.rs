//! The line the hub logs when a machine pairs (#1307).

use domain::fleet::{AgentStatus, Fingerprint, PairRequest, Pairing};

/// What a person at the hub's terminal or in its log matches against the
/// machine's own screen: the machine's hostname, escaped as the log escapes
/// outside text, its agent ID and key fingerprint, and whether it waits for
/// approval. The token, the pairing key and every seed stay out (I4).
pub(super) fn paired(request: &PairRequest, outcome: &Pairing) -> String {
    let (what, then) = match outcome {
        Pairing::New(_) => ("paired", "it waits for approval"),
        Pairing::Again(agent) => (
            "paired again",
            match agent.status {
                AgentStatus::Pending { .. } => "it still waits for approval",
                AgentStatus::Approved { .. } => "it is approved already",
            },
        ),
        Pairing::Repeated => ("repeated its pairing", "nothing changed"),
    };
    format!(
        "machine \"{}\" {what} as agent {} with key {}; {then}",
        request.hostname.as_str().escape_debug(),
        request.agent_id.as_uuid(),
        Fingerprint::of_key(&request.agent_key),
    )
}

#[cfg(test)]
mod tests;
