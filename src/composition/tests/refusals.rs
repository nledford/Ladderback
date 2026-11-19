use super::support::*;
use super::*;

fn said(refusal: &Refusal) -> String {
    let shown = english().error(refusal.code, &refusal.args, refusal.detail.as_deref());
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    shown.into_text()
}

#[test]
fn an_event_told_while_the_configuration_cant_be_read_is_logged_by_its_code() {
    let line = report_line(
        &english(),
        &Report::ConfigUnavailable(ConfigError::Unavailable {
            file: "/home/demo/.config/ladderback/config.toml".to_owned(),
            detail: "permission denied".to_owned(),
        }),
    );
    for expected in [
        "ConfigUnavailable",
        "/home/demo/.config/ladderback/config.toml",
        "permission denied",
    ] {
        assert!(line.contains(expected), "{expected} in {line}");
    }
}

/// The notifier says an unusable configuration by the code, the arguments
/// and the detail every route refuses it with, whichever way it can't be
/// used.
#[test]
fn each_way_a_configuration_cant_be_used_is_logged_as_the_routes_refuse_it() {
    for (error, code, args, detail) in super::service_support::unusable_configurations() {
        let message = english().error(code, &args, detail);
        assert_eq!(
            report_line(&english(), &Report::ConfigUnavailable(error.clone())),
            format!("no notification was sent for an event: {code:?}: {message}"),
            "{error:?}"
        );
    }
}

#[test]
fn a_notification_not_delivered_is_logged_by_its_code() {
    use domain::notifications::{ChannelName, EventKind, RuleName};
    let line = report_line(
        &english(),
        &Report::Undelivered(app::notifications::Undelivered {
            event: EventKind::BackupFailed,
            rule: RuleName::parse("failures").expect("a rule name"),
            channel: ChannelName::parse("ops").expect("a channel name"),
            reason: app::notifications::Unsent::Delivery(app::ports::NotifyError::TimedOut),
        }),
    );
    for expected in [
        "NotificationUndelivered",
        "backup.failed",
        "failures",
        "ops",
        "did not answer in time",
    ] {
        assert!(line.contains(expected), "{expected} in {line}");
    }
}

fn invalid(key: Option<&str>, at: Option<(usize, usize)>) -> ImportError {
    ImportError::Profile(ProfileError::Invalid {
        file: "nas.toml".to_owned(),
        key: key.map(str::to_owned),
        at: at.map(|(line, column)| app::ports::Position { line, column }),
        problem: app::ports::ProfileProblem::UnknownKey,
        detail: String::new(),
    })
}

#[test]
fn an_invalid_profile_names_its_file_its_key_and_its_line() {
    let refusal = import_refusal(
        invalid(Some("forget.keep-lots"), Some((7, 3))),
        std::path::Path::new("/cfg/config.toml"),
    );
    assert_eq!(refusal.code, ErrorCode::ProfileInvalid);
    let said = said(&refusal);
    for part in ["nas.toml", "forget.keep-lots", "line 7", "UnknownKey"] {
        assert!(said.contains(part), "{part}: {said}");
    }
}

#[test]
fn an_invalid_profile_with_no_place_does_not_say_none() {
    let refusal = import_refusal(invalid(None, None), std::path::Path::new("/cfg/c.toml"));
    assert!(refusal.args.iter().all(|(name, _)| *name == "file"));
    let said = said(&refusal);
    assert!(said.contains("nas.toml"), "{said}");
    assert!(!said.contains("none") && !said.contains("line"), "{said}");
}

#[test]
fn an_unsupported_setting_names_the_key_and_the_line_when_known() {
    let unsupported = |at| {
        ImportError::Profile(ProfileError::Unsupported {
            file: "nas.toml".to_owned(),
            key: "forget.keep-tags".to_owned(),
            at,
        })
    };
    let path = std::path::Path::new("/cfg/c.toml");
    let placed = said(&import_refusal(
        unsupported(Some(app::ports::Position {
            line: 12,
            column: 1,
        })),
        path,
    ));
    assert!(placed.contains("forget.keep-tags") && placed.contains("line 12"));
    let unplaced = said(&import_refusal(unsupported(None), path));
    assert!(unplaced.contains("forget.keep-tags") && !unplaced.contains("line"));
}

#[test]
fn a_profile_that_cannot_be_read_names_the_file_and_the_limit() {
    let path = std::path::Path::new("/cfg/c.toml");
    let unreadable = import_refusal(
        ImportError::Profile(ProfileError::Unreadable {
            file: "missing.toml".to_owned(),
        }),
        path,
    );
    assert_eq!(unreadable.code, ErrorCode::ProfileUnreadable);
    assert!(said(&unreadable).contains("missing.toml"));
    let large = import_refusal(
        ImportError::Profile(ProfileError::TooLarge {
            file: "big.toml".to_owned(),
            limit: 1 << 20,
        }),
        path,
    );
    assert_eq!(large.code, ErrorCode::ProfileTooLarge);
    let said = said(&large);
    assert!(
        said.contains("big.toml") && said.contains("1 MiB"),
        "{said}"
    );
}

#[test]
fn an_export_that_cannot_be_made_says_which_plan_or_which_setting() {
    let path = std::path::Path::new("/cfg/c.toml");
    let unknown = export_refusal(
        ExportError::PlanUnknown(PlanId::parse("ghost").expect("a slug")),
        path,
    );
    assert_eq!(unknown.code, ErrorCode::PlanUnknown);
    assert!(said(&unknown).contains("ghost"));
    let inline = export_refusal(ExportError::Render(ExportRefusal::InlineSecret), path);
    assert_eq!(inline.code, ErrorCode::InlineSecret);
    assert!(said(&inline).contains("password"));
    let reference = export_refusal(
        ExportError::Render(ExportRefusal::Reference {
            key: "repository".to_owned(),
        }),
        path,
    );
    assert_eq!(reference.code, ErrorCode::ProfileExportReference);
    assert!(said(&reference).contains("repository"));
    let url = export_refusal(
        ExportError::Render(ExportRefusal::UrlPassword {
            key: "repo-hot".to_owned(),
        }),
        path,
    );
    assert_eq!(url.code, ErrorCode::ProfileExportUrlPassword);
    assert!(said(&url).contains("repo-hot"));
    let tag = export_refusal(
        ExportError::Render(ExportRefusal::TagComma {
            tag: "a,b".to_owned(),
        }),
        path,
    );
    assert_eq!(tag.code, ErrorCode::ProfileExportTagComma);
    assert!(said(&tag).contains("a,b"));
    let config = export_refusal(
        ExportError::Config(ConfigError::SchemaMissing {
            file: "/cfg/c.toml".to_owned(),
        }),
        path,
    );
    assert_eq!(config.code, ErrorCode::ConfigSchemaMissing);
}

#[test]
fn a_clash_names_the_alias_or_the_plan() {
    let path = std::path::Path::new("/cfg/c.toml");
    let alias = import_refusal(
        ImportError::AliasTaken(RepositoryAlias::parse("nas").expect("a slug")),
        path,
    );
    assert_eq!(alias.code, ErrorCode::RepositoryAliasTaken);
    assert!(said(&alias).contains("nas"));
    let configured = import_refusal(
        ImportError::AlreadyConfigured(RepositoryAlias::parse("nas").expect("a slug")),
        path,
    );
    assert_eq!(configured.code, ErrorCode::RepositoryAlreadyConfigured);
    assert!(said(&configured).contains("nas"));
    let plan = import_refusal(
        ImportError::PlanTaken(PlanId::parse("home").expect("a slug")),
        path,
    );
    assert_eq!(plan.code, ErrorCode::PlanIdTaken);
    assert!(said(&plan).contains("home"));
}

#[test]
fn a_name_that_is_not_an_alias_is_refused_by_its_code_alone() {
    let refusal = repository_alias_invalid();
    assert_eq!(refusal.code, ErrorCode::RepositoryAliasInvalid);
    assert!(refusal.args.is_empty());
    assert_eq!(refusal.detail, None);
    assert!(said(&refusal).contains("lower-case letters"));
}

#[test]
fn no_folder_for_the_configuration_is_refused_by_its_code_alone() {
    let refusal = config_folder_unknown();
    assert_eq!(refusal.code, ErrorCode::ConfigFolderUnknown);
    assert!(refusal.args.is_empty());
    assert_eq!(refusal.detail, None);
    let shown = said(&refusal);
    assert!(shown.contains("LADDERBACK_CONFIG_DIR"), "{shown}");
}

#[test]
fn a_runtime_that_did_not_start_is_refused_with_the_operating_systems_words() {
    let refusal = runtime_unavailable(&std::io::Error::other("Too many open files"));
    assert_eq!(refusal.code, ErrorCode::RuntimeUnavailable);
    assert!(refusal.args.is_empty());
    assert_eq!(refusal.detail.as_deref(), Some("Too many open files"));
    let shown = said(&refusal);
    assert!(shown.contains("Too many open files"), "{shown}");
    assert!(shown.contains("open files and threads"), "{shown}");
}

#[test]
fn a_merged_configuration_that_is_not_valid_names_the_configuration_file() {
    let refusal = import_refusal(
        ImportError::Config(ConfigError::Invalid {
            file: String::new(),
            key: None,
            at: None,
            problem: app::ports::Problem::Violation(domain::planning::Violation::NoSources {
                index: 0,
                plan: PlanId::parse("home").expect("a slug"),
            }),
            detail: String::new(),
        }),
        std::path::Path::new("/cfg/config.toml"),
    );
    assert_eq!(refusal.code, ErrorCode::ConfigInvalid);
    let said = said(&refusal);
    assert!(said.contains("/cfg/config.toml"), "{said}");
}

#[test]
fn a_store_that_cannot_be_read_is_the_configuration_codes() {
    let path = std::path::Path::new("/cfg/config.toml");
    let unavailable = import_refusal(
        ImportError::Config(ConfigError::Unavailable {
            file: "/cfg/config.toml".to_owned(),
            detail: "permission denied".to_owned(),
        }),
        path,
    );
    assert_eq!(unavailable.code, ErrorCode::ConfigUnavailable);
    assert!(said(&unavailable).contains("permission denied"));
    let newer = import_refusal(
        ImportError::Config(ConfigError::SchemaNewer {
            file: "/cfg/config.toml".to_owned(),
            found: 9,
            supported: 1,
        }),
        path,
    );
    assert_eq!(newer.code, ErrorCode::ConfigSchemaNewer);
    assert!(said(&newer).contains('9'));
}

#[test]
fn every_notice_is_a_warning_that_renders_what_it_names() {
    let notices = [
        (ProfileNotice::KeyIgnored { key: "copy".into() }, "copy"),
        (ProfileNotice::Prune, "Pruning will run from one place"),
        (ProfileNotice::EnvIgnored, "global.env"),
        (ProfileNotice::HooksPerPlan, "once per plan"),
        (
            ProfileNotice::UsedProfileMissing {
                name: "ghost".into(),
            },
            "ghost",
        ),
        (
            ProfileNotice::InlineSecret {
                field: "repository.password".into(),
            },
            "repository.password",
        ),
        (
            ProfileNotice::LocationSecret {
                field: "repository.repository".into(),
                rclone: false,
            },
            "The address in the repository.repository setting holds a password as text",
        ),
        (
            ProfileNotice::LocationSecret {
                field: "repository.repo-hot".into(),
                rclone: true,
            },
            "Set up the remote in rclone's own configuration file",
        ),
        (
            ProfileNotice::GlobNeverMatches {
                plan: "home".into(),
                glob: "!.local/share/containers".into(),
                absolute: Some("!/home/user/.local/share/containers".into()),
            },
            "Write the whole path instead, as in !/home/user/.local/share/containers.",
        ),
        (
            ProfileNotice::GlobNeverMatches {
                plan: "home".into(),
                glob: "!docker/app/cache".into(),
                absolute: None,
            },
            "Write the whole path instead, starting with the folder of the source it belongs to.",
        ),
        (
            ProfileNotice::TagDropped {
                tag: "ladderback-plan:x".into(),
            },
            "ladderback-plan:x",
        ),
        (
            ProfileNotice::OptionLeftOut {
                option: "secret-key".into(),
                variable: "OPENDAL_SECRET_KEY".into(),
            },
            "OPENDAL_SECRET_KEY",
        ),
        (ProfileNotice::RetentionEmpty, "keep-*"),
        (
            ProfileNotice::RepositorySettingKept {
                repository: "demo-nas".into(),
                setting: "group-by".into(),
            },
            "demo-nas was already configured, so the profile's plans were added to it, and it kept its own group-by",
        ),
        (
            ProfileNotice::LocationsNotExported { further: 1 },
            "1 further location was left out",
        ),
        (
            ProfileNotice::LocationsNotExported { further: 2 },
            "2 further locations were left out",
        ),
    ];
    for (notice, expected) in notices {
        let notice = profile_notice(notice);
        let shown = english().warning(notice.code, &notice.args, None);
        assert!(shown.problems().is_empty(), "{:?}", shown.problems());
        assert!(shown.text().contains(expected), "{expected}: {shown}");
    }
}

/// A keyring command the export's PATH lacks is a warning that names it,
/// and for `secret-tool` the packages it comes in, by distribution.
#[test]
fn a_missing_keyring_command_is_a_warning_naming_its_package() {
    for (program, expected) in [
        (
            "secret-tool",
            &[
                "with secret-tool, which is not on this computer's PATH",
                "the libsecret package on Fedora",
                "libsecret-tools on Debian and Ubuntu",
            ][..],
        ),
        (
            "security",
            &["with security, which is not on this computer's PATH"][..],
        ),
    ] {
        let notice = profile_notice(ProfileNotice::KeyringCommandMissing {
            program: program.to_owned(),
        });
        assert_eq!(notice.code, WarningCode::ProfileKeyringCommandMissing);
        assert_eq!(notice.args, vec![("program", text(program))]);
        let shown = english().warning(notice.code, &notice.args, None);
        assert!(shown.problems().is_empty(), "{:?}", shown.problems());
        for expected in expected {
            assert!(shown.text().contains(expected), "{expected}: {shown}");
        }
    }
}

/// What a check found reaches the page and the operation's detail under
/// the code of its level, not as a warning this version can't describe.
#[test]
fn a_checks_findings_reach_the_page_under_the_codes_of_their_levels() {
    for (kind, code) in [
        (WarningKind::CheckWarning, WarningCode::CheckWarning),
        (WarningKind::CheckError, WarningCode::CheckError),
    ] {
        assert_eq!(warning_code(kind), code);
        let mut publish = Publish {
            operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
            progress: LiveProgress::new(),
        };
        let found = EngineEvent::Warning(domain::execution::Warning {
            kind,
            item: None,
            during: "check".to_owned(),
            detail: "pack 5ca1ab1e not referenced in index".to_owned(),
        });
        assert_eq!(
            publish.ui(&found),
            Some(UiEvent::OperationWarning {
                operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
                code,
                args: Vec::new(),
            })
        );
    }
}

/// A secret file others can read reaches the page with the two
/// arguments its message takes: the file's path and its mode (#386).
#[test]
fn a_readable_secret_file_reaches_the_page_by_its_path_and_mode() {
    let mut publish = Publish {
        operation: "018f0000-0000-7000-8000-000000000001".to_owned(),
        progress: LiveProgress::new(),
    };
    let found = EngineEvent::Warning(domain::execution::Warning {
        kind: WarningKind::SecretFileReadable,
        item: Some("/home/demo/nas-password".to_owned()),
        during: "secret".to_owned(),
        detail: "0644".to_owned(),
    });
    let Some(UiEvent::OperationWarning { code, args, .. }) = publish.ui(&found) else {
        panic!("the warning reaches the page");
    };
    assert_eq!(code, WarningCode::SecretFileReadable);
    assert_eq!(
        args,
        [
            MsgArg::Text("/home/demo/nas-password".to_owned()),
            MsgArg::Text("0644".to_owned()),
        ]
    );
}
