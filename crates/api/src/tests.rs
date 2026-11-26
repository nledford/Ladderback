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
        maintenance: RepositoryMaintenanceDto::Local,
        maintained_here: true,
        maintenance_paused_since: None,
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
        r#"{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintenance":"local","maintained-here":true,"create-offered":true,"warnings":[{"code":"config-inline-secret","args":{"field":{"type":"text","value":"password"}}}]}"#
    );
    assert_eq!(serde_json::from_str::<RepositoryDto>(&json).unwrap(), row);
    let bare = r#"{"alias":"demo","repository":"/x","password":{"source":"inline"}}"#;
    let bare = serde_json::from_str::<RepositoryDto>(bare).unwrap();
    assert!(bare.warnings.is_empty());
    assert_eq!(bare.maintenance, RepositoryMaintenanceDto::None);
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
        ErrorCode::Superseded,
        ErrorCode::PlanRemoved,
        ErrorCode::PlanMoved,
        ErrorCode::ConfigUnusable,
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
        ErrorCode::ApiTokenNoPermission,
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
    assert_eq!(
        WarningCode::RcloneCommandSecret.args(),
        &[
            text("field", false),
            text("flag", false),
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
        RepositoryMaintenanceDto, SnapshotDto, SnapshotsDto, WarningCode, WarningDto,
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
                maintenance: RepositoryMaintenanceDto::None,
                maintained_here: false,
                maintenance_paused_since: None,
                create_offered: false,
                warnings: Vec::new(),
                locations: Vec::new(),
            }],
        };
        assert_eq!(
            to_string(&repositories).unwrap(),
            r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintenance":"none","maintained-here":false}]}"#
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
            maintenance: RepositoryMaintenanceDto::Hub,
            maintained_here: false,
            maintenance_paused_since: None,
            create_offered: false,
            warnings: Vec::new(),
            locations: Vec::new(),
        };
        assert_eq!(
            to_string(&row).unwrap(),
            r#"{"alias":"nas","repository":"rest:https://u:***@nas.example/repo","repo-hot":"/mnt/hot","password":{"source":"inline"},"maintenance":"hub","maintained-here":false}"#
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

/// The page rejects or removes a machine at a path of its own under the
/// hub's machines, beside its approval, as the hub routes it.
#[test]
fn a_machines_removal_has_a_path_of_its_own() {
    let id = "0194f3a2-6b1c-7d2e-8f30-415263748596";
    assert_eq!(
        machine_removal_path(id),
        "/api/v1/hub/machines/0194f3a2-6b1c-7d2e-8f30-415263748596/removal"
    );
    assert_ne!(machine_removal_path(id), machine_approval_path(id));
}

/// The hub's page reads and changes its machines under `machines`, and its
/// dashboard at a path of its own; every field that names a machine is
/// `machine-id` (§16, decision 163).
#[test]
fn the_hubs_machines_go_by_machine_on_the_wire() {
    assert_eq!(HUB_MACHINES_PATH, "/api/v1/hub/machines");
    assert_eq!(
        machine_approval_path("m"),
        "/api/v1/hub/machines/m/approval"
    );
    assert_eq!(HUB_DASHBOARD_PATH, "/api/v1/hub/dashboard");
    let listed = serde_json::json!({"machines": [{
        "machine-id": "m", "hostname": "h", "key": "k", "fingerprint": "f",
        "status": "pending", "since": "s", "seconds-ago": 1,
        "looks-like": {"machine-id": "o", "hostname": "h2", "last-seen": "t"},
    }]});
    let read: HubAgentsDto = serde_json::from_value(listed.clone()).unwrap();
    assert_eq!(serde_json::to_value(read).unwrap(), listed);
    let dashboard = serde_json::json!({"machines": [{
        "machine-id": "m", "hostname": "h", "last-seen": "t", "online": true,
        "reports-dropped": 0, "operations": [],
    }]});
    let read: HubMachinesDto = serde_json::from_value(dashboard.clone()).unwrap();
    assert_eq!(serde_json::to_value(read).unwrap(), dashboard);
}

/// The hub's pairing and step-up DTOs name themselves and what a person may
/// read in `Debug`, and hide the token and the confirmation (I4).
#[test]
fn the_hubs_dtos_show_what_they_bind_to_and_hide_their_secrets_in_debug() {
    let make = MakePairingTokenDto {
        address: "https://hub.lan".to_owned(),
        certificate: TokenCertificateDto::Pinned,
        life_minutes: Some(5),
        confirmation: "canary-admin-password".to_owned(),
    };
    assert_eq!(
        format!("{make:?}"),
        r#"MakePairingTokenDto { address: "https://hub.lan", certificate: Pinned, life_minutes: Some(5), .. }"#
    );
    let made = PairingTokenDto {
        token: "canary-token".to_owned(),
        token_id: "t1".to_owned(),
        fingerprint: "f1".to_owned(),
        address: "https://hub.lan".to_owned(),
        expires_at: "2026-11-25T10:00:00Z".to_owned(),
        seconds_left: 60,
        certificate: None,
    };
    assert_eq!(
        format!("{made:?}"),
        r#"PairingTokenDto { token_id: "t1", fingerprint: "f1", address: "https://hub.lan", certificate: None, expires_at: "2026-11-25T10:00:00Z", seconds_left: 60, .. }"#
    );
    let approve = ApproveAgentDto {
        key: "k1".to_owned(),
        confirmation: "canary-admin-password".to_owned(),
    };
    assert_eq!(
        format!("{approve:?}"),
        r#"ApproveAgentDto { key: "k1", .. }"#
    );
    let end = EndRecoveryDto {
        confirmation: "canary-admin-password".to_owned(),
    };
    assert_eq!(format!("{end:?}"), "EndRecoveryDto { .. }");
}
