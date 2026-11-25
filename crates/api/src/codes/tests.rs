use serde_json::{from_str, json, to_string, to_value};

use crate::{ErrorCode, ErrorDto, FailureDto, MsgArg, ProfileImportDto, WarningCode, WarningDto};

fn text(value: &str) -> MsgArg {
    MsgArg::Text(value.to_owned())
}

#[test]
fn an_error_carries_its_code_its_arguments_by_name_and_its_detail() {
    let error = ErrorDto::new(
        ErrorCode::ProfileInvalid,
        [("file", text("nas.toml")), ("line", text("7"))],
        Some("unknown field".to_owned()),
    );
    assert_eq!(
        to_value(&error).unwrap(),
        json!({
            "code": "profile-invalid",
            "args": {
                "file": {"type": "text", "value": "nas.toml"},
                "line": {"type": "text", "value": "7"},
            },
            "detail": "unknown field",
        })
    );
}

#[test]
fn an_error_with_no_arguments_and_no_detail_is_its_code_alone() {
    let error = ErrorDto::new(ErrorCode::InlineSecret, [], None);
    assert_eq!(to_string(&error).unwrap(), r#"{"code":"inline-secret"}"#);
}

#[test]
fn a_warning_carries_its_code_and_its_arguments_by_name() {
    let warning = WarningDto::new(
        WarningCode::ProfileOptionLeftOut,
        [
            ("option", text("password")),
            ("variable", text("RUSTIC_PASSWORD")),
        ],
    );
    assert_eq!(
        to_value(&warning).unwrap(),
        json!({
            "code": "profile-option-left-out",
            "args": {
                "option": {"type": "text", "value": "password"},
                "variable": {"type": "text", "value": "RUSTIC_PASSWORD"},
            },
        })
    );
}

#[test]
fn a_warning_with_no_arguments_is_its_code_alone() {
    let warning = WarningDto::new(WarningCode::ProfilePrune, []);
    assert_eq!(to_string(&warning).unwrap(), r#"{"code":"profile-prune"}"#);
}

#[test]
fn an_import_is_its_repository_its_plans_and_its_notices() {
    let import = ProfileImportDto {
        repository: "nas".to_owned(),
        plans: vec!["docs".to_owned(), "home".to_owned()],
        notices: vec![WarningDto::new(
            WarningCode::ProfileKeyIgnored,
            [("key", text("copy"))],
        )],
    };
    assert_eq!(
        to_value(&import).unwrap(),
        json!({
            "repository": "nas",
            "plans": ["docs", "home"],
            "notices": [
                {
                    "code": "profile-key-ignored",
                    "args": {"key": {"type": "text", "value": "copy"}},
                },
            ],
        })
    );
}

#[test]
fn an_import_that_added_no_plan_and_noted_nothing_still_says_so() {
    let import = ProfileImportDto {
        repository: "nas".to_owned(),
        plans: Vec::new(),
        notices: Vec::new(),
    };
    assert_eq!(
        to_string(&import).unwrap(),
        r#"{"repository":"nas","plans":[],"notices":[]}"#
    );
}

#[test]
fn a_failure_is_the_typed_error_under_the_one_key_error() {
    let failure = FailureDto {
        error: ErrorDto::new(ErrorCode::PlanIdTaken, [("plan", text("home"))], None),
    };
    assert_eq!(
        to_string(&failure).unwrap(),
        r#"{"error":{"code":"plan-id-taken","args":{"plan":{"type":"text","value":"home"}}}}"#
    );
}

#[test]
fn every_document_reads_back_as_it_was_written() {
    let error = ErrorDto::new(
        ErrorCode::ProfileTooLarge,
        [
            ("file", text("nas.toml")),
            ("limit", MsgArg::Bytes(1_048_576)),
        ],
        Some("a detail".to_owned()),
    );
    let warning = WarningDto::new(
        WarningCode::ProfileTagDropped,
        [("tag", text("ladderback-plan:home"))],
    );
    let import = ProfileImportDto {
        repository: "nas".to_owned(),
        plans: vec!["home".to_owned()],
        notices: vec![warning.clone()],
    };
    let failure = FailureDto {
        error: error.clone(),
    };
    assert_eq!(
        from_str::<ErrorDto>(&to_string(&error).unwrap()).unwrap(),
        error
    );
    assert_eq!(
        from_str::<WarningDto>(&to_string(&warning).unwrap()).unwrap(),
        warning
    );
    assert_eq!(
        from_str::<ProfileImportDto>(&to_string(&import).unwrap()).unwrap(),
        import
    );
    assert_eq!(
        from_str::<FailureDto>(&to_string(&failure).unwrap()).unwrap(),
        failure
    );
}

#[test]
fn a_document_that_leaves_out_the_arguments_and_the_detail_reads_as_having_none() {
    assert_eq!(
        from_str::<ErrorDto>(r#"{"code":"inline-secret"}"#).unwrap(),
        ErrorDto::new(ErrorCode::InlineSecret, [], None)
    );
    assert_eq!(
        from_str::<WarningDto>(r#"{"code":"profile-prune"}"#).unwrap(),
        WarningDto::new(WarningCode::ProfilePrune, [])
    );
}

/// An event tells a failure's arguments by position: those the code
/// declares, in its order, then the engine's own words. Read back, each
/// has its name again and the words are the detail.
#[test]
fn a_failure_told_by_position_reads_back_by_name_with_its_detail() {
    let cases = [
        ErrorDto::new(
            ErrorCode::OperationFailed,
            [],
            Some("no key opens the repository".to_owned()),
        ),
        ErrorDto::new(ErrorCode::EngineExited, [("exit_code", text("3"))], None),
        ErrorDto::new(ErrorCode::EngineExited, [], None),
        ErrorDto::new(
            ErrorCode::SecretsTooLargeToMask,
            [
                ("limit", MsgArg::Bytes(262_144)),
                ("field", text("HTTPS_PROXY")),
            ],
            None,
        ),
        ErrorDto::new(
            ErrorCode::SecretsTooLargeToMask,
            [("limit", MsgArg::Bytes(262_144))],
            None,
        ),
    ];
    for error in cases {
        let told = error.positional();
        assert_eq!(
            ErrorDto::from_positional(error.code, &told),
            error,
            "{told:?}"
        );
    }
}

#[test]
fn a_failure_is_told_in_the_order_its_code_declares_then_the_engines_words() {
    let error = ErrorDto::new(
        ErrorCode::SecretsTooLargeToMask,
        [
            ("field", text("HTTPS_PROXY")),
            ("limit", MsgArg::Bytes(262_144)),
        ],
        None,
    );
    assert_eq!(
        error.positional(),
        [MsgArg::Bytes(262_144), text("HTTPS_PROXY")],
        "the declared order, not the names'"
    );
    let failed = ErrorDto::new(ErrorCode::OperationFailed, [], Some("said".to_owned()));
    assert_eq!(failed.positional(), [text("said")]);
    // An argument the code doesn't declare is not told, and one it
    // declares that the error lacks ends the arguments told.
    let odd = ErrorDto::new(
        ErrorCode::SecretsTooLargeToMask,
        [("field", text("HTTPS_PROXY")), ("other", text("x"))],
        Some("said".to_owned()),
    );
    assert_eq!(odd.positional(), [text("said")]);
}

#[test]
fn a_failure_read_by_position_takes_the_first_text_after_the_declared_ones_as_its_detail() {
    assert_eq!(
        ErrorDto::from_positional(
            ErrorCode::EngineExited,
            &[text("3"), MsgArg::Count(7), text("said"), text("more")],
        ),
        ErrorDto::new(
            ErrorCode::EngineExited,
            [("exit_code", text("3"))],
            Some("said".to_owned())
        )
    );
    assert_eq!(
        ErrorDto::from_positional(ErrorCode::OperationFailed, &[]),
        ErrorDto::new(ErrorCode::OperationFailed, [], None)
    );
}

/// The codes that name one of a hub's machines say `machine` on the wire
/// (§16, decision 163), and their old `agent-` names read as no code.
#[test]
fn the_codes_that_name_a_machine_say_machine_on_the_wire() {
    for code in [
        "machine-key-mismatch",
        "machine-removed",
        "machine-key-removed",
        "machine-unknown",
        "machine-approved-already",
    ] {
        let read: ErrorCode = from_str(&format!("\"{code}\"")).unwrap();
        assert_eq!(to_value(read).unwrap(), json!(code));
    }
    for code in [
        "agent-key-mismatch",
        "agent-removed",
        "agent-key-removed",
        "agent-unknown",
        "agent-approved-already",
    ] {
        let read: ErrorCode = from_str(&format!("\"{code}\"")).unwrap();
        assert_eq!(read, ErrorCode::Unknown, "{code}");
    }
}
