use serde_json::{from_str, json, to_value};

use crate::{
    ApiTokenDto, ApiTokensDto, ChangeApiTokenPermissionsDto, ChangeWithoutTokenDto,
    CreateApiTokenDto, CreatedApiTokenDto, PermissionLevelDto, PermissionsDto,
};

fn listed() -> ApiTokenDto {
    ApiTokenDto {
        id: "abcdefghijkl".to_owned(),
        label: "nightly-mcp".to_owned(),
        permissions: PermissionsDto {
            backup_runs: PermissionLevelDto::StartAndCancel,
            daemon_log: PermissionLevelDto::Read,
            ..PermissionsDto::default()
        },
        created_at: "2026-11-14T10:00:00Z".to_owned(),
        last_used_at: None,
    }
}

#[test]
fn a_listed_token_names_every_permission_and_a_null_last_use_when_never_used() {
    let wire = to_value(ApiTokensDto {
        tokens: vec![listed()],
    })
    .unwrap();
    assert_eq!(
        wire,
        json!({"tokens": [{
            "id": "abcdefghijkl",
            "label": "nightly-mcp",
            "permissions": {
                "backups": "none",
                "backup-runs": "start-and-cancel",
                "previews": "none",
                "configuration": "none",
                "snapshot-contents": "none",
                "daemon-log": "read",
                "repository-check": "none",
            },
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
fn a_permission_left_out_is_no_access_and_no_other_name_is_a_permission_or_a_level() {
    let body: CreateApiTokenDto =
        from_str(r#"{"label":"runner","permissions":{"backup-runs":"start-and-cancel"}}"#).unwrap();
    assert_eq!(
        body.permissions,
        PermissionsDto {
            backup_runs: PermissionLevelDto::StartAndCancel,
            ..PermissionsDto::default()
        }
    );
    for wrong in [
        r#"{"label":"x","permissions":{"admin":"read"}}"#,
        r#"{"label":"x","permissions":{"backups":"Read"}}"#,
        r#"{"label":"x","permissions":{"backups":"write"}}"#,
    ] {
        assert!(from_str::<CreateApiTokenDto>(wrong).is_err(), "{wrong}");
    }
}

#[test]
fn a_create_body_with_a_field_of_its_own_is_refused() {
    assert!(from_str::<CreateApiTokenDto>(r#"{"label":"x","permissions":{}}"#).is_ok());
    assert!(
        from_str::<CreateApiTokenDto>(r#"{"label":"x","permissions":{},"expires":1}"#).is_err()
    );
}

#[test]
fn a_permissions_change_never_shows_the_password_in_debug() {
    let change: ChangeApiTokenPermissionsDto =
        from_str(r#"{"permissions":{"backups":"read"},"current-password":"canary-password"}"#)
            .unwrap();
    assert_eq!(change.permissions.backups, PermissionLevelDto::Read);
    let shown = format!("{change:?}");
    assert!(!shown.contains("canary-password"), "{shown}");
    assert!(
        shown.starts_with("ChangeApiTokenPermissionsDto { permissions: "),
        "{shown}"
    );
    assert!(shown.ends_with("current_password: [hidden] }"), "{shown}");
}

#[test]
fn a_without_token_change_never_shows_the_password_in_debug() {
    let change: ChangeWithoutTokenDto =
        from_str(r#"{"permissions":{"daemon-log":"read"},"current-password":"canary-password"}"#)
            .unwrap();
    assert_eq!(change.permissions.daemon_log, PermissionLevelDto::Read);
    let shown = format!("{change:?}");
    assert!(!shown.contains("canary-password"), "{shown}");
    assert!(shown.contains("daemon_log: Read"), "{shown}");
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
