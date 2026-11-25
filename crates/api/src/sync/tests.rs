use super::*;

#[test]
fn a_connect_is_json_named_by_its_type() {
    let frame = AgentFrame::Connect {
        machine_id: "0194f6a2-0000-7000-8000-000000000001".into(),
        machine_nonce: "AAAA".into(),
        binding_kind: 1,
    };
    assert_eq!(
        frame.to_text(),
        r#"{"type":"connect","machine_id":"0194f6a2-0000-7000-8000-000000000001","machine_nonce":"AAAA","binding_kind":1}"#
    );
    assert_eq!(AgentFrame::from_text(&frame.to_text()), Ok(frame));
}

/// Every handshake field that names the machine says `machine` on the wire
/// (§16, decision 163): the frames and payloads read and write these names
/// and no others.
#[test]
fn the_handshake_names_the_machine_on_the_wire() {
    for text in [
        r#"{"type":"connect","machine_id":"x","machine_nonce":"y","binding_kind":0}"#,
        r#"{"type":"pair","pairing_key":"pk","machine_key":"mk","counter":1,"hostname":"nas","pairing_signature":"ps","machine_signature":"ms"}"#,
        r#"{"type":"proof","counter":1,"machine_proof":"p"}"#,
    ] {
        let frame = AgentFrame::from_text(text).unwrap();
        assert_eq!(frame.to_text(), text);
    }
    let hello = br#"{"machine_version":"v","hostname":"nas","protocols":[1,1],"clock":1}"#;
    let read = HelloPayload::from_bytes(hello).unwrap();
    assert_eq!(read.to_bytes(), hello.to_vec());
    let welcome = br#"{"protocol":1,"hub_protocols":[1,1],"machine_protocols":[1,1],"generation":1,"hub_handshakes":1,"hub_version":"v","signed_at":1,"status":"approved"}"#;
    let read = WelcomePayload::from_bytes(welcome).unwrap();
    assert_eq!(read.to_bytes(), welcome.to_vec());
    for text in [
        r#"{"type":"connect","agent_id":"x","agent_nonce":"y","binding_kind":0}"#,
        r#"{"type":"proof","counter":1,"agent_proof":"p"}"#,
    ] {
        assert_eq!(AgentFrame::from_text(text), Err(UnreadableFrame), "{text}");
    }
}

/// The handshake's refusals that name the machine go by `machine-` codes
/// (§16, decision 163), and the old `agent-` codes are none of them.
#[test]
fn the_refusals_name_the_machine_on_the_wire() {
    for code in [
        "machine-key-mismatch",
        "machine-removed",
        "machine-key-removed",
    ] {
        assert_ne!(refusal_code(code), ErrorCode::Unknown, "{code}");
        assert_eq!(code_name(refusal_code(code)), code);
    }
    for code in ["agent-key-mismatch", "agent-removed", "agent-key-removed"] {
        assert_eq!(refusal_code(code), ErrorCode::Unknown, "{code}");
    }
}

#[test]
fn every_frame_reads_back_as_written() {
    let agent = [AgentFrame::Pair {
        pairing_key: "pk".into(),
        machine_key: "ak".into(),
        counter: u64::MAX,
        hostname: "Nick's NAS \u{e9}".into(),
        zone: Some("America/Chicago".into()),
        pairing_signature: "ps".into(),
        machine_signature: "as".into(),
    }];
    for frame in agent {
        assert_eq!(AgentFrame::from_text(&frame.to_text()), Ok(frame));
    }
    let hub = [
        HubFrame::Challenge {
            hub_nonce: "n".into(),
            hub_key: "k".into(),
            hub_handshakes: 1,
            hub_proof: "p".into(),
        },
        HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: None,
            status: None,
        },
        HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: Some("laptop".into()),
            status: Some(AgentApproval::Approved),
        },
        HubFrame::Refused {
            code: "token-used".into(),
            holder: None,
            signature: "s".into(),
        },
        HubFrame::Refused {
            code: "hostname-taken".into(),
            holder: Some(HolderFrame {
                hostname: "laptop".into(),
                last_seen: 1_700_000_000,
            }),
            signature: "s".into(),
        },
    ];
    for frame in hub {
        assert_eq!(HubFrame::from_text(&frame.to_text()), Ok(frame));
    }
}

/// A later hub may add a field; this build reads the message without it.
#[test]
fn a_field_this_build_does_not_know_is_ignored() {
    assert_eq!(
        HubFrame::from_text(
            r#"{"type":"challenge","hub_nonce":"n","hub_key":"k","hub_handshakes":3,"hub_proof":"p","handovers":[]}"#
        ),
        Ok(HubFrame::Challenge {
            hub_nonce: "n".into(),
            hub_key: "k".into(),
            hub_handshakes: 3,
            hub_proof: "p".into(),
        })
    );
}

#[test]
fn a_frame_that_is_no_message_is_unreadable() {
    for text in [
        "",
        "not json",
        "{}",
        r#"{"type":"hello"}"#,
        r#"{"type":"connect","machine_id":"x","machine_nonce":"y"}"#,
        r#"{"type":"connect","machine_id":"x","machine_nonce":"y","binding_kind":256}"#,
        r#"{"type":"connect","machine_id":"x","machine_nonce":"y","binding_kind":-1}"#,
        r#"{"type":"challenge","hub_nonce":"n","hub_key":"k","hub_handshakes":1,"hub_proof":"p"}"#,
    ] {
        assert_eq!(AgentFrame::from_text(text), Err(UnreadableFrame), "{text}");
    }
    assert_eq!(
        HubFrame::from_text(
            r#"{"type":"connect","machine_id":"x","machine_nonce":"y","binding_kind":0}"#
        ),
        Err(UnreadableFrame)
    );
}

#[test]
fn a_refusal_names_its_error_code() {
    for (code, expected) in [
        ("token-used", ErrorCode::TokenUsed),
        ("token-expired", ErrorCode::TokenExpired),
        ("token-revoked", ErrorCode::TokenRevoked),
        ("hostname-taken", ErrorCode::HostnameTaken),
        ("machine-key-mismatch", ErrorCode::MachineKeyMismatch),
        ("hub-key-mismatch", ErrorCode::HubKeyMismatch),
        ("proof-refused", ErrorCode::ProofRefused),
        ("pending-limit", ErrorCode::PendingLimit),
        ("machine-removed", ErrorCode::MachineRemoved),
        ("machine-key-removed", ErrorCode::MachineKeyRemoved),
        ("binding-unavailable", ErrorCode::BindingUnavailable),
        ("identity-conflict", ErrorCode::IdentityConflict),
        ("a-code-of-a-later-hub", ErrorCode::Unknown),
        ("", ErrorCode::Unknown),
    ] {
        assert_eq!(refusal_code(code), expected, "{code}");
    }
}

/// A report and its acknowledgement are the JSON the other side reads, and
/// read back as written; a failure's code goes by its wire name (§7.2).
#[test]
fn reports_and_acks_read_back_as_written() {
    let report = ReportPayload {
        epoch: "e".to_owned(),
        seq: 7,
        op_id: "o".to_owned(),
        event: "failed".to_owned(),
        at: 1_800_000_000,
        code: Some(code_name(ErrorCode::TokenUsed)),
    };
    let bytes = report.to_bytes();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"epoch":"e","seq":7,"op_id":"o","event":"failed","at":1800000000,"code":"token-used"}"#
    );
    assert_eq!(ReportPayload::from_bytes(&bytes), Ok(report));
    let ack = AckPayload {
        epoch: "e".to_owned(),
        seq: 7,
    };
    let bytes = ack.to_bytes();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"epoch":"e","seq":7}"#
    );
    assert_eq!(AckPayload::from_bytes(&bytes), Ok(ack));
}

/// A `paired` frame from a hub that predates the hostname or the status has
/// neither, and one from a later hub with a field this build doesn't know
/// still reads (decision 120: optional fields stay within a protocol).
#[test]
fn a_paired_frame_reads_with_or_without_the_hostname_and_unknown_fields() {
    let without = r#"{"type":"paired","recovery_key":"r","signature":"s"}"#;
    assert_eq!(
        HubFrame::from_text(without),
        Ok(HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: None,
            status: None,
        })
    );
    let sent = HubFrame::Paired {
        recovery_key: "r".into(),
        signature: "s".into(),
        hostname: None,
        status: None,
    }
    .to_text();
    assert!(
        !sent.contains("hostname") && !sent.contains("status"),
        "{sent}"
    );
    let later = r#"{"type":"paired","recovery_key":"r","signature":"s","hostname":"laptop","status":"approved","a_later_field":1}"#;
    assert_eq!(
        HubFrame::from_text(later),
        Ok(HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: Some("laptop".into()),
            status: Some(AgentApproval::Approved),
        })
    );
}

/// A `paired` status this build doesn't know, or of another type, reads as
/// none, as from a hub that doesn't say: the status is shown, never decided
/// on, so a later hub's word can't refuse the whole frame (#1341). The
/// signed `Welcome`'s status, which the machine acts on, stays strict.
#[test]
fn a_paired_status_this_build_doesnt_know_reads_as_none() {
    for status in [r#""suspended""#, "7", "null"] {
        let text =
            format!(r#"{{"type":"paired","recovery_key":"r","signature":"s","status":{status}}}"#);
        assert_eq!(
            HubFrame::from_text(&text),
            Ok(HubFrame::Paired {
                recovery_key: "r".into(),
                signature: "s".into(),
                hostname: None,
                status: None,
            }),
            "{status}"
        );
    }
    let welcome = |status: &str| {
        format!(
            r#"{{"hub_protocols":[1,1],"machine_protocols":[1,1],"generation":1,"hub_handshakes":1,"hub_version":"v","signed_at":1,"status":"{status}"}}"#
        )
    };
    assert!(WelcomePayload::from_bytes(welcome("pending").as_bytes()).is_ok());
    assert_eq!(
        WelcomePayload::from_bytes(welcome("suspended").as_bytes()),
        Err(UnreadableFrame)
    );
}
