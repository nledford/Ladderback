use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, Fingerprint, Hostname, PairRequest, Pairing, PublicKey,
    TokenTls, Tombstone,
};
use domain::{AgentId, Timestamp, Uuid};

use super::{approved as approved_line, connected, lapsed, paired, recovered, recovering, removed};

fn key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; 32]).expect("a key")
}

fn request(hostname: &str) -> PairRequest {
    PairRequest {
        pairing_key: key(0x11),
        agent_id: AgentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001)),
        agent_key: key(0x22),
        hostname: Hostname::parse(hostname).expect("a hostname"),
        zone: None,
        counter: 1,
        kind: BindingKind::of_token(TokenTls::Ca),
        hub_key: key(0x33),
    }
}

fn record(request: &PairRequest, status: AgentStatus) -> AgentRecord {
    AgentRecord {
        agent_id: request.agent_id,
        key: request.agent_key,
        hostname: request.hostname.clone(),
        status,
        counter: request.counter,
        last_seen: Timestamp::from_second(1_800_000_000).expect("a time"),
        zone: None,
    }
}

fn pending() -> AgentStatus {
    AgentStatus::Pending {
        since: Timestamp::from_second(1_800_000_000).expect("a time"),
    }
}

fn approved() -> AgentStatus {
    AgentStatus::Approved {
        at: Timestamp::from_second(1_800_000_000).expect("a time"),
    }
}

/// A new machine is named as the person at the hub matches it: hostname,
/// agent ID and key fingerprint, waiting for approval.
#[test]
fn a_new_machine_is_named_by_hostname_agent_id_and_key_and_waits_for_approval() {
    let request = request("e2e-laptop");
    let outcome = Pairing::New(record(&request, pending()));
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    assert_eq!(
        paired(&request, &outcome),
        format!(
            "machine \"e2e-laptop\" paired as agent 01900000-0000-7000-8000-000000000001 \
             with key {fingerprint}; it waits for approval"
        )
    );
}

/// A machine already known says whether it still waits or is approved
/// already, so a pairing nobody expected stands out from a routine one.
#[test]
fn a_machine_known_already_says_whether_it_waits_or_is_approved() {
    let request = request("e2e-laptop");
    let waiting = paired(&request, &Pairing::Again(record(&request, pending())));
    assert!(
        waiting.contains("paired again") && waiting.contains("it still waits for approval"),
        "{waiting}"
    );
    let known = paired(&request, &Pairing::Again(record(&request, approved())));
    assert!(
        known.contains("paired again") && known.contains("it is approved already"),
        "{known}"
    );
}

/// A pairing repeated on the token it spent, as when `Paired` was lost,
/// changes nothing, and says so.
#[test]
fn a_pairing_repeated_on_its_spent_token_says_nothing_changed() {
    let request = request("e2e-laptop");
    let line = paired(&request, &Pairing::Repeated(record(&request, approved())));
    assert!(
        line.contains("repeated its pairing") && line.contains("nothing changed"),
        "{line}"
    );
}

/// A hostname is what a machine says of itself: a quote or a backslash in
/// it can't end its quotes in the log, and no part of the pairing key, which
/// the token holds, is in the line (I4).
#[test]
fn a_hostname_is_escaped_and_no_secret_is_in_the_line() {
    let request = request("Nick's \"Mac\\Book\"");
    let line = paired(&request, &Pairing::New(record(&request, pending())));
    assert!(
        line.contains(r#"machine "Nick\'s \"Mac\\Book\"" paired"#),
        "{line}"
    );
    let pairing_key = Fingerprint::of_key(&request.pairing_key);
    assert!(!line.contains(&pairing_key.to_string()), "{line}");
    assert!(!line.contains(&pairing_key.full_hex()), "{line}");
}

/// The record the hub holds for `request`'s agent under `hostname`, the one
/// it first paired under.
fn held_as(request: &PairRequest, hostname: &str, status: AgentStatus) -> AgentRecord {
    AgentRecord {
        hostname: Hostname::parse(hostname).expect("a hostname"),
        ..record(request, status)
    }
}

/// A machine that pairs again under another hostname is named by the one
/// the hub holds, which its pages list, and the one it asked for, each
/// escaped, as the machine's own line names both (#1342).
#[test]
fn a_machine_pairing_again_under_another_hostname_is_named_by_the_one_held() {
    let request = request("other\"one");
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    assert_eq!(
        paired(
            &request,
            &Pairing::Again(held_as(&request, "first", approved()))
        ),
        format!(
            "machine \"first\" (asked as \"other\\\"one\") paired again as agent \
             01900000-0000-7000-8000-000000000001 with key {fingerprint}; it is approved already"
        )
    );
    let repeated = paired(
        &request,
        &Pairing::Repeated(held_as(&request, "first", pending())),
    );
    assert!(
        repeated.starts_with("machine \"first\" (asked as \"other\\\"one\") repeated its pairing"),
        "{repeated}"
    );
}

/// An approval is named as a pairing is, by hostname, agent ID and key
/// fingerprint, so the person can match it with the machine's screen.
#[test]
fn an_approval_is_named_by_hostname_agent_id_and_key() {
    let request = request("Nick's laptop");
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    assert_eq!(
        approved_line(&record(&request, approved())),
        format!(
            "machine \"Nick\\'s laptop\" approved as agent \
             01900000-0000-7000-8000-000000000001 with key {fingerprint}"
        )
    );
}

/// An approved machine's first connection after its approval is named as
/// a pairing is, and says it is the first.
#[test]
fn a_first_connection_after_approval_is_named_by_hostname_agent_id_and_key() {
    let request = request("e2e-laptop");
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    assert_eq!(
        connected(&record(&request, approved())),
        format!(
            "machine \"e2e-laptop\" connected as agent 01900000-0000-7000-8000-000000000001 \
             with key {fingerprint}, its first connection since it was approved"
        )
    );
}

/// A removal is named as a pairing is, by the hostname its tombstone keeps,
/// and says what the hub refuses from now on.
#[test]
fn a_removal_is_named_by_hostname_agent_id_and_key() {
    let request = request("Nick's laptop");
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    let tombstone = Tombstone {
        agent_id: request.agent_id,
        hostname: request.hostname.clone(),
        key: request.agent_key,
        removed_at: Timestamp::from_second(1_800_000_000).expect("a time"),
    };
    assert_eq!(
        removed(&tombstone),
        format!(
            "machine \"Nick\\'s laptop\" removed as agent \
             01900000-0000-7000-8000-000000000001 with key {fingerprint}; the hub refuses its \
             connections and its pairings with an ordinary token"
        )
    );
}

/// A lapse is named as a pairing is, and says the machine may pair again.
#[test]
fn a_lapse_is_named_by_hostname_agent_id_and_key() {
    let request = request("e2e-laptop");
    let fingerprint = Fingerprint::of_key(&request.agent_key);
    assert_eq!(
        lapsed(&record(&request, pending())),
        format!(
            "machine \"e2e-laptop\" (agent 01900000-0000-7000-8000-000000000001 with key \
             {fingerprint}) lapsed: nobody approved it within 7 days; it may pair again with a \
             new token"
        )
    );
}

/// Entering recovery mode names the agent whose report found it there, what
/// the hub stops doing and the way out; ending it says what the person
/// confirmed and what the epochs continue from.
#[test]
fn recovery_mode_is_named_as_it_starts_and_as_a_person_ends_it() {
    let agent = request("e2e-laptop").agent_id;
    assert_eq!(
        recovering(agent),
        "agent 01900000-0000-7000-8000-000000000001 proved a lease above what this hub \
         counts, as after a restore from an older copy: the hub grants and renews no lease \
         until a person confirms on the Machines tab that the old hub is stopped or gone"
    );
    assert_eq!(
        recovered(),
        "a person confirmed the old hub is stopped or gone: recovery mode ended, and each \
         repository's epochs continue above the floors its machines proved"
    );
}
