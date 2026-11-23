//! The lines the hub logs when a machine pairs (#1307), when a person
//! approves one, when an approved one first connects (#1342), when it
//! first lets go of an agent's reports past the cap (§7.2), and when an
//! agent first sends a report far behind the last it took (#1387).

use domain::AgentId;
use domain::fleet::{
    AgentRecord, AgentStatus, Fingerprint, MAX_REPORTS_HELD, PairRequest, Pairing, ReportSeq,
};

/// The line for the first time `agent` sends a report the hub took before,
/// at `seq` of its outbox, further behind the last the hub took of its
/// epoch than a report sent again lies: its outbox numbers its reports
/// again from an older place, as one restored from an older database does,
/// and the hub keeps none of them (#1387). It names the agent and the
/// place only, nothing a report says (I4).
pub(super) fn reports_behind(agent: AgentId, seq: ReportSeq) -> String {
    format!(
        "agent {} sent a report at place {} of its outbox, far behind the last the hub took \
         of it: its database may have been restored from an older copy; the hub takes such \
         reports as held already, keeps none of them, and doesn't log the next ones",
        agent.as_uuid(),
        seq.get(),
    )
}

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

/// The line for the first time the hub lets go of `agent`'s oldest reports
/// to keep its last [`MAX_REPORTS_HELD`], `dropped` of them so far. It
/// names the agent and the count only, nothing a report says (I4).
pub(super) fn reports_dropped(agent: AgentId, dropped: u64) -> String {
    format!(
        "agent {} has more reports than the {MAX_REPORTS_HELD} the hub keeps of one machine: \
         its oldest {dropped} were let go of; the hub counts the next ones without logging each",
        agent.as_uuid(),
    )
}

/// The line for the first time the hub refuses a second computer under
/// `agent`'s ID since it started, as a copied data folder makes (decision
/// 150). It names the machine and the way back: the computer that should
/// keep the ID is taken once a person forgets the one the hub took. The
/// fingerprints stay out: they tell a person nothing.
pub(super) fn second_machine(agent: &AgentRecord) -> String {
    let hostname = agent.hostname.as_str().escape_debug();
    format!(
        "refused a second computer connecting as {} ({}): the hub took another computer under \
         this machine ID first, as when a data folder is copied, and takes no reports from \
         this one; the next refusals aren't logged. If the computer refused is a copy, take a \
         new machine ID on its start page and pair it again. If it is the one that should keep \
         the ID, stop Ladderback on the other one or give that one a new ID, then stop the \
         hub's daemon and run `ladderback pairing-token --forget-computer {hostname}` on the hub",
        machine(agent),
        identity(agent),
    )
}

/// The line for a person forgetting the computer the hub took under
/// `agent`'s ID: the next to connect is taken in its place.
pub(super) fn forgot_machine(agent: &AgentRecord) -> String {
    format!(
        "{} ({}): the hub forgot the computer it took under this machine ID, and takes the \
         next one to connect",
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
