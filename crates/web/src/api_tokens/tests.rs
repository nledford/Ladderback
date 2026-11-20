use api::{ApiTokenDto, ErrorCode, MsgArg, PermissionLevelDto, PermissionsDto};
use l10n::{MessageId, msg};

use super::*;

const NOW: &str = "2026-11-14T12:00:00Z";

fn now_ms() -> i64 {
    NOW.parse::<jiff::Timestamp>().unwrap().as_millisecond()
}

/// The last use `ms` milliseconds before [`NOW`], as the daemon writes it.
fn used_ago(ms: i64) -> LastUsed {
    let then = jiff::Timestamp::from_millisecond(now_ms() - ms).unwrap();
    LastUsed::of(Some(&then.to_string()), now_ms())
}

fn ago(unit: &'static str, count: u64) -> LastUsed {
    LastUsed::Ago { unit, count }
}

fn form(label: &str, scopes: &[Scope]) -> TokenForm {
    TokenForm {
        label: label.to_owned(),
        scopes: scopes.to_vec(),
    }
}

/// What `problems_of` asks the catalog for, as text a test can compare.
fn said(id: MessageId, args: &[(&'static str, MsgArg)]) -> String {
    format!("{id:?} {args:?}")
}

#[test]
fn a_form_without_a_label_or_a_scope_lacks_both_in_the_page_s_order() {
    assert_eq!(form("", &[]).missing(), [Field::Label, Field::Scopes]);
}

#[test]
fn a_label_of_only_spaces_is_none() {
    assert_eq!(form("   ", &[Scope::Read]).missing(), [Field::Label]);
}

#[test]
fn a_token_needs_a_scope() {
    assert_eq!(form("nightly-mcp", &[]).missing(), [Field::Scopes]);
}

#[test]
fn a_label_and_a_scope_lack_nothing() {
    assert!(form("nightly-mcp", &[Scope::Run]).missing().is_empty());
}

#[test]
fn the_request_has_the_label_without_its_spaces_and_the_permissions_of_each_scope_chosen() {
    let request = form(
        "  nightly mcp  ",
        &[Scope::Propose, Scope::Read, Scope::Propose],
    )
    .request();
    assert_eq!(request.label, "nightly mcp");
    assert_eq!(
        request.permissions,
        PermissionsDto {
            backups: PermissionLevelDto::Read,
            configuration: PermissionLevelDto::Propose,
            snapshot_contents: PermissionLevelDto::Read,
            daemon_log: PermissionLevelDto::Read,
            ..PermissionsDto::default()
        }
    );
}

/// Each scope gives what a token of that scope was migrated to (decision
/// 127), so a token made with the form can do what it could before.
#[test]
fn each_scope_gives_the_permissions_its_tokens_were_migrated_to() {
    let none = PermissionsDto::default();
    for (scope, permissions) in [
        (
            Scope::Read,
            PermissionsDto {
                backups: PermissionLevelDto::Read,
                snapshot_contents: PermissionLevelDto::Read,
                daemon_log: PermissionLevelDto::Read,
                ..none
            },
        ),
        (
            Scope::Run,
            PermissionsDto {
                backup_runs: PermissionLevelDto::StartAndCancel,
                ..none
            },
        ),
        (
            Scope::Preview,
            PermissionsDto {
                previews: PermissionLevelDto::Preview,
                ..none
            },
        ),
        (
            Scope::Propose,
            PermissionsDto {
                configuration: PermissionLevelDto::Propose,
                ..none
            },
        ),
    ] {
        assert_eq!(
            form("t", &[scope]).request().permissions,
            permissions,
            "{scope:?}"
        );
        assert_eq!(scopes_of(permissions), [scope], "{scope:?}");
    }
}

/// A token given only part of what a scope gives, over the API, still
/// lists that scope, so no permission it has goes unsaid in its row.
#[test]
fn a_row_lists_each_scope_that_gives_any_of_the_token_s_permissions() {
    let daemon_log_only = PermissionsDto {
        daemon_log: PermissionLevelDto::Read,
        ..PermissionsDto::default()
    };
    assert_eq!(scopes_of(daemon_log_only), [Scope::Read]);
    assert!(scopes_of(PermissionsDto::default()).is_empty());
}

#[test]
fn each_problem_links_to_its_field_and_says_the_daemon_s_own_words() {
    let items = problems_of(&form("", &[]), said).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].field, Some("api-token-label"));
    assert_eq!(
        items[0].sentence.plain(),
        said(
            msg::ERROR_API_TOKEN_LABEL,
            &[("max", MsgArg::Count(LABEL_MAX_CHARS))]
        )
    );
    assert_eq!(items[1].field, Some("api-token-scope-read"));
    assert_eq!(
        items[1].sentence.plain(),
        said(msg::ERROR_API_TOKEN_NO_PERMISSION, &[])
    );
}

#[test]
fn a_complete_form_has_no_problems() {
    assert_eq!(
        problems_of(&form("nightly-mcp", &[Scope::Read]), said),
        None
    );
}

#[test]
fn a_field_shows_the_problem_that_is_about_it() {
    let items = problems_of(&form("", &[Scope::Read]), said).unwrap();
    assert_eq!(
        error_of(&items, Field::Label),
        Some(items[0].sentence.plain())
    );
    assert_eq!(error_of(&items, Field::Scopes), None);
}

#[test]
fn a_refused_label_or_scope_is_about_its_field_and_the_cap_about_none() {
    assert_eq!(field_of(ErrorCode::ApiTokenLabel), Some(Field::Label));
    assert_eq!(
        field_of(ErrorCode::ApiTokenNoPermission),
        Some(Field::Scopes)
    );
    assert_eq!(field_of(ErrorCode::ApiTokenLimit), None);
    assert_eq!(field_of(ErrorCode::ApiTokensUnavailable), None);
    assert_eq!(field_of(ErrorCode::SignInOff), None);
}

#[test]
fn a_created_token_is_read_from_the_answer_and_another_answer_is_none() {
    let answer = r#"{"token":{"id":"abcdefghijkl","label":"nightly-mcp","permissions":{"backups":"read","backup-runs":"none","previews":"none","configuration":"none","snapshot-contents":"none","daemon-log":"none","repository-check":"none"},"created-at":"2026-11-14T12:00:00Z","last-used-at":null},"secret":"ladderback_abcdefghijkl_c2VjcmV0"}"#;
    let token = created(answer).unwrap();
    assert_eq!(token.secret, "ladderback_abcdefghijkl_c2VjcmV0");
    assert_eq!(
        token.token,
        ApiTokenDto {
            id: "abcdefghijkl".to_owned(),
            label: "nightly-mcp".to_owned(),
            permissions: PermissionsDto {
                backups: PermissionLevelDto::Read,
                ..PermissionsDto::default()
            },
            created_at: NOW.to_owned(),
            last_used_at: None,
        }
    );
    assert!(created("{}").is_none());
}

#[test]
fn a_token_never_used_says_so() {
    assert_eq!(LastUsed::of(None, now_ms()), LastUsed::Never);
}

#[test]
fn a_use_under_a_minute_ago_is_now() {
    assert_eq!(used_ago(0), ago("now", 0));
    assert_eq!(used_ago(59_999), ago("now", 0));
}

#[test]
fn a_use_after_the_page_s_clock_is_now() {
    assert_eq!(used_ago(-3_600_000), ago("now", 0));
}

#[test]
fn minutes_are_counted_down_to_the_whole_minute() {
    assert_eq!(used_ago(60_000), ago("minutes", 1));
    assert_eq!(used_ago(3_599_999), ago("minutes", 59));
}

#[test]
fn hours_begin_at_the_hour() {
    assert_eq!(used_ago(3_600_000), ago("hours", 1));
    assert_eq!(used_ago(86_399_999), ago("hours", 23));
}

#[test]
fn days_begin_at_the_day() {
    assert_eq!(used_ago(86_400_000), ago("days", 1));
    assert_eq!(used_ago(10 * 86_400_000 + 1), ago("days", 10));
}

#[test]
fn a_last_use_that_does_not_parse_stands_as_it_came() {
    assert_eq!(
        LastUsed::of(Some("yesterday"), now_ms()),
        LastUsed::Unread("yesterday".to_owned())
    );
    assert_eq!(
        LastUsed::Unread("yesterday".to_owned()).said(),
        Err("yesterday".to_owned())
    );
}

#[test]
fn each_last_use_is_said_through_the_catalog() {
    assert_eq!(
        LastUsed::Never.said(),
        Ok((msg::UI_API_TOKEN_NEVER_USED, Vec::new()))
    );
    assert_eq!(
        ago("hours", 3).said(),
        Ok((
            msg::UI_API_TOKEN_LAST_USED,
            vec![
                ("unit", MsgArg::Text("hours".to_owned())),
                ("count", MsgArg::Count(3)),
            ]
        ))
    );
}

#[test]
fn each_scope_has_its_code_its_checkbox_and_its_words() {
    let codes: Vec<_> = SCOPES.into_iter().map(scope_code).collect();
    assert_eq!(codes, ["read", "run", "preview", "propose"]);
    let ids: Vec<_> = SCOPES.into_iter().map(scope_id).collect();
    assert_eq!(
        ids,
        [
            "api-token-scope-read",
            "api-token-scope-run",
            "api-token-scope-preview",
            "api-token-scope-propose"
        ]
    );
    assert_eq!(
        scope_name(Scope::Run),
        (
            msg::UI_API_TOKEN_SCOPE,
            vec![("scope", MsgArg::Text("run".to_owned()))]
        )
    );
    assert_eq!(
        scope_hint(Scope::Preview),
        (
            msg::UI_API_TOKEN_SCOPE_HINT,
            vec![("scope", MsgArg::Text("preview".to_owned()))]
        )
    );
}

#[test]
fn a_token_is_revoked_at_its_own_route_from_its_own_button() {
    assert_eq!(revoke_path("abcdefghijkl"), "/api/v1/tokens/abcdefghijkl");
    assert_eq!(revoke_id("abcdefghijkl"), "api-token-revoke-abcdefghijkl");
}

#[test]
fn the_buttons_and_dialogs_of_a_token_are_named_for_its_label() {
    let label = vec![("label", MsgArg::Text("nightly-mcp".to_owned()))];
    assert_eq!(
        revoke_label("nightly-mcp"),
        (msg::UI_API_TOKEN_REVOKE, label.clone())
    );
    assert_eq!(
        revoke_title("nightly-mcp"),
        (msg::UI_API_TOKEN_REVOKE_TITLE, label.clone())
    );
    assert_eq!(
        revoked("nightly-mcp"),
        (msg::UI_API_TOKEN_REVOKED, label.clone())
    );
    assert_eq!(
        created_body("nightly-mcp"),
        (msg::UI_API_TOKEN_CREATED_BODY, label)
    );
}

/// Decision 127's defaults: a new token sees and starts backups, previews
/// and proposes, and is given what can show a file's name or a log line
/// only when chosen.
#[test]
fn a_new_token_starts_at_the_defaults_of_a_new_token() {
    assert_eq!(
        chosen_of(&new_token()),
        [
            "read",
            "start-and-cancel",
            "preview",
            "propose",
            "none",
            "none",
            "none"
        ]
    );
}

/// Each permission's group holds its level or no access, and gives back
/// what it was set from.
#[test]
fn each_group_gives_back_the_permission_it_was_set_from() {
    for (row, permissions) in ROWS.into_iter().zip([
        PermissionsDto {
            backups: PermissionLevelDto::Read,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            backup_runs: PermissionLevelDto::StartAndCancel,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            previews: PermissionLevelDto::Preview,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            configuration: PermissionLevelDto::Propose,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            snapshot_contents: PermissionLevelDto::Read,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            daemon_log: PermissionLevelDto::Read,
            ..PermissionsDto::default()
        },
        PermissionsDto {
            repository_check: PermissionLevelDto::Run,
            ..PermissionsDto::default()
        },
    ]) {
        let chosen = chosen_of(&permissions);
        for (other, value) in ROWS.into_iter().zip(chosen) {
            let expected = if other == row {
                row.level_code()
            } else {
                NO_ACCESS
            };
            assert_eq!(value, expected, "{row:?}: {other:?}");
        }
        assert_eq!(permissions_from(chosen), permissions, "{row:?}");
        assert_eq!(granted_of(&permissions), [row], "{row:?}");
    }
}

#[test]
fn each_row_has_its_api_code_and_level() {
    let codes: Vec<_> = ROWS
        .into_iter()
        .map(|row| (row.code(), row.level_code()))
        .collect();
    assert_eq!(
        codes,
        [
            ("backups", "read"),
            ("backup-runs", "start-and-cancel"),
            ("previews", "preview"),
            ("configuration", "propose"),
            ("snapshot-contents", "read"),
            ("daemon-log", "read"),
            ("repository-check", "run"),
        ]
    );
}

/// A value that isn't the row's own level, such as another row's, grants
/// nothing.
#[test]
fn a_value_that_isnt_the_rows_level_is_no_access() {
    let chosen = ["run", "read", "propose", "preview", "none", "none", "none"];
    assert_eq!(permissions_from(chosen), PermissionsDto::default());
}

/// 1a's Scopes column said "Read" for a token with only Daemon log, and
/// nothing for one with only Repository check (#1214): each permission is
/// listed by its own name.
#[test]
fn a_row_lists_each_permission_the_token_holds_by_its_own_name() {
    let mixed = PermissionsDto {
        backups: PermissionLevelDto::Read,
        daemon_log: PermissionLevelDto::Read,
        repository_check: PermissionLevelDto::Run,
        ..PermissionsDto::default()
    };
    assert_eq!(
        granted_of(&mixed),
        [Row::Backups, Row::DaemonLog, Row::RepositoryCheck]
    );
    assert!(granted_of(&PermissionsDto::default()).is_empty());
    let code = |row: Row| vec![("permission", MsgArg::Text(row.code().to_owned()))];
    assert_eq!(
        Row::DaemonLog.granted(),
        (msg::UI_PERMISSION_GRANTED, code(Row::DaemonLog))
    );
    assert_eq!(
        Row::Backups.name(),
        (msg::UI_PERMISSION, code(Row::Backups))
    );
    assert_eq!(
        Row::Previews.hint(),
        (msg::UI_PERMISSION_HINT, code(Row::Previews))
    );
}

/// A group offers no access first, then the row's level, each in the
/// catalog's words.
#[test]
fn a_group_offers_no_access_then_its_level() {
    let level = |code: &str| {
        (
            msg::UI_PERMISSION_LEVEL,
            vec![("level", MsgArg::Text(code.to_owned()))],
        )
    };
    assert_eq!(
        Row::BackupRuns.choices(),
        [
            ("none", level("none")),
            ("start-and-cancel", level("start-and-cancel")),
        ]
    );
}

/// A page can hold two forms of permissions, a token's dialog over the new
/// token form: no two groups share a name.
#[test]
fn every_group_of_every_form_has_a_name_of_its_own() {
    let mut names: Vec<&str> = [Form::Create, Form::Change, Form::WithoutToken]
        .into_iter()
        .flat_map(|form| ROWS.into_iter().map(move |row| row.group(form)))
        .collect();
    assert_eq!(Row::Backups.group(Form::Create), "api-token-backups");
    assert_eq!(
        Row::DaemonLog.group(Form::Change),
        "api-token-change-daemon-log"
    );
    assert_eq!(
        Row::RepositoryCheck.group(Form::WithoutToken),
        "without-token-repository-check"
    );
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 21);
}

#[test]
fn a_change_goes_to_the_token_s_own_route_with_the_admin_password() {
    assert_eq!(
        change_path("abcdefghijkl"),
        "/api/v1/tokens/abcdefghijkl/permissions"
    );
    let change = change_request(new_token(), "demo-admin-pass".to_owned());
    assert_eq!(change.permissions, new_token());
    assert_eq!(change.current_password, "demo-admin-pass");
}

/// With sign-in off there is no admin password to give, so none is sent.
#[test]
fn what_ai_agents_may_do_is_sent_with_the_password_only_with_sign_in_on() {
    let on = without_token_request(new_token(), "demo-admin-pass".to_owned(), true);
    assert_eq!(on.permissions, new_token());
    assert_eq!(on.current_password.as_deref(), Some("demo-admin-pass"));
    let off = without_token_request(new_token(), "typed anyway".to_owned(), false);
    assert_eq!(off.current_password, None);
}
