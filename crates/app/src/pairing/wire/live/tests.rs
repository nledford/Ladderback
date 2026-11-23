use api::sync::{AgentFrame, MAX_FRAME_BYTES};
use domain::Uuid;
use domain::fleet::{AgentEnvelope, MAX_PLANS_REPORTED, SIGNATURE_BYTES, Signature, plans_kind};

use super::*;
use crate::pairing::wire::agent_envelope_frame;

/// A signature whose every byte the frame writes at its longest that a
/// signature canonically holds.
fn longest_signature() -> Signature {
    let mut bytes = [0; SIGNATURE_BYTES];
    bytes[..SIGNATURE_BYTES / 2].fill(0xff);
    Signature::from_bytes(bytes).unwrap()
}

fn plan(name: &str, last: Option<LastRun>, next: Option<u64>) -> ReportedPlan {
    ReportedPlan {
        plan: PlanId::parse(name).unwrap(),
        last,
        next_run: next.map(EpochSeconds::from_seconds),
    }
}

fn report() -> PlansReport {
    PlansReport::from_parts(
        vec![
            plan(
                "home",
                Some(LastRun {
                    at: EpochSeconds::from_seconds(1_800_000_000),
                    outcome: LastRunOutcome::SucceededWithWarnings,
                }),
                Some(1_800_003_600),
            ),
            plan("photos", None, None),
        ],
        3,
    )
    .unwrap()
}

#[test]
fn a_snapshot_of_plans_reads_back_as_it_was_written() {
    let bytes = plans_payload(&report());
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        concat!(
            r#"{"plans":[{"plan":"home","last":{"at":1800000000,"outcome":"succeeded-with-warnings"},"#,
            r#""next":1800003600},{"plan":"photos"}],"left_out":3}"#
        )
    );
    assert_eq!(read_plans(&bytes), Ok(report()));
}

/// With every plan ID at its longest and every number at its largest the
/// hub's store holds, a full snapshot still fits one frame.
#[test]
fn the_largest_snapshot_fits_one_frame() {
    let largest = i64::MAX.unsigned_abs();
    let longest = |n: usize| format!("{}{n:02}", "p".repeat(62));
    let last = LastRun {
        at: EpochSeconds::from_seconds(largest),
        outcome: LastRunOutcome::SucceededWithWarnings,
    };
    let plans = (0..MAX_PLANS_REPORTED)
        .map(|n| plan(&longest(n), Some(last), Some(largest)))
        .collect();
    let report = PlansReport::from_parts(plans, largest).unwrap();
    assert_eq!(read_plans(&plans_payload(&report)), Ok(report.clone()));
    let frame = agent_envelope_frame(&AgentEnvelope {
        kind: plans_kind(),
        seq: u64::MAX,
        payload: plans_payload(&report),
        signature: longest_signature(),
    });
    let text = frame.to_text();
    assert!(
        text.len() <= MAX_FRAME_BYTES,
        "{} bytes in a frame of at most {MAX_FRAME_BYTES}",
        text.len()
    );
    assert!(matches!(
        AgentFrame::from_text(&text),
        Ok(AgentFrame::Signed { .. })
    ));
}

#[test]
fn a_snapshot_with_too_many_plans_or_a_field_that_isnt_one_is_refused() {
    let names: Vec<String> = (0..=MAX_PLANS_REPORTED).map(|n| format!("p{n}")).collect();
    let too_many = names
        .iter()
        .map(|name| format!(r#"{{"plan":"{name}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let refused = [
        format!(r#"{{"plans":[{too_many}],"left_out":0}}"#),
        r#"{"plans":[{"plan":"Not A Slug"}],"left_out":0}"#.to_owned(),
        r#"{"plans":[{"plan":"home","last":{"at":1,"outcome":"exploded"}}],"left_out":0}"#
            .to_owned(),
        r#"{"plans":[]}"#.to_owned(),
        r#"{"plans":[],"left_out":9223372036854775808}"#.to_owned(),
        r#"{"plans":[{"plan":"home","next":9223372036854775808}],"left_out":0}"#.to_owned(),
        r#"{"plans":[{"plan":"home","last":{"at":9223372036854775808,"outcome":"failed"}}],"left_out":0}"#
            .to_owned(),
        // Past the last second of year 9999, which no timestamp holds.
        r#"{"plans":[{"plan":"home","next":253402300800}],"left_out":0}"#.to_owned(),
        r#"{"plans":[{"plan":"home","last":{"at":253402300800,"outcome":"failed"}}],"left_out":0}"#
            .to_owned(),
    ];
    for text in refused {
        assert_eq!(read_plans(text.as_bytes()), Err(WireRefused), "{text}");
    }
}

#[test]
fn the_hubs_answer_to_plans_reads_back() {
    let bytes = plans_ack_payload();
    assert_eq!(bytes, b"{}");
    assert_eq!(read_plans_ack(&bytes), Ok(()));
    for refused in [&b"null"[..], b"{\"seq\":1}"] {
        assert_eq!(read_plans_ack(refused), Err(WireRefused));
    }
}

fn progress() -> ReportedProgress {
    ReportedProgress {
        op_id: OperationId::from_uuid(Uuid::from_u128(7)),
        step: Some(WireName::parse("backing-up").unwrap()),
        unit: WireName::parse("bytes").unwrap(),
        current: 512,
        total: Some(2048),
        at: EpochSeconds::from_seconds(1_800_000_000),
    }
}

#[test]
fn a_progress_reads_back_as_it_was_written() {
    let bytes = progress_payload(&progress());
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        concat!(
            r#"{"op_id":"00000000-0000-0000-0000-000000000007","step":"backing-up","#,
            r#""unit":"bytes","current":512,"total":2048,"at":1800000000}"#
        )
    );
    assert_eq!(read_progress(&bytes), Ok(progress()));
    let bare = ReportedProgress {
        step: None,
        total: None,
        ..progress()
    };
    assert_eq!(read_progress(&progress_payload(&bare)), Ok(bare));
    let last_second = ReportedProgress {
        at: EpochSeconds::from_seconds(253_402_300_799),
        ..progress()
    };
    assert_eq!(
        read_progress(&progress_payload(&last_second)),
        Ok(last_second)
    );
}

#[test]
fn a_progress_with_a_field_that_isnt_one_is_refused() {
    let refused = [
        r#"{"op_id":"seven","unit":"bytes","current":1,"at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"","current":1,"at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","step":"","unit":"bytes","current":1,"at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"bytes","at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"bytes","current":9223372036854775808,"at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"bytes","current":1,"total":9223372036854775808,"at":1}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"bytes","current":1,"at":9223372036854775808}"#,
        r#"{"op_id":"00000000-0000-0000-0000-000000000007","unit":"bytes","current":1,"at":253402300800}"#,
    ];
    for text in refused {
        assert_eq!(read_progress(text.as_bytes()), Err(WireRefused), "{text}");
    }
}
