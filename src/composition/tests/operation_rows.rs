use super::support::*;
use super::*;

fn listed(paths_truncated: bool, tags_truncated: bool) -> SnapshotSummary {
    SnapshotSummary {
        id: domain::SnapshotId::parse(&"ab".repeat(32)).expect("a snapshot ID"),
        time: domain::Timestamp::from_second(1_800_000_000).expect("a time"),
        offset: domain::Offset::UTC,
        hostname: "demo-host".to_owned(),
        label: "home".to_owned(),
        tags: vec!["demo".to_owned()],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated,
        tags_truncated,
    }
}

#[test]
fn a_listed_snapshot_tells_the_page_what_was_cut_from_it() {
    let dto = snapshot_dto(&listed(true, false)).expect("a valid snapshot");
    assert!(dto.paths_truncated && !dto.tags_truncated);
    let dto = snapshot_dto(&listed(false, true)).expect("a valid snapshot");
    assert!(!dto.paths_truncated && dto.tags_truncated);
    let dto = snapshot_dto(&listed(false, false)).expect("a valid snapshot");
    assert!(!dto.paths_truncated && !dto.tags_truncated);
    assert_eq!(dto.label, "home");
}

#[test]
fn a_listed_snapshot_tells_the_page_the_offset_of_the_clock_that_took_it() {
    let chicago = domain::Offset::from_seconds(-6 * 3600).expect("an offset");
    let dto = snapshot_dto(&SnapshotSummary {
        offset: chicago,
        ..listed(false, false)
    })
    .expect("a valid snapshot");
    assert_eq!(dto.offset, Some(api::ClockOffset::new(chicago)));
    assert_eq!(dto.time, "2027-01-15T08:00:00Z", "the instant, in UTC");
}

/// A succeeded operation of `kind` as the log keeps it, with `result`
/// and the alias of its repository.
fn logged(
    kind: OperationKind,
    result: Option<EngineResult>,
    alias: Option<&str>,
) -> OperationRecord {
    let at = |second: i64| domain::Timestamp::from_second(1_800_000_000 + second).expect("a time");
    OperationRecord {
        id: OperationId::from_uuid(Uuid::from_u128(0x0192)),
        kind,
        trigger: Trigger::Manual,
        target: domain::execution::Target {
            plan: None,
            repository: Some(demo_repository()),
        },
        status: OperationStatus::Succeeded,
        queued_at: at(0),
        started_at: Some(at(10)),
        finished_at: Some(at(70)),
        warning_count: 0,
        warning_kinds: Vec::new(),
        result,
        repository_alias: alias.map(|alias| RepositoryAlias::parse(alias).expect("an alias")),
        locations: None,
    }
}

fn backed_up(bytes_processed: Option<u64>) -> EngineResult {
    EngineResult::Backup(app::ports::BackupSummary {
        repository: demo_repository(),
        snapshot: domain::SnapshotId::parse(&"cd".repeat(32)).expect("a snapshot ID"),
        parent: None,
        files_new: 1,
        files_changed: 2,
        files_unmodified: 3,
        bytes_processed,
        data_added: 4096,
        data_added_packed: 1024,
        error_count: 0,
    })
}

#[test]
fn a_maintenance_row_names_its_repository_by_the_alias_it_ran_under() {
    let check = logged(OperationKind::Check, None, Some("demo"));
    let dto = summary(&check).expect("a row the page can show");
    assert_eq!(dto.repository.as_deref(), Some("demo"));
    // A row logged before the alias was kept names none.
    let old = logged(OperationKind::Check, None, None);
    assert_eq!(summary(&old).expect("a row").repository, None);
}

/// The page names an operation's warnings by their codes, each once,
/// as the log keeps their kinds (#508).
#[test]
fn a_row_names_its_warnings_by_code() {
    let mut warned = logged(OperationKind::Backup, None, None);
    warned.status = OperationStatus::SucceededWithWarnings;
    warned.warning_count = 3;
    warned.warning_kinds = vec![WarningKind::SourceUnreadable, WarningKind::CheckWarning];
    assert_eq!(
        summary(&warned)
            .expect("a row the page can show")
            .warning_codes,
        [WarningCode::SourceUnreadable, WarningCode::CheckWarning]
    );
}

#[test]
fn a_backup_row_carries_its_snapshot_and_the_bytes_it_went_through() {
    let backup = logged(OperationKind::Backup, Some(backed_up(Some(65_536))), None);
    let dto = summary(&backup).expect("a row the page can show");
    assert_eq!(
        dto.result,
        Some(api::OperationResultDto::Backup {
            files: 6,
            bytes: 4096,
            duration_ms: 60_000,
            snapshot: Some("cd".repeat(32)),
            bytes_processed: Some(65_536),
        })
    );
    // A result logged before the bytes were kept shows none, not zero.
    let old = logged(OperationKind::Backup, Some(backed_up(None)), None);
    let Some(api::OperationResultDto::Backup {
        bytes_processed, ..
    }) = summary(&old).expect("a row").result
    else {
        panic!("a backup's result");
    };
    assert_eq!(bytes_processed, None);
}

#[test]
fn a_backups_file_count_stops_at_the_largest_number_it_can_hold() {
    // The engine's counts are its own: three that overflow when added
    // are shown as the most a count holds, never a panic of the read.
    let EngineResult::Backup(mut huge) = backed_up(None) else {
        panic!("a backup's result");
    };
    huge.files_new = u64::MAX;
    let backup = logged(
        OperationKind::Backup,
        Some(EngineResult::Backup(huge)),
        None,
    );
    let Some(api::OperationResultDto::Backup { files, .. }) =
        summary(&backup).expect("a row").result
    else {
        panic!("a backup's result");
    };
    assert_eq!(files, u64::MAX);
}

/// A failed row says why, in the engine's own words, which the engine
/// masked; a row that didn't fail has no failure.
#[test]
fn a_failed_operations_row_carries_its_failure_in_the_engines_words() {
    let mut backup = logged(OperationKind::Backup, None, None);
    backup.status = OperationStatus::Failed {
        reason: FailureReason::EngineFailed {
            detail: "no key opens the repository".to_owned(),
        },
    };
    assert_eq!(
        summary(&backup).expect("a row the page can show").failure,
        Some(api::ErrorDto::new(
            ErrorCode::OperationFailed,
            [],
            Some("no key opens the repository".to_owned())
        ))
    );
    let succeeded = logged(OperationKind::Backup, Some(backed_up(None)), None);
    assert_eq!(summary(&succeeded).expect("a row").failure, None);
}

/// `/api/v1` is published (§12.1): the failure is a field added beside
/// `failure-code`, which keeps its name and value, and a row that didn't
/// fail still says `failure-code: null` and has no `failure`.
#[test]
fn a_failed_operations_row_keeps_its_failure_code_beside_the_failure() {
    let mut backup = logged(OperationKind::Backup, None, None);
    backup.status = OperationStatus::Failed {
        reason: FailureReason::EngineExited { exit_code: Some(3) },
    };
    let row = summary(&backup).expect("a row the page can show");
    let wire = serde_json::to_value(&row).expect("a row is JSON");
    assert_eq!(
        wire.get("failure-code"),
        Some(&serde_json::json!("engine-exited")),
        "{wire}"
    );
    assert_eq!(
        wire.get("failure"),
        Some(&serde_json::json!({
            "code": "engine-exited",
            "args": {"exit_code": {"type": "text", "value": "3"}},
        })),
        "{wire}"
    );
    let succeeded = logged(OperationKind::Backup, Some(backed_up(None)), None);
    let wire = serde_json::to_value(summary(&succeeded).expect("a row")).expect("JSON");
    assert_eq!(wire.get("failure-code"), Some(&serde_json::Value::Null));
    assert_eq!(wire.get("failure"), None, "{wire}");
}
