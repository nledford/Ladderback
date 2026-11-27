use api::sync::{AgentFrame, MAX_FRAME_BYTES};
use domain::Uuid;
use domain::fleet::{
    AgentEnvelope, HubEnvelope, MAX_PLANS_REPORTED, SIGNATURE_BYTES, Signature, heartbeat_ack_kind,
    heartbeat_kind, plans_kind,
};
use domain::maintenance::MAX_RUNNING_BACKUPS;

use super::*;
use crate::pairing::wire::{agent_envelope_frame, hub_envelope_frame};

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

/// The last second a timestamp holds, the latest time a machine can send.
fn latest() -> u64 {
    domain::Timestamp::MAX.as_second().unsigned_abs()
}

/// With every plan ID at its longest, every time at its latest and every
/// other number at its largest the hub's store holds, a full snapshot
/// still fits one frame.
#[test]
fn the_largest_snapshot_fits_one_frame() {
    let largest = i64::MAX.unsigned_abs();
    let longest = |n: usize| format!("{}{n:02}", "p".repeat(62));
    let last = LastRun {
        at: EpochSeconds::from_seconds(latest()),
        outcome: LastRunOutcome::SucceededWithWarnings,
    };
    let plans = (0..MAX_PLANS_REPORTED)
        .map(|n| plan(&longest(n), Some(last), Some(latest())))
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

/// The bytes of the frame `report` goes in at its longest: the largest
/// sequence number and a signature the frame writes at its longest.
fn frame_bytes(report: &PlansReport) -> usize {
    agent_envelope_frame(&AgentEnvelope {
        kind: plans_kind(),
        seq: u64::MAX,
        payload: plans_payload(report),
        signature: longest_signature(),
    })
    .to_text()
    .len()
}

/// A full snapshot of the largest plans: every plan ID at its longest,
/// every time at its latest, and 3 plans past the cap left out.
fn largest_plans() -> PlansReport {
    let longest = |n: usize| format!("{}{n:02}", "p".repeat(62));
    let last = LastRun {
        at: EpochSeconds::from_seconds(latest()),
        outcome: LastRunOutcome::SucceededWithWarnings,
    };
    let plans = (0..MAX_PLANS_REPORTED)
        .map(|n| plan(&longest(n), Some(last), Some(latest())))
        .collect();
    PlansReport::from_parts(plans, 3).unwrap()
}

/// `report`'s first `kept` plans, the others counted as left out.
fn first(report: &PlansReport, kept: usize) -> PlansReport {
    let others = u64::try_from(report.plans().len() - kept).unwrap();
    PlansReport::from_parts(report.plans()[..kept].to_vec(), report.left_out() + others).unwrap()
}

/// A snapshot whose frame would pass the limit keeps its first plans, in
/// the configuration's order, while the frame fits, and counts each plan
/// it leaves out with those past the cap (#1698).
#[test]
fn plans_past_one_frame_are_left_out_from_the_last_and_counted() {
    let largest = largest_plans();
    for kept in [MAX_PLANS_REPORTED, 10, 1, 0] {
        let fits = first(&largest, kept);
        let limit = frame_bytes(&fits);
        let sent = plans_within(largest.clone(), limit);
        assert_eq!(sent, fits, "a limit of {limit} bytes");
        assert!(frame_bytes(&sent) <= limit);
    }
    for kept in [MAX_PLANS_REPORTED, 10, 1] {
        let limit = frame_bytes(&first(&largest, kept)) - 1;
        assert_eq!(
            plans_within(largest.clone(), limit),
            first(&largest, kept - 1),
            "a limit of {limit} bytes"
        );
    }
    assert_eq!(plans_within(largest.clone(), 0), first(&largest, 0));
}

/// In the frame the sync sends, the largest snapshot goes whole, and so
/// does a small one.
#[test]
fn the_largest_snapshot_goes_whole_in_the_frame_the_sync_sends() {
    assert_eq!(plans_in_one_frame(largest_plans()), largest_plans());
    assert_eq!(plans_in_one_frame(report()), report());
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
        // Past the last second a timestamp holds, in year 9999.
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
        at: EpochSeconds::from_seconds(latest()),
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

fn repository(byte: u8) -> RepositoryId {
    RepositoryId::parse(&format!("{byte:02x}").repeat(32)).unwrap()
}

fn running(n: u128, repository: Option<RepositoryId>) -> RunningBackup {
    RunningBackup {
        op_id: OperationId::from_uuid(Uuid::from_u128(n)),
        repository,
    }
}

/// A heartbeat says the backups the machine runs, each with its repository
/// when it knows it, and one that can't tell leaves the list out (§16,
/// decision 166).
#[test]
fn a_heartbeat_reads_back_the_backups_it_names_or_that_it_cant_tell() {
    let named = [running(7, Some(repository(1))), running(8, None)];
    let bytes = heartbeat_payload(Some(&named));
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            r#"{{"running_ops":[{{"op_id":"00000000-0000-0000-0000-000000000007","kind":"backup","repo_id":"{}"}},{{"op_id":"00000000-0000-0000-0000-000000000008","kind":"backup"}}]}}"#,
            "01".repeat(32)
        )
    );
    assert_eq!(read_heartbeat(&bytes), Ok(Some(named.to_vec())));
    assert_eq!(
        read_heartbeat(&heartbeat_payload(Some(&[]))),
        Ok(Some(Vec::new()))
    );
    assert_eq!(heartbeat_payload(None), b"{}");
    assert_eq!(
        read_heartbeat(b"{}"),
        Ok(None),
        "unknown, never nothing running"
    );
}

#[test]
fn a_heartbeat_passes_over_an_operation_of_another_kind() {
    let text = format!(
        r#"{{"running_ops":[{{"op_id":"00000000-0000-0000-0000-000000000007","kind":"restore","repo_id":"{}"}}]}}"#,
        "01".repeat(32)
    );
    assert_eq!(read_heartbeat(text.as_bytes()), Ok(Some(Vec::new())));
}

#[test]
fn a_heartbeat_with_too_many_operations_or_an_id_that_isnt_one_is_refused() {
    let op = |n: usize| format!(r#"{{"op_id":"00000000-0000-0000-0000-{n:012}","kind":"backup"}}"#);
    let too_many = (0..=MAX_RUNNING_BACKUPS)
        .map(op)
        .collect::<Vec<_>>()
        .join(",");
    let refused = [
        format!(r#"{{"running_ops":[{too_many}]}}"#),
        r#"{"running_ops":[{"op_id":"seven","kind":"backup"}]}"#.to_owned(),
        r#"{"running_ops":[{"op_id":"00000000-0000-0000-0000-000000000007","kind":"backup","repo_id":"short"}]}"#.to_owned(),
        r#"{"running_ops":[{"op_id":"00000000-0000-0000-0000-000000000007"}]}"#.to_owned(),
    ];
    for text in refused {
        assert_eq!(read_heartbeat(text.as_bytes()), Err(WireRefused), "{text}");
    }
}

/// The hub's answer names the repositories whose prune it holds, and one
/// that can't tell leaves the list out.
#[test]
fn the_hubs_answer_to_a_heartbeat_reads_back_the_prunes_it_holds() {
    let held = [repository(1), repository(2)];
    let bytes = heartbeat_ack_payload(Some(&held));
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            r#"{{"prune_held":["{}","{}"]}}"#,
            "01".repeat(32),
            "02".repeat(32)
        )
    );
    assert_eq!(read_heartbeat_ack(&bytes), Ok(Some(held.to_vec())));
    assert_eq!(heartbeat_ack_payload(None), b"{}");
    assert_eq!(
        read_heartbeat_ack(b"{}"),
        Ok(None),
        "unknown holds every prune"
    );
    let too_many = (0..=MAX_RUNNING_BACKUPS)
        .map(|n| format!(r#""{}""#, format!("{n:02x}").repeat(32)))
        .collect::<Vec<_>>()
        .join(",");
    for text in [
        format!(r#"{{"prune_held":[{too_many}]}}"#),
        r#"{"prune_held":["short"]}"#.to_owned(),
    ] {
        assert_eq!(
            read_heartbeat_ack(text.as_bytes()),
            Err(WireRefused),
            "{text}"
        );
    }
}

/// The longest heartbeat and the longest answer each fit one frame.
#[test]
fn the_longest_heartbeat_and_answer_fit_one_frame() {
    let named: Vec<_> = (0..MAX_RUNNING_BACKUPS)
        .map(|n| running(u128::MAX - n as u128, Some(repository(0xff))))
        .collect();
    let payload = heartbeat_payload(Some(&named));
    assert_eq!(read_heartbeat(&payload), Ok(Some(named)));
    let heartbeat = agent_envelope_frame(&AgentEnvelope {
        kind: heartbeat_kind(),
        seq: u64::MAX,
        payload,
        signature: longest_signature(),
    })
    .to_text();
    assert!(
        heartbeat.len() <= MAX_FRAME_BYTES,
        "{} bytes",
        heartbeat.len()
    );
    let held: Vec<_> = (0..MAX_RUNNING_BACKUPS)
        .map(|n| repository(u8::try_from(n).unwrap()))
        .collect();
    assert_eq!(
        read_heartbeat_ack(&heartbeat_ack_payload(Some(&held))),
        Ok(Some(held.clone()))
    );
    let answer = hub_envelope_frame(&HubEnvelope {
        kind: heartbeat_ack_kind(),
        seq: u64::MAX,
        expires_at: EpochSeconds::from_seconds(latest()),
        payload: heartbeat_ack_payload(Some(&held)),
        signature: longest_signature(),
    })
    .to_text();
    assert!(answer.len() <= MAX_FRAME_BYTES, "{} bytes", answer.len());
}
