//! What the store puts in the database, column by column. This is the format
//! other versions and other programs read, so it is pinned exactly: the
//! tests read the rows with a connection of their own.

mod support;

use app::ports::{BackupSummary, CheckSummary, EngineResult, PruneSummary};
use domain::Timestamp;
use domain::execution::{
    FailureReason, Operation, OperationKind, Target, Transition, Trigger, Warning, WarningKind,
};
use serde_json::{Value, json};
use support::*;

/// What one operation's row must hold, spelled out in full.
struct Row {
    op: Operation,
    result: Option<EngineResult>,
    kind: &'static str,
    trigger: &'static str,
    status: &'static str,
    plan_id: Option<&'static str>,
    repo_uuid: Option<&'static str>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    warning_count: i64,
}

fn repository_only() -> Target {
    Target {
        plan: None,
        repository: Some(repository()),
    }
}

/// One operation per kind, trigger and status, so each spelling is seen once.
fn rows() -> Vec<Row> {
    let mut finished_with_warnings = with_warnings(
        started(queued(
            0x13,
            OperationKind::Prune,
            Trigger::Retry,
            repository_only(),
        )),
        2,
    );
    finished_with_warnings
        .succeed(at(70))
        .expect("a running operation succeeds");
    let mut cancelled = queued(0x14, OperationKind::Check, Trigger::Hub, repository_only());
    cancelled.cancel(at(5)).expect("a queued operation cancels");
    vec![
        Row {
            op: queued(
                0x10,
                OperationKind::Fake,
                Trigger::Schedule,
                Target::default(),
            ),
            result: None,
            kind: "fake",
            trigger: "schedule",
            status: "queued",
            plan_id: None,
            repo_uuid: None,
            started_at: None,
            finished_at: None,
            warning_count: 0,
        },
        Row {
            op: started(queued(
                0x11,
                OperationKind::Identify,
                Trigger::Manual,
                repository_only(),
            )),
            result: None,
            kind: "identify",
            trigger: "manual",
            status: "running",
            plan_id: None,
            repo_uuid: Some(REPOSITORY_HEX),
            started_at: Some(ms(10)),
            finished_at: None,
            warning_count: 0,
        },
        Row {
            op: succeeded(queued(
                0x12,
                OperationKind::Backup,
                Trigger::CatchUp,
                backup_target("demo-home"),
            )),
            result: Some(backup_result()),
            kind: "backup",
            trigger: "catch-up",
            status: "succeeded",
            plan_id: Some("demo-home"),
            repo_uuid: Some(REPOSITORY_HEX),
            started_at: Some(ms(10)),
            finished_at: Some(ms(70)),
            warning_count: 0,
        },
        Row {
            op: finished_with_warnings,
            result: Some(EngineResult::Prune(PruneSummary {
                repository: repository(),
                packs_marked: 1,
                packs_deleted: 2,
                packs_recovered: 3,
            })),
            kind: "prune",
            trigger: "retry",
            status: "succeeded_with_warnings",
            plan_id: None,
            repo_uuid: Some(REPOSITORY_HEX),
            started_at: Some(ms(10)),
            finished_at: Some(ms(70)),
            warning_count: 2,
        },
        Row {
            op: cancelled,
            result: None,
            kind: "check",
            trigger: "hub",
            status: "cancelled",
            plan_id: None,
            repo_uuid: Some(REPOSITORY_HEX),
            started_at: None,
            finished_at: Some(ms(5)),
            warning_count: 0,
        },
        Row {
            op: failed(
                queued(
                    0x15,
                    OperationKind::Backup,
                    Trigger::Mcp,
                    backup_target("demo-photos"),
                ),
                FailureReason::DeadlinePassed,
            ),
            result: None,
            kind: "backup",
            trigger: "mcp",
            status: "failed",
            plan_id: Some("demo-photos"),
            repo_uuid: Some(REPOSITORY_HEX),
            started_at: Some(ms(10)),
            finished_at: Some(ms(70)),
            warning_count: 0,
        },
    ]
}

#[tokio::test]
async fn an_operation_is_stored_as_its_own_columns() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    for row in rows() {
        save(&store, &row.op, row.result.as_ref()).await;
        let n = row.op.id().as_uuid().as_u128();
        let stored = stored_row(&mut raw, n).await;
        assert_eq!(stored.agent_id, "local", "{}", row.status);
        assert_eq!(stored.kind, row.kind);
        assert_eq!(stored.trigger, row.trigger);
        assert_eq!(stored.status, row.status);
        assert_eq!(stored.plan_id.as_deref(), row.plan_id, "{}", row.status);
        assert_eq!(stored.repo_uuid.as_deref(), row.repo_uuid, "{}", row.status);
        assert_eq!(stored.queued_at, ms(0), "{}", row.status);
        assert_eq!(stored.started_at, row.started_at, "{}", row.status);
        assert_eq!(stored.finished_at, row.finished_at, "{}", row.status);
        assert_eq!(stored.warning_count, row.warning_count, "{}", row.status);
        if row.status != "failed" {
            assert_eq!(
                (stored.error_code, stored.error_args, stored.error_detail),
                (None, None, None),
                "an operation that didn't fail has no error columns: {}",
                row.status
            );
        }
        assert_eq!(
            stored.summary_json.is_some(),
            row.result.is_some(),
            "{}",
            row.status
        );
    }
}

#[tokio::test]
async fn times_are_stored_as_milliseconds_cut_never_rounded() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let mut op = Operation::queue(
        id(0x20),
        OperationKind::Backup,
        Trigger::Manual,
        backup_target("demo-home"),
        Timestamp::constant(1_800_000_000, 999_999_999),
    );
    op.start(Timestamp::constant(1_800_000_010, 1))
        .expect("starts");
    op.succeed(Timestamp::constant(1_800_000_070, 500_000_000))
        .expect("succeeds");
    save(&store, &op, None).await;

    let stored = stored_row(&mut raw, 0x20).await;
    assert_eq!(stored.queued_at, 1_800_000_000_999);
    assert_eq!(stored.started_at, Some(1_800_000_010_000));
    assert_eq!(stored.finished_at, Some(1_800_000_070_500));
}

/// What each way of failing is stored as: the code, the arguments (exact
/// JSON text) and the engine's own text.
fn failures() -> Vec<(
    FailureReason,
    &'static str,
    &'static str,
    Option<&'static str>,
)> {
    let exited = |exit_code| FailureReason::EngineExited { exit_code };
    let violated = |transition| FailureReason::ProtocolViolated { transition };
    vec![
        (
            FailureReason::EngineUnavailable {
                detail: "No such file or directory (os error 2)".to_owned(),
            },
            "engine-unavailable",
            "[]",
            Some("No such file or directory (os error 2)"),
        ),
        (
            FailureReason::JobRejected {
                detail: "unknown field `bogus`".to_owned(),
            },
            "invalid-job-spec",
            "[]",
            Some("unknown field `bogus`"),
        ),
        (
            FailureReason::EngineFailed {
                detail: "repository is locked by another process".to_owned(),
            },
            "operation-failed",
            "[]",
            Some("repository is locked by another process"),
        ),
        (
            FailureReason::EngineFailed {
                detail: String::new(),
            },
            "operation-failed",
            "[]",
            Some(""),
        ),
        (
            exited(Some(3)),
            "engine-exited",
            r#"[["exit_code",{"type":"text","value":"3"}]]"#,
            None,
        ),
        (
            exited(Some(0)),
            "engine-exited",
            r#"[["exit_code",{"type":"text","value":"0"}]]"#,
            None,
        ),
        (
            exited(Some(i32::MIN)),
            "engine-exited",
            r#"[["exit_code",{"type":"text","value":"-2147483648"}]]"#,
            None,
        ),
        (
            exited(Some(i32::MAX)),
            "engine-exited",
            r#"[["exit_code",{"type":"text","value":"2147483647"}]]"#,
            None,
        ),
        (exited(None), "engine-exited", "[]", None),
        (
            violated(Transition::Start),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"start"}]]"#,
            None,
        ),
        (
            violated(Transition::ReportProgress),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"report-progress"}]]"#,
            None,
        ),
        (
            violated(Transition::RecordWarning),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"record-warning"}]]"#,
            None,
        ),
        (
            violated(Transition::Succeed),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"succeed"}]]"#,
            None,
        ),
        (
            violated(Transition::Fail),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"fail"}]]"#,
            None,
        ),
        (
            violated(Transition::Cancel),
            "protocol-violated",
            r#"[["transition",{"type":"text","value":"cancel"}]]"#,
            None,
        ),
        (FailureReason::DeadlinePassed, "deadline-passed", "[]", None),
        (
            FailureReason::EngineStartTimedOut,
            "engine-start-timed-out",
            "[]",
            None,
        ),
        (
            FailureReason::EngineExitTimedOut,
            "engine-exit-timed-out",
            "[]",
            None,
        ),
    ]
}

#[tokio::test]
async fn every_failure_is_stored_as_a_code_arguments_and_the_engines_own_text() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    for (n, (reason, code, args, detail)) in (0x100_u128..).zip(failures()) {
        let op = failed(queued_backup(n), reason.clone());
        save(&store, &op, None).await;

        let stored = stored_row(&mut raw, n).await;
        let what = format!("{reason:?}");
        assert_eq!(stored.status, "failed", "{what}");
        assert_eq!(stored.error_code.as_deref(), Some(code), "{what}");
        assert_eq!(stored.error_args.as_deref(), Some(args), "{what}");
        assert_eq!(stored.error_detail.as_deref(), detail, "{what}");
        assert_eq!(stored.summary_json, None, "{what}");
        assert_eq!(stored.snapshot_id, None, "{what}");
    }
}

#[tokio::test]
async fn an_operation_that_failed_before_it_started_has_no_start() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let mut op = queued_backup(0x30);
    op.fail(FailureReason::EngineStartTimedOut, at(5))
        .expect("a queued operation fails");
    save(&store, &op, None).await;

    let stored = stored_row(&mut raw, 0x30).await;
    assert_eq!(stored.status, "failed");
    assert_eq!(stored.started_at, None);
    assert_eq!(stored.finished_at, Some(ms(5)));
}

fn summary_of(row: &Stored) -> Value {
    let text = row.summary_json.as_deref().expect("a result is stored");
    serde_json::from_str(text).expect("the stored result is JSON")
}

#[tokio::test]
async fn every_result_is_stored_as_json_of_its_type() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let big = EngineResult::Backup(BackupSummary {
        data_added: u64::MAX,
        parent: None,
        ..backup_summary()
    });
    let results: Vec<(EngineResult, Value, Option<&str>)> = vec![
        (
            EngineResult::Fake {
                steps_completed: u32::MAX,
            },
            json!({"type": "fake", "steps_completed": 4_294_967_295_u64}),
            None,
        ),
        (
            EngineResult::Repository { id: repository() },
            json!({"type": "repository", "id": REPOSITORY_HEX}),
            None,
        ),
        (
            backup_result(),
            json!({
                "type": "backup", "repository": REPOSITORY_HEX, "snapshot": SNAPSHOT_HEX,
                "parent": PARENT_HEX, "files_new": 1, "files_changed": 2,
                "files_unmodified": 3, "data_added": 4, "data_added_packed": 5, "error_count": 6
            }),
            Some(SNAPSHOT_HEX),
        ),
        (
            big,
            json!({
                "type": "backup", "repository": REPOSITORY_HEX, "snapshot": SNAPSHOT_HEX,
                "parent": null, "files_new": 1, "files_changed": 2,
                "files_unmodified": 3, "data_added": u64::MAX, "data_added_packed": 5,
                "error_count": 6
            }),
            Some(SNAPSHOT_HEX),
        ),
        (
            EngineResult::Prune(PruneSummary {
                repository: repository(),
                packs_marked: 1,
                packs_deleted: 2,
                packs_recovered: 3,
            }),
            json!({
                "type": "prune", "repository": REPOSITORY_HEX, "packs_marked": 1,
                "packs_deleted": 2, "packs_recovered": 3
            }),
            None,
        ),
        (
            EngineResult::Check(CheckSummary {
                repository: repository(),
                errors: 1,
                warnings: 2,
            }),
            json!({"type": "check", "repository": REPOSITORY_HEX, "errors": 1, "warnings": 2}),
            None,
        ),
    ];

    for (n, (result, summary, snapshot_id)) in (0x200_u128..).zip(results) {
        let op = succeeded(queued_backup(n));
        save(&store, &op, Some(&result)).await;

        let stored = stored_row(&mut raw, n).await;
        assert_eq!(summary_of(&stored), summary, "{result:?}");
        assert_eq!(
            stored.snapshot_id.as_deref(),
            snapshot_id,
            "only a backup's snapshot has a column of its own: {result:?}"
        );
    }
}

#[tokio::test]
async fn warnings_are_stored_one_row_each_in_the_order_they_were_recorded() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let mut op = started(queued_backup(0x40));
    let recorded = [
        warning("/home/demo/zebra"),
        Warning {
            kind: WarningKind::Other,
            item: None,
            during: "listing".to_owned(),
            detail: "no item to name".to_owned(),
        },
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(String::new()),
            during: String::new(),
            detail: String::new(),
        },
        warning("/home/demo/apple"),
    ];
    for warning in &recorded {
        op.record_warning(warning.clone())
            .expect("a running operation records warnings");
    }
    save(&store, &op, None).await;

    let rows = stored_warnings(&mut raw, 0x40).await;
    let expected: Vec<StoredWarning> = [
        (
            Some("/home/demo/zebra"),
            "backup",
            "source-unreadable",
            "permission denied",
        ),
        (None, "listing", "unknown", "no item to name"),
        (Some(""), "", "source-unreadable", ""),
        (
            Some("/home/demo/apple"),
            "backup",
            "source-unreadable",
            "permission denied",
        ),
    ]
    .into_iter()
    .map(|(item, during, code, detail)| StoredWarning {
        item: item.map(str::to_owned),
        during: during.to_owned(),
        code: code.to_owned(),
        detail: detail.to_owned(),
    })
    .collect();
    assert_eq!(rows, expected);
    assert_eq!(stored_row(&mut raw, 0x40).await.warning_count, 4);
}

#[tokio::test]
async fn warnings_past_the_cap_are_counted_but_not_stored() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let op = with_warnings(started(queued_backup(0x50)), 10_003);
    assert_eq!(
        op.warning_count(),
        10_003,
        "the domain counts every warning"
    );
    save(&store, &op, None).await;

    assert_eq!(stored_row(&mut raw, 0x50).await.warning_count, 10_003);
    let items: Vec<Option<String>> = stored_warnings(&mut raw, 0x50)
        .await
        .into_iter()
        .map(|row| row.item)
        .collect();
    let expected: Vec<Option<String>> = (0..10_000)
        .map(|n| Some(format!("/home/demo/file-{n}")))
        .collect();
    assert_eq!(items.len(), 10_000);
    assert!(items == expected, "the first 10,000 are kept, in order");
}

#[tokio::test]
async fn a_later_save_replaces_every_column_and_every_warning() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let n = 0x60;

    // It failed, with every error column set...
    let first = failed(
        queued_backup(n),
        FailureReason::EngineExited { exit_code: Some(9) },
    );
    save(&store, &first, None).await;
    let after_first = stored_row(&mut raw, n).await;
    assert_eq!(after_first.error_code.as_deref(), Some("engine-exited"));

    // ...then it is saved as a success with a result and warnings: nothing of
    // the failure is left.
    let mut second = with_warnings(started(queued_backup(n)), 3);
    second.succeed(at(70)).expect("succeeds");
    save(&store, &second, Some(&backup_result())).await;
    let after_second = stored_row(&mut raw, n).await;
    assert_eq!(after_second.status, "succeeded_with_warnings");
    assert_eq!(
        (
            after_second.error_code,
            after_second.error_args,
            after_second.error_detail
        ),
        (None, None, None)
    );
    assert_eq!(after_second.snapshot_id.as_deref(), Some(SNAPSHOT_HEX));
    assert_eq!(after_second.warning_count, 3);
    assert_eq!(stored_warnings(&mut raw, n).await.len(), 3);

    // ...then as a queued operation of another kind, with no target: nothing
    // of the success is left, not the result, the snapshot or the warnings.
    let third = queued(n, OperationKind::Prune, Trigger::Manual, Target::default());
    save(&store, &third, None).await;
    let after_third = stored_row(&mut raw, n).await;
    assert_eq!(
        after_third,
        Stored {
            agent_id: "local".to_owned(),
            kind: "prune".to_owned(),
            plan_id: None,
            repo_uuid: None,
            trigger: "manual".to_owned(),
            status: "queued".to_owned(),
            queued_at: ms(0),
            started_at: None,
            finished_at: None,
            snapshot_id: None,
            summary_json: None,
            error_code: None,
            error_args: None,
            error_detail: None,
            warning_count: 0,
        }
    );
    assert_eq!(stored_warnings(&mut raw, n).await, []);
    assert_eq!(rows_of(&mut raw, "operations", "id", n).await, 1);
}
