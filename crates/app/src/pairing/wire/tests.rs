use api::sync::{AgentFrame, HolderFrame, HubFrame};
use domain::fleet::{
    BindingKind, Challenge, Connect, EpochSeconds, Handshakes, Holder, Hostname, KEY_BYTES, Nonce,
    Pair, PairAnswer, Paired, PublicKey, Refused, SIGNATURE_BYTES, Signature, WireName,
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
