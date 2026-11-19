use serde_json::{from_str, json, to_value};

use crate::{ApiTokenDto, ApiTokenScopeDto, ApiTokensDto, CreateApiTokenDto, CreatedApiTokenDto};

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
        from_str(r#"{"label":"every-scope","scopes":["read","run","preview","propose"]}"#).unwrap();
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
        from_str::<CreateApiTokenDto>(r#"{"label":"x","scopes":["read"],"expires":1}"#).is_err()
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
