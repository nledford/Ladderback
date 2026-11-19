use std::collections::BTreeMap;

use super::*;

#[test]
fn codes_travel_as_kebab_case_strings() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::InvalidJobSpec).unwrap(),
        r#""invalid-job-spec""#
    );
    assert_eq!(
        serde_json::to_string(&WarningCode::SourceUnreadable).unwrap(),
        r#""source-unreadable""#
    );
}

#[test]
fn other_hosts_have_their_own_code_counted_and_named() {
    assert_eq!(
        serde_json::to_string(&WarningCode::MaintenanceOtherHosts).unwrap(),
        r#""maintenance-other-hosts""#
    );
    let spec = |name, kind| ArgSpec {
        name,
        kind,
        optional: false,
    };
    assert_eq!(
        WarningCode::MaintenanceOtherHosts.args(),
        &[spec("count", ArgKind::Count), spec("hosts", ArgKind::Text)]
    );
    let named = ["demo-desktop".to_owned(), "demo-nas".to_owned()];
    let warning = WarningDto::other_hosts(7, &named);
    assert_eq!(warning.code, WarningCode::MaintenanceOtherHosts);
    assert_eq!(
        warning.args,
        BTreeMap::from([
            ("count".to_owned(), MsgArg::Count(7)),
            (
                "hosts".to_owned(),
                MsgArg::Text("demo-desktop, demo-nas".to_owned())
            ),
        ])
    );
}

#[test]
fn a_partial_maintenance_run_has_its_own_code_and_declares_its_arguments() {
    assert_eq!(
        serde_json::to_string(&WarningCode::MaintenancePartial).unwrap(),
        r#""maintenance-partial""#
    );
    let spec = |name, kind| ArgSpec {
        name,
        kind,
        optional: false,
    };
    assert_eq!(
        WarningCode::MaintenancePartial.args(),
        &[
            spec("what", ArgKind::Text),
            spec("done", ArgKind::Count),
            spec("total", ArgKind::Count),
        ]
    );
}

#[test]
fn a_passed_deadline_travels_as_its_own_code() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::DeadlinePassed).unwrap(),
        r#""deadline-passed""#
    );
    let read: ErrorCode = serde_json::from_str(r#""deadline-passed""#).unwrap();
    assert_eq!(read, ErrorCode::DeadlinePassed);
}

#[test]
fn a_code_from_a_newer_peer_reads_as_unknown() {
    let error: ErrorCode = serde_json::from_str(r#""quota-exceeded""#).unwrap();
    assert_eq!(error, ErrorCode::Unknown);
    let warning: WarningCode = serde_json::from_str(r#""xattr-unreadable""#).unwrap();
    assert_eq!(warning, WarningCode::Unknown);
}

#[test]
fn message_arguments_keep_their_type() {
    for arg in [
        MsgArg::Count(3),
        MsgArg::Bytes(1 << 30),
        MsgArg::DurationMs(90_000),
        MsgArg::Text("9898".to_owned()),
    ] {
        let json = serde_json::to_string(&arg).unwrap();
        assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), arg);
    }
}

#[test]
fn a_repository_row_carries_its_warnings_as_a_code_and_arguments_and_leaves_none_out() {
    let row = RepositoryDto {
        alias: "demo".to_owned(),
        repository: "/srv/demo/repo".to_owned(),
        repo_hot: None,
        password: PasswordSourceDto::Inline,
        maintained_here: true,
        create_offered: true,
        warnings: vec![WarningDto {
            code: WarningCode::ConfigInlineSecret,
            args: [("field".to_owned(), MsgArg::Text("password".to_owned()))].into(),
        }],
        locations: Vec::new(),
    };
    let json = serde_json::to_string(&row).unwrap();
    assert_eq!(
        json,
        r#"{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":true,"create-offered":true,"warnings":[{"code":"config-inline-secret","args":{"field":{"type":"text","value":"password"}}}]}"#
    );
    assert_eq!(serde_json::from_str::<RepositoryDto>(&json).unwrap(), row);
    let bare = r#"{"alias":"demo","repository":"/x","password":{"source":"inline"}}"#;
    assert!(
        serde_json::from_str::<RepositoryDto>(bare)
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn a_zone_name_with_the_shape_of_one_is_accepted_up_to_the_length_cap() {
    let longest = "a".repeat(IanaZone::MAX_LEN);
    for name in [
        "UTC",
        "Europe/Berlin",
        "America/Argentina/Buenos_Aires",
        "America/Port-au-Prince",
        "Etc/GMT+5",
        longest.as_str(),
    ] {
        let zone = IanaZone::new(name).unwrap_or_else(|_| panic!("{name} is a valid shape"));
        assert_eq!(zone.as_str(), name);
    }
}

#[test]
fn a_zone_name_that_could_walk_a_path_or_overflow_is_rejected() {
    let too_long = "a".repeat(IanaZone::MAX_LEN + 1);
    for name in [
        "",
        ".",
        "..",
        "../../etc/passwd",
        "/etc/localtime",
        "Europe/../Berlin",
        "Europe//Berlin",
        "Europe/Berlin/",
        "Europe\\Berlin",
        "Europe/Ber lin",
        "Europe/Berlin\n",
        "Europe/Zürich",
        too_long.as_str(),
    ] {
        assert_eq!(IanaZone::new(name), Err(InvalidZone), "{name:?}");
    }
}

#[test]
fn a_timestamp_travels_as_utc_plus_an_optional_zone() {
    let zoned = MsgArg::Timestamp {
        utc_ms: 1_794_144_600_000,
        zone: Some(IanaZone::new("Europe/Berlin").unwrap()),
    };
    let json = serde_json::to_string(&zoned).unwrap();
    assert_eq!(
        json,
        r#"{"type":"timestamp","value":{"utc_ms":1794144600000,"zone":"Europe/Berlin"}}"#
    );
    assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), zoned);

    // Before the epoch, and without a zone: the field is left out, and a
    // peer that writes `null` instead is read the same way.
    let unzoned = MsgArg::Timestamp {
        utc_ms: -1,
        zone: None,
    };
    let json = serde_json::to_string(&unzoned).unwrap();
    assert_eq!(json, r#"{"type":"timestamp","value":{"utc_ms":-1}}"#);
    assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), unzoned);
    let explicit_null = r#"{"type":"timestamp","value":{"utc_ms":-1,"zone":null}}"#;
    assert_eq!(
        serde_json::from_str::<MsgArg>(explicit_null).unwrap(),
        unzoned
    );
}

#[test]
fn a_timestamp_naming_a_hostile_zone_is_rejected_when_read() {
    let hostile = r#"{"type":"timestamp","value":{"utc_ms":0,"zone":"../../etc/passwd"}}"#;
    assert!(serde_json::from_str::<MsgArg>(hostile).is_err());
}

#[test]
fn a_code_declares_the_arguments_its_producers_send() {
    let spec = |name, kind, optional| ArgSpec {
        name,
        kind,
        optional,
    };
    assert_eq!(
        ErrorCode::EngineExited.args(),
        &[spec("exit_code", ArgKind::Text, true)]
    );
    assert_eq!(
        ErrorCode::ProtocolViolated.args(),
        &[spec("transition", ArgKind::Text, false)]
    );
    let named = [
        spec("field", ArgKind::Text, false),
        spec("source", ArgKind::Text, false),
    ];
    for code in [
        ErrorCode::SecretMissing,
        ErrorCode::SecretUnreadable,
        ErrorCode::SecretKeyringUnavailable,
        ErrorCode::SecretKeyringLocked,
        ErrorCode::SecretNotText,
        ErrorCode::SecretCommandFailed,
        ErrorCode::SecretCommandOutputOpen,
    ] {
        assert_eq!(code.args(), &named, "{code:?}");
    }
    assert_eq!(
        ErrorCode::SecretTooLarge.args(),
        &[named[0], named[1], spec("limit", ArgKind::Bytes, false)]
    );
    for code in [
        ErrorCode::SecretCommandTimedOut,
        ErrorCode::SecretKeyringTimedOut,
        ErrorCode::SecretFileTimedOut,
    ] {
        assert_eq!(
            code.args(),
            &[named[0], named[1], spec("seconds", ArgKind::Count, false)],
            "{code:?}"
        );
    }
    for code in [
        ErrorCode::RepositoryAliasTaken,
        ErrorCode::RepositoryAlreadyConfigured,
    ] {
        assert_eq!(
            code.args(),
            &[spec("alias", ArgKind::Text, false)],
            "{code:?}"
        );
    }
    for code in [
        ErrorCode::SecretInProcessArguments,
        ErrorCode::LocationTooManyCredentials,
        ErrorCode::RepositoryPathRelative,
    ] {
        assert_eq!(code.args(), &[named[0]], "{code:?}");
    }
    for code in [
        ErrorCode::KeyringUnavailable,
        ErrorCode::KeyringLocked,
        ErrorCode::KeyringUnwritable,
        ErrorCode::KeyringEntryTaken,
    ] {
        assert_eq!(
            code.args(),
            &[spec("entry", ArgKind::Text, false)],
            "{code:?}"
        );
    }
    assert_eq!(
        ErrorCode::SecretExpansionTooLong.args(),
        &[named[0], spec("limit", ArgKind::Bytes, false)]
    );
    assert_eq!(
        ErrorCode::SecretsTooLargeToMask.args(),
        &[
            spec("limit", ArgKind::Bytes, false),
            spec("field", ArgKind::Text, true)
        ]
    );
    assert_eq!(
        ErrorCode::TemplateInvalid.args(),
        &[named[0], spec("at", ArgKind::Text, true)]
    );
    assert_eq!(
        ErrorCode::CredentialsRefused.args(),
        &[spec("half", ArgKind::Text, true)]
    );
    for code in [
        ErrorCode::InvalidJobSpec,
        ErrorCode::OperationFailed,
        ErrorCode::DeadlinePassed,
        ErrorCode::EngineUnavailable,
        ErrorCode::EngineStartTimedOut,
        ErrorCode::EngineExitTimedOut,
        ErrorCode::RepositoryTimedOut,
        ErrorCode::RepositoryReplaced,
        ErrorCode::RepositoryConfigNotFound,
        ErrorCode::DaemonStopped,
        ErrorCode::DaemonShutDown,
        ErrorCode::HostNotAllowed,
        ErrorCode::OriginNotAllowed,
        ErrorCode::JsonRequired,
        ErrorCode::SignInRequired,
        ErrorCode::SignInFailed,
        ErrorCode::CsrfTokenInvalid,
        ErrorCode::SignInUnavailable,
        ErrorCode::ApiTokenInvalid,
        ErrorCode::SignInOff,
        ErrorCode::RestartUnavailable,
        ErrorCode::RestartPending,
        ErrorCode::ApiTokenNoScope,
        ErrorCode::ApiTokensUnavailable,
        ErrorCode::ProposalsUnavailable,
        ErrorCode::DryRunBusy,
        ErrorCode::DryRunGivesWay,
        ErrorCode::DryRunFilter,
        ErrorCode::DryRunFailed,
        ErrorCode::ProposalChanged,
        ErrorCode::ProposalNeedsForgetPreview,
        ErrorCode::ProposalForgetPreviewChanged,
        ErrorCode::CurrentPasswordWrong,
        ErrorCode::AdminPasswordUnavailable,
        ErrorCode::SessionsUnavailable,
        ErrorCode::DaemonUnreachable,
        ErrorCode::DaemonAnswerLost,
        ErrorCode::ConfigFolderUnknown,
        ErrorCode::RuntimeUnavailable,
        ErrorCode::Unknown,
    ] {
        assert!(code.args().is_empty(), "{code:?}");
    }
    for code in [WarningCode::SourceUnreadable, WarningCode::Unknown] {
        assert!(code.args().is_empty(), "{code:?}");
    }
}

#[test]
fn a_sign_in_wait_names_the_seconds_left() {
    assert_eq!(
        ErrorCode::SignInWait.args(),
        &[ArgSpec {
            name: "seconds",
            kind: ArgKind::Count,
            optional: false,
        }]
    );
    assert_eq!(
        serde_json::to_string(&ErrorCode::CsrfTokenInvalid).unwrap(),
        r#""csrf-token-invalid""#
    );
}

#[test]
fn a_session_answer_carries_its_csrf_token_and_limits_only_when_signed_in() {
    let signed_in = SessionDto {
        sign_in: SignInStateDto::SignedIn,
        csrf_token: Some("ab".to_owned()),
        idle_seconds: Some(43_200),
        ends_in_seconds: Some(60),
        exposure: ExposureDto::Loopback,
        admin_password: None,
        keep_signed_in_offered: None,
        build: None,
        available_update: None,
    };
    assert_eq!(
        serde_json::to_value(&signed_in).unwrap(),
        serde_json::json!({
            "sign-in": "signed-in",
            "csrf-token": "ab",
            "idle-seconds": 43_200,
            "ends-in-seconds": 60
        })
    );
    let off = SessionDto {
        sign_in: SignInStateDto::Off,
        csrf_token: None,
        idle_seconds: None,
        ends_in_seconds: None,
        exposure: ExposureDto::Loopback,
        admin_password: None,
        keep_signed_in_offered: None,
        build: None,
        available_update: None,
    };
    assert_eq!(
        serde_json::to_value(&off).unwrap(),
        serde_json::json!({"sign-in": "off"})
    );
    // Loopback is left out, as before the field existed; the network is
    // said, and an answer without it reads as loopback.
    let on_the_network = SessionDto {
        exposure: ExposureDto::Network,
        ..off
    };
    assert_eq!(
        serde_json::to_value(&on_the_network).unwrap(),
        serde_json::json!({"sign-in": "off", "exposure": "network"})
    );
    let read: SessionDto = serde_json::from_value(serde_json::json!({"sign-in": "off"})).unwrap();
    assert_eq!(read.exposure, ExposureDto::Loopback);
}

fn build(commit: Option<&str>, dirty: bool, release: bool) -> BuildDto {
    BuildDto {
        version: "0.3.0".to_owned(),
        commit: commit.map(str::to_owned),
        dirty,
        release,
    }
}

const COMMIT: &str = "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90";

#[test]
fn a_clean_build_of_its_release_tag_shows_the_version_alone() {
    assert_eq!(build(Some(COMMIT), false, true).display(), "0.3.0");
}

#[test]
fn a_build_between_releases_shows_its_short_commit() {
    assert_eq!(build(Some(COMMIT), false, false).display(), "0.3.0+3f937cf");
}

#[test]
fn a_build_with_changes_says_so_even_at_its_release_tag() {
    assert_eq!(
        build(Some(COMMIT), true, false).display(),
        "0.3.0+3f937cf.dirty"
    );
    assert_eq!(
        build(Some(COMMIT), true, true).display(),
        "0.3.0+3f937cf.dirty"
    );
}

#[test]
fn a_build_with_no_known_commit_shows_the_version_alone() {
    assert_eq!(build(None, false, false).display(), "0.3.0");
    assert_eq!(build(None, true, false).display(), "0.3.0");
}

#[test]
fn a_commit_shorter_than_a_short_hash_is_shown_whole() {
    assert_eq!(build(Some("3f93"), false, false).display(), "0.3.0+3f93");
}

#[test]
fn a_session_answer_names_the_build_in_fields_of_its_own() {
    let off = SessionDto {
        sign_in: SignInStateDto::Off,
        csrf_token: None,
        idle_seconds: None,
        ends_in_seconds: None,
        exposure: ExposureDto::Loopback,
        admin_password: None,
        keep_signed_in_offered: None,
        build: Some(build(Some(COMMIT), true, false)),
        available_update: None,
    };
    let written = serde_json::json!({
        "sign-in": "off",
        "version": "0.3.0",
        "commit": COMMIT,
        "dirty": true,
        "release": false
    });
    assert_eq!(serde_json::to_value(&off).unwrap(), written);
    assert_eq!(serde_json::from_value::<SessionDto>(written).unwrap(), off);
    // A build with no known commit says so with `null`.
    let unknown = SessionDto {
        build: Some(build(None, false, false)),
        ..off.clone()
    };
    let written = serde_json::json!({
        "sign-in": "off",
        "version": "0.3.0",
        "commit": null,
        "dirty": false,
        "release": false
    });
    assert_eq!(serde_json::to_value(&unknown).unwrap(), written);
    assert_eq!(
        serde_json::from_value::<SessionDto>(written).unwrap(),
        unknown
    );
    // An answer that doesn't name the build reads as none.
    let read: SessionDto =
        serde_json::from_value(serde_json::json!({"sign-in": "signed-out"})).unwrap();
    assert_eq!(read.build, None);
}

#[test]
fn a_stopped_daemons_code_is_spelled_daemon_stopped() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::DaemonStopped).unwrap(),
        r#""daemon-stopped""#
    );
}

#[test]
fn a_body_too_large_declares_the_limit_it_names() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::BodyTooLarge).unwrap(),
        r#""body-too-large""#
    );
    assert_eq!(
        ErrorCode::BodyTooLarge.args(),
        &[ArgSpec {
            name: "limit",
            kind: ArgKind::Bytes,
            optional: false,
        }]
    );
}

#[test]
fn config_codes_declare_the_arguments_their_producers_send() {
    let text = |name, optional| ArgSpec {
        name,
        kind: ArgKind::Text,
        optional,
    };
    assert_eq!(ErrorCode::ConfigUnavailable.args(), &[text("file", false)]);
    assert_eq!(
        ErrorCode::ConfigTooLarge.args(),
        &[
            text("file", false),
            ArgSpec {
                name: "limit",
                kind: ArgKind::Bytes,
                optional: false,
            },
        ]
    );
    assert_eq!(
        ErrorCode::ConfigInvalid.args(),
        &[text("file", false), text("line", true)]
    );
    assert_eq!(
        ErrorCode::ConfigSchemaMissing.args(),
        &[text("file", false)]
    );
    assert_eq!(
        ErrorCode::ConfigSchemaNewer.args(),
        &[
            text("file", false),
            text("found", false),
            text("supported", false),
        ]
    );
}

#[test]
fn profile_codes_declare_the_arguments_their_producers_send() {
    let text = |name, optional| ArgSpec {
        name,
        kind: ArgKind::Text,
        optional,
    };
    assert_eq!(ErrorCode::ProfileUnreadable.args(), &[text("file", false)]);
    assert_eq!(
        ErrorCode::ProfileExportReference.args(),
        &[text("key", false)]
    );
    assert_eq!(
        ErrorCode::ProfileExportUrlPassword.args(),
        &[text("key", false)]
    );
    assert_eq!(
        ErrorCode::ProfileExportTagComma.args(),
        &[text("tag", false)]
    );
    assert!(ErrorCode::InlineSecret.args().is_empty());
    assert_eq!(
        ErrorCode::ProfileInvalid.args(),
        &[text("file", false), text("key", true), text("line", true)]
    );
    assert_eq!(
        ErrorCode::ProfileUnsupported.args(),
        &[text("file", false), text("key", false), text("line", true)]
    );
    assert_eq!(ErrorCode::PlanIdTaken.args(), &[text("plan", false)]);
    assert_eq!(
        ErrorCode::PlanInvalid.args(),
        &[text("field", false), text("problem", false)]
    );
    assert_eq!(
        ErrorCode::InvalidArguments.args(),
        &[text("field", false), text("problem", false)]
    );
    assert_eq!(
        ErrorCode::PlanInUse.args(),
        &[text("plan", false), text("rule", false)]
    );
    assert_eq!(
        WarningCode::ProfileOptionLeftOut.args(),
        &[text("option", false), text("variable", false)]
    );
    assert_eq!(
        WarningCode::ConfigInlineSecret.args(),
        &[text("field", false), text("position", true)]
    );
    assert_eq!(
        WarningCode::LocationSecret.args(),
        &[
            text("field", false),
            text("kind", false),
            text("position", true)
        ]
    );
    // A repository of one location is told about itself, never about
    // "location 1" (#473).
    assert_eq!(
        WarningCode::LocationPlainHttpCredentials.args(),
        &[
            text("position", false),
            text("cause", false),
            text("rule", false),
            text("locations", false)
        ]
    );
    assert_eq!(
        WarningCode::LocationEveryRuled.args(),
        &[text("locations", false)]
    );
    assert_eq!(
        WarningCode::LocationLastRuledNoRetry.args(),
        &[text("plan", false), text("locations", false)]
    );
    assert_eq!(
        WarningCode::LocationRuleNameUnreadable.args(),
        &[text("position", false), text("locations", false)]
    );
    assert_eq!(
        WarningCode::LocationRuleMixedLink.args(),
        &[
            text("ipv4", false),
            text("ipv6", false),
            text("locations", false)
        ]
    );
    assert_eq!(
        WarningCode::GlobNeverMatches.args(),
        &[
            text("plan", false),
            text("glob", false),
            text("absolute", true)
        ]
    );
    assert_eq!(
        WarningCode::ProfileKeyringCommandMissing.args(),
        &[text("program", false)]
    );
    for code in [
        WarningCode::ProfilePrune,
        WarningCode::ProfileEnvIgnored,
        WarningCode::ProfileHooksPerPlan,
        WarningCode::ProfileRetentionEmpty,
    ] {
        assert!(code.args().is_empty(), "{code:?}");
    }
}

#[test]
fn the_codes_of_a_command_that_cannot_start_are_named_on_the_wire() {
    for (code, wire) in [
        (ErrorCode::ConfigFolderUnknown, r#""config-folder-unknown""#),
        (ErrorCode::RuntimeUnavailable, r#""runtime-unavailable""#),
    ] {
        assert_eq!(serde_json::to_string(&code).unwrap(), wire);
        assert_eq!(serde_json::from_str::<ErrorCode>(wire).unwrap(), code);
    }
}

#[test]
fn the_codes_of_a_restore_are_named_on_the_wire_and_only_a_running_one_names_its_operation() {
    for (code, wire) in [
        (ErrorCode::StepUpFailed, r#""step-up-failed""#),
        (
            ErrorCode::RestoreTargetUnwritable,
            r#""restore-target-unwritable""#,
        ),
        (ErrorCode::RestoreRunning, r#""restore-running""#),
        (
            ErrorCode::RestoreInPlaceNotHere,
            r#""restore-in-place-not-here""#,
        ),
        (
            ErrorCode::RestoreHubNotAllowed,
            r#""restore-hub-not-allowed""#,
        ),
        (ErrorCode::RestoreFolderUnset, r#""restore-folder-unset""#),
        (
            ErrorCode::FullDiskAccessNotLocal,
            r#""full-disk-access-not-local""#,
        ),
        (
            ErrorCode::FullDiskAccessSettingsUnopened,
            r#""full-disk-access-settings-unopened""#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&code).unwrap(), wire);
        assert_eq!(serde_json::from_str::<ErrorCode>(wire).unwrap(), code);
        let args: Vec<&str> = code.args().iter().map(|arg| arg.name).collect();
        let named: &[&str] = if code == ErrorCode::RestoreRunning {
            &["operation"]
        } else {
            &[]
        };
        assert_eq!(args, named, "{code:?}");
    }
}

#[test]
fn the_codes_of_a_download_are_named_on_the_wire_and_only_an_unknown_entry_names_its_path() {
    for (code, wire) in [
        (ErrorCode::DownloadsBusy, r#""downloads-busy""#),
        (
            ErrorCode::DownloadsUnavailable,
            r#""downloads-unavailable""#,
        ),
        (ErrorCode::EntryUnknown, r#""entry-unknown""#),
        (ErrorCode::DownloadUnknown, r#""download-unknown""#),
    ] {
        assert_eq!(serde_json::to_string(&code).unwrap(), wire);
        assert_eq!(serde_json::from_str::<ErrorCode>(wire).unwrap(), code);
        let args: Vec<&str> = code.args().iter().map(|arg| arg.name).collect();
        let named: &[&str] = if code == ErrorCode::EntryUnknown {
            &["path"]
        } else {
            &[]
        };
        assert_eq!(args, named, "{code:?}");
    }
}

#[test]
fn a_download_request_travels_in_kebab_case_and_hides_its_confirmation_and_ticket() {
    let request: DownloadRequestDto = serde_json::from_str(
        r#"{"path":"home/docs","format":"zip","confirmation":"canary-admin-password"}"#,
    )
    .unwrap();
    assert_eq!(request.format, ArchiveFormatDto::Zip);
    let shown = format!("{request:?}");
    assert!(!shown.contains("canary-admin-password"), "{shown}");
    assert!(shown.contains("home/docs"), "{shown}");
    assert!(
        serde_json::from_str::<DownloadRequestDto>(
            r#"{"path":"","format":"tar","confirmation":"x","extra":1}"#
        )
        .is_err()
    );
    let started = DownloadStartedDto {
        ticket: "canary-ticket".to_owned(),
    };
    assert_eq!(
        format!("{started:?}"),
        r#"DownloadStartedDto { ticket: "[hidden]" }"#
    );
    assert_eq!(
        serde_json::to_string(&started).unwrap(),
        r#"{"ticket":"canary-ticket"}"#
    );
}

#[test]
fn an_approval_shows_what_it_binds_to_and_hides_its_confirmation() {
    let approval = ApproveProposalDto {
        revision: "demo-revision".to_owned(),
        diff_hash: "demo-diff".to_owned(),
        forget_digest: Some("demo-forget".to_owned()),
        confirmation: "canary-admin-password".to_owned(),
    };
    assert_eq!(
        format!("{approval:?}"),
        r#"ApproveProposalDto { revision: "demo-revision", diff_hash: "demo-diff", forget_digest: Some("demo-forget"), confirmation: "[hidden]" }"#
    );
    // A page that showed no forget preview sends none.
    let unpreviewed: ApproveProposalDto = serde_json::from_str(
        r#"{"revision": "demo-revision", "diff-hash": "demo-diff", "confirmation": "x"}"#,
    )
    .unwrap();
    assert_eq!(unpreviewed.forget_digest, None);
    assert_eq!(
        serde_json::to_value(&approval).unwrap()["forget-digest"],
        "demo-forget"
    );
}

#[test]
fn a_restore_request_travels_in_kebab_case_and_its_debug_hides_the_confirmation() {
    let request: RestoreRequestDto = serde_json::from_str(
        r#"{"path":"home/docs","destination":"in-place","confirmation":"canary-admin-password"}"#,
    )
    .unwrap();
    assert_eq!(request.destination, RestoreDestinationDto::InPlace);
    let shown = format!("{request:?}");
    assert!(!shown.contains("canary-admin-password"), "{shown}");
    assert!(shown.contains("home/docs"), "{shown}");
    assert!(
        serde_json::from_str::<RestoreRequestDto>(
            r#"{"path":"","destination":"new-folder","confirmation":"x","extra":1}"#
        )
        .is_err()
    );
}

// The `plan-invalid` message and the Plans page select on these names,
// so each is its own, as the catalog's arms spell it.
#[test]
fn each_plan_problem_is_named_on_the_wire() {
    for (problem, wire) in [
        (PlanProblem::Missing, "missing"),
        (PlanProblem::UnknownKey, "unknown-key"),
        (PlanProblem::WrongType, "wrong-type"),
        (PlanProblem::Invalid, "invalid"),
        (PlanProblem::Taken, "taken"),
        (PlanProblem::UnknownRepository, "unknown-repository"),
        (PlanProblem::NoSources, "no-sources"),
    ] {
        assert_eq!(problem.as_str(), wire, "{problem:?}");
    }
}

// The `invalid-arguments` message selects on these names, and an agent
// reads them, so each is its own, as the catalog's arms spell it.
#[test]
fn each_argument_problem_and_its_code_are_named_on_the_wire() {
    for (problem, wire) in [
        (ArgumentProblem::Missing, "missing"),
        (ArgumentProblem::UnknownKey, "unknown-key"),
        (ArgumentProblem::Invalid, "invalid"),
    ] {
        assert_eq!(problem.as_str(), wire, "{problem:?}");
    }
    assert_eq!(
        serde_json::to_string(&ErrorCode::InvalidArguments).unwrap(),
        r#""invalid-arguments""#
    );
}

// The `proposal-invalid` message selects on these names, and an agent
// reads them: a plan's problem goes as the plan routes name it, the
// others each as its own, as the catalog's arms spell them.
#[test]
fn each_change_problem_is_named_on_the_wire() {
    for (problem, wire) in [
        (ChangeProblem::Plan(PlanProblem::Missing), "missing"),
        (ChangeProblem::Plan(PlanProblem::NoSources), "no-sources"),
        (ChangeProblem::UnknownPlan, "unknown-plan"),
        (ChangeProblem::InUse, "in-use"),
        (ChangeProblem::Hooks, "hooks"),
    ] {
        assert_eq!(problem.as_str(), wire, "{problem:?}");
    }
}

#[cfg(test)]
mod catalog_dto_tests {
    use serde_json::{from_str, to_string};

    use super::{
        MsgArg, PasswordSourceDto, PlanSummaryDto, PlansDto, RepositoriesDto, RepositoryDto,
        SnapshotDto, SnapshotsDto, WarningCode, WarningDto,
    };

    #[test]
    fn the_catalog_lists_carry_their_own_field_names() {
        let plans = PlansDto {
            plans: vec![
                PlanSummaryDto {
                    id: "demo-home".to_owned(),
                    repository: "demo".to_owned(),
                    scheduled: true,
                    next_run: Some("2026-11-13T03:00:00Z".to_owned()),
                    warnings: Vec::new(),
                },
                PlanSummaryDto {
                    id: "demo-docs".to_owned(),
                    repository: "demo".to_owned(),
                    scheduled: false,
                    next_run: None,
                    warnings: vec![WarningDto::new(
                        WarningCode::GlobNeverMatches,
                        [
                            ("plan", MsgArg::Text("demo-docs".to_owned())),
                            ("glob", MsgArg::Text("!a/b".to_owned())),
                            ("absolute", MsgArg::Text("!/home/user/a/b".to_owned())),
                        ],
                    )],
                },
            ],
        };
        assert_eq!(
            to_string(&plans).unwrap(),
            r#"{"plans":[{"id":"demo-home","repository":"demo","scheduled":true,"next-run":"2026-11-13T03:00:00Z"},{"id":"demo-docs","repository":"demo","scheduled":false,"next-run":null,"warnings":[{"code":"glob-never-matches","args":{"absolute":{"type":"text","value":"!/home/user/a/b"},"glob":{"type":"text","value":"!a/b"},"plan":{"type":"text","value":"demo-docs"}}}]}]}"#
        );
        assert_eq!(
            from_str::<PlansDto>(&to_string(&plans).unwrap()).unwrap(),
            plans
        );

        let repositories = RepositoriesDto {
            repositories: vec![RepositoryDto {
                alias: "demo".to_owned(),
                repository: "/srv/demo/repo".to_owned(),
                repo_hot: None,
                password: PasswordSourceDto::Inline,
                maintained_here: false,
                create_offered: false,
                warnings: Vec::new(),
                locations: Vec::new(),
            }],
        };
        assert_eq!(
            to_string(&repositories).unwrap(),
            r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":false}]}"#
        );
        assert_eq!(
            from_str::<RepositoriesDto>(&to_string(&repositories).unwrap()).unwrap(),
            repositories
        );
    }

    #[test]
    fn a_repository_row_names_where_its_password_comes_from_and_never_its_value() {
        for (source, wire) in [
            (PasswordSourceDto::Inline, r#"{"source":"inline"}"#),
            (
                PasswordSourceDto::File {
                    path: "/run/pw".to_owned(),
                },
                r#"{"source":"file","path":"/run/pw"}"#,
            ),
            (
                PasswordSourceDto::Keyring {
                    entry: "ladderback/nas".to_owned(),
                },
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
            ),
            (
                PasswordSourceDto::Env {
                    name: "PW".to_owned(),
                },
                r#"{"source":"env","name":"PW"}"#,
            ),
            (
                PasswordSourceDto::Command {
                    program: "pass".to_owned(),
                },
                r#"{"source":"command","program":"pass"}"#,
            ),
        ] {
            assert_eq!(to_string(&source).unwrap(), wire);
            assert_eq!(from_str::<PasswordSourceDto>(wire).unwrap(), source);
        }
        let row = RepositoryDto {
            alias: "nas".to_owned(),
            repository: "rest:https://u:***@nas.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordSourceDto::Inline,
            maintained_here: false,
            create_offered: false,
            warnings: Vec::new(),
            locations: Vec::new(),
        };
        assert_eq!(
            to_string(&row).unwrap(),
            r#"{"alias":"nas","repository":"rest:https://u:***@nas.example/repo","repo-hot":"/mnt/hot","password":{"source":"inline"},"maintained-here":false}"#
        );
    }

    #[test]
    fn a_repository_snapshot_list_round_trips() {
        let snapshot = SnapshotDto::new(
            "ab".repeat(32),
            "2026-11-09T10:15:00Z".to_owned(),
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["ladderback-plan:demo-home".to_owned()],
            vec!["/home/demo".to_owned()],
        )
        .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
        let list = SnapshotsDto {
            snapshots: vec![snapshot],
        };
        let wire = to_string(&list).unwrap();
        assert_eq!(from_str::<SnapshotsDto>(&wire).unwrap(), list);
    }

    #[test]
    fn a_stats_reading_carries_its_own_field_names_and_leaves_out_no_compression() {
        let reading = |compression_saved_percent| super::StatsReadingDto {
            taken_at: "2026-11-15T09:00:00Z".to_owned(),
            snapshots: 3,
            total_size: 3000,
            stored_size: 1200,
            compression_saved_percent,
        };
        for (stats, wire) in [
            (
                super::RepositoryStatsDto {
                    latest: Some(reading(Some(50))),
                },
                r#"{"latest":{"taken-at":"2026-11-15T09:00:00Z","snapshots":3,"total-size":3000,"stored-size":1200,"compression-saved-percent":50}}"#,
            ),
            (
                super::RepositoryStatsDto {
                    latest: Some(reading(None)),
                },
                r#"{"latest":{"taken-at":"2026-11-15T09:00:00Z","snapshots":3,"total-size":3000,"stored-size":1200}}"#,
            ),
            (super::RepositoryStatsDto { latest: None }, "{}"),
        ] {
            assert_eq!(to_string(&stats).unwrap(), wire);
            assert_eq!(from_str::<super::RepositoryStatsDto>(wire).unwrap(), stats);
        }
    }
}

#[cfg(test)]
mod repository_request_tests {
    use serde_json::{from_str, json, to_string};

    use super::{
        ConnectRepositoryDto, ErrorCode, InitRepositoryDto, PasswordDto, RepositorySettingsDto,
        TestRepositoryDto,
    };

    fn settings() -> RepositorySettingsDto {
        RepositorySettingsDto {
            repository: "rest:https://user:hunter2@backup.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordDto::Inline {
                value: "correct-horse".to_owned(),
            },
            options: [("api-key".to_owned(), "s3cret".to_owned())].into(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        }
    }

    #[test]
    fn settings_carry_the_configurations_own_field_names() {
        let wire = to_string(&settings()).unwrap();
        assert_eq!(
            from_str::<serde_json::Value>(&wire).unwrap(),
            json!({
                "repository": "rest:https://user:hunter2@backup.example/repo",
                "repo-hot": "/mnt/hot",
                "password": {"source": "inline", "value": "correct-horse"},
                "options": {"api-key": "s3cret"},
            })
        );
        assert_eq!(
            from_str::<RepositorySettingsDto>(&wire).unwrap(),
            settings()
        );
    }

    #[test]
    fn what_a_request_leaves_out_is_empty() {
        let read: RepositorySettingsDto =
            from_str(r#"{"repository":"/x","password":{"source":"env","name":"PW"}}"#).unwrap();
        assert_eq!(read.repo_hot, None);
        assert!(read.options.is_empty() && read.options_hot.is_empty());
        assert!(read.options_cold.is_empty());
        assert_eq!(read.password, PasswordDto::Env { name: "PW".into() });
    }

    #[test]
    fn every_password_source_the_api_takes_reads_back() {
        for (wire, expected) in [
            (
                r#"{"source":"inline","value":"p"}"#,
                PasswordDto::Inline { value: "p".into() },
            ),
            (
                r#"{"source":"file","path":"/run/pw"}"#,
                PasswordDto::File {
                    path: "/run/pw".into(),
                },
            ),
            (
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
                PasswordDto::Keyring {
                    entry: "ladderback/nas".into(),
                },
            ),
            (
                r#"{"source":"env","name":"PW"}"#,
                PasswordDto::Env { name: "PW".into() },
            ),
        ] {
            assert_eq!(from_str::<PasswordDto>(wire).unwrap(), expected, "{wire}");
            assert_eq!(
                from_str::<serde_json::Value>(&to_string(&expected).unwrap()).unwrap(),
                from_str::<serde_json::Value>(wire).unwrap()
            );
        }
    }

    #[test]
    fn a_body_the_route_does_not_take_is_not_read() {
        for wire in [
            // The command source runs a program: not over this API.
            r#"{"source":"command","command":"pass show x"}"#,
            r#"{"source":"inline"}"#,
            r#"{"source":"inline","value":"p","extra":1}"#,
            r#"{"value":"p"}"#,
            r#"{"source":"file","path":1}"#,
        ] {
            assert!(from_str::<PasswordDto>(wire).is_err(), "{wire}");
        }
        for wire in [
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"repo_hot":"/y"}"#,
            r#"{"repository":"/x"}"#,
            r#"{"password":{"source":"env","name":"PW"}}"#,
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"options":{"a":1}}"#,
        ] {
            assert!(from_str::<RepositorySettingsDto>(wire).is_err(), "{wire}");
        }
    }

    #[test]
    fn the_requests_name_their_parts() {
        let body = json!({"settings": settings()});
        assert_eq!(
            from_str::<TestRepositoryDto>(&body.to_string())
                .unwrap()
                .settings,
            settings()
        );
        assert!(from_str::<TestRepositoryDto>("{}").is_err());
        assert!(from_str::<TestRepositoryDto>(r#"{"settings":null}"#).is_err());

        let connect = json!({"alias": "demo", "settings": settings()});
        let connect: ConnectRepositoryDto = from_str(&connect.to_string()).unwrap();
        assert_eq!(connect.alias, "demo");
        assert!(!connect.maintain_here, "left out, nobody is asked");
        let maintained = json!({"alias": "demo", "settings": settings(), "maintain-here": true});
        let maintained: ConnectRepositoryDto = from_str(&maintained.to_string()).unwrap();
        assert!(maintained.maintain_here);
        assert!(
            from_str::<ConnectRepositoryDto>(&json!({"settings": settings()}).to_string()).is_err()
        );

        let init: InitRepositoryDto = from_str(&body.to_string()).unwrap();
        assert_eq!(init.alias, None);
        let init: InitRepositoryDto =
            from_str(&json!({"alias": "fresh", "settings": settings()}).to_string()).unwrap();
        assert_eq!(init.alias.as_deref(), Some("fresh"));
        // Unknown fields are refused at every level of a request.
        assert!(
            from_str::<InitRepositoryDto>(&json!({"settings": settings(), "x": 1}).to_string())
                .is_err()
        );
    }

    #[test]
    fn debug_shows_no_text_a_person_could_have_put_a_secret_in() {
        let shown = format!(
            "{:?} {:?}",
            settings(),
            ConnectRepositoryDto {
                alias: "demo".into(),
                settings: settings(),
                maintain_here: false,
                keep_in_keyring: false,
            }
        );
        for secret in [
            "hunter2",
            "correct-horse",
            "s3cret",
            "backup.example",
            "/mnt/hot",
        ] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert!(shown.contains("api-key"), "an option's name helps: {shown}");
    }

    #[test]
    fn debug_names_where_a_password_comes_from_but_never_a_typed_one() {
        let inline = format!(
            "{:?}",
            PasswordDto::Inline {
                value: "hunter2".into()
            }
        );
        assert!(!inline.contains("hunter2"), "{inline}");
        assert!(inline.contains("Inline"), "the source helps: {inline}");
        let file = format!(
            "{:?}",
            PasswordDto::File {
                path: "/etc/demo/pw".into()
            }
        );
        assert!(file.contains("/etc/demo/pw"), "a path is no secret: {file}");
    }

    #[test]
    fn debug_of_a_sign_in_names_it_but_never_the_password_typed() {
        let shown = format!(
            "{:?}",
            super::SignInDto {
                password: "demo-admin-pass".into(),
                keep_signed_in: true,
            }
        );
        assert!(!shown.contains("demo-admin-pass"), "{shown}");
        assert!(shown.contains("SignInDto"), "the type helps: {shown}");
    }

    #[test]
    fn debug_of_a_new_admin_password_names_it_but_never_either_password_typed() {
        let shown = format!(
            "{:?}",
            super::SetAdminPasswordDto {
                current_password: Some("demo-admin-pass".into()),
                new_password: "another-admin-pass".into(),
            }
        );
        assert!(!shown.contains("demo-admin-pass"), "{shown}");
        assert!(!shown.contains("another-admin-pass"), "{shown}");
        assert!(
            shown.contains("SetAdminPasswordDto"),
            "the type helps: {shown}"
        );
    }

    #[test]
    fn a_repository_that_did_not_answer_is_a_code_with_no_arguments() {
        assert_eq!(
            to_string(&ErrorCode::RepositoryTimedOut).unwrap(),
            r#""repository-timed-out""#
        );
        assert!(ErrorCode::RepositoryTimedOut.args().is_empty());
    }

    #[test]
    fn a_name_that_is_not_an_alias_is_a_code_with_no_arguments() {
        assert_eq!(
            to_string(&ErrorCode::RepositoryAliasInvalid).unwrap(),
            r#""repository-alias-invalid""#
        );
        assert!(ErrorCode::RepositoryAliasInvalid.args().is_empty());
    }
}

#[cfg(test)]
mod maintenance_dto_tests {
    use serde_json::{from_str, json, to_string, to_value};

    use super::{
        AdoptionFromDto, ErrorCode, ForgetPlanDto, LeftAloneDto, MAX_GROUP_SNAPSHOTS,
        MAX_PREVIEW_SNAPSHOTS, MaintenancePreviewDto, MaintenanceRequestDto, OrphanedGroupDto,
        PlanGroupDto, SnapshotDto, SnapshotGroupsDto, TallyDto, UnmanagedGroupDto, WarningDto,
    };

    fn snapshot() -> SnapshotDto {
        SnapshotDto {
            id: "ab".repeat(32),
            time: "2026-11-09T10:15:00Z".to_owned(),
            offset: None,
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["ladderback-plan:demo-home".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
            tags_truncated: false,
        }
    }

    fn previews() -> [MaintenancePreviewDto; 4] {
        [
            MaintenancePreviewDto::Forget {
                plans: vec![ForgetPlanDto {
                    plan: "demo-home".to_owned(),
                    total: 3,
                    remove: 2,
                    listed: vec![snapshot()],
                }],
                left_alone: LeftAloneDto {
                    orphaned: TallyDto {
                        groups: 1,
                        snapshots: 4,
                    },
                    unmanaged: TallyDto::default(),
                },
                selection: "68497038cf038672".to_owned(),
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Adopt {
                into: "demo-home".to_owned(),
                count: 1,
                listed: vec![snapshot()],
                selection: "68497038cf038672".to_owned(),
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Prune {
                packs_marked: 2,
                packs_deleted: 1,
                packs_recovered: 0,
                bytes_deleted: 4096,
                keep_delete_hours: 144,
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Check { read_data: false },
        ]
    }

    #[test]
    fn each_preview_names_its_kind_on_the_wire_and_reads_back() {
        for (preview, kind) in previews()
            .into_iter()
            .zip(["forget", "adopt", "prune", "check"])
        {
            let wire = to_value(&preview).unwrap();
            assert_eq!(wire["kind"], kind);
            assert_eq!(
                from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
                preview
            );
        }
    }

    #[test]
    fn a_preview_carries_kebab_case_field_names() {
        let [forget, adopt, prune, check] = previews();
        let forget = to_value(&forget).unwrap();
        assert_eq!(
            forget["left-alone"],
            json!({
                "orphaned": {"groups": 1, "snapshots": 4},
                "unmanaged": {"groups": 0, "snapshots": 0},
            })
        );
        assert_eq!(forget["plans"][0]["remove"], 2);
        assert_eq!(forget["selection"], "68497038cf038672");
        let adopt = to_value(&adopt).unwrap();
        assert_eq!(adopt["into"], "demo-home");
        assert_eq!(adopt["count"], 1);
        assert_eq!(
            to_value(&prune).unwrap(),
            json!({
                "kind": "prune",
                "packs-marked": 2,
                "packs-deleted": 1,
                "packs-recovered": 0,
                "bytes-deleted": 4096,
                "keep-delete-hours": 144,
            })
        );
        assert_eq!(
            to_value(&check).unwrap(),
            json!({"kind": "check", "read-data": false})
        );
    }

    #[test]
    fn a_preview_warns_of_what_it_is_given_and_a_check_of_nothing() {
        let warned = vec![WarningDto::other_hosts(1, &["demo-desktop".to_owned()])];
        let [forget, adopt, prune, check] = previews();
        for preview in [forget, adopt, prune] {
            assert!(preview.warnings().is_empty(), "{preview:?}");
            let preview = preview.warned(warned.clone());
            assert_eq!(preview.warnings(), warned.as_slice());
            let wire = to_value(&preview).unwrap();
            assert_eq!(
                wire["warnings"],
                json!([{
                    "code": "maintenance-other-hosts",
                    "args": {
                        "count": {"type": "count", "value": 1},
                        "hosts": {"type": "text", "value": "demo-desktop"},
                    },
                }])
            );
            assert_eq!(
                from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
                preview
            );
        }
        let check = check.warned(warned);
        assert!(check.warnings().is_empty(), "{check:?}");
        assert_eq!(
            to_value(&check).unwrap(),
            json!({"kind": "check", "read-data": false})
        );
    }

    #[test]
    fn a_request_names_only_what_it_holds() {
        let bare = MaintenanceRequestDto {
            kind: "forget".to_owned(),
            into: None,
            from: None,
            selection: None,
        };
        assert_eq!(to_string(&bare).unwrap(), r#"{"kind":"forget"}"#);
        assert_eq!(
            from_str::<MaintenanceRequestDto>(r#"{"kind":"forget"}"#).unwrap(),
            bare
        );
    }

    #[test]
    fn an_adoption_request_names_its_destination_source_and_selection() {
        for (from, wire) in [
            (
                AdoptionFromDto::Orphaned {
                    agent: "agent-1".to_owned(),
                    plan: "old-home".to_owned(),
                },
                json!({"from": "orphaned", "agent": "agent-1", "plan": "old-home"}),
            ),
            (AdoptionFromDto::Unmanaged, json!({"from": "unmanaged"})),
        ] {
            let request = MaintenanceRequestDto {
                kind: "adopt".to_owned(),
                into: Some("demo-home".to_owned()),
                from: Some(from),
                selection: Some("68497038cf038672".to_owned()),
            };
            assert_eq!(
                to_value(&request).unwrap(),
                json!({
                    "kind": "adopt",
                    "into": "demo-home",
                    "from": wire,
                    "selection": "68497038cf038672",
                })
            );
            assert_eq!(
                from_str::<MaintenanceRequestDto>(&to_string(&request).unwrap()).unwrap(),
                request
            );
        }
    }

    #[test]
    fn a_source_that_is_no_kind_of_group_is_not_read() {
        assert!(from_str::<AdoptionFromDto>(r#"{"from":"somewhere"}"#).is_err());
        assert!(from_str::<AdoptionFromDto>(r#"{"agent":"a","plan":"p"}"#).is_err());
    }

    #[test]
    fn a_source_with_a_key_its_kind_does_not_know_is_not_read() {
        for body in [
            r#"{"kind":"adopt","into":"demo-home","from":{"from":"unmanaged","agent":"a"}}"#,
            r#"{"kind":"adopt","into":"demo-home","from":{"from":"orphaned","agent":"a","plan":"p","extra":1}}"#,
        ] {
            assert!(from_str::<MaintenanceRequestDto>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn the_groups_carry_their_own_field_names_and_read_back() {
        let groups = SnapshotGroupsDto {
            plans: vec![PlanGroupDto {
                plan: "demo-home".to_owned(),
                count: 1,
                snapshots: vec![snapshot()],
            }],
            orphaned: vec![OrphanedGroupDto {
                agent: Some("agent-1".to_owned()),
                plan: None,
                count: 1,
                snapshots: vec![snapshot()],
                adopt_into: vec!["demo-home".to_owned()],
            }],
            unmanaged: vec![UnmanagedGroupDto {
                hostname: "demo-host".to_owned(),
                label: String::new(),
                paths: vec!["/home/demo".to_owned()],
                count: 1,
                snapshots: vec![snapshot()],
                adopt_into: vec![],
            }],
            warnings: Vec::new(),
        };
        let wire = to_value(&groups).unwrap();
        assert_eq!(wire["orphaned"][0]["adopt-into"], json!(["demo-home"]));
        assert!(wire["orphaned"][0]["plan"].is_null());
        assert_eq!(wire["unmanaged"][0]["adopt-into"], json!([]));
        assert_eq!(wire["plans"][0]["count"], 1);
        assert_eq!(
            from_str::<SnapshotGroupsDto>(&wire.to_string()).unwrap(),
            groups
        );
    }

    #[test]
    fn the_codes_of_a_changed_selection_and_a_slow_read_take_no_arguments() {
        for (code, wire) in [
            (
                ErrorCode::MaintenanceSelectionChanged,
                r#""maintenance-selection-changed""#,
            ),
            (
                ErrorCode::RepositoryReadTimedOut,
                r#""repository-read-timed-out""#,
            ),
        ] {
            assert_eq!(to_string(&code).unwrap(), wire);
            assert_eq!(from_str::<ErrorCode>(wire).unwrap(), code);
            assert!(code.args().is_empty());
        }
    }

    /// Snapshot `n` as the wire would show it: its ID is `n`.
    fn numbered(n: &u32) -> Option<SnapshotDto> {
        Some(SnapshotDto {
            id: format!("{n:064x}"),
            ..snapshot()
        })
    }

    fn ids(list: &[SnapshotDto]) -> Vec<String> {
        list.iter().map(|snapshot| snapshot.id.clone()).collect()
    }

    fn first(count: u32) -> Vec<String> {
        (0..count).map(|n| format!("{n:064x}")).collect()
    }

    /// More snapshots than any list shows.
    fn many() -> Vec<u32> {
        (0..=u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap()).collect()
    }

    #[test]
    fn a_group_lists_its_first_snapshots_and_counts_them_all() {
        let items = many();
        let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();

        let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
        assert_eq!((plan.plan.as_str(), plan.count), ("demo-home", 501));
        assert_eq!(ids(&plan.snapshots), first(cap));

        let orphaned = OrphanedGroupDto::new(
            Some("agent-1".to_owned()),
            None,
            &items,
            vec!["demo-home".to_owned()],
            numbered,
        );
        assert_eq!(orphaned.agent.as_deref(), Some("agent-1"));
        assert_eq!(orphaned.plan, None);
        assert_eq!(orphaned.adopt_into, ["demo-home"]);
        assert_eq!(orphaned.count, 501);
        assert_eq!(ids(&orphaned.snapshots), first(cap));

        let unmanaged = UnmanagedGroupDto::new(
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["/home/demo".to_owned()],
            &items,
            Vec::new(),
            numbered,
        );
        assert_eq!(unmanaged.hostname, "demo-host");
        assert_eq!(unmanaged.label, "home");
        assert_eq!(unmanaged.paths, ["/home/demo"]);
        assert!(unmanaged.adopt_into.is_empty());
        assert_eq!(unmanaged.count, 501);
        assert_eq!(ids(&unmanaged.snapshots), first(cap));
    }

    #[test]
    fn a_group_that_fits_lists_every_snapshot() {
        let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();
        for held in [0, 3, cap] {
            let items: Vec<u32> = (0..held).collect();
            let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
            assert_eq!(plan.count, u64::from(held));
            assert_eq!(ids(&plan.snapshots), first(held));
        }
    }

    #[test]
    fn a_preview_lists_fewer_snapshots_than_a_group_does() {
        let items = many();
        let cap = u32::try_from(MAX_PREVIEW_SNAPSHOTS).unwrap();

        let forget = ForgetPlanDto::new("demo-home".to_owned(), 900, &items, numbered);
        assert_eq!(forget.plan, "demo-home");
        assert_eq!((forget.total, forget.remove), (900, 501));
        assert_eq!(ids(&forget.listed), first(cap));

        let adopt = MaintenancePreviewDto::adopt(
            "demo-home".to_owned(),
            &items,
            "68497038cf038672".to_owned(),
            numbered,
        );
        let MaintenancePreviewDto::Adopt {
            into,
            count,
            listed,
            selection,
            ..
        } = adopt
        else {
            panic!("an adoption preview");
        };
        assert_eq!(
            (into.as_str(), count, selection.as_str()),
            ("demo-home", 501, "68497038cf038672")
        );
        assert_eq!(ids(&listed), first(cap));

        let few: Vec<u32> = (0..3).collect();
        let forget = ForgetPlanDto::new("demo-home".to_owned(), 3, &few, numbered);
        assert_eq!((forget.remove, ids(&forget.listed)), (3, first(3)));
    }

    #[test]
    fn only_the_snapshots_a_list_shows_are_built() {
        let items = many();
        let mut built = 0;
        let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
            built += 1;
            numbered(n)
        });
        assert_eq!(built, MAX_GROUP_SNAPSHOTS);
        assert_eq!(group.count, 501);
    }

    #[test]
    fn a_snapshot_the_wire_cannot_carry_is_left_out_and_still_counted() {
        let items: Vec<u32> = (0..3).collect();
        let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
            (*n != 1).then(|| numbered(n)).flatten()
        });
        assert_eq!(group.count, 3);
        assert_eq!(
            ids(&group.snapshots),
            [first(1), vec![format!("{:064x}", 2)]].concat()
        );
    }

    #[test]
    fn a_request_with_a_key_it_does_not_know_is_not_read() {
        assert!(
            from_str::<MaintenanceRequestDto>(
                r#"{"kind":"forget","selectoin":"68497038cf038672"}"#
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod api_token_dto_tests {
    use serde_json::{from_str, json, to_value};

    use super::{
        ApiTokenDto, ApiTokenScopeDto, ApiTokensDto, CreateApiTokenDto, CreatedApiTokenDto,
    };

    fn listed() -> ApiTokenDto {
        ApiTokenDto {
            id: "abcdefghijkl".to_owned(),
            label: "nightly-mcp".to_owned(),
            scopes: vec![ApiTokenScopeDto::Read, ApiTokenScopeDto::Run],
            created_at: "2026-11-14T10:00:00Z".to_owned(),
            last_used_at: None,
        }
    }

    #[test]
    fn a_listed_token_has_kebab_case_fields_and_a_null_last_use_when_never_used() {
        let wire = to_value(ApiTokensDto {
            tokens: vec![listed()],
        })
        .unwrap();
        assert_eq!(
            wire,
            json!({"tokens": [{
                "id": "abcdefghijkl",
                "label": "nightly-mcp",
                "scopes": ["read", "run"],
                "created-at": "2026-11-14T10:00:00Z",
                "last-used-at": null,
            }]})
        );
        assert_eq!(
            serde_json::from_value::<ApiTokensDto>(wire).unwrap().tokens,
            vec![listed()]
        );
    }

    #[test]
    fn every_scope_reads_by_its_name_and_no_other_name_is_a_scope() {
        let body: CreateApiTokenDto =
            from_str(r#"{"label":"every-scope","scopes":["read","run","preview","propose"]}"#)
                .unwrap();
        assert_eq!(
            body.scopes,
            [
                ApiTokenScopeDto::Read,
                ApiTokenScopeDto::Run,
                ApiTokenScopeDto::Preview,
                ApiTokenScopeDto::Propose
            ]
        );
        assert!(from_str::<CreateApiTokenDto>(r#"{"label":"x","scopes":["admin"]}"#).is_err());
        assert!(from_str::<CreateApiTokenDto>(r#"{"label":"x","scopes":["Read"]}"#).is_err());
    }

    #[test]
    fn a_create_body_with_a_field_of_its_own_is_refused() {
        assert!(from_str::<CreateApiTokenDto>(r#"{"label":"x","scopes":["read"]}"#).is_ok());
        assert!(
            from_str::<CreateApiTokenDto>(r#"{"label":"x","scopes":["read"],"expires":1}"#)
                .is_err()
        );
    }

    #[test]
    fn a_created_token_shows_its_secret_on_the_wire_but_never_in_debug() {
        let created = CreatedApiTokenDto {
            token: listed(),
            secret: "ladderback_abcdefghijkl_canary-secret".to_owned(),
        };
        assert_eq!(
            to_value(&created).unwrap()["secret"],
            "ladderback_abcdefghijkl_canary-secret"
        );
        let shown = format!("{created:?}");
        assert!(!shown.contains("canary-secret"), "{shown}");
        assert!(shown.contains("nightly-mcp"), "{shown}");
    }
}
