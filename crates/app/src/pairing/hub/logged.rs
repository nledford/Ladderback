//! The lines the hub logs when a machine pairs (#1307), when a person
//! approves one, and when an approved one first connects (#1342).

use domain::fleet::{AgentRecord, AgentStatus, Fingerprint, PairRequest, Pairing};

/// What a person at the hub's terminal or in its log matches against the
/// machine's own screen: the machine's hostname, between double quotes and
/// escaped with `escape_debug` as the other-hosts warning escapes a host
/// name from outside, its agent ID and key fingerprint, and whether it waits
/// for approval. The hostname is the one the hub holds, which its pages
/// list, followed by the one asked for when that isn't it, as the machine's
/// own line names both (#1342). The token, the pairing key and every seed
/// stay out (I4).
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
        Pairing::Repeated(_) => ("repeated its pairing", "nothing changed"),
    };
    let held = &outcome.record().hostname;
    let name = if *held == request.hostname {
        format!("\"{}\"", held.as_str().escape_debug())
    } else {
        format!(
            "\"{}\" (asked as \"{}\")",
            held.as_str().escape_debug(),
            request.hostname.as_str().escape_debug(),
        )
    };
    format!(
        "machine {name} {what} as agent {} with key {}; {then}",
        request.agent_id.as_uuid(),
        Fingerprint::of_key(&request.agent_key),
    )
}

/// The line for `agent`, which a person has just approved, named as a
/// pairing names it, so it reads beside the line of that pairing.
pub(super) fn approved(agent: &AgentRecord) -> String {
    format!("{} approved as {}", machine(agent), identity(agent))
}

/// The line for `agent`'s first connection since a person approved it,
/// named as a pairing names it.
pub(super) fn connected(agent: &AgentRecord) -> String {
    format!(
        "{} connected as {}, its first connection since it was approved",
        machine(agent),
        identity(agent),
    )
}

fn machine(agent: &AgentRecord) -> String {
    format!("machine \"{}\"", agent.hostname.as_str().escape_debug())
}

fn identity(agent: &AgentRecord) -> String {
    format!(
        "agent {} with key {}",
        agent.agent_id.as_uuid(),
        Fingerprint::of_key(&agent.key),
    )
}

#[cfg(test)]
mod tests;
