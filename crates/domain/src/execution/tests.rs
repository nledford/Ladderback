use super::*;
use uuid::Uuid;

fn t(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn queued() -> Operation {
    Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(7)),
        OperationKind::Backup,
        Trigger::Manual,
        Target::default(),
        t(0),
    )
}

/// Only a run the schedule started is unattended: one a person, a hub,
/// a tool or a program asked for has someone waiting on its answer.
#[test]
fn only_a_run_the_schedule_started_is_unattended() {
    for trigger in [Trigger::Schedule, Trigger::CatchUp, Trigger::Retry] {
        assert!(trigger.is_unattended(), "{trigger:?}");
    }
    let token = Trigger::ApiToken {
        id: crate::access::api_token::ApiTokenId::from_bytes(&[7; crate::access::TOKEN_BYTES]),
        label: crate::access::api_token::ApiTokenLabel::new("nightly-mcp".to_owned()).unwrap(),
    };
    for trigger in [Trigger::Manual, Trigger::Hub, Trigger::Mcp, token] {
        assert!(!trigger.is_unattended(), "{trigger:?}");
    }
}

fn running() -> Operation {
    let mut op = queued();
    op.start(t(10)).unwrap();
    op
}

fn unreadable(path: &str) -> Warning {
    Warning {
        kind: WarningKind::SourceUnreadable,
        item: Some(path.to_owned()),
        during: "backup".to_owned(),
        detail: "permission denied".to_owned(),
    }
}

/// The history names each kind once, in the order its first warning
/// came, however many of it there are (#508).
#[test]
fn an_operations_warnings_are_named_by_kind_each_once_in_the_order_they_came() {
    let check = Warning {
        kind: WarningKind::CheckWarning,
        item: None,
        during: "check".to_owned(),
        detail: "pack not referenced in index".to_owned(),
    };
    let warnings = [
        unreadable("/home/demo/a"),
        check.clone(),
        unreadable("/home/demo/b"),
        check,
        unreadable("/home/demo/c"),
    ];
    assert_eq!(
        warning_kinds(&warnings),
        [WarningKind::SourceUnreadable, WarningKind::CheckWarning]
    );
    assert_eq!(warning_kinds(&[]), Vec::new());
}

/// Each warning is told by the code the history names it by, which a
/// notification's `{code}` repeats (#548).
#[test]
fn each_warning_kind_has_the_code_the_history_names_it_by() {
    let codes: Vec<&str> = [
        WarningKind::SourceUnreadable,
        WarningKind::Hook,
        WarningKind::MaintenancePartial,
        WarningKind::CheckWarning,
        WarningKind::CheckError,
        WarningKind::OtherHosts,
        WarningKind::LocationSkipped,
        WarningKind::SecretFileReadable,
        WarningKind::SecretFileWritable,
        WarningKind::FullDiskAccessMissing,
        WarningKind::Other,
    ]
    .into_iter()
    .map(WarningKind::code)
    .collect();
    assert_eq!(
        codes,
        [
            "source-unreadable",
            "hook-warning",
            "maintenance-partial",
            "check-warning",
            "check-error",
            "maintenance-other-hosts",
            "location-skipped",
            "secret-file-readable",
            "secret-file-writable",
            "full-disk-access-missing",
            "unknown",
        ]
    );
}

/// An operation names a readable secret file once, however many of its
/// reads met it: at its start and again in its choice of location
/// (#386). Another file, or another kind of warning on the same path,
/// is still said.
#[test]
fn an_operation_names_a_readable_secret_file_once() {
    let readable = |path: &str| Warning {
        kind: WarningKind::SecretFileReadable,
        item: Some(path.to_owned()),
        during: "secret".to_owned(),
        detail: "0644".to_owned(),
    };
    let mut op = queued();
    op.attach_warning(readable("/home/demo/nas-password"))
        .unwrap();
    op.start(t(10)).unwrap();
    op.record_warning(readable("/home/demo/nas-password"))
        .unwrap();
    op.record_warning(readable("/home/demo/lan-key")).unwrap();
    op.record_warning(unreadable("/home/demo/nas-password"))
        .unwrap();
    op.record_warning(readable("/home/demo/lan-key")).unwrap();
    assert_eq!(
        op.warnings(),
        [
            readable("/home/demo/nas-password"),
            readable("/home/demo/lan-key"),
            unreadable("/home/demo/nas-password"),
        ]
    );
    assert_eq!(op.warning_count(), 3);
}

/// An operation names a secret file others can change once too, however
/// many of its reads met it (#1137).
#[test]
fn an_operation_names_a_changeable_secret_file_once() {
    let changeable = |path: &str| Warning {
        kind: WarningKind::SecretFileWritable,
        item: Some(path.to_owned()),
        during: "secret".to_owned(),
        detail: "0620".to_owned(),
    };
    let mut op = queued();
    op.attach_warning(changeable("/home/demo/nas-password"))
        .unwrap();
    op.start(t(10)).unwrap();
    op.record_warning(changeable("/home/demo/nas-password"))
        .unwrap();
    op.record_warning(changeable("/home/demo/lan-key")).unwrap();
    op.record_warning(changeable("/home/demo/lan-key")).unwrap();
    assert_eq!(
        op.warnings(),
        [
            changeable("/home/demo/nas-password"),
            changeable("/home/demo/lan-key"),
        ]
    );
    assert_eq!(op.warning_count(), 2);
}

fn failed() -> OperationStatus {
    OperationStatus::Failed {
        reason: FailureReason::EngineExited { exit_code: Some(1) },
    }
}

/// A running operation that has recorded `count` distinct warnings.
fn warned(count: usize) -> Operation {
    let mut op = running();
    for n in 0..count {
        op.record_warning(unreadable(&format!("/home/demo/file-{n}")))
            .unwrap();
    }
    op
}

fn repository(digit: &str) -> RepositoryId {
    RepositoryId::parse(&digit.repeat(64)).unwrap()
}

fn on(repository: Option<RepositoryId>) -> Target {
    Target {
        plan: None,
        repository,
    }
}

#[test]
fn a_target_is_on_the_repository_it_names_and_on_no_other() {
    let named = on(Some(repository("a")));
    assert!(named.is_on(Some(&repository("a"))));
    assert!(!named.is_on(Some(&repository("b"))));
    assert!(!named.is_on(None));
}

#[test]
fn a_target_that_names_no_repository_is_on_any() {
    let unnamed = on(None);
    assert!(unnamed.is_on(Some(&repository("a"))));
    assert!(unnamed.is_on(None));
}

fn queued_on(repository: Option<RepositoryId>) -> Operation {
    Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(7)),
        OperationKind::Backup,
        Trigger::Manual,
        on(repository),
        t(0),
    )
}

#[test]
fn an_operation_queued_before_its_repository_was_known_names_the_one_it_started_on() {
    let mut op = queued_on(None);
    op.start_on(t(10), Some(&repository("a"))).unwrap();
    assert_eq!(op.status(), &OperationStatus::Running);
    assert_eq!(op.started_at(), Some(t(10)));
    assert_eq!(op.target(), &on(Some(repository("a"))));
}

#[test]
fn an_operation_that_names_its_repository_keeps_it_when_it_starts() {
    let mut op = queued_on(Some(repository("a")));
    op.start_on(t(10), Some(&repository("b"))).unwrap();
    assert_eq!(op.target(), &on(Some(repository("a"))));
    let mut op = queued_on(Some(repository("a")));
    op.start_on(t(10), None).unwrap();
    assert_eq!(op.target(), &on(Some(repository("a"))));
}

#[test]
fn a_start_that_opened_no_repository_names_none() {
    let mut op = queued_on(None);
    op.start_on(t(10), None).unwrap();
    assert_eq!(op.status(), &OperationStatus::Running);
    assert_eq!(op.target(), &on(None));
}

#[test]
fn an_operation_that_cannot_start_names_no_repository() {
    let mut op = queued_on(None);
    op.start(t(5)).unwrap();
    let error = op.start_on(t(10), Some(&repository("a"))).unwrap_err();
    assert_eq!(error.transition, Transition::Start);
    assert_eq!(op.target(), &on(None));
    assert_eq!(op.started_at(), Some(t(5)));
}

#[test]
fn an_operation_remembers_what_it_is_for() {
    let target = Target {
        plan: Some(PlanId::parse("demo-home").unwrap()),
        repository: Some(RepositoryId::parse(&"ab".repeat(32)).unwrap()),
    };
    let op = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(8)),
        OperationKind::Prune,
        Trigger::CatchUp,
        target.clone(),
        t(0),
    );
    assert_eq!(op.kind(), OperationKind::Prune);
    assert_eq!(op.trigger(), &Trigger::CatchUp);
    assert_eq!(op.target(), &target);
}

#[test]
fn an_operation_keeps_the_alias_of_the_repository_it_runs_on() {
    let alias = RepositoryAlias::parse("demo").unwrap();
    assert_eq!(queued().repository_alias(), None);
    let mut op = queued().with_repository_alias(alias.clone());
    op.start(t(10)).unwrap();
    op.succeed(t(20)).unwrap();
    assert_eq!(op.repository_alias(), Some(&alias));
}

#[test]
fn an_operation_keeps_the_entry_it_reads_through_its_run() {
    let entry = logged_entry::LoggedEntry::kept("/demo/demo-1.txt".to_owned());
    assert_eq!(queued().entry(), None);
    let mut op = queued().with_entry(entry.clone());
    op.start(t(10)).unwrap();
    op.succeed(t(20)).unwrap();
    assert_eq!(op.entry(), Some(&entry));
}

#[test]
fn warnings_up_to_the_cap_are_all_kept() {
    let op = warned(MAX_STORED_WARNINGS);
    assert_eq!(op.warnings().len(), MAX_STORED_WARNINGS);
    assert_eq!(op.warning_count(), MAX_STORED_WARNINGS as u64);
}

#[test]
fn warnings_past_the_cap_are_counted_but_not_kept() {
    let op = warned(MAX_STORED_WARNINGS + 5);
    assert_eq!(op.warnings().len(), MAX_STORED_WARNINGS);
    assert_eq!(op.warning_count(), MAX_STORED_WARNINGS as u64 + 5);
}

#[test]
fn the_warnings_kept_are_the_first_ones_in_order() {
    let op = warned(MAX_STORED_WARNINGS + 1);
    let last_kept = format!("/home/demo/file-{}", MAX_STORED_WARNINGS - 1);
    assert_eq!(
        op.warnings().first(),
        Some(&unreadable("/home/demo/file-0"))
    );
    assert_eq!(op.warnings().last(), Some(&unreadable(&last_kept)));
}

#[test]
fn a_refused_warning_is_not_counted() {
    let mut op = queued();
    assert!(op.record_warning(unreadable("/home/demo/a")).is_err());
    assert_eq!(op.warning_count(), 0);
}

/// The most of a text that survives a cut: the cap less the room the
/// marker takes.
const ROOM: usize = MAX_WARNING_TEXT_BYTES - WARNING_TEXT_CUT.len();

/// `text` cut after its first `bytes` bytes, and marked.
fn cut_after(text: &str, bytes: usize) -> String {
    let kept = text.get(..bytes).expect("a cut at a character's edge");
    format!("{kept}{WARNING_TEXT_CUT}")
}

#[test]
fn text_up_to_the_cap_is_kept_whole() {
    for len in [
        0,
        1,
        ROOM,
        MAX_WARNING_TEXT_BYTES - 1,
        MAX_WARNING_TEXT_BYTES,
    ] {
        let text = "x".repeat(len);
        assert_eq!(cut_warning_text(text.clone()), text, "{len} bytes");
    }
}

#[test]
fn text_one_byte_past_the_cap_is_cut_to_the_cap_and_marked() {
    let text = "x".repeat(MAX_WARNING_TEXT_BYTES + 1);
    let got = cut_warning_text(text.clone());
    assert_eq!(got, cut_after(&text, ROOM));
    assert_eq!(got.len(), MAX_WARNING_TEXT_BYTES);
}

/// A kept warning lives in the operation until it's saved, so a cut text
/// must give back the room of the text it was cut from, not only shorten.
#[test]
fn a_cut_text_holds_no_more_memory_than_the_cap() {
    let megabyte = "x".repeat(1024 * 1024);
    let got = cut_warning_text(megabyte);
    assert!(
        got.capacity() <= 2 * MAX_WARNING_TEXT_BYTES,
        "capacity {} after the cut",
        got.capacity()
    );
}

/// The other tests state the cap through its constant; this one pins the
/// value the design gives, 4 KiB, with the marker inside it.
#[test]
fn the_cap_is_4096_bytes_and_a_cut_text_is_4093_bytes_and_a_marker() {
    let whole = "x".repeat(4096);
    assert_eq!(cut_warning_text(whole.clone()), whole);

    let cut = cut_warning_text("x".repeat(4097));
    assert_eq!(cut.len(), 4096);
    assert_eq!(cut, format!("{}…", "x".repeat(4093)));
}

#[test]
fn text_as_long_as_a_worker_may_send_is_cut_to_the_cap_too() {
    // One worker protocol message is at most 1 MiB (technical design §4.1).
    let text = "x".repeat(1024 * 1024);
    let got = cut_warning_text(text.clone());
    assert_eq!(got, cut_after(&text, ROOM));
    assert_eq!(got.len(), MAX_WARNING_TEXT_BYTES);
}

#[test]
fn a_cut_never_splits_a_character_and_keeps_every_whole_one_that_fits() {
    // 2, 3 and 4 bytes wide.
    for c in ['é', '€', '😀'] {
        let width = c.len_utf8();
        // Every way the cut can fall: just before a `c`, inside one, and
        // just after one.
        for pad in (ROOM - width)..=ROOM {
            let mut text = "a".repeat(pad);
            while text.len() <= MAX_WARNING_TEXT_BYTES {
                text.push(c);
            }
            let fits = (ROOM - pad) / width;
            let want = format!(
                "{}{}{WARNING_TEXT_CUT}",
                "a".repeat(pad),
                c.to_string().repeat(fits)
            );

            let got = cut_warning_text(text);

            assert_eq!(got, want, "{c:?} after {pad} bytes");
            assert!(got.len() <= MAX_WARNING_TEXT_BYTES, "{c:?} after {pad}");
        }
    }
}

#[test]
fn text_of_exactly_the_cap_is_kept_whole_with_a_character_where_a_cut_would_fall() {
    for c in ['é', '€', '😀'] {
        let width = c.len_utf8();
        let mut text = c.to_string().repeat(MAX_WARNING_TEXT_BYTES / width);
        text.push_str(&"a".repeat(MAX_WARNING_TEXT_BYTES % width));
        assert_eq!(text.len(), MAX_WARNING_TEXT_BYTES);

        assert_eq!(cut_warning_text(text.clone()), text, "{c:?}");
    }
}

/// What a running operation keeps of `warning`, recorded alone.
fn kept(warning: Warning) -> Warning {
    let mut op = running();
    op.record_warning(warning).unwrap();
    assert_eq!(op.warnings().len(), 1);
    op.warnings().first().cloned().unwrap()
}

#[test]
fn each_text_field_of_a_kept_warning_is_cut_on_its_own() {
    let short = unreadable("/home/demo/locked.txt");
    let long = "y".repeat(MAX_WARNING_TEXT_BYTES + 100);
    let cut = cut_after(&long, ROOM);

    let long_item = Warning {
        item: Some(long.clone()),
        ..short.clone()
    };
    let long_during = Warning {
        during: long.clone(),
        ..short.clone()
    };
    let long_detail = Warning {
        detail: long,
        ..short.clone()
    };

    assert_eq!(
        kept(long_item),
        Warning {
            item: Some(cut.clone()),
            ..short.clone()
        }
    );
    assert_eq!(
        kept(long_during),
        Warning {
            during: cut.clone(),
            ..short.clone()
        }
    );
    assert_eq!(
        kept(long_detail),
        Warning {
            detail: cut,
            ..short
        }
    );
}

#[test]
fn a_missing_item_and_empty_text_stay_as_they_are_when_other_text_is_cut() {
    let long = "x".repeat(MAX_WARNING_TEXT_BYTES + 1);
    let warning = Warning {
        kind: WarningKind::Other,
        item: None,
        during: String::new(),
        detail: long.clone(),
    };

    assert_eq!(
        kept(warning),
        Warning {
            kind: WarningKind::Other,
            item: None,
            during: String::new(),
            detail: cut_after(&long, ROOM),
        }
    );
}

#[test]
fn a_cut_warning_is_counted_once_like_any_other() {
    let mut op = running();
    op.record_warning(unreadable(&"x".repeat(MAX_WARNING_TEXT_BYTES * 2)))
        .unwrap();
    assert_eq!(op.warning_count(), 1);
    assert_eq!(op.warnings().len(), 1);
    assert_eq!(
        op.warnings()
            .first()
            .and_then(|w| w.item.as_deref())
            .map(str::len),
        Some(MAX_WARNING_TEXT_BYTES)
    );
}

#[test]
fn only_a_running_operation_reports_progress() {
    assert_eq!(
        queued().report_progress(),
        Err(LifecycleError {
            transition: Transition::ReportProgress,
            status: OperationStatus::Queued,
        })
    );
    assert_eq!(running().report_progress(), Ok(()));
    let mut done = running();
    done.succeed(t(20)).unwrap();
    assert!(done.report_progress().is_err());
}

#[test]
fn a_new_operation_is_queued_with_no_start_or_finish() {
    let op = queued();
    assert_eq!(op.status(), &OperationStatus::Queued);
    assert_eq!(op.queued_at(), t(0));
    assert_eq!(op.started_at(), None);
    assert_eq!(op.finished_at(), None);
    assert_eq!(op.duration(), None);
}

#[test]
fn starting_a_queued_operation_makes_it_running() {
    let op = running();
    assert_eq!(op.status(), &OperationStatus::Running);
    assert_eq!(op.started_at(), Some(t(10)));
}

#[test]
fn an_operation_without_warnings_succeeds() {
    let mut op = running();
    op.succeed(t(70)).unwrap();
    assert_eq!(op.status(), &OperationStatus::Succeeded);
    assert_eq!(op.finished_at(), Some(t(70)));
    assert_eq!(op.duration(), Some(SignedDuration::from_secs(60)));
}

#[test]
fn warnings_turn_success_into_success_with_warnings() {
    let mut op = running();
    op.record_warning(unreadable("/a")).unwrap();
    op.record_warning(unreadable("/b")).unwrap();
    op.succeed(t(20)).unwrap();
    assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);
    assert_eq!(op.warnings(), &[unreadable("/a"), unreadable("/b")]);
}

#[test]
fn a_choice_of_location_is_recorded_with_its_warnings_until_the_operation_ends() {
    use locations::{LocationOutcome, LocationTried, LocationsRecord};
    let record = LocationsRecord {
        locations: vec![
            LocationTried {
                position: 1,
                address: "/srv/a".to_owned(),
                outcome: LocationOutcome::OtherRepository,
                source: None,
            },
            LocationTried {
                position: 2,
                address: "/srv/b".to_owned(),
                outcome: LocationOutcome::Used,
                source: None,
            },
        ],
    };
    let mut op = queued();
    assert_eq!(op.locations(), None);
    op.record_locations(record.clone(), |_| false).unwrap();
    assert_eq!(op.locations(), Some(&record));
    op.start(t(10)).unwrap();
    op.succeed(t(20)).unwrap();
    assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);
    assert_eq!(op.warnings().len(), 1);
    assert!(op.record_locations(record, |_| false).is_err());
}

#[test]
fn a_failed_operation_keeps_its_reason_and_warnings() {
    let mut op = running();
    op.record_warning(unreadable("/a")).unwrap();
    op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(15))
        .unwrap();
    assert_eq!(op.status(), &failed());
    assert_eq!(op.warnings().len(), 1);
    assert_eq!(op.duration(), Some(SignedDuration::from_secs(5)));
}

#[test]
fn a_queued_operation_can_fail_before_it_starts() {
    let mut op = queued();
    op.fail(
        FailureReason::EngineFailed {
            detail: "worker not found".to_owned(),
        },
        t(1),
    )
    .unwrap();
    assert!(op.status().is_finished());
    assert_eq!(op.finished_at(), Some(t(1)));
    assert_eq!(op.duration(), None);
}

#[test]
fn queued_and_running_operations_can_be_cancelled() {
    for mut op in [queued(), running()] {
        op.cancel(t(30)).unwrap();
        assert_eq!(op.status(), &OperationStatus::Cancelled);
        assert_eq!(op.finished_at(), Some(t(30)));
    }
}

#[test]
fn only_a_queued_operation_can_start() {
    let mut op = running();
    let err = op.start(t(11)).unwrap_err();
    assert_eq!(err.transition, Transition::Start);
    assert_eq!(err.status, OperationStatus::Running);
    assert_eq!(op.started_at(), Some(t(10)), "the first start is kept");
}

#[test]
fn a_queued_operation_can_neither_succeed_nor_record_warnings() {
    let mut op = queued();
    assert_eq!(
        op.succeed(t(1)).unwrap_err().transition,
        Transition::Succeed
    );
    assert_eq!(
        op.record_warning(unreadable("/a")).unwrap_err().transition,
        Transition::RecordWarning
    );
    assert_eq!(op, queued(), "refused transitions change nothing");
}

#[test]
fn a_finished_operation_refuses_every_transition() {
    let mut succeeded = running();
    succeeded.succeed(t(20)).unwrap();
    let mut with_warnings = running();
    with_warnings.record_warning(unreadable("/a")).unwrap();
    with_warnings.succeed(t(20)).unwrap();
    let mut failed = running();
    failed
        .fail(FailureReason::EngineExited { exit_code: None }, t(20))
        .unwrap();
    let mut cancelled = running();
    cancelled.cancel(t(20)).unwrap();

    for finished in [succeeded, with_warnings, failed, cancelled] {
        assert!(finished.status().is_finished());
        let mut op = finished.clone();
        assert!(op.start(t(40)).is_err());
        assert!(op.record_warning(unreadable("/z")).is_err());
        assert!(op.succeed(t(40)).is_err());
        assert!(
            op.fail(FailureReason::EngineExited { exit_code: None }, t(40))
                .is_err()
        );
        assert!(op.cancel(t(40)).is_err());
        assert_eq!(op, finished, "refused transitions change nothing");
    }
}

#[test]
fn queued_and_running_are_not_finished() {
    assert!(!OperationStatus::Queued.is_finished());
    assert!(!OperationStatus::Running.is_finished());
}

#[test]
fn prune_and_check_are_maintenance() {
    assert!(OperationKind::Prune.is_maintenance());
    assert!(OperationKind::Check.is_maintenance());
}

#[test]
fn backups_repository_identification_and_diagnostic_jobs_are_not_maintenance() {
    for kind in [
        OperationKind::Backup,
        OperationKind::Identify,
        OperationKind::Fake,
    ] {
        assert!(!kind.is_maintenance(), "{kind:?}");
    }
}

fn hook_warning(detail: &str) -> Warning {
    Warning {
        kind: WarningKind::Hook,
        item: None,
        during: "before".to_owned(),
        detail: detail.to_owned(),
    }
}

/// The level of the machine's own hooks is `machine` in the operation log
/// and on the wire (§16, decision 163); `agent` names no level.
#[test]
fn the_machines_hook_level_is_machine() {
    assert_eq!(HookLevel::Agent.code(), "machine");
    assert_eq!(HookLevel::parse("machine"), Some(HookLevel::Agent));
    assert_eq!(HookLevel::parse("agent"), None);
}

#[test]
fn hook_levels_and_ends_read_back_from_their_codes() {
    for level in [HookLevel::Agent, HookLevel::Repository, HookLevel::Plan] {
        assert_eq!(HookLevel::parse(level.code()), Some(level));
    }
    assert_eq!(
        [HookLevel::Agent, HookLevel::Repository, HookLevel::Plan].map(HookLevel::code),
        ["machine", "repository", "plan"]
    );
    assert_eq!(HookLevel::parse("agent"), None);
    for end in [
        HookEnd::Exited { code: 3 },
        HookEnd::Signalled,
        HookEnd::TimedOut,
        HookEnd::NotStarted,
    ] {
        assert_eq!(HookEnd::parse(end.code(), end.exit_code()), Some(end));
    }
    assert_eq!(
        [
            HookEnd::Exited { code: 3 },
            HookEnd::Signalled,
            HookEnd::TimedOut,
            HookEnd::NotStarted,
        ]
        .map(HookEnd::code),
        ["exit", "signal", "timeout", "not-started"]
    );
    assert_eq!(HookEnd::Exited { code: -1 }.exit_code(), Some(-1));
    assert_eq!(HookEnd::TimedOut.exit_code(), None);
    // An exit without its status, or a status on anything but an exit,
    // is no end a failure records.
    assert_eq!(HookEnd::parse("exit", None), None);
    assert_eq!(HookEnd::parse("signal", Some(3)), None);
    assert_eq!(HookEnd::parse("crashed", None), None);
}

#[test]
fn a_hook_failure_codes_as_hook_failed() {
    assert_eq!(
        FailureReason::HookFailed {
            hook: None,
            ended: Some(HookEnd::Exited { code: 3 }),
        }
        .code(),
        "hook-failed"
    );
    assert_eq!(
        FailureReason::HookFailed {
            hook: None,
            ended: None,
        }
        .code(),
        "hook-failed"
    );
    assert_eq!(FailureReason::RepositoryExists.code(), "repository-exists");
}

#[test]
fn a_daemon_that_stopped_codes_as_daemon_stopped() {
    assert_eq!(FailureReason::DaemonStopped.code(), "daemon-stopped");
}

#[test]
fn a_daemon_that_was_shut_down_codes_as_daemon_shut_down() {
    assert_eq!(FailureReason::DaemonShutDown.code(), "daemon-shut-down");
}

#[test]
fn a_backup_that_gave_way_to_a_newer_run_codes_as_superseded() {
    assert_eq!(FailureReason::Superseded.code(), "superseded");
}

/// A queued backup that waited for its keyring and couldn't run once the
/// wait was over codes as why, never as a stopped daemon (#1293).
#[test]
fn a_backup_that_waited_and_never_ran_codes_as_why() {
    assert_eq!(FailureReason::PlanRemoved.code(), "plan-removed");
    assert_eq!(FailureReason::PlanMoved.code(), "plan-moved");
    assert_eq!(FailureReason::ConfigUnusable.code(), "config-unusable");
    assert_eq!(
        FailureReason::KeyFilesNotLeftOut.code(),
        "key-files-not-left-out"
    );
}

#[test]
fn a_job_with_too_much_to_mask_codes_as_secrets_too_large_to_mask() {
    let reason = FailureReason::SecretsTooLargeToMask {
        field: Some("HTTPS_PROXY".to_owned()),
        limit: 262_144,
    };
    assert_eq!(reason.code(), "secrets-too-large-to-mask");
}

#[test]
fn a_replaced_repository_codes_as_repository_replaced() {
    assert_eq!(
        FailureReason::RepositoryReplaced.code(),
        "repository-replaced"
    );
}

#[test]
fn a_prune_whose_lease_lapsed_codes_as_maintenance_lease_lapsed() {
    assert_eq!(
        FailureReason::LeaseLapsed.code(),
        "maintenance-lease-lapsed"
    );
}

#[test]
fn a_prune_that_heard_no_answer_codes_as_maintenance_deletion_unanswered() {
    assert_eq!(
        FailureReason::DeletionUnanswered.code(),
        "maintenance-deletion-unanswered"
    );
}

#[test]
fn a_refusal_of_the_credentials_codes_as_credentials_refused() {
    for half in [None, Some(RepositoryHalf::Cold), Some(RepositoryHalf::Hot)] {
        assert_eq!(
            FailureReason::CredentialsRefused { half }.code(),
            "credentials-refused"
        );
    }
}

#[test]
fn no_repository_config_at_the_location_codes_as_repository_config_not_found() {
    let reason = FailureReason::RepositoryConfigNotFound {
        detail: "no config file".to_owned(),
    };
    assert_eq!(reason.code(), "repository-config-not-found");
}

/// A half is named `cold` or `hot`, and only those names read back.
#[test]
fn a_half_reads_back_from_its_name_and_from_nothing_else() {
    assert_eq!(RepositoryHalf::Cold.code(), "cold");
    assert_eq!(RepositoryHalf::Hot.code(), "hot");
    for half in [RepositoryHalf::Cold, RepositoryHalf::Hot] {
        assert_eq!(RepositoryHalf::parse(half.code()), Some(half));
    }
    for other in ["", "Hot", "warm", "hot "] {
        assert_eq!(RepositoryHalf::parse(other), None, "{other:?}");
    }
}

#[test]
fn only_a_failure_for_no_repository_at_the_location_found_none() {
    let failed = |reason| OperationStatus::Failed { reason };
    assert!(failed(FailureReason::RepositoryNotFound).found_no_repository());
    for status in [
        OperationStatus::Queued,
        OperationStatus::Running,
        OperationStatus::Succeeded,
        OperationStatus::SucceededWithWarnings,
        OperationStatus::Cancelled,
        failed(FailureReason::RepositoryExists),
        failed(FailureReason::DaemonStopped),
        failed(FailureReason::RepositoryConfigNotFound {
            detail: "no config file".to_owned(),
        }),
    ] {
        assert!(!status.found_no_repository(), "{status:?}");
    }
}

#[test]
fn no_repository_at_the_location_codes_as_repository_not_found() {
    assert_eq!(
        FailureReason::RepositoryNotFound.code(),
        "repository-not-found"
    );
}

#[test]
fn an_attached_warning_promotes_only_a_bare_success() {
    let mut op = running();
    op.succeed(t(20)).unwrap();
    op.attach_warning(hook_warning("3")).unwrap();
    assert_eq!(op.status(), &OperationStatus::SucceededWithWarnings);

    let mut op = running();
    op.fail(FailureReason::EngineExited { exit_code: Some(1) }, t(15))
        .unwrap();
    op.attach_warning(hook_warning("timeout")).unwrap();
    assert_eq!(op.status(), &failed());
    assert_eq!(op.warnings().len(), 1);
}

#[test]
fn a_cancelled_operation_takes_no_attached_warning() {
    let mut op = running();
    op.cancel(t(20)).unwrap();
    assert!(op.attach_warning(hook_warning("3")).is_err());
}

/// A run refused before anything ran is queued and failed in the same
/// instant, with the refusal as its reason, and never started: no time
/// of the run is the start of anything that ran (§5).
#[test]
fn a_refused_run_is_queued_and_failed_in_one_instant_and_never_started() {
    let reason = FailureReason::SecretRefused {
        field: "password".to_owned(),
        refusal: refusal::SettingRefusal::SecretMissing {
            source: "ladderback/demo".to_owned(),
        },
    };
    assert_eq!(reason.code(), "secret-missing");
    let target = Target {
        plan: Some(PlanId::parse("home").unwrap()),
        repository: None,
    };
    let op = Operation::refused(
        OperationId::from_uuid(Uuid::from_u128(7)),
        OperationKind::Backup,
        Trigger::Schedule,
        target.clone(),
        reason.clone(),
        t(30),
    );
    assert_eq!(op.status(), &OperationStatus::Failed { reason });
    assert_eq!(op.started_at(), None);
    assert_eq!(op.queued_at(), t(30));
    assert_eq!(op.finished_at(), Some(t(30)));
    assert_eq!(op.trigger(), &Trigger::Schedule);
    assert_eq!(op.kind(), OperationKind::Backup);
    assert_eq!(op.target(), &target);
}

#[test]
fn a_success_tells_its_repository_was_read() {
    for status in [
        OperationStatus::Succeeded,
        OperationStatus::SucceededWithWarnings,
    ] {
        assert_eq!(status.repository_reading(), Some(RepositoryReading::Read));
    }
}

#[test]
fn a_failure_to_open_or_read_the_repository_tells_it_unreadable() {
    let refusal = refusal::SettingRefusal::SecretMissing {
        source: "file:/srv/missing".to_owned(),
    };
    for reason in [
        FailureReason::RepositoryReplaced,
        FailureReason::RepositoryNotFound,
        FailureReason::RepositoryConfigNotFound {
            detail: "no config file".to_owned(),
        },
        FailureReason::CredentialsRefused { half: None },
        FailureReason::NoLocationUsable,
        FailureReason::NoLocationRuleHolds,
        FailureReason::PasswordNotAccepted,
        FailureReason::SecretRefused {
            field: "password".to_owned(),
            refusal,
        },
    ] {
        let status = OperationStatus::Failed {
            reason: reason.clone(),
        };
        assert_eq!(
            status.repository_reading(),
            Some(RepositoryReading::Unreadable),
            "{reason:?}"
        );
    }
}

#[test]
fn an_operation_unfinished_cancelled_or_failed_for_its_own_reason_tells_nothing() {
    let failed = |reason| OperationStatus::Failed { reason };
    for status in [
        OperationStatus::Queued,
        OperationStatus::Running,
        OperationStatus::Cancelled,
        failed(FailureReason::EngineFailed {
            detail: "source unreadable".to_owned(),
        }),
        failed(FailureReason::HookFailed {
            hook: None,
            ended: Some(HookEnd::Exited { code: 1 }),
        }),
        failed(FailureReason::DeadlinePassed),
        failed(FailureReason::DaemonStopped),
        failed(FailureReason::DaemonShutDown),
        failed(FailureReason::EngineUnavailable {
            detail: "worker not found".to_owned(),
        }),
        failed(FailureReason::RepositoryExists),
        failed(FailureReason::LeaseLapsed),
        failed(FailureReason::DeletionUnanswered),
    ] {
        assert_eq!(status.repository_reading(), None, "{status:?}");
    }
}

#[test]
fn an_operation_keeps_where_its_restore_writes() {
    assert_eq!(queued().restored_to(), None);
    let restored = queued().with_restored_to(logged_entry::RestoredTo::InPlace);
    assert_eq!(
        restored.restored_to(),
        Some(&logged_entry::RestoredTo::InPlace)
    );
}
