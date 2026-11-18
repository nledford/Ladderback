use uuid::Uuid;

use super::*;

fn key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; KEY_BYTES]).unwrap()
}

fn agent() -> AgentId {
    AgentId::from_uuid(Uuid::parse_str("0194f3a2-7b1c-7d2e-8f30-415263748596").unwrap())
}

fn nonce(byte: u8) -> Nonce {
    Nonce::from_bytes([byte; KEY_BYTES])
}

fn binding() -> Binding {
    Binding::TlsExporter([0xcc; KEY_BYTES])
}

fn hex(lines: &[&str]) -> String {
    lines.concat()
}

fn hostname() -> Hostname {
    Hostname::parse("Nick's MacBook Pro").unwrap()
}

fn name(text: &str) -> WireName {
    WireName::parse(text).unwrap()
}

/// Every message of decision 120's table with the fixtures the vectors were
/// made from, in the table's order, with its vector.
fn every_message<'a>(
    host: &'a Hostname,
    grants: &'a WireName,
    heartbeat: &'a WireName,
    refused: &'a WireName,
) -> Vec<(SignedMessage<'a>, String)> {
    let payload = PayloadDigest::of(br#"{"a":1}"#);
    let made_at = EpochSeconds::from_seconds(1_800_000_000);
    vec![
        (
            SignedMessage::HubProof {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                handshakes: Handshakes::from_bits(1),
            },
            hex(HUB_PROOF),
        ),
        (
            SignedMessage::AgentProof {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_key: key(0x22),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                counter: 7,
            },
            hex(AGENT_PROOF),
        ),
        (
            SignedMessage::Pair {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_key: key(0x22),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                counter: 7,
                hostname: host,
            },
            hex(PAIR),
        ),
        (
            SignedMessage::Paired {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_key: key(0x22),
                recovery_key: key(0x33),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
            },
            hex(PAIRED),
        ),
        (
            SignedMessage::HubMessage {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                sequence: 3,
                expires_at: EpochSeconds::from_seconds(1_800_000_600),
                kind: grants,
                payload,
            },
            hex(HUB_MESSAGE),
        ),
        (
            SignedMessage::AgentMessage {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                sequence: 3,
                kind: heartbeat,
                payload,
            },
            hex(AGENT_MESSAGE),
        ),
        (
            SignedMessage::Refusal {
                hub_key: key(0x11),
                agent_id: agent(),
                agent_nonce: nonce(0xaa),
                hub_nonce: nonce(0xbb),
                binding: binding(),
                code: refused,
            },
            hex(REFUSAL),
        ),
        (
            SignedMessage::Handover {
                old_key: key(0x11),
                new_key: key(0x44),
                made_at,
            },
            hex(HANDOVER),
        ),
        (
            SignedMessage::RecoveryHandover {
                recovery_key: key(0x33),
                new_hub_key: key(0x44),
                recovery_number: 2,
                made_at,
            },
            hex(RECOVERY_HANDOVER),
        ),
        (
            SignedMessage::RecoveryRotation {
                old_recovery_key: key(0x33),
                new_recovery_key: key(0x44),
                recovery_number: 2,
                made_at,
            },
            hex(RECOVERY_ROTATION),
        ),
    ]
}

/// The vectors were made apart from this code, from decision 120's table,
/// so a change to the layout fails here rather than between two builds.
#[test]
fn each_message_encodes_to_its_fixed_vector() {
    let (host, grants, heartbeat, refused) = (
        hostname(),
        name("RepoGrants"),
        name("Heartbeat"),
        name("proof-refused"),
    );
    for (message, vector) in every_message(&host, &grants, &heartbeat, &refused) {
        assert_eq!(
            lower_hex(message.to_bytes().as_bytes()),
            vector,
            "{}",
            message.label()
        );
    }
}

/// Each label is `ladderback-sync-v1 ` and the message's name, so no two
/// messages share one, and a signature of one never verifies as another.
#[test]
fn every_message_has_its_own_label() {
    let (host, grants, heartbeat, refused) = (
        hostname(),
        name("RepoGrants"),
        name("Heartbeat"),
        name("proof-refused"),
    );
    let messages = every_message(&host, &grants, &heartbeat, &refused);
    let mut labels: Vec<_> = messages.iter().map(|(m, _)| m.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), messages.len());
    for (message, _) in &messages {
        let bytes = message.to_bytes();
        let label = format!("{LABEL_PREFIX}{}", message.label());
        assert_eq!(usize::from(bytes.as_bytes()[0]), label.len());
        assert_eq!(&bytes.as_bytes()[1..=label.len()], label.as_bytes());
    }
}

/// The hub proof and the agent proof share a field prefix; their labels
/// keep a hub proof from being an agent proof and the reverse.
#[test]
fn the_two_proofs_never_encode_alike() {
    let hub = SignedMessage::HubProof {
        hub_key: key(0x11),
        agent_id: agent(),
        agent_nonce: nonce(0xaa),
        hub_nonce: nonce(0xbb),
        binding: binding(),
        handshakes: Handshakes::from_bits(1),
    };
    let agent_proof = SignedMessage::AgentProof {
        hub_key: key(0x11),
        agent_id: agent(),
        agent_key: key(0x11),
        agent_nonce: nonce(0xaa),
        hub_nonce: nonce(0xbb),
        binding: binding(),
        counter: 1,
    };
    assert_ne!(hub.to_bytes(), agent_proof.to_bytes());
    // The bare nonce, which a careless protocol would sign, is neither.
    assert!(hub.to_bytes().as_bytes().len() > KEY_BYTES);
}

/// Each field a hub proof covers changes its bytes: another hub, agent,
/// connection, binding value or kind, or set of handshakes.
#[test]
fn every_field_of_a_proof_changes_its_bytes() {
    let base = |change: &dyn Fn(&mut [u8; 6])| {
        let mut picks = [0x11, 0, 0xaa, 0xbb, 0xcc, 1];
        change(&mut picks);
        SignedMessage::HubProof {
            hub_key: key(picks[0]),
            agent_id: AgentId::from_uuid(Uuid::from_u128(u128::from(picks[1]))),
            agent_nonce: nonce(picks[2]),
            hub_nonce: nonce(picks[3]),
            binding: if picks[4] == 0 {
                Binding::None
            } else {
                Binding::TlsExporter([picks[4]; KEY_BYTES])
            },
            handshakes: Handshakes::from_bits(u64::from(picks[5])),
        }
        .to_bytes()
    };
    let original = base(&|_| {});
    for field in 0..6 {
        let changed = base(&|picks| picks[field] = picks[field].wrapping_add(1));
        assert_ne!(changed, original, "field {field}");
    }
    // Kind 0 with zeros differs from kind 1 with zeros.
    let none = base(&|picks| picks[4] = 0);
    let mut zero_exporter = none.as_bytes().to_vec();
    let at = 1 + LABEL_PREFIX.len() + "hub-proof".len() + 32 + 16 + 32 + 32;
    assert_eq!(zero_exporter[at], 0);
    assert!(zero_exporter[at + 1..at + 33].iter().all(|&b| b == 0));
    zero_exporter[at] = 1;
    assert_ne!(none.as_bytes(), zero_exporter.as_slice());
}

/// A variable field carries its length, so moving a byte from one message's
/// kind into its digest can't make another message.
#[test]
fn a_variable_field_carries_its_length() {
    let payload = PayloadDigest::of(b"");
    let message = |kind: &WireName| {
        SignedMessage::AgentMessage {
            hub_key: key(0x11),
            agent_id: agent(),
            agent_nonce: nonce(0xaa),
            hub_nonce: nonce(0xbb),
            binding: Binding::None,
            sequence: 1,
            kind,
            payload,
        }
        .to_bytes()
    };
    let (short, long) = (name("Ack"), name("AckX"));
    let (short, long) = (message(&short), message(&long));
    assert_eq!(long.as_bytes().len(), short.as_bytes().len() + 1);
    let at = short.as_bytes().len() - KEY_BYTES - 4;
    assert_eq!(short.as_bytes()[at], 3);
    assert_eq!(long.as_bytes()[at], 4);
}

#[test]
fn a_wire_name_is_1_to_255_visible_ascii_characters() {
    assert_eq!(name("proof-refused").as_str(), "proof-refused");
    assert!(WireName::parse(&"a".repeat(255)).is_ok());
    for text in ["", "has space", "tab\t", "é", "\u{7f}"] {
        assert_eq!(WireName::parse(text), Err(InvalidWireName), "{text:?}");
    }
    assert_eq!(WireName::parse(&"a".repeat(256)), Err(InvalidWireName));
}

#[test]
fn a_time_before_the_epoch_has_no_encoding() {
    let at = |seconds| EpochSeconds::from_timestamp(Timestamp::from_second(seconds).unwrap());
    assert_eq!(
        at(1_800_000_000),
        Some(EpochSeconds::from_seconds(1_800_000_000))
    );
    assert_eq!(at(0), Some(EpochSeconds::from_seconds(0)));
    assert_eq!(at(-1), None);
}

#[test]
fn a_payload_digest_is_the_sha256_of_its_bytes() {
    let digest = PayloadDigest::of(b"abc");
    let message = SignedMessage::AgentMessage {
        hub_key: key(0x11),
        agent_id: agent(),
        agent_nonce: nonce(0xaa),
        hub_nonce: nonce(0xbb),
        binding: Binding::None,
        sequence: 1,
        kind: &name("Ack"),
        payload: digest,
    };
    let bytes = lower_hex(message.to_bytes().as_bytes());
    assert!(bytes.ends_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
}

#[test]
fn signed_bytes_and_nonces_show_their_hex() {
    assert_eq!(
        format!("{:?}", nonce(0x0f)),
        format!("Nonce({})", "0f".repeat(32))
    );
    let bytes = SignedMessage::Handover {
        old_key: key(0x11),
        new_key: key(0x44),
        made_at: EpochSeconds::from_seconds(1_800_000_000),
    }
    .to_bytes();
    assert_eq!(
        format!("{bytes:?}"),
        format!("SignedBytes({})", hex(HANDOVER))
    );
}

#[test]
fn a_binding_names_its_kind() {
    assert_eq!(Binding::None.kind(), 0);
    assert_eq!(binding().kind(), 1);
}

/// `hub-proof`.
const HUB_PROOF: &[&str] = &[
    "1c6c61646465726261636b2d73796e632d7631206875622d70726f6f661111111111111111111111",
    "1111111111111111111111111111111111111111110194f3a27b1c7d2e8f30415263748596aaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccccccccc",
    "cccccccccccccccccccccccccccc0000000000000001",
];

/// `agent-proof`.
const AGENT_PROOF: &[&str] = &[
    "1e6c61646465726261636b2d73796e632d7631206167656e742d70726f6f66111111111111111111",
    "11111111111111111111111111111111111111111111110194f3a27b1c7d2e8f3041526374859622",
    "22222222222222222222222222222222222222222222222222222222222222aaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccccccccccccccccccccc",
    "cccccccccccccccc0000000000000007",
];

/// `pair`.
const PAIR: &[&str] = &[
    "176c61646465726261636b2d73796e632d7631207061697211111111111111111111111111111111",
    "111111111111111111111111111111110194f3a27b1c7d2e8f304152637485962222222222222222",
    "222222222222222222222222222222222222222222222222aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    "cc0000000000000007124e69636b2773204d6163426f6f6b2050726f",
];

/// `paired`.
const PAIRED: &[&str] = &[
    "196c61646465726261636b2d73796e632d7631207061697265641111111111111111111111111111",
    "1111111111111111111111111111111111110194f3a27b1c7d2e8f30415263748596222222222222",
    "22222222222222222222222222222222222222222222222222223333333333333333333333333333",
    "333333333333333333333333333333333333aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "bbbb01cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
];

/// `hub-signed`.
const HUB_MESSAGE: &[&str] = &[
    "1d6c61646465726261636b2d73796e632d7631206875622d7369676e656411111111111111111111",
    "111111111111111111111111111111111111111111110194f3a27b1c7d2e8f30415263748596aaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccccccc",
    "cccccccccccccccccccccccccccccc0000000000000003000000006b49d4580a5265706f4772616e",
    "7473015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862",
];

/// `agent-signed`.
const AGENT_MESSAGE: &[&str] = &[
    "1f6c61646465726261636b2d73796e632d7631206167656e742d7369676e65641111111111111111",
    "1111111111111111111111111111111111111111111111110194f3a27b1c7d2e8f30415263748596",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccc",
    "cccccccccccccccccccccccccccccccccc000000000000000309486561727462656174015abd7f5c",
    "c57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862",
];

/// `hub-refused`.
const REFUSAL: &[&str] = &[
    "1e6c61646465726261636b2d73796e632d7631206875622d72656675736564111111111111111111",
    "11111111111111111111111111111111111111111111110194f3a27b1c7d2e8f30415263748596aa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbb",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01cccccccccccccccccccccccccccccccc",
    "cccccccccccccccccccccccccccccccc0d70726f6f662d72656675736564",
];

/// `handover`.
const HANDOVER: &[&str] = &[
    "1b6c61646465726261636b2d73796e632d76312068616e646f766572111111111111111111111111",
    "11111111111111111111111111111111111111114444444444444444444444444444444444444444",
    "444444444444444444444444000000006b49d200",
];

/// `recovery-handover`.
const RECOVERY_HANDOVER: &[&str] = &[
    "246c61646465726261636b2d73796e632d7631207265636f766572792d68616e646f766572333333",
    "33333333333333333333333333333333333333333333333333333333334444444444444444444444",
    "4444444444444444444444444444444444444444440000000000000002000000006b49d200",
];

/// `recovery-rotation`.
const RECOVERY_ROTATION: &[&str] = &[
    "246c61646465726261636b2d73796e632d7631207265636f766572792d726f746174696f6e333333",
    "33333333333333333333333333333333333333333333333333333333334444444444444444444444",
    "4444444444444444444444444444444444444444440000000000000002000000006b49d200",
];
