use serde_json::{from_str, json, to_string};

use crate::{
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
        crate::SignInDto {
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
        crate::SetAdminPasswordDto {
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
