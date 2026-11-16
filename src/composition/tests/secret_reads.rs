use api::{ErrorCode, MsgArg, PasswordDto};
use app::fakes::{FakeSecrets, InMemoryConfigStore};

use super::service_support::*;
use super::*;

#[tokio::test]
async fn a_secret_that_cannot_be_read_names_where_it_is() {
    for (mut settings, field) in [
        (settings_dto(), "password"),
        (settings_dto(), "repository"),
        (settings_dto(), "repo-hot"),
        (settings_dto(), "options.token"),
    ] {
        match field {
            "password" => {
                settings.password = PasswordDto::Env {
                    name: "NOT_SET_ANYWHERE".into(),
                }
            }
            "repository" => settings.repository = "${NOT_SET_ANYWHERE}/repo".into(),
            "repo-hot" => settings.repo_hot = Some("${NOT_SET_ANYWHERE}/hot".into()),
            _ => settings.options = [("token".to_owned(), "${NOT_SET_ANYWHERE}".to_owned())].into(),
        }
        let engine = engine();
        let answer = service(&engine, Arc::new(InMemoryConfigStore::new()))
            .test(api::TestRepositoryDto { settings })
            .await;
        assert_eq!(
            answer,
            Err(RepositoriesError::SecretRefused(SecretRefusal {
                code: ErrorCode::SecretMissing,
                args: vec![
                    ("field", MsgArg::Text(field.to_owned())),
                    ("source", MsgArg::Text("NOT_SET_ANYWHERE".to_owned())),
                ],
                detail: None,
            }))
        );
        assert_eq!(jobs(&engine), 0, "{field}");
    }
}

fn env_source(name: &str) -> domain::secrets::SourceName {
    domain::secrets::SourceName::Env {
        name: name.to_owned(),
    }
}

/// Every cause a secret or a template can fail by has a code of its own,
/// with the names the message reads: the setting, and the source.
#[tokio::test]
async fn a_secret_that_failed_to_resolve_is_refused_with_the_code_of_its_cause() {
    use domain::secrets::{Problem, SourceName, TemplateError};
    let text = |value: &str| MsgArg::Text(value.to_owned());
    let count = |value: u64| MsgArg::Count(value);
    let settings = settings_of(settings_dto()).expect("settings");
    let file = || SourceName::File {
        path: "/run/secrets/pw".to_owned(),
    };
    let command = || SourceName::Command {
        program: "pass".to_owned(),
    };
    let at = |field: &str, source: &str| vec![("field", text(field)), ("source", text(source))];
    type Case = (
        ResolveError,
        ErrorCode,
        Vec<(&'static str, MsgArg)>,
        Option<&'static str>,
    );
    let cases: Vec<Case> = vec![
        (
            unresolved(Field::Password, env_source("PW"), Problem::Missing),
            ErrorCode::SecretMissing,
            at("password", "PW"),
            None,
        ),
        (
            unresolved(
                Field::Password,
                file(),
                Problem::Unreachable {
                    detail: "permission denied".to_owned(),
                },
            ),
            ErrorCode::SecretUnreadable,
            at("password", "/run/secrets/pw"),
            Some("permission denied"),
        ),
        (
            unresolved(
                Field::Option("token".to_owned()),
                file(),
                Problem::NotUnicode,
            ),
            ErrorCode::SecretNotText,
            at("options.token", "/run/secrets/pw"),
            None,
        ),
        (
            unresolved(
                Field::OptionHot("token".to_owned()),
                file(),
                Problem::TooLarge { limit: 4096 },
            ),
            ErrorCode::SecretTooLarge,
            vec![
                ("field", text("options-hot.token")),
                ("source", text("/run/secrets/pw")),
                ("limit", MsgArg::Bytes(4096)),
            ],
            None,
        ),
        (
            unresolved(
                Field::Password,
                command(),
                Problem::CommandFailed {
                    status: Some(3),
                    detail: "no such entry".to_owned(),
                },
            ),
            ErrorCode::SecretCommandFailed,
            at("password", "pass"),
            Some("no such entry"),
        ),
        (
            unresolved(
                Field::Password,
                command(),
                Problem::TimedOut { seconds: 60 },
            ),
            ErrorCode::SecretCommandTimedOut,
            vec![
                ("field", text("password")),
                ("source", text("pass")),
                ("seconds", count(60)),
            ],
            None,
        ),
        (
            unresolved(Field::Password, command(), Problem::OutputCut),
            ErrorCode::SecretCommandOutputOpen,
            at("password", "pass"),
            None,
        ),
        (
            ResolveError {
                field: Field::Repository,
                cause: Cause::ReferenceInProcessArguments,
            },
            ErrorCode::SecretInProcessArguments,
            vec![("field", text("repository"))],
            None,
        ),
        (
            ResolveError {
                field: Field::RepoHot,
                cause: Cause::ExpansionTooLong { limit: 4096 },
            },
            ErrorCode::SecretExpansionTooLong,
            vec![("field", text("repo-hot")), ("limit", MsgArg::Bytes(4096))],
            None,
        ),
        (
            ResolveError {
                field: Field::Option("note".to_owned()),
                cause: Cause::TooMuchToMask { limit: 262_144 },
            },
            ErrorCode::SecretsTooLargeToMask,
            vec![
                ("limit", MsgArg::Bytes(262_144)),
                ("field", text("options.note")),
            ],
            None,
        ),
        (
            ResolveError {
                field: Field::RepoHot,
                cause: Cause::TooManyCredentials { limit: 16 },
            },
            ErrorCode::LocationTooManyCredentials,
            vec![("field", text("repo-hot"))],
            None,
        ),
        (
            ResolveError {
                field: Field::OptionCold("token".to_owned()),
                cause: Cause::Invalid(TemplateError::Unterminated { at: 3 }),
            },
            ErrorCode::TemplateInvalid,
            vec![("field", text("options-cold.token")), ("at", text("3"))],
            None,
        ),
        (
            ResolveError {
                field: Field::Repository,
                cause: Cause::Invalid(TemplateError::TooLong { limit: 4096 }),
            },
            ErrorCode::TemplateInvalid,
            vec![("field", text("repository"))],
            None,
        ),
    ];
    for (error, code, args, detail) in cases {
        assert_eq!(
            secret_refusal(&error, &settings, &SystemSecrets::default()).await,
            SecretRefusal {
                code,
                args,
                detail: detail.map(str::to_owned),
            },
            "{error:?}"
        );
    }
}

/// A refusal that shows nothing a secret could be in reads no secret: a
/// location with more credentials than a job takes, or a reference that
/// would reach the arguments of a process, is refused before anything is
/// read (§10.2), and so is its answer. One that shows what a source
/// said reads the repository's secrets again, to mask them.
#[tokio::test]
async fn a_refusal_that_shows_no_words_reads_no_secret() {
    use domain::secrets::{Problem, TemplateError};
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.password = SecretRef::Env("DEMO_PASSWORD".to_owned());
    settings.options = [("token".to_owned(), "${DEMO_TOKEN}".to_owned())].into();
    let secrets = FakeSecrets::new();
    secrets.set_env("DEMO_PASSWORD", "demo-password");
    secrets.set_env("DEMO_TOKEN", "demo-token");
    for cause in [
        Cause::TooManyCredentials { limit: 16 },
        Cause::ReferenceInProcessArguments,
        Cause::ExpansionTooLong { limit: 4096 },
        Cause::TooMuchToMask { limit: 262_144 },
        Cause::Invalid(TemplateError::TooLong { limit: 4096 }),
    ] {
        let error = ResolveError {
            field: Field::Repository,
            cause,
        };
        let _ = secret_refusal(&error, &settings, &secrets).await;
        assert_eq!(secrets.reads(), [], "{error:?}");
    }
    let error = unresolved(Field::Password, env_source("PW"), Problem::Missing);
    let _ = secret_refusal(&error, &settings, &secrets).await;
    assert_eq!(secrets.reads().len(), 2);
}

/// A location a reference made relative is refused as one written so is
/// at load and at the repository routes: `repository-path-relative`,
/// naming the setting. Nothing a reference held is in it, and it reads
/// no secret (I4, #247).
#[tokio::test]
async fn a_location_resolved_to_a_relative_path_is_refused_by_its_setting_alone() {
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.repository = "${DEMO_REPO}".to_owned();
    settings.repo_hot = Some("local:${DEMO_HOT}".to_owned());
    let secrets = FakeSecrets::new();
    secrets.set_env("DEMO_REPO", "CANARY-relative/repo");
    secrets.set_env("DEMO_HOT", "CANARY-relative/hot");
    for (field, name) in [
        (Field::Repository, "repository"),
        (Field::RepoHot, "repo-hot"),
    ] {
        let error = ResolveError {
            field,
            cause: Cause::RelativePath,
        };
        assert_eq!(
            secret_refusal(&error, &settings, &secrets).await,
            SecretRefusal {
                code: ErrorCode::RepositoryPathRelative,
                args: vec![("field", MsgArg::Text(name.to_owned()))],
                detail: None,
            },
        );
    }
    assert_eq!(secrets.reads(), []);
}

/// What a command said, and what a platform said, is masked of every
/// secret of the repository before it is an answer: the typed password,
/// a credential option written inline, and a credential a reference in
/// the settings resolves to (I4).
#[tokio::test]
async fn the_words_of_a_failed_secret_are_masked_of_every_secret_of_the_repository() {
    use domain::secrets::Problem;
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let referenced = scratch.path().join("token");
    std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.options = [
        ("secret_access_key".to_owned(), "canary-inline".to_owned()),
        (
            "application_key".to_owned(),
            format!("${{file:{}}}", referenced.display()),
        ),
    ]
    .into();
    let said = "wrote canary-inline, canary-from-a-file and demo-password to stderr";
    let error = unresolved(
        Field::Password,
        domain::secrets::SourceName::Command {
            program: "pass".to_owned(),
        },
        Problem::CommandFailed {
            status: Some(1),
            detail: said.to_owned(),
        },
    );
    let refusal = secret_refusal(&error, &settings, &SystemSecrets::default()).await;
    let detail = refusal.detail.expect("the command's words are kept");
    assert_eq!(
        detail,
        format!(
            "wrote {0}, {0} and {0} to stderr",
            domain::secrets::REDACTED
        )
    );
    // Not the name of an option or of a program, which are not secrets.
    assert_eq!(refusal.code, ErrorCode::SecretCommandFailed);
}

/// The secrets read again for a redactor stop where they pass what one
/// masks: the rest isn't read, and what the failure says is masked
/// whole, since an unread secret could be in it (I4).
#[tokio::test]
async fn the_words_of_a_failed_secret_are_masked_whole_past_what_a_redactor_masks() {
    use domain::secrets::{MAX_MASKED_BYTES, MAX_SECRET_BYTES, Problem, REDACTED};
    let mut settings = settings_of(settings_dto()).expect("settings");
    let secrets = FakeSecrets::new();
    let count = MAX_MASKED_BYTES / MAX_SECRET_BYTES + 1;
    for at in 1..=count {
        let letter = char::from_digit(u32::try_from(at + 9).expect("small"), 36).expect("a letter");
        secrets.set_env(
            &format!("DEMO_{at}"),
            &letter.to_string().repeat(MAX_SECRET_BYTES),
        );
        settings
            .options
            .insert(format!("note-{at}"), format!("${{DEMO_{at}}}"));
    }
    settings
        .options
        .insert("note-9".to_owned(), "${DEMO_LATER}".to_owned());
    secrets.set_env("DEMO_LATER", "demo-later");
    let error = unresolved(
        Field::Password,
        domain::secrets::SourceName::Command {
            program: "pass".to_owned(),
        },
        Problem::CommandFailed {
            status: Some(1),
            detail: "nothing of a secret here".to_owned(),
        },
    );
    let refusal = secret_refusal(&error, &settings, &secrets).await;
    assert_eq!(refusal.detail.as_deref(), Some(REDACTED));
    assert_eq!(
        secrets.read_count(&SecretRef::Env("DEMO_LATER".to_owned())),
        0
    );
}
