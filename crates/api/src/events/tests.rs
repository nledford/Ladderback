use serde_json::{Value, from_value, to_value};

use crate::*;

fn started() -> UiEvent {
    UiEvent::OperationStarted {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        repository: Some("home.demo".into()),
        started_at: "2026-11-09T10:00:00Z".into(),
    }
}

fn progress() -> UiEvent {
    UiEvent::OperationProgress {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        step: None,
        unit: ProgressUnit::Count,
        current: 3,
        total: Some(10),
        at: Some("2026-11-09T10:01:00Z".into()),
    }
}

fn succeeded() -> UiEvent {
    UiEvent::OperationSucceeded {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        finished_at: "2026-11-09T10:05:00Z".into(),
    }
}

fn retrying() -> UiEvent {
    UiEvent::BackupRetrying {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        plan: "home".into(),
        attempt: 2,
        attempts: 3,
        at: "2026-11-09T10:20:00Z".into(),
    }
}

#[test]
fn each_event_serializes_with_its_kebab_case_type() {
    for (event, kind) in [
        (started(), "operation-started"),
        (progress(), "operation-progress"),
        (
            UiEvent::OperationWarning {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: WarningCode::SourceUnreadable,
                args: Vec::new(),
            },
            "operation-warning",
        ),
        (succeeded(), "operation-succeeded"),
        (
            UiEvent::OperationCanceled {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                finished_at: "2026-11-09T10:05:00Z".into(),
            },
            "operation-cancelled",
        ),
        (
            UiEvent::OperationFailed {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                finished_at: "2026-11-09T10:05:00Z".into(),
            },
            "operation-failed",
        ),
        (
            UiEvent::MachineOnline {
                machine: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
            "machine-online",
        ),
        (
            UiEvent::MachineOffline {
                machine: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
            "machine-offline",
        ),
        (retrying(), "backup-retrying"),
    ] {
        let wire = to_value(&event).unwrap();
        assert_eq!(
            wire.get("type").and_then(Value::as_str),
            Some(kind),
            "{event:?}"
        );
    }
}

#[test]
fn the_fields_of_each_event_carry_their_own_names() {
    let wire = to_value(started()).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "type": "operation-started",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "repository": "home.demo",
            "started_at": "2026-11-09T10:00:00Z",
        })
    );
    let wire = to_value(progress()).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "type": "operation-progress",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "step": null,
            "unit": "count",
            "current": 3,
            "total": 10,
            "at": "2026-11-09T10:01:00Z",
        })
    );
    let wire = to_value(&UiEvent::OperationProgress {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        step: None,
        unit: ProgressUnit::Count,
        current: 1,
        total: None,
        at: None,
    })
    .unwrap();
    assert_eq!(wire.get("total"), Some(&Value::Null));
    assert_eq!(
        wire.get("at"),
        None,
        "a report with no instant leaves it out"
    );
    let wire = to_value(retrying()).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "type": "backup-retrying",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "plan": "home",
            "attempt": 2,
            "attempts": 3,
            "at": "2026-11-09T10:20:00Z",
        })
    );
}

/// Every event about one of the hub's machines is `machine-` and names it
/// in its `machine` field (§16, decision 163); the old `agent-` events are
/// none.
#[test]
fn the_events_of_the_hubs_machines_name_the_machine() {
    let progress = serde_json::json!({
        "operation": "op", "unit": "count", "current": 1, "at": "2026-11-09T10:00:00Z",
    });
    for wire in [
        serde_json::json!({"type": "machine-online", "machine": "m", "at": "t"}),
        serde_json::json!({"type": "machine-offline", "machine": "m", "at": "t"}),
        serde_json::json!({"type": "machine-pending", "machine": "m", "hostname": "h", "at": "t"}),
        serde_json::json!({"type": "machine-approved", "machine": "m", "hostname": "h", "at": "t"}),
        serde_json::json!({"type": "machine-removed", "machine": "m", "hostname": "h", "at": "t"}),
        serde_json::json!({"type": "machine-reported", "machine": "m", "operation": "op", "outcome": "running"}),
        serde_json::json!({"type": "machine-plans", "machine": "m"}),
        serde_json::json!({"type": "machine-progress", "machine": "m", "progress": progress}),
    ] {
        let event = from_value::<UiEvent>(wire.clone()).unwrap();
        assert_eq!(to_value(&event).unwrap(), wire);
    }
    for wire in [
        serde_json::json!({"type": "agent-online", "agent": "m", "at": "t"}),
        serde_json::json!({"type": "agent-plans", "agent": "m"}),
    ] {
        assert!(from_value::<UiEvent>(wire).is_err());
    }
}

#[test]
fn every_event_round_trips_through_its_wire_form() {
    let events = [
        started(),
        progress(),
        UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step: None,
            unit: ProgressUnit::Count,
            current: 1,
            total: None,
            at: None,
        },
        UiEvent::OperationWarning {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            code: WarningCode::SourceUnreadable,
            args: Vec::new(),
        },
        succeeded(),
        UiEvent::OperationFailed {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            code: ErrorCode::EngineUnavailable,
            args: Vec::new(),
            finished_at: "2026-11-09T10:05:00Z".into(),
        },
        UiEvent::MachineOnline {
            machine: "demo".into(),
            at: "2026-11-09T10:00:00Z".into(),
        },
        UiEvent::MachineOffline {
            machine: "demo".into(),
            at: "2026-11-09T10:00:00Z".into(),
        },
        retrying(),
    ];
    for event in events {
        assert_eq!(
            from_value::<UiEvent>(to_value(&event).unwrap()).unwrap(),
            event
        );
    }
}

/// Each step's code on the wire, pinned: a page reads these, and a page
/// of another version reads them too.
#[test]
fn a_progress_event_names_its_step_by_a_code_of_its_own() {
    use OperationStep as S;
    for (step, code) in [
        (S::ReadingIndex, "reading-index"),
        (S::GettingSnapshots, "getting-snapshots"),
        (S::BackingUp, "backing-up"),
        (S::ListingPacks, "listing-packs"),
        (S::CheckingCache, "checking-cache"),
        (S::CheckingHotRepository, "checking-hot-repository"),
        (S::CheckingTrees, "checking-trees"),
        (S::ReadingPackData, "reading-pack-data"),
        (S::FindingUsedData, "finding-used-data"),
        (S::MarkingUnusedPacks, "marking-unused-packs"),
        (S::Repacking, "repacking"),
        (S::RebuildingIndex, "rebuilding-index"),
        (S::DeletingUnusedData, "deleting-unused-data"),
        (S::DeletingSnapshots, "deleting-snapshots"),
        (S::SavingSnapshots, "saving-snapshots"),
    ] {
        let event = UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step: Some(step),
            unit: ProgressUnit::Count,
            current: 3,
            total: Some(10),
            at: None,
        };
        let wire = to_value(&event).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "operation-progress",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "step": code,
                "unit": "count",
                "current": 3,
                "total": 10,
            })
        );
        assert_eq!(from_value::<UiEvent>(wire).unwrap(), event);
    }
}

/// A page reads the progress of a daemon older than the step as no step,
/// and a step newer than itself as one it doesn't know: the progress
/// still counts.
#[test]
fn a_progress_event_with_no_step_or_one_the_page_doesnt_know_still_reads() {
    let progress_with = |step: Option<Value>| {
        let mut wire = serde_json::json!({
            "type": "operation-progress",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "current": 3,
            "total": 10,
        });
        if let (Some(step), Some(fields)) = (step, wire.as_object_mut()) {
            fields.insert("step".to_owned(), step);
        }
        from_value::<UiEvent>(wire).unwrap()
    };
    let read = |step| UiEvent::OperationProgress {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        step,
        unit: ProgressUnit::Count,
        current: 3,
        total: Some(10),
        at: None,
    };
    assert_eq!(progress_with(None), read(None));
    assert_eq!(progress_with(Some(Value::Null)), read(None));
    assert_eq!(
        progress_with(Some(Value::from("defragmenting"))),
        read(Some(OperationStep::Unknown))
    );
    assert_eq!(
        progress_with(Some(Value::from("backing-up"))),
        read(Some(OperationStep::BackingUp))
    );
}

/// A bar of bytes says so, so a page can show its amounts as sizes. A
/// page reads the progress of a daemon older than the unit as a count,
/// and a unit newer than itself as one it doesn't know: the progress
/// still counts.
#[test]
fn a_progress_event_says_what_it_counts_and_reads_without_a_unit_it_knows() {
    let wire = |unit: Option<&str>| {
        let mut wire = serde_json::json!({
            "type": "operation-progress",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "step": "backing-up",
            "current": 3,
            "total": 10,
        });
        if let (Some(unit), Some(fields)) = (unit, wire.as_object_mut()) {
            fields.insert("unit".to_owned(), Value::from(unit));
        }
        wire
    };
    let read = |unit| UiEvent::OperationProgress {
        operation: "018f0000-0000-7000-8000-000000000001".into(),
        step: Some(OperationStep::BackingUp),
        unit,
        current: 3,
        total: Some(10),
        at: None,
    };
    assert_eq!(
        to_value(read(ProgressUnit::Bytes)).unwrap(),
        wire(Some("bytes"))
    );
    assert_eq!(
        to_value(read(ProgressUnit::Count)).unwrap(),
        wire(Some("count"))
    );
    assert_eq!(
        from_value::<UiEvent>(wire(None)).unwrap(),
        read(ProgressUnit::Count)
    );
    assert_eq!(
        from_value::<UiEvent>(wire(Some("furlongs"))).unwrap(),
        read(ProgressUnit::Unknown)
    );
}

#[test]
fn an_event_of_an_unknown_type_is_not_one() {
    let wire = serde_json::json!({"type": "operation-paused"});
    assert!(from_value::<UiEvent>(wire).is_err());
}
