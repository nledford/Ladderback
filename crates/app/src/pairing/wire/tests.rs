use api::sync::{AgentFrame, HolderFrame, HubFrame};
use domain::fleet::{
    BindingKind, Challenge, Connect, EpochSeconds, Handshakes, Holder, Hostname, KEY_BYTES,
    MachineZone, Nonce, Pair, PairAnswer, Paired, PublicKey, Refused, SIGNATURE_BYTES, Signature,
    WireName,
};
use domain::{AgentId, Uuid};

use super::*;

fn key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; KEY_BYTES]).unwrap()
}

/// A signature of `byte`s with a scalar half of zeros, which is canonical.
fn signature(byte: u8) -> Signature {
    let mut bytes = [0; SIGNATURE_BYTES];
    bytes[..KEY_BYTES].fill(byte);
    Signature::from_bytes(bytes).unwrap()
}

fn connect() -> Connect {
    Connect {
        agent_id: AgentId::from_uuid(Uuid::from_u128(0x0123_4567_89ab_4def_8123_4567_89ab_cdef)),
        agent_nonce: Nonce::from_bytes([0x21; KEY_BYTES]),
        binding_kind: BindingKind::from_byte(1).unwrap(),
    }
}

fn pair() -> Pair {
    Pair {
        pairing_key: key(0x11),
        agent_key: key(0x22),
        counter: 7,
        hostname: Hostname::parse("nas").unwrap(),
        zone: None,
        pairing_signature: signature(0x33),
        agent_signature: signature(0x44),
    }
}

fn challenge() -> Challenge {
    Challenge {
        hub_nonce: Nonce::from_bytes([0x55; KEY_BYTES]),
        hub_key: key(0x66),
        hub_handshakes: Handshakes::from_bits(1),
        hub_proof: signature(0x77),
    }
}

fn refused() -> PairAnswer {
    PairAnswer::Refused(Refused {
        code: WireName::parse("hostname-taken").unwrap(),
        holder: Some(Holder {
            hostname: Hostname::parse("NAS").unwrap(),
            last_seen: EpochSeconds::from_seconds(1_800_000_000),
        }),
        signature: signature(0x88),
    })
}

/// Each message goes through its frame's text and back unchanged.
#[test]
fn each_message_reads_back_from_its_frame() {
    for (frame, message) in [
        (connect_frame(&connect()), AgentMessage::Connect(connect())),
        (pair_frame(&pair()), AgentMessage::Pair(pair())),
    ] {
        let read = AgentFrame::from_text(&frame.to_text()).unwrap();
        assert_eq!(agent_message(&read), Ok(message));
    }
    let paired = PairAnswer::Paired(Paired {
        recovery_key: key(0x99),
        signature: signature(0xaa),
        hostname: None,
        approval: None,
    });
    for (frame, message) in [
        (
            challenge_frame(&challenge()),
            HubMessage::Challenge(challenge()),
        ),
        (answer_frame(&paired), HubMessage::Answer(paired.clone())),
        (answer_frame(&refused()), HubMessage::Answer(refused())),
    ] {
        let read = HubFrame::from_text(&frame.to_text()).unwrap();
        assert_eq!(hub_message(&read), Ok(message));
    }
}

/// The hostname the hub holds the agent under goes in `Paired`'s frame and
/// comes back from it.
#[test]
fn a_paired_answer_carries_the_hostname_the_hub_holds() {
    let paired = PairAnswer::Paired(Paired {
        recovery_key: key(0x99),
        signature: signature(0xaa),
        hostname: Some(Hostname::parse("laptop").unwrap()),
        approval: None,
    });
    let frame = answer_frame(&paired);
    let HubFrame::Paired { hostname, .. } = &frame else {
        panic!("not a Paired");
    };
    assert_eq!(hostname.as_deref(), Some("laptop"));
    let read = HubFrame::from_text(&frame.to_text()).unwrap();
    assert_eq!(hub_message(&read), Ok(HubMessage::Answer(paired)));
}

/// Whether a person has approved the agent goes in `Paired`'s frame and
/// comes back from it; a hub that predates it says nothing, which reads as
/// no word on it (#1341), and so does a status this build doesn't know.
#[test]
fn a_paired_answer_carries_whether_the_hub_approved_the_agent() {
    for (approval, status) in [
        (Approval::Approved, AgentApproval::Approved),
        (Approval::Pending, AgentApproval::Pending),
    ] {
        let paired = PairAnswer::Paired(Paired {
            recovery_key: key(0x99),
            signature: signature(0xaa),
            hostname: Some(Hostname::parse("laptop").unwrap()),
            approval: Some(approval),
        });
        let frame = answer_frame(&paired);
        let HubFrame::Paired { status: sent, .. } = &frame else {
            panic!("not a Paired");
        };
        assert_eq!(*sent, Some(status));
        let read = HubFrame::from_text(&frame.to_text()).unwrap();
        assert_eq!(hub_message(&read), Ok(HubMessage::Answer(paired)));
    }
    let older = HubFrame::Paired {
        recovery_key: encoded(key(0x99).as_bytes()),
        signature: encoded(signature(0xaa).as_bytes()),
        hostname: None,
        status: None,
    };
    let later = HubFrame::from_text(&format!(
        r#"{{"type":"paired","recovery_key":"{}","signature":"{}","status":"suspended"}}"#,
        encoded(key(0x99).as_bytes()),
        encoded(signature(0xaa).as_bytes()),
    ));
    for frame in [Ok(older), later] {
        assert_eq!(
            frame.map(|frame| hub_message(&frame)),
            Ok(Ok(HubMessage::Answer(PairAnswer::Paired(Paired {
                recovery_key: key(0x99),
                signature: signature(0xaa),
                hostname: None,
                approval: None,
            }))))
        );
    }
}

/// A hostname in `Paired` that isn't one refuses the hub's frame, as a
/// holder's does.
#[test]
fn a_paired_answer_with_a_hostname_out_of_shape_is_refused() {
    let HubFrame::Paired {
        recovery_key,
        signature,
        ..
    } = answer_frame(&PairAnswer::Paired(Paired {
        recovery_key: key(0x99),
        signature: signature(0xaa),
        hostname: None,
        approval: None,
    }))
    else {
        panic!("not a Paired");
    };
    for hostname in ["", " padded ", "gone\u{202e}wrong"] {
        let frame = HubFrame::Paired {
            recovery_key: recovery_key.clone(),
            signature: signature.clone(),
            hostname: Some(hostname.to_owned()),
            status: None,
        };
        assert_eq!(hub_message(&frame), Err(WireRefused), "{hostname:?}");
    }
}

/// Bytes go as unpadded base64url and an agent ID as its UUID's text, so a
/// frame is the same whoever writes it (decision 120, "The handshake").
#[test]
fn a_frame_holds_its_fields_as_the_wire_defines_them() {
    let AgentFrame::Connect {
        agent_id,
        agent_nonce,
        binding_kind,
    } = connect_frame(&connect())
    else {
        panic!("not a Connect");
    };
    assert_eq!(agent_id, "01234567-89ab-4def-8123-456789abcdef");
    assert_eq!(agent_nonce, "ISEhISEhISEhISEhISEhISEhISEhISEhISEhISEhISE");
    assert_eq!(binding_kind, 1);
    let HubFrame::Refused { code, holder, .. } = answer_frame(&refused()) else {
        panic!("not a refusal");
    };
    assert_eq!(code, "hostname-taken");
    assert_eq!(
        holder,
        Some(HolderFrame {
            hostname: "NAS".to_owned(),
            last_seen: 1_800_000_000,
        })
    );
}

fn connect_with(change: impl FnOnce(&mut AgentFrame)) -> AgentFrame {
    let mut frame = connect_frame(&connect());
    change(&mut frame);
    frame
}

/// A field that doesn't hold what it must refuses the frame: a nonce of
/// another length, padded or in the other alphabet, an ID that isn't a
/// UUID, a binding kind the protocol doesn't name.
#[test]
fn a_connect_with_a_field_out_of_shape_is_refused() {
    let nonce = |text: &str| {
        let text = text.to_owned();
        connect_with(move |frame| {
            if let AgentFrame::Connect { agent_nonce, .. } = frame {
                *agent_nonce = text;
            }
        })
    };
    let good = "ISEhISEhISEhISEhISEhISEhISEhISEhISEhISEhISE";
    for frame in [
        nonce(good.get(1..).unwrap()),
        nonce(&format!("{good}A")),
        nonce(&format!("{good}=")),
        nonce(&good.replacen('I', "+", 1)),
        nonce(&good.replacen('I', "/", 1)),
        // The last character carries bits past the 32 bytes.
        nonce(&format!("{}F", good.get(..good.len() - 1).unwrap())),
        connect_with(|frame| {
            if let AgentFrame::Connect { agent_id, .. } = frame {
                *agent_id = "nas".to_owned();
            }
        }),
        connect_with(|frame| {
            if let AgentFrame::Connect { binding_kind, .. } = frame {
                *binding_kind = 2;
            }
        }),
    ] {
        assert_eq!(agent_message(&frame), Err(WireRefused), "{frame:?}");
    }
}

/// A key the protocol refuses (all zeros is of small order), a hostname
/// that isn't one, or a signature of another length refuses a `Pair`.
#[test]
fn a_pair_with_a_field_out_of_shape_is_refused() {
    let with = |change: fn(&mut AgentFrame)| {
        let mut frame = pair_frame(&pair());
        change(&mut frame);
        frame
    };
    for frame in [
        with(|frame| {
            if let AgentFrame::Pair { agent_key, .. } = frame {
                *agent_key = "A".repeat(43);
            }
        }),
        with(|frame| {
            if let AgentFrame::Pair { hostname, .. } = frame {
                hostname.clear();
            }
        }),
        with(|frame| {
            if let AgentFrame::Pair {
                pairing_signature, ..
            } = frame
            {
                pairing_signature.truncate(43);
            }
        }),
    ] {
        assert_eq!(agent_message(&frame), Err(WireRefused), "{frame:?}");
    }
}

/// A refusal's code that isn't a code, or a holder whose hostname isn't one,
/// refuses the hub's frame.
#[test]
fn a_refusal_with_a_field_out_of_shape_is_refused() {
    let HubFrame::Refused {
        holder, signature, ..
    } = answer_frame(&refused())
    else {
        panic!("not a refusal");
    };
    for frame in [
        HubFrame::Refused {
            code: "Not A Code".to_owned(),
            holder: holder.clone(),
            signature: signature.clone(),
        },
        HubFrame::Refused {
            code: "hostname-taken".to_owned(),
            holder: Some(HolderFrame {
                hostname: String::new(),
                last_seen: 0,
            }),
            signature: signature.clone(),
        },
        HubFrame::Challenge {
            hub_nonce: "short".to_owned(),
            hub_key: String::new(),
            hub_handshakes: 1,
            hub_proof: signature,
        },
    ] {
        assert_eq!(hub_message(&frame), Err(WireRefused), "{frame:?}");
    }
}

fn welcome() -> Welcome {
    Welcome {
        protocol: Some(1),
        hub_protocols: domain::fleet::PROTOCOLS,
        agent_protocols: domain::fleet::Protocols::new(1, 3).unwrap(),
        generation: 2,
        hub_handshakes: Handshakes::from_bits(1),
        hub_version: "0.5.0+def5678".to_owned(),
        signed_at: EpochSeconds::from_seconds(1_800_000_000),
        approval: domain::fleet::Approval::Approved,
    }
}

fn hello() -> Hello {
    Hello {
        agent_version: "0.5.0+abc1234".to_owned(),
        hostname: Hostname::parse("Nick's NAS").unwrap(),
        protocols: domain::fleet::PROTOCOLS,
        clock: EpochSeconds::from_seconds(1_800_000_001),
        zone: None,
        machine: None,
    }
}

#[test]
fn the_proof_and_the_envelopes_read_back_from_their_frames() {
    let proof = Proof {
        counter: u64::MAX,
        agent_proof: signature(0x55),
    };
    assert_eq!(
        agent_message(&proof_frame(&proof)),
        Ok(AgentMessage::Proof(proof))
    );
    let sent = AgentEnvelope {
        kind: domain::fleet::hello_kind(),
        seq: 1,
        payload: b"{\"a\":1}".to_vec(),
        signature: signature(0x66),
    };
    let frame = agent_envelope_frame(&sent);
    assert_eq!(
        frame,
        AgentFrame::Signed {
            kind: "Hello".to_owned(),
            seq: 1,
            payload: "eyJhIjoxfQ".to_owned(),
            signature: encoded(signature(0x66).as_bytes()),
        }
    );
    assert_eq!(agent_message(&frame), Ok(AgentMessage::Signed(sent)));
    let answered = HubEnvelope {
        kind: domain::fleet::welcome_kind(),
        seq: 1,
        expires_at: EpochSeconds::from_seconds(1_800_000_600),
        payload: Vec::new(),
        signature: signature(0x77),
    };
    assert_eq!(
        hub_message(&hub_envelope_frame(&answered)),
        Ok(HubMessage::Signed(answered))
    );
}

#[test]
fn the_payloads_read_back_from_their_bytes() {
    assert_eq!(read_hello(&hello_payload(&hello())), Ok(hello()));
    assert_eq!(read_welcome(&welcome_payload(&welcome())), Ok(welcome()));
    let mut apart = welcome();
    apart.protocol = None;
    apart.approval = domain::fleet::Approval::Pending;
    assert_eq!(read_welcome(&welcome_payload(&apart)), Ok(apart));
    assert_eq!(
        String::from_utf8(welcome_payload(&welcome())).unwrap(),
        concat!(
            r#"{"protocol":1,"hub_protocols":[1,1],"agent_protocols":[1,3],"generation":2,"#,
            r#""hub_handshakes":1,"hub_version":"0.5.0+def5678","signed_at":1800000000,"#,
            r#""status":"approved"}"#
        )
    );
}

#[test]
fn a_payload_out_of_shape_is_refused() {
    for bytes in [
        &b"not json"[..],
        br#"{"agent_version":"x","hostname":"","protocols":[1,1],"clock":1}"#,
        br#"{"agent_version":"x","hostname":"nas","protocols":[0,1],"clock":1}"#,
        br#"{"agent_version":"x","hostname":"nas","protocols":[2,1],"clock":1}"#,
    ] {
        assert_eq!(read_hello(bytes), Err(WireRefused), "{bytes:?}");
    }
    let mut welcome = String::from_utf8(welcome_payload(&welcome())).unwrap();
    welcome = welcome.replace("[1,3]", "[3,1]");
    assert_eq!(read_welcome(welcome.as_bytes()), Err(WireRefused));
}

fn report_entry(happening: Happening) -> OutboxEntry {
    OutboxEntry {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe0)),
        seq: ReportSeq::new(3).unwrap(),
        report: AgentReport {
            op_id: OperationId::from_uuid(Uuid::from_u128(0x0b)),
            happening,
            at: EpochSeconds::from_seconds(1_800_000_000),
        },
    }
}

/// A report of each event reads back from its payload, a failure with its
/// code; an event with a code it can't have, or without one it needs, is
/// refused, as is a place the hub's store can't hold (past `i64::MAX`).
#[test]
fn a_report_of_each_event_reads_back_from_its_payload() {
    let code = WireName::parse("repo-locked").unwrap();
    for happening in [
        Happening::Started,
        Happening::Succeeded,
        Happening::Failed { code },
    ] {
        let entry = report_entry(happening);
        assert_eq!(read_report(&report_payload(&entry)), Ok(entry));
    }
    let mut last = report_entry(Happening::Started);
    last.seq = ReportSeq::new(i64::MAX.unsigned_abs()).unwrap();
    assert_eq!(read_report(&report_payload(&last)), Ok(last));
    let started = String::from_utf8(report_payload(&report_entry(Happening::Started))).unwrap();
    for changed in [
        started.replace(r#""started""#, r#""failed""#),
        started.replace(r#""started""#, r#""cancelled""#),
        started.replace(r#""at""#, r#""code":"repo-locked","at""#),
        started.replace(r#""seq":3"#, r#""seq":0"#),
        started.replace(r#""seq":3"#, r#""seq":9223372036854775808"#),
    ] {
        assert_eq!(
            read_report(changed.as_bytes()),
            Err(WireRefused),
            "{changed}"
        );
    }
}

/// The machine zone travels in `Pair` and in `Hello`, and is left out of
/// both when there is none, so a frame without one reads as before (#114).
#[test]
fn the_machine_zone_travels_in_a_pair_and_a_hello() {
    let zone = Some(MachineZone::parse("America/Chicago").unwrap());
    let zoned_pair = Pair {
        zone: zone.clone(),
        ..pair()
    };
    let frame = AgentFrame::from_text(&pair_frame(&zoned_pair).to_text()).unwrap();
    assert_eq!(agent_message(&frame), Ok(AgentMessage::Pair(zoned_pair)));
    let zoned_hello = Hello { zone, ..hello() };
    assert_eq!(read_hello(&hello_payload(&zoned_hello)), Ok(zoned_hello));
    assert!(!pair_frame(&pair()).to_text().contains("zone"));
    let plain = String::from_utf8(hello_payload(&hello())).unwrap();
    assert!(!plain.contains("zone"), "{plain}");
}

/// A `Hello` carries the machine fingerprint as 64 lower-case hex digits,
/// and leaves it out when there is none, so an older hub reads it as
/// before; a fingerprint out of shape refuses the frame (decision 150).
#[test]
fn the_machine_fingerprint_travels_in_a_hello() {
    let machine = domain::fleet::MachineFingerprint::from_hex(&"5a".repeat(32)).unwrap();
    let told = Hello {
        machine: Some(machine),
        ..hello()
    };
    let payload = String::from_utf8(hello_payload(&told)).unwrap();
    assert!(
        payload.contains(&format!(r#""machine":"{}""#, "5a".repeat(32))),
        "{payload}"
    );
    assert_eq!(read_hello(payload.as_bytes()), Ok(told));
    let plain = String::from_utf8(hello_payload(&hello())).unwrap();
    assert!(!plain.contains("machine"), "{plain}");
    let upper = format!(
        r#"{{"agent_version":"x","hostname":"nas","protocols":[1,1],"clock":1,"machine":"{}"}}"#,
        "5A".repeat(32)
    );
    assert_eq!(read_hello(upper.as_bytes()), Err(WireRefused));
}

/// A zone that isn't an IANA-shaped name refuses the frame, as a hostname
/// that isn't one does (#114).
#[test]
fn a_zone_out_of_shape_is_refused() {
    let mut frame = pair_frame(&pair());
    if let AgentFrame::Pair { zone, .. } = &mut frame {
        *zone = Some("../etc/passwd".to_owned());
    }
    assert_eq!(agent_message(&frame), Err(WireRefused));
    let hello =
        br#"{"agent_version":"x","hostname":"nas","protocols":[1,1],"clock":1,"zone":"Europe/"}"#;
    assert_eq!(read_hello(hello), Err(WireRefused));
}

#[test]
fn an_envelope_or_proof_out_of_shape_is_refused() {
    let signature = encoded(signature(0x66).as_bytes());
    for frame in [
        AgentFrame::Proof {
            counter: 1,
            agent_proof: "short".to_owned(),
        },
        AgentFrame::Signed {
            kind: "Hello".to_owned(),
            seq: 1,
            payload: "eyJhIjoxfQ==".to_owned(),
            signature: signature.clone(),
        },
        AgentFrame::Signed {
            kind: "Hello".to_owned(),
            seq: 1,
            payload: "eyJhIjoxfR".to_owned(),
            signature: signature.clone(),
        },
        AgentFrame::Signed {
            kind: "Hel lo".to_owned(),
            seq: 1,
            payload: "eyJhIjoxfQ".to_owned(),
            signature: signature.clone(),
        },
        AgentFrame::Signed {
            kind: "Hello".to_owned(),
            seq: 1,
            payload: "eyJhIjoxfQ+".to_owned(),
            signature: signature.clone(),
        },
    ] {
        assert_eq!(agent_message(&frame), Err(WireRefused), "{frame:?}");
    }
    let frame = HubFrame::Signed {
        kind: "Welcome".to_owned(),
        seq: 1,
        expires_at: 1,
        payload: "A".repeat(MAX_FRAME_BYTES + 1),
        signature,
    };
    assert_eq!(hub_message(&frame), Err(WireRefused));
}

/// A signing time with any part of a second past it rounds up to the next
/// second, and a whole second stays as it is, so a grant's lease counted
/// from it never runs longer than the hub's.
#[test]
fn a_signing_time_rounds_up_to_its_second_and_an_expiry_down() {
    let whole = Timestamp::new(1_000, 0).unwrap();
    let past = Timestamp::new(1_000, 1).unwrap();
    assert_eq!(seconds_up(whole), 1_000);
    assert_eq!(seconds_up(past), 1_001);
    assert_eq!(seconds_down(whole), 1_000);
    assert_eq!(seconds_down(past), 1_000);
}

/// The hub's grants read back from their bytes with the generation and the
/// epochs it signed, up to the largest the agent's store holds; one past it
/// is refused (§7.2, I1).
#[test]
fn grants_read_back_from_their_bytes_up_to_the_largest_the_store_holds() {
    use domain::RepositoryId;
    use domain::maintenance::{Grant, LeaseEpoch, RepoGrants};

    let grants = |generation: u64, epoch: u64| RepoGrants {
        generation,
        signed_at: Timestamp::from_second(1_000).unwrap(),
        grants: vec![Grant {
            repository: RepositoryId::parse(&"0a".repeat(32)).unwrap(),
            epoch: LeaseEpoch::new(epoch).unwrap(),
            expires_at: Timestamp::from_second(2_000).unwrap(),
        }],
    };
    let largest = i64::MAX as u64;
    for (generation, epoch) in [(7, 3), (largest, largest)] {
        let sent = grants(generation, epoch);
        assert_eq!(read_grants(&grants_payload(&sent)), Ok(sent));
    }
    assert_eq!(
        read_grants(&grants_payload(&grants(largest + 1, 3))),
        Err(WireRefused)
    );
    assert_eq!(
        read_grants(&grants_payload(&grants(7, largest + 1))),
        Err(WireRefused)
    );
}
