//! A row this version didn't write is reported as corrupt, naming the column
//! and the operation: never read as a default, never a panic. The rows are
//! inserted with a connection of our own, the way damage or another program
//! would put them there.

mod support;

use app::ports::{
    BackupSummary, EngineResult, HistoryQuery, OperationRecord, OperationStore, PageSize,
    StoreError,
};
use domain::Timestamp;
use domain::execution::{FailureReason, OperationKind, OperationStatus, Target, Trigger};
use domain::execution::{Warning, WarningKind};
use sqlx::sqlite::SqliteConnection;
use support::*;

/// One value of one column.
#[derive(Clone, Debug)]
enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<i64> for Cell {
    fn from(number: i64) -> Self {
        Self::Int(number)
    }
}

impl From<f64> for Cell {
    fn from(number: f64) -> Self {
        Self::Real(number)
    }
}

/// Milliseconds since the Unix epoch, for `seconds` after the tests' instant.
fn ms(seconds: i64) -> i64 {
    (1_800_000_000 + seconds) * 1000
}

/// One row of `operations`, column by column.
#[derive(Clone, Debug)]
struct RawRow {
    cells: Vec<(&'static str, Cell)>,
}

impl RawRow {
    /// The row this version writes for a succeeded backup with its result:
    /// one rule away from every row the cases below break.
    fn succeeded_backup(id: &str) -> Self {
        let summary = format!(
            r#"{{"type":"backup","repository":"{REPOSITORY_HEX}","snapshot":"{SNAPSHOT_HEX}","parent":null,"files_new":1,"files_changed":2,"files_unmodified":3,"data_added":4,"data_added_packed":5,"error_count":6}}"#
        );
        Self {
            cells: vec![
                ("id", id.into()),
                ("agent_id", "local".into()),
                ("kind", "backup".into()),
                ("plan_id", "demo-home".into()),
                ("repo_uuid", REPOSITORY_HEX.into()),
                ("trigger", "schedule".into()),
                ("status", "succeeded".into()),
                ("queued_at", ms(0).into()),
                ("started_at", ms(10).into()),
                ("finished_at", ms(70).into()),
                ("snapshot_id", SNAPSHOT_HEX.into()),
                ("summary_json", summary.into()),
                ("error_code", Cell::Null),
                ("error_args", Cell::Null),
                ("error_detail", Cell::Null),
                ("warning_count", 0_i64.into()),
            ],
        }
    }

    /// The row this version writes for a backup that failed with a message
    /// from the engine.
    fn failed(id: &str) -> Self {
        Self::succeeded_backup(id)
            .with("status", "failed")
            .null("snapshot_id")
            .null("summary_json")
            .with("error_code", "operation-failed")
            .with("error_args", "[]")
            .with("error_detail", "repository not found")
    }

    fn with(mut self, column: &'static str, cell: impl Into<Cell>) -> Self {
        let slot = self
            .cells
            .iter_mut()
            .find(|(name, _)| *name == column)
            .expect("a column of `operations`");
        slot.1 = cell.into();
        self
    }

    fn null(self, column: &'static str) -> Self {
        self.with(column, Cell::Null)
    }

    /// A backup whose `summary_json` is `summary` and which has no snapshot
    /// column of its own, so only the summary can be wrong.
    fn summarised(id: &str, summary: impl Into<Cell>) -> Self {
        Self::succeeded_backup(id)
            .null("snapshot_id")
            .with("summary_json", summary)
    }

    async fn insert(&self, conn: &mut SqliteConnection) {
        let columns: Vec<String> = self
            .cells
            .iter()
            .map(|(name, _)| format!("\"{name}\""))
            .collect();
        let marks = vec!["?"; self.cells.len()];
        let sql = format!(
            "INSERT INTO operations ({}) VALUES ({})",
            columns.join(", "),
            marks.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for (_, cell) in &self.cells {
            query = match cell {
                Cell::Null => query.bind(None::<i64>),
                Cell::Int(number) => query.bind(*number),
                Cell::Real(number) => query.bind(*number),
                Cell::Text(text) => query.bind(text.clone()),
            };
        }
        query
            .execute(&mut *conn)
            .await
            .expect("the raw row is inserted");
    }
}

/// A row, and the one rule it breaks.
struct Case {
    name: &'static str,
    row: RawRow,
    /// The column the report must name.
    column: &'static str,
    /// Whether `get` can address the row: one whose ID isn't the text this
    /// version writes for any ID can only be met by listing.
    found_by_id: bool,
}

/// The ID the cases' rows have, and the neighbour's, which is newer.
const BAD: u128 = 0x01;
const GOOD: u128 = 0x7f;

fn broken(name: &'static str, column: &'static str, row: RawRow) -> Case {
    Case {
        name,
        row,
        column,
        found_by_id: true,
    }
}

fn unaddressable(name: &'static str, row: RawRow) -> Case {
    Case {
        name,
        row,
        column: "id",
        found_by_id: false,
    }
}

/// A failed backup whose error columns are `code`, `args` and `detail`.
fn error_of(code: &str, args: Cell, detail: Cell) -> RawRow {
    RawRow::failed(&id_text(BAD))
        .with("error_code", code)
        .with("error_args", args)
        .with("error_detail", detail)
}

fn bad_summary(summary: &str) -> RawRow {
    RawRow::summarised(&id_text(BAD), summary)
}

fn good() -> RawRow {
    RawRow::succeeded_backup(&id_text(BAD))
}

fn cases() -> Vec<Case> {
    let text = |value: &str| Cell::from(value);
    let wrong_args = |exit_code: &str| {
        text(&format!(
            r#"[["exit_code",{{"type":"text","value":"{exit_code}"}}]]"#
        ))
    };
    vec![
        // What the operation is and where it stands.
        broken(
            "an unknown status",
            "status",
            good().with("status", "paused"),
        ),
        broken(
            "a status in the wrong case",
            "status",
            good().with("status", "Succeeded"),
        ),
        broken("an empty status", "status", good().with("status", "")),
        broken("an unknown kind", "kind", good().with("kind", "restore")),
        broken(
            "an unknown trigger",
            "trigger",
            good().with("trigger", "cron"),
        ),
        // Why it failed.
        broken(
            "a failed operation with no error code",
            "error_code",
            RawRow::failed(&id_text(BAD)).null("error_code"),
        ),
        broken(
            "an error code from a newer version",
            "error_code",
            error_of("quota-exceeded", text("[]"), text("x")),
        ),
        broken(
            "the unknown error code itself",
            "error_code",
            error_of("unknown", text("[]"), text("x")),
        ),
        broken(
            "a failed operation with no arguments",
            "error_args",
            error_of("deadline-passed", Cell::Null, Cell::Null),
        ),
        broken(
            "arguments that are not JSON",
            "error_args",
            error_of("deadline-passed", text("exit_code=3"), Cell::Null),
        ),
        broken(
            "arguments that are an object, not a list of pairs",
            "error_args",
            error_of("engine-exited", text(r#"{"exit_code":"3"}"#), Cell::Null),
        ),
        broken(
            "an argument that is not a pair",
            "error_args",
            error_of("engine-exited", text(r#"[["exit_code"]]"#), Cell::Null),
        ),
        broken(
            "an exit code that is not text",
            "error_args",
            error_of(
                "engine-exited",
                text(r#"[["exit_code",{"type":"count","value":3}]]"#),
                Cell::Null,
            ),
        ),
        broken(
            "an exit code that is not a number",
            "error_args",
            error_of("engine-exited", wrong_args("three"), Cell::Null),
        ),
        broken(
            "an exit code too large for a process",
            "error_args",
            error_of("engine-exited", wrong_args("2147483648"), Cell::Null),
        ),
        broken(
            "a transition nobody knows",
            "error_args",
            error_of(
                "protocol-violated",
                text(r#"[["transition",{"type":"text","value":"teleport"}]]"#),
                Cell::Null,
            ),
        ),
        broken(
            "a protocol violation that names no transition",
            "error_args",
            error_of("protocol-violated", text("[]"), Cell::Null),
        ),
        broken(
            "a transition that is not text",
            "error_args",
            error_of(
                "protocol-violated",
                text(r#"[["transition",{"type":"count","value":1}]]"#),
                Cell::Null,
            ),
        ),
        broken(
            "an engine failure without its message",
            "error_detail",
            error_of("operation-failed", text("[]"), Cell::Null),
        ),
        broken(
            "an engine that was unavailable, without its message",
            "error_detail",
            error_of("engine-unavailable", text("[]"), Cell::Null),
        ),
        broken(
            "a rejected job without its message",
            "error_detail",
            error_of("invalid-job-spec", text("[]"), Cell::Null),
        ),
        broken(
            "an error code on an operation that succeeded",
            "error_code",
            good().with("error_code", "deadline-passed"),
        ),
        broken(
            "error arguments on an operation that succeeded",
            "error_args",
            good().with("error_args", "[]"),
        ),
        broken(
            "an error message on an operation that succeeded",
            "error_detail",
            good().with("error_detail", "x"),
        ),
        // What it produced.
        broken(
            "a summary that is not JSON",
            "summary_json",
            bad_summary("{not json"),
        ),
        broken(
            "a summary that is the JSON null",
            "summary_json",
            bad_summary("null"),
        ),
        broken(
            "a summary of an unknown type",
            "summary_json",
            bad_summary(r#"{"type":"restore"}"#),
        ),
        broken(
            "a summary with no type",
            "summary_json",
            bad_summary(r#"{"steps_completed":3}"#),
        ),
        broken(
            "a count that is text",
            "summary_json",
            bad_summary(r#"{"type":"fake","steps_completed":"many"}"#),
        ),
        broken(
            "a count that is negative",
            "summary_json",
            bad_summary(r#"{"type":"fake","steps_completed":-1}"#),
        ),
        broken(
            "a count that is missing",
            "summary_json",
            bad_summary(r#"{"type":"fake"}"#),
        ),
        broken(
            "a backup summary with its fields missing",
            "summary_json",
            bad_summary(r#"{"type":"backup"}"#),
        ),
        broken(
            "a summary with an invalid snapshot ID",
            "summary_json",
            bad_summary(&format!(
                r#"{{"type":"backup","repository":"{REPOSITORY_HEX}","snapshot":"zz","parent":null,"files_new":1,"files_changed":2,"files_unmodified":3,"data_added":4,"data_added_packed":5,"error_count":6}}"#
            )),
        ),
        broken(
            "a summary with an invalid repository ID",
            "summary_json",
            bad_summary(r#"{"type":"repository","id":"abc"}"#),
        ),
        // The IDs.
        unaddressable(
            "an ID that is not a UUID",
            RawRow::succeeded_backup("not-a-uuid"),
        ),
        unaddressable(
            "an ID in upper case",
            RawRow::succeeded_backup(&id_text(0xab).to_uppercase()),
        ),
        unaddressable(
            "an ID without hyphens",
            RawRow::succeeded_backup(&format!("{BAD:032x}")),
        ),
        unaddressable(
            "an ID in braces",
            RawRow::succeeded_backup(&format!("{{{}}}", id_text(BAD))),
        ),
        broken(
            "an invalid plan ID",
            "plan_id",
            good().with("plan_id", "Demo Plan"),
        ),
        broken(
            "an invalid repository ID",
            "repo_uuid",
            good().with("repo_uuid", "repo"),
        ),
        broken(
            "an invalid snapshot ID",
            "snapshot_id",
            good().with("snapshot_id", "snap"),
        ),
        broken(
            "a snapshot that isn't the backup's",
            "snapshot_id",
            good().with("snapshot_id", PARENT_HEX),
        ),
        broken(
            "a snapshot on an operation with no result",
            "snapshot_id",
            good().null("summary_json"),
        ),
        broken(
            "a backup result on an operation with no snapshot",
            "snapshot_id",
            good().null("snapshot_id"),
        ),
        // The times.
        broken(
            "a queued time past the last timestamp",
            "queued_at",
            good().with("queued_at", i64::MAX),
        ),
        broken(
            "a start in the year 10000",
            "started_at",
            good().with("started_at", 253_402_300_800_000_i64),
        ),
        broken(
            "a finish before the first timestamp",
            "finished_at",
            good().with("finished_at", i64::MIN),
        ),
        broken(
            "a queued time that is text",
            "queued_at",
            good().with("queued_at", "yesterday"),
        ),
        broken(
            "a start that is a fraction",
            "started_at",
            good().with("started_at", 1.5_f64),
        ),
        // The warning count.
        broken(
            "a negative warning count",
            "warning_count",
            good().with("warning_count", -1_i64),
        ),
        broken(
            "a warning count that is text",
            "warning_count",
            good().with("warning_count", "many"),
        ),
        broken(
            "a warning count that is a fraction",
            "warning_count",
            good().with("warning_count", 2.5_f64),
        ),
    ]
}

/// The report of a corrupt row, or a failed test.
fn corrupt<T: std::fmt::Debug>(outcome: Result<T, StoreError>, what: &str) -> String {
    match outcome {
        Err(StoreError::Corrupt { detail }) => detail,
        other => panic!("{what}: expected a corrupt-row report, got {other:?}"),
    }
}

fn expected_good_record() -> OperationRecord {
    OperationRecord {
        id: id(BAD),
        kind: OperationKind::Backup,
        trigger: Trigger::Schedule,
        target: Target {
            plan: Some(plan("demo-home")),
            repository: Some(repository()),
        },
        status: OperationStatus::Succeeded,
        queued_at: at(0),
        started_at: Some(at(10)),
        finished_at: Some(at(70)),
        warning_count: 0,
        result: Some(EngineResult::Backup(BackupSummary {
            parent: None,
            ..backup_summary()
        })),
    }
}

#[tokio::test]
async fn the_rows_the_cases_start_from_are_good() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    RawRow::succeeded_backup(&id_text(BAD))
        .insert(&mut raw)
        .await;
    RawRow::failed(&id_text(0x02)).insert(&mut raw).await;

    let succeeded_one = store.get(id(BAD)).await;
    assert_eq!(
        succeeded_one.map(|found| found.map(|detail| detail.record)),
        Ok(Some(expected_good_record()))
    );
    let failed_one = store.get(id(0x02)).await.expect("reading works");
    assert_eq!(
        failed_one.map(|detail| detail.record.status),
        Some(OperationStatus::Failed {
            reason: FailureReason::EngineFailed {
                detail: "repository not found".to_owned()
            }
        })
    );
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("listing works");
    assert_eq!(page.operations.len(), 2);
}

#[tokio::test]
async fn a_row_this_version_didnt_write_is_reported_as_corrupt() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let neighbour = succeeded(queued_backup(GOOD));
    save(&store, &neighbour, None).await;
    let mut raw = scratch.raw().await;

    for case in cases() {
        case.row.insert(&mut raw).await;

        if case.found_by_id {
            let report = corrupt(store.get(id(BAD)).await, case.name);
            assert!(
                report.contains(&id_text(BAD)),
                "{}: the report names the operation: {report}",
                case.name
            );
            assert!(
                report.contains(case.column),
                "{}: the report names the column {}: {report}",
                case.name,
                case.column
            );
        }
        // The neighbour reads as it was saved, however damaged its neighbour.
        let found = store.get(id(GOOD)).await;
        assert_eq!(
            found.map(|found| found.map(|detail| detail.record)),
            Ok(Some(OperationRecord::of(&neighbour, None))),
            "{}: the neighbour is unaffected",
            case.name
        );
        // A page that holds the row can't be listed.
        let report = corrupt(store.list(&HistoryQuery::default()).await, case.name);
        assert!(
            report.contains(case.column),
            "{}: the listing's report names the column {}: {report}",
            case.name,
            case.column
        );

        sqlx::query("DELETE FROM operations WHERE id <> ?1")
            .bind(id_text(GOOD))
            .execute(&mut raw)
            .await
            .expect("the damaged row is removed");
    }
}

#[tokio::test]
async fn a_row_after_the_page_is_not_read() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let newest = succeeded(queued_backup(GOOD));
    save(&store, &newest, None).await;
    let mut raw = scratch.raw().await;
    good().with("status", "paused").insert(&mut raw).await;

    // The page ends before the damaged row, which is only looked at to tell
    // that older operations remain.
    let query = HistoryQuery {
        limit: PageSize::new(1),
        ..HistoryQuery::default()
    };
    let page = store
        .list(&query)
        .await
        .expect("a page that stops short of it");
    assert_eq!(
        page.operations,
        [OperationRecord::of(&newest, None)],
        "the newest operation is listed"
    );
    assert_eq!(page.next, Some(id(GOOD)));

    // The next page reaches it.
    let after = HistoryQuery {
        after: page.next,
        ..HistoryQuery::default()
    };
    let report = corrupt(store.list(&after).await, "the page that holds it");
    assert!(report.contains("status"), "{report}");
}

#[tokio::test]
async fn a_warning_row_with_a_field_missing_is_reported_as_corrupt() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    RawRow::succeeded_backup(&id_text(BAD))
        .insert(&mut raw)
        .await;

    for column in ["during", "code", "detail"] {
        sqlx::query("DELETE FROM op_warnings")
            .execute(&mut raw)
            .await
            .expect("the warnings are cleared");
        let mut values = [Some("backup"), Some("source-unreadable"), Some("denied")];
        let slot = ["during", "code", "detail"]
            .iter()
            .position(|name| *name == column)
            .expect("a warning column");
        values[slot] = None;
        sqlx::query(
            "INSERT INTO op_warnings (op_id, item, during, code, detail) VALUES (?1, NULL, ?2, ?3, ?4)",
        )
        .bind(id_text(BAD))
        .bind(values[0])
        .bind(values[1])
        .bind(values[2])
        .execute(&mut raw)
        .await
        .expect("the raw warning is inserted");

        let report = corrupt(store.get(id(BAD)).await, column);
        assert!(report.contains(column), "{column}: {report}");
        assert!(report.contains(&id_text(BAD)), "{column}: {report}");
        // The listing doesn't read warnings.
        let page = store
            .list(&HistoryQuery::default())
            .await
            .expect("listing works");
        assert_eq!(page.operations.len(), 1, "{column}");
    }
}

#[tokio::test]
async fn what_this_version_adds_nothing_to_is_still_read() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;

    // A result with a field this version doesn't know.
    let extra = format!(
        r#"{{"type":"backup","repository":"{REPOSITORY_HEX}","snapshot":"{SNAPSHOT_HEX}","parent":null,"files_new":1,"files_changed":2,"files_unmodified":3,"data_added":4,"data_added_packed":5,"error_count":6,"duration_ms":1500}}"#
    );
    RawRow::succeeded_backup(&id_text(0x01))
        .with("summary_json", extra)
        .insert(&mut raw)
        .await;
    // Error arguments in someone else's formatting, with an argument it doesn't know.
    RawRow::failed(&id_text(0x02))
        .with("error_code", "engine-exited")
        .with(
            "error_args",
            r#" [ ["exit_code", {"type": "text", "value": "3"}], ["signal", {"type": "text", "value": "9"}] ] "#,
        )
        .null("error_detail")
        .insert(&mut raw)
        .await;
    // Times far from now: before the epoch, and in the year 9000.
    let year_9000: Timestamp = "9000-01-01T00:00:00Z".parse().expect("a timestamp");
    RawRow::succeeded_backup(&id_text(0x03))
        .with("queued_at", -1_000_i64)
        .with("finished_at", year_9000.as_millisecond())
        .insert(&mut raw)
        .await;
    // A warning of a kind from a newer version, and one filed as unknown.
    RawRow::succeeded_backup(&id_text(0x04))
        .with("warning_count", 2_i64)
        .insert(&mut raw)
        .await;
    for (item, code) in [
        ("/home/demo/a", "xattr-unreadable"),
        ("/home/demo/b", "unknown"),
    ] {
        sqlx::query(
            "INSERT INTO op_warnings (op_id, item, during, code, detail) VALUES (?1, ?2, 'backup', ?3, 'denied')",
        )
        .bind(id_text(0x04))
        .bind(item)
        .bind(code)
        .execute(&mut raw)
        .await
        .expect("the raw warning is inserted");
    }

    let one = store
        .get(id(0x01))
        .await
        .expect("reading works")
        .expect("found");
    assert_eq!(one.record.result, expected_good_record().result);
    let two = store
        .get(id(0x02))
        .await
        .expect("reading works")
        .expect("found");
    assert_eq!(
        two.record.status,
        OperationStatus::Failed {
            reason: FailureReason::EngineExited { exit_code: Some(3) }
        }
    );
    let three = store
        .get(id(0x03))
        .await
        .expect("reading works")
        .expect("found");
    assert_eq!(
        three.record.queued_at,
        Timestamp::from_second(-1).expect("a timestamp")
    );
    assert_eq!(three.record.finished_at, Some(year_9000));
    let four = store
        .get(id(0x04))
        .await
        .expect("reading works")
        .expect("found");
    let other = |item: &str| Warning {
        kind: WarningKind::Other,
        item: Some(item.to_owned()),
        during: "backup".to_owned(),
        detail: "denied".to_owned(),
    };
    assert_eq!(
        four.warnings,
        [other("/home/demo/a"), other("/home/demo/b")],
        "a code this version doesn't know is a warning of another kind"
    );
}

#[tokio::test]
async fn a_report_of_a_corrupt_row_is_short_however_long_the_stored_value_is() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let huge = "x".repeat(1_000_000);

    for (n, row) in [
        (
            0x01,
            RawRow::succeeded_backup(&id_text(0x01)).with("status", huge.clone()),
        ),
        (
            0x02,
            RawRow::summarised(
                &id_text(0x02),
                format!(r#"{{"type":"fake","steps_completed":"{huge}"}}"#),
            ),
        ),
        (
            0x03,
            RawRow::succeeded_backup(&id_text(0x03)).with("plan_id", huge.clone()),
        ),
        (
            0x04,
            RawRow::failed(&id_text(0x04))
                .with("error_code", "engine-exited")
                .with(
                    "error_args",
                    format!(r#"[["exit_code",{{"type":"text","value":"{huge}"}}]]"#),
                )
                .null("error_detail"),
        ),
    ] {
        row.insert(&mut raw).await;
        let report = corrupt(store.get(id(n)).await, "a huge value");
        assert!(report.len() < 1_000, "{n}: {} bytes", report.len());
    }
}
