use serde_json::{from_str, to_string};

use crate::{
    InvalidOperation, LocationTriedDto, OperationDetailDto, OperationResultDto,
    OperationSummaryDto, OperationWarningDto, OperationsPageDto, WarningCode,
};

fn summary() -> OperationSummaryDto {
    OperationSummaryDto::new(
        "018f0000-0000-7000-8000-000000000001".to_owned(),
        "home".to_owned(),
        "backup".to_owned(),
        "succeeded".to_owned(),
        "2026-11-09T10:00:00Z".to_owned(),
        Some("2026-11-09T10:00:01Z".to_owned()),
        Some("2026-11-09T10:00:02Z".to_owned()),
        vec![WarningCode::SourceUnreadable],
        None,
        Some(OperationResultDto::Backup {
            files: 3,
            bytes: 4096,
            duration_ms: 1000,
            snapshot: Some("cd".repeat(32)),
            bytes_processed: Some(65_536),
        }),
        None,
    )
    .unwrap_or_else(|e| panic!("a valid summary: {e:?}"))
}

/// A summary whose run saw `/srv/lan`, then `/srv/public`, with these
/// outcomes.
fn of_two(outcomes: [&str; 2]) -> OperationSummaryDto {
    let tried = |position: u8, address: &str, outcome: &str| LocationTriedDto {
        position,
        address: address.to_owned(),
        outcome: outcome.to_owned(),
        source: None,
    };
    summary().with_locations(vec![
        tried(1, "/srv/lan", outcomes[0]),
        tried(2, "/srv/public", outcomes[1]),
    ])
}

/// A run on a repository reached at several locations names the one it
/// used, by position and address, and how many the repository had,
/// wherever the used one is in the order; a run that used none, and one
/// on a repository of one location, name none (decision 80).
#[test]
fn a_summary_names_the_location_its_run_used_of_how_many() {
    let used = |dto: &OperationSummaryDto| {
        dto.used_location()
            .map(|(used, count)| (used.position, used.address.clone(), count))
    };
    assert_eq!(
        used(&of_two(["silent", "used"])),
        Some((2, "/srv/public".to_owned(), 2))
    );
    assert_eq!(
        used(&of_two(["used", "not-tried"])),
        Some((1, "/srv/lan".to_owned(), 2))
    );
    assert_eq!(used(&of_two(["silent", "refused"])), None);
    assert_eq!(used(&summary()), None);
}

/// The summary carries every location its run saw, on the wire; one of
/// a repository of one location leaves the key out.
#[test]
fn a_summary_carries_the_locations_its_run_saw() {
    let dto = of_two(["silent", "used"]);
    let json = to_string(&dto).unwrap();
    assert!(
        json.contains(
            r#""locations":[{"position":1,"address":"/srv/lan","outcome":"silent"},{"position":2,"address":"/srv/public","outcome":"used"}]"#
        ),
        "{json}"
    );
    assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
    assert!(!to_string(&summary()).unwrap().contains("locations"));
}

/// A restore's or a download's summary names the entry it read, on the
/// wire; a summary of anything else leaves the key out (#847).
#[test]
fn a_summary_carries_the_entry_its_run_read() {
    let dto = summary().with_entry("/demo/demo-1.txt".to_owned());
    assert_eq!(dto.entry.as_deref(), Some("/demo/demo-1.txt"));
    let json = to_string(&dto).unwrap();
    assert!(json.contains(r#""entry":"/demo/demo-1.txt""#), "{json}");
    assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
    assert!(!to_string(&summary()).unwrap().contains("\"entry\""));
}

#[test]
fn a_summary_round_trips_through_json() {
    let dto = summary();
    let json = to_string(&dto).unwrap();
    assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
    assert!(
        json.contains("\"queued-at\""),
        "times are kebab-case on the wire: {json}"
    );

    // A maintenance row names its repository; a row an earlier version
    // sent has no such key and reads without one.
    let mut check = summary();
    check.kind = "check".to_owned();
    check.result = None;
    check.repository = Some("demo".to_owned());
    let json = to_string(&check).unwrap();
    assert!(json.contains(r#""repository":"demo""#), "{json}");
    assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), check);
    check.repository = None;
    let earlier = to_string(&check).unwrap();
    assert!(!earlier.contains("repository"), "{earlier}");
    assert_eq!(from_str::<OperationSummaryDto>(&earlier).unwrap(), check);
}

#[test]
fn a_summary_refuses_an_id_that_is_not_a_uuid() {
    let dto = summary();
    assert_eq!(
        OperationSummaryDto::new(
            "nope".to_owned(),
            dto.plan,
            dto.kind,
            dto.status,
            dto.queued_at,
            dto.started_at,
            dto.finished_at,
            dto.warning_codes,
            dto.failure,
            dto.result,
            dto.repository,
        ),
        Err(InvalidOperation::Id)
    );
}

#[test]
fn a_summary_refuses_an_id_of_the_wrong_length() {
    let dto = summary();
    // A digit short: the right shape, not a UUID.
    let short = "018f0000-0000-7000-8000-00000000000".to_owned();
    assert_eq!(
        OperationSummaryDto::new(
            short,
            dto.plan,
            dto.kind,
            dto.status,
            dto.queued_at,
            dto.started_at,
            dto.finished_at,
            dto.warning_codes,
            dto.failure,
            dto.result,
            dto.repository,
        ),
        Err(InvalidOperation::Id)
    );
}

#[test]
fn a_summary_refuses_a_time_that_is_not_rfc_3339() {
    let dto = summary();
    assert_eq!(
        OperationSummaryDto::new(
            dto.id,
            dto.plan,
            dto.kind,
            dto.status,
            "not a time".to_owned(),
            dto.started_at,
            dto.finished_at,
            dto.warning_codes,
            dto.failure,
            dto.result,
            dto.repository,
        ),
        Err(InvalidOperation::Time)
    );
}

#[test]
fn a_detail_round_trips_through_json() {
    let detail = OperationDetailDto {
        operation: summary(),
        warnings: vec![OperationWarningDto {
            code: WarningCode::SourceUnreadable,
            item: Some("/home/demo/lost".to_owned()),
            during: "backup".to_owned(),
            detail: "unreadable".to_owned(),
        }],
    };
    let json = to_string(&detail).unwrap();
    assert_eq!(from_str::<OperationDetailDto>(&json).unwrap(), detail);
}

#[test]
fn a_page_round_trips_through_json() {
    let page = OperationsPageDto {
        operations: vec![summary()],
        next: Some("018f0000-0000-7000-8000-000000000000".to_owned()),
    };
    let json = to_string(&page).unwrap();
    assert_eq!(from_str::<OperationsPageDto>(&json).unwrap(), page);
}

#[test]
fn a_result_names_its_kind_on_the_wire() {
    let results = [
        OperationResultDto::Backup {
            files: 3,
            bytes: 4096,
            duration_ms: 1000,
            snapshot: Some("cd".repeat(32)),
            bytes_processed: Some(65_536),
        },
        OperationResultDto::Prune {
            packs_deleted: 2,
            duration_ms: 1000,
        },
        OperationResultDto::Check {
            errors: 0,
            warnings: 1,
            duration_ms: 1000,
        },
    ];
    for result in results {
        let json = to_string(&result).unwrap();
        assert_eq!(from_str::<OperationResultDto>(&json).unwrap(), result);
    }
    // A result an earlier version sent, without the snapshot and the
    // bytes processed, reads as one, and is sent as it was.
    let earlier = r#"{"type":"backup","files":3,"bytes":4096,"duration-ms":1000}"#;
    let read = OperationResultDto::Backup {
        files: 3,
        bytes: 4096,
        duration_ms: 1000,
        snapshot: None,
        bytes_processed: None,
    };
    assert_eq!(from_str::<OperationResultDto>(earlier).unwrap(), read);
    assert_eq!(to_string(&read).unwrap(), earlier);
}

/// A summary told where its restore wrote keeps it; one never told names
/// nowhere (#813).
#[test]
fn a_summary_with_restored_to_names_where_the_restore_wrote() {
    assert_eq!(summary().restored_to, None);
    let folder = crate::RestoredToDto::Folder {
        path: "/srv/restored".to_owned(),
    };
    assert_eq!(
        summary().with_restored_to(folder.clone()).restored_to,
        Some(folder)
    );
}
