use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, Fingerprint, Hostname, PairRequest, Pairing, PublicKey,
    TokenTls,
};
use domain::{AgentId, Timestamp, Uuid};

use super::paired;

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
    let line = paired(&request, &Pairing::Repeated);
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
