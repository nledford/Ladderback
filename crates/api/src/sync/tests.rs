use super::*;

#[test]
fn a_connect_is_json_named_by_its_type() {
    let frame = AgentFrame::Connect {
        agent_id: "0194f6a2-0000-7000-8000-000000000001".into(),
        agent_nonce: "AAAA".into(),
        binding_kind: 1,
    };
    assert_eq!(
        frame.to_text(),
        r#"{"type":"connect","agent_id":"0194f6a2-0000-7000-8000-000000000001","agent_nonce":"AAAA","binding_kind":1}"#
    );
    assert_eq!(AgentFrame::from_text(&frame.to_text()), Ok(frame));
}

#[test]
fn every_frame_reads_back_as_written() {
    let agent = [AgentFrame::Pair {
        pairing_key: "pk".into(),
        agent_key: "ak".into(),
        counter: u64::MAX,
        hostname: "Nick's NAS \u{e9}".into(),
        zone: Some("America/Chicago".into()),
        pairing_signature: "ps".into(),
        agent_signature: "as".into(),
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
        },
        HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: Some("laptop".into()),
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
        r#"{"type":"connect","agent_id":"x","agent_nonce":"y"}"#,
        r#"{"type":"connect","agent_id":"x","agent_nonce":"y","binding_kind":256}"#,
        r#"{"type":"connect","agent_id":"x","agent_nonce":"y","binding_kind":-1}"#,
        r#"{"type":"challenge","hub_nonce":"n","hub_key":"k","hub_handshakes":1,"hub_proof":"p"}"#,
    ] {
        assert_eq!(AgentFrame::from_text(text), Err(UnreadableFrame), "{text}");
    }
    assert_eq!(
        HubFrame::from_text(
            r#"{"type":"connect","agent_id":"x","agent_nonce":"y","binding_kind":0}"#
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
        ("agent-key-mismatch", ErrorCode::AgentKeyMismatch),
        ("hub-key-mismatch", ErrorCode::HubKeyMismatch),
        ("proof-refused", ErrorCode::ProofRefused),
        ("pending-limit", ErrorCode::PendingLimit),
        ("binding-unavailable", ErrorCode::BindingUnavailable),
        ("a-code-of-a-later-hub", ErrorCode::Unknown),
        ("", ErrorCode::Unknown),
    ] {
        assert_eq!(refusal_code(code), expected, "{code}");
    }
}

/// A `paired` frame from a hub that predates the hostname has none, and one
/// from a later hub with a field this build doesn't know still reads (decision
/// 120: optional fields stay within a protocol).
#[test]
fn a_paired_frame_reads_with_or_without_the_hostname_and_unknown_fields() {
    let without = r#"{"type":"paired","recovery_key":"r","signature":"s"}"#;
    assert_eq!(
        HubFrame::from_text(without),
        Ok(HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: None,
        })
    );
    assert!(
        !HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: None,
        }
        .to_text()
        .contains("hostname")
    );
    let later = r#"{"type":"paired","recovery_key":"r","signature":"s","hostname":"laptop","a_later_field":1}"#;
    assert_eq!(
        HubFrame::from_text(later),
        Ok(HubFrame::Paired {
            recovery_key: "r".into(),
            signature: "s".into(),
            hostname: Some("laptop".into()),
        })
    );
}
