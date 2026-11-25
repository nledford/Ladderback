//! The baseline schema (§16, decision 165): a database the migrations
//! before it left is refused, never half migrated; a removed machine's
//! current state goes with it while its history stays; and the schema
//! refuses a value the store never writes.

mod support;

use std::assert_matches;
use std::fs;

use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};
use store::OpenError;
use support::*;

/// The database `tests/fixtures/at-0029.sql` describes, at `scratch`'s
/// path: as the 29 migrations before the baseline left it, in WAL mode as
/// the store keeps it, with no connection left open.
async fn database_at_0029(scratch: &Scratch) {
    let mut raw = SqliteConnectOptions::new()
        .filename(scratch.path())
        .create_if_missing(true)
        .connect()
        .await
        .expect("the old database is created");
    sqlx::raw_sql(include_str!("fixtures/at-0029.sql"))
        .execute(&mut raw)
        .await
        .expect("the old database is filled");
    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&mut raw)
        .await
        .expect("the old database is in WAL mode");
    raw.close().await.expect("the old database is closed");
}

#[tokio::test]
async fn a_database_the_old_migrations_left_is_refused_as_old_and_left_as_it_was() {
    let scratch = Scratch::new();
    database_at_0029(&scratch).await;
    let before = fs::read(scratch.path()).expect("the database file is readable");

    let error = open_error(&scratch.path()).await;

    assert_matches!(error, OpenError::OldSchema { version: 29 });
    assert_eq!(
        fs::read(scratch.path()).expect("the database file is readable"),
        before,
        "the old database is left byte for byte as it was"
    );
}

/// A row of `operations` with `kind`, `trigger` and `status`, as the store
/// writes one, finished but for a failure's columns.
async fn insert_operation(
    raw: &mut SqliteConnection,
    n: u128,
    kind: &str,
    trigger: &str,
    status: &str,
) -> Result<(), String> {
    let failed = status == "failed";
    sqlx::query(
        r#"INSERT INTO operations (id, kind, "trigger", status, queued_at, error_code, error_args)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(id_text(n))
    .bind(kind)
    .bind(trigger)
    .bind(status)
    .bind(ms(0))
    .bind(failed.then_some("operation-failed"))
    .bind(failed.then_some("[]"))
    .execute(raw)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[tokio::test]
async fn the_log_keeps_the_kinds_triggers_and_statuses_the_store_writes_and_refuses_others() {
    let scratch = Scratch::new();
    let _store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let kinds = [
        "fake", "identify", "init", "backup", "forget", "adopt", "prune", "check", "ls", "restore",
        "download",
    ];
    let triggers = [
        "schedule",
        "manual",
        "catch-up",
        "retry",
        "hub",
        "mcp",
        "api-token",
    ];
    let statuses = [
        "queued",
        "running",
        "succeeded",
        "succeeded_with_warnings",
        "failed",
        "cancelled",
    ];
    let mut n = 0;
    for kind in kinds {
        for trigger in triggers {
            for status in statuses {
                n += 1;
                // One running maintenance operation per repository: these
                // name none, so none holds one.
                assert_eq!(
                    insert_operation(&mut raw, n, kind, trigger, status).await,
                    Ok(()),
                    "{kind}, {trigger}, {status}"
                );
            }
        }
    }
    for (kind, trigger, status) in [
        ("bakcup", "manual", "queued"),
        ("backup", "by-hand", "queued"),
        ("backup", "manual", "done"),
    ] {
        n += 1;
        let refused = insert_operation(&mut raw, n, kind, trigger, status).await;
        assert!(
            refused
                .as_ref()
                .is_err_and(|error| error.contains("CHECK constraint failed")),
            "{kind}, {trigger}, {status}: {refused:?}"
        );
    }
}

#[tokio::test]
async fn a_failure_is_kept_with_its_code_and_only_a_failure() {
    let scratch = Scratch::new();
    let _store = scratch.open().await;
    let mut raw = scratch.raw().await;
    assert_eq!(
        insert_operation(&mut raw, 3, "backup", "manual", "failed").await,
        Ok(()),
        "a failure with its code"
    );
    assert_eq!(
        insert_operation(&mut raw, 4, "backup", "manual", "succeeded").await,
        Ok(()),
        "a success with none"
    );
    let refused = sqlx::query(
        r#"INSERT INTO operations (id, kind, "trigger", status, queued_at)
           VALUES (?, 'backup', 'manual', 'failed', ?)"#,
    )
    .bind(id_text(1))
    .bind(ms(0))
    .execute(&mut raw)
    .await;
    assert!(refused.is_err(), "a failure with no code: {refused:?}");
    let refused = sqlx::query(
        r#"INSERT INTO operations (id, kind, "trigger", status, queued_at, error_code, error_args)
           VALUES (?, 'backup', 'manual', 'succeeded', ?, 'operation-failed', '[]')"#,
    )
    .bind(id_text(2))
    .bind(ms(0))
    .execute(&mut raw)
    .await;
    assert!(refused.is_err(), "a success with a code: {refused:?}");
}

/// The hub forgets a removed machine's latest plans, progress and
/// ownership reports with it, its current state (decision 165), and keeps
/// its operation reports, their tally and its watermarks, its history
/// (decision 154), and the lease it holds until that expires (I1).
#[cfg(feature = "hub")]
#[tokio::test]
async fn removing_a_machine_forgets_its_current_state_and_keeps_its_history_and_lease() {
    use app::ports::{AgentRegistry as _, OwnershipRegistry as _, ReportInbox as _};
    use domain::fleet::{
        AgentReport, EpochSeconds, Happening, OutboxEpoch, PlansReport, PublicKey, ReportSeq,
        ReportedPlan, ReportedProgress, WireName,
    };
    use domain::maintenance::{ClaimOutcome, RepositoryReport, RepositoryReports, StartWindow};
    use domain::planning::Maintenance;
    use domain::{AgentId, Uuid};

    let scratch = Scratch::new();
    let store = scratch.open().await;
    let mut raw = scratch.raw().await;
    let removed = AgentId::from_uuid(Uuid::from_u128(0xa1));
    let kept = AgentId::from_uuid(Uuid::from_u128(0xa2));
    for machine in [removed, kept] {
        register_machine(&mut raw, machine).await;
    }
    for (n, machine) in [(1_u128, removed), (2, kept)] {
        let plans = PlansReport::from_parts(
            vec![ReportedPlan {
                plan: plan("demo-home"),
                last: None,
                next_run: None,
            }],
            0,
        )
        .expect("a snapshot of plans");
        store
            .keep_plans(machine, &plans)
            .await
            .expect("the plans are kept");
        let progress = ReportedProgress {
            op_id: id(n),
            step: None,
            unit: WireName::parse("bytes").expect("a unit"),
            current: 1,
            total: None,
            at: EpochSeconds::from_seconds(1_800_000_000),
        };
        store
            .keep_progress(machine, &progress)
            .await
            .expect("the progress is kept");
        let entry = app::ports::OutboxEntry {
            epoch: OutboxEpoch::from_uuid(Uuid::from_u128(0xe0 + n)),
            seq: ReportSeq::FIRST,
            report: AgentReport {
                op_id: id(n),
                happening: Happening::Started,
                at: EpochSeconds::from_seconds(1_800_000_000),
            },
        };
        store
            .ingest(machine, &entry, at(0))
            .await
            .expect("the report is taken");
    }
    let repository = domain::RepositoryId::parse(REPOSITORY_HEX).expect("a repository ID");
    let reports = RepositoryReports::new(vec![RepositoryReport {
        repository: repository.clone(),
        request: Maintenance::Local,
        holds_other_agents_snapshots: false,
        floor: None,
    }]);
    let decided = store
        .report(removed, &reports, &[], at(0), &StartWindow::default())
        .await
        .expect("the report is decided");
    assert_matches!(
        decided.first().map(|decision| &decision.outcome),
        Some(ClaimOutcome::Granted(_))
    );
    // The other machine uses the repository too, and waits for the lease.
    store
        .report(kept, &reports, &[], at(0), &StartWindow::default())
        .await
        .expect("the other machine's report is decided");
    let lease_before = store
        .ownership(&repository)
        .await
        .expect("the ownership is read")
        .lease;

    let key = PublicKey::from_bytes(machine_key_bytes(removed)).expect("the machine's key");
    let removal = store
        .remove(removed, key, at(5))
        .await
        .expect("the removal is stored");
    assert!(removal.is_ok(), "the machine is removed: {removal:?}");

    // A message of the removed machine that was on its way: the hub keeps
    // nothing of it and decides nothing, as the removal leaves no state,
    // and says so as no failure of the store (decision 165).
    let late_plans = PlansReport::from_parts(Vec::new(), 0).expect("a snapshot of plans");
    store
        .keep_plans(removed, &late_plans)
        .await
        .expect("plans of a removed machine are no store failure");
    let late_progress = ReportedProgress {
        op_id: id(9),
        step: None,
        unit: WireName::parse("bytes").expect("a unit"),
        current: 2,
        total: None,
        at: EpochSeconds::from_seconds(1_800_000_001),
    };
    store
        .keep_progress(removed, &late_progress)
        .await
        .expect("progress of a removed machine is no store failure");
    let late = store
        .report(removed, &reports, &[], at(6), &StartWindow::default())
        .await
        .expect("a report of a removed machine is no store failure");
    assert!(
        late.is_empty(),
        "nothing is decided for a removed machine: {late:?}"
    );

    for (table, held) in [
        ("sync_plans", 0),
        ("sync_plan_snapshots", 0),
        ("sync_progress", 0),
        ("ownership_reports", 0),
        ("sync_reports", 1),
        ("sync_report_tallies", 1),
        ("sync_watermarks", 1),
    ] {
        assert_eq!(
            machine_rows(&mut raw, table, removed).await,
            held,
            "{table} of the removed machine"
        );
        assert_eq!(
            machine_rows(&mut raw, table, kept).await,
            1,
            "{table} of the machine kept"
        );
    }
    assert_eq!(
        store
            .ownership(&repository)
            .await
            .expect("the ownership is read")
            .lease,
        lease_before,
        "the removed machine's lease stays until it expires"
    );
}

/// How many rows of the hub's `table` hold `machine`'s ID.
#[cfg(feature = "hub")]
async fn machine_rows(raw: &mut SqliteConnection, table: &str, machine: domain::AgentId) -> i64 {
    let sql = match table {
        "sync_plans" => "SELECT COUNT(*) FROM sync_plans WHERE machine_id = ?",
        "sync_plan_snapshots" => "SELECT COUNT(*) FROM sync_plan_snapshots WHERE machine_id = ?",
        "sync_progress" => "SELECT COUNT(*) FROM sync_progress WHERE machine_id = ?",
        "ownership_reports" => "SELECT COUNT(*) FROM ownership_reports WHERE machine_id = ?",
        "sync_reports" => "SELECT COUNT(*) FROM sync_reports WHERE machine_id = ?",
        "sync_report_tallies" => "SELECT COUNT(*) FROM sync_report_tallies WHERE machine_id = ?",
        "sync_watermarks" => "SELECT COUNT(*) FROM sync_watermarks WHERE machine_id = ?",
        _ => panic!("no such table of the hub's: {table}"),
    };
    sqlx::query_scalar(sql)
        .bind(machine.as_uuid().to_string())
        .fetch_one(raw)
        .await
        .expect("a count")
}
