use std::assert_matches;

use std::collections::BTreeMap;
use std::sync::PoisonError;

use api::{PasswordDto, RepositorySettingsDto};
use app::fakes::{FakeSecrets, InMemoryConfigStore};

use super::service_support::*;
use super::*;

#[tokio::test]
async fn a_test_reports_the_id_and_saves_nothing() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let answer = service(&engine, config.clone())
        .test(api::TestRepositoryDto {
            settings: settings_dto(),
        })
        .await;
    assert_eq!(answer, Ok(api::RepositoryIdDto { id: "a".repeat(64) }));
    assert_matches!(
        engine.seen.lock().unwrap_or_else(PoisonError::into_inner)[0],
        Job::Identify(_)
    );
    assert!(saved(&*config).await.repositories().is_empty());
}

#[tokio::test]
async fn a_connection_saves_the_settings_as_written() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let mut settings = settings_dto();
    settings.repo_hot = Some("/srv/hot".to_owned());
    settings.password = PasswordDto::File {
        path: "/run/secrets/pw".to_owned(),
    };
    settings.options = [("a".to_owned(), "1".to_owned())].into();
    settings.options_hot = [("b".to_owned(), "2".to_owned())].into();
    settings.options_cold = [("c".to_owned(), "3".to_owned())].into();
    let secrets = FakeSecrets::new();
    secrets.write_file("/run/secrets/pw", b"demo-password\n", 0o600);
    let answer = RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(Arc::new(secrets)),
        config.clone(),
        Arc::new(app::fakes::InMemoryOperationStore::new()),
        Arc::default(),
    )
    .connect(connect("demo", settings))
    .await;
    assert_eq!(answer.map(|id| id.id), Ok("a".repeat(64)));
    let config = saved(&*config).await;
    let repository = config.repository(&alias("demo")).expect("saved");
    let settings = &repository.settings;
    assert_eq!(settings.repository, "/srv/demo/repo");
    assert_eq!(settings.repo_hot.as_deref(), Some("/srv/hot"));
    // A reference stays a reference: the file is read when a job needs it.
    assert_eq!(
        settings.password,
        domain::secrets::SecretRef::File {
            path: "/run/secrets/pw".to_owned(),
            read: domain::secrets::FileRead::FirstLine,
        }
    );
    assert_eq!(settings.options.get("a").map(String::as_str), Some("1"));
    assert_eq!(settings.options_hot.get("b").map(String::as_str), Some("2"));
    assert_eq!(
        settings.options_cold.get("c").map(String::as_str),
        Some("3")
    );
}

#[tokio::test]
async fn a_connection_asks_this_agent_to_maintain_the_repository_only_when_the_request_does() {
    for (maintain_here, saved_as) in [(true, Maintenance::Local), (false, Maintenance::None)] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        service(&engine, config.clone())
            .connect(api::ConnectRepositoryDto {
                maintain_here,
                ..connect("demo", settings_dto())
            })
            .await
            .unwrap_or_else(|e| panic!("{maintain_here}: {e:?}"));
        let config = saved(&*config).await;
        assert_eq!(
            config.repository(&alias("demo")).map(|r| r.maintenance),
            Some(saved_as),
            "maintain-here {maintain_here}"
        );
    }
}

#[tokio::test]
async fn a_value_the_settings_cannot_take_is_an_invalid_body_and_nothing_runs() {
    for password in [
        PasswordDto::File {
            path: String::new(),
        },
        PasswordDto::Keyring {
            entry: "no-slash".into(),
        },
        PasswordDto::Keyring {
            entry: "/name".into(),
        },
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        let mut settings = settings_dto();
        settings.password = password.clone();
        assert_eq!(
            service
                .test(api::TestRepositoryDto {
                    settings: settings.clone()
                })
                .await,
            Err(RepositoriesError::InvalidBody),
            "{password:?}"
        );
        assert_eq!(
            service.connect(connect("demo", settings.clone())).await,
            Err(RepositoriesError::InvalidBody),
            "{password:?}"
        );
        assert_eq!(
            service.init(init(None, settings)).await,
            Err(RepositoriesError::InvalidBody),
            "{password:?}"
        );
        assert_eq!(jobs(&engine), 0, "{password:?}");
    }
}

#[tokio::test]
async fn a_name_that_is_not_an_alias_is_refused_before_anything_runs() {
    let long = "a".repeat(65);
    for name in [
        "Not A Name",
        "",
        "-lead",
        "under_score",
        "demo/x",
        long.as_str(),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        assert_eq!(
            service.connect(connect(name, settings_dto())).await,
            Err(RepositoriesError::AliasInvalid),
            "{name:?}"
        );
        // An initialisation that would be saved is refused before it
        // creates a repository it couldn't then keep.
        assert_eq!(
            service.init(init(Some(name), settings_dto())).await,
            Err(RepositoriesError::AliasInvalid),
            "{name:?}"
        );
        assert_eq!(jobs(&engine), 0, "{name:?} ran a job");
        assert!(saved(&*config).await.repositories().is_empty());
    }
}

#[tokio::test]
async fn an_alias_already_used_is_refused_and_the_first_repository_stays() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the first connection");
    let mut other = settings_dto();
    other.repository = "/srv/other".to_owned();
    assert_eq!(
        service.connect(connect("demo", other)).await,
        Err(RepositoriesError::AliasTaken {
            alias: "demo".to_owned()
        })
    );
    let config = saved(&*config).await;
    assert_eq!(config.repositories().len(), 1);
    assert_eq!(
        config
            .repository(&alias("demo"))
            .expect("kept")
            .settings
            .repository,
        "/srv/demo/repo"
    );
}

#[tokio::test]
async fn an_initialization_is_saved_only_when_it_names_an_alias() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    let answer = service.init(init(None, settings_dto())).await;
    assert_eq!(
        answer,
        Ok(api::RepositoryInitDto {
            id: "a".repeat(64),
            saved: false
        })
    );
    assert_matches!(
        engine.seen.lock().unwrap_or_else(PoisonError::into_inner)[0],
        Job::Init(_)
    );
    assert!(saved(&*config).await.repositories().is_empty());

    let answer = service.init(init(Some("fresh"), settings_dto())).await;
    assert_eq!(
        answer,
        Ok(api::RepositoryInitDto {
            id: "a".repeat(64),
            saved: true
        })
    );
    assert!(saved(&*config).await.repository(&alias("fresh")).is_some());
}

#[tokio::test]
async fn an_empty_password_or_location_is_an_invalid_body_and_nothing_runs() {
    let mut empty_password = settings_dto();
    empty_password.password = PasswordDto::Inline {
        value: String::new(),
    };
    let mut empty_location = settings_dto();
    empty_location.repository = String::new();
    let mut empty_hot = settings_dto();
    empty_hot.repo_hot = Some(String::new());
    for (what, settings) in [
        ("password", empty_password),
        ("location", empty_location),
        ("hot location", empty_hot),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        assert_eq!(
            service
                .test(api::TestRepositoryDto {
                    settings: settings.clone()
                })
                .await,
            Err(RepositoriesError::InvalidBody),
            "an empty {what}"
        );
        assert_eq!(
            service.connect(connect("demo", settings.clone())).await,
            Err(RepositoriesError::InvalidBody),
            "an empty {what}"
        );
        assert_eq!(
            service.init(init(Some("demo"), settings)).await,
            Err(RepositoriesError::InvalidBody),
            "an empty {what}"
        );
        assert_eq!(jobs(&engine), 0, "an empty {what} ran a job");
        assert!(saved(&*config).await.repositories().is_empty());
    }
}

// The configuration refuses settings longer than a job takes when it
// loads (#236), so a request that would save them is refused first: the
// daemon would otherwise not start again.
#[tokio::test]
async fn settings_over_a_cap_are_an_invalid_body_and_nothing_runs() {
    use domain::secrets::{MAX_LOCATION_BYTES, MAX_OPTION_VALUE_BYTES, MAX_OPTIONS};
    let long = |bytes: usize| format!("/{}", "v".repeat(bytes - 1));
    let long_option =
        || BTreeMap::from([("endpoint".to_owned(), long(MAX_OPTION_VALUE_BYTES + 1))]);
    let many = || {
        (0..=MAX_OPTIONS)
            .map(|n| (format!("o{n}"), "demo".to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    let with = |change: &dyn Fn(&mut RepositorySettingsDto)| {
        let mut settings = settings_dto();
        change(&mut settings);
        settings
    };
    for (what, settings) in [
        (
            "location",
            with(&|s| s.repository = long(MAX_LOCATION_BYTES + 1)),
        ),
        (
            "hot location",
            with(&|s| s.repo_hot = Some(long(MAX_LOCATION_BYTES + 1))),
        ),
        ("option", with(&|s| s.options = long_option())),
        ("hot option", with(&|s| s.options_hot = long_option())),
        ("cold option", with(&|s| s.options_cold = long_option())),
        ("table of options", with(&|s| s.options = many())),
        ("table of hot options", with(&|s| s.options_hot = many())),
        ("table of cold options", with(&|s| s.options_cold = many())),
        // Refused for its length before it is found relative.
        (
            "relative location",
            with(&|s| s.repository = format!("backups/{}", "r".repeat(MAX_LOCATION_BYTES))),
        ),
        (
            "relative option",
            with(&|s| {
                s.repository = "opendal:fs".to_owned();
                s.options = BTreeMap::from([(
                    "root".to_owned(),
                    format!("backups/{}", "r".repeat(MAX_OPTION_VALUE_BYTES)),
                )]);
            }),
        ),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        assert_eq!(
            service
                .test(api::TestRepositoryDto {
                    settings: settings.clone()
                })
                .await,
            Err(RepositoriesError::InvalidBody),
            "a long {what}"
        );
        assert_eq!(
            service.connect(connect("demo", settings.clone())).await,
            Err(RepositoriesError::InvalidBody),
            "a long {what}"
        );
        assert_eq!(
            service.init(init(Some("demo"), settings)).await,
            Err(RepositoriesError::InvalidBody),
            "a long {what}"
        );
        assert_eq!(jobs(&engine), 0, "a long {what} ran a job");
        assert!(saved(&*config).await.repositories().is_empty());
    }
}

// The configuration refuses these when it loads, so a saved one would
// stop the daemon from starting again. The refusal names the setting, so
// the page can show it there.
#[tokio::test]
async fn a_relative_path_on_this_machine_is_refused_at_its_setting_and_nothing_runs() {
    let mut relative = settings_dto();
    relative.repository = "backups/repo".to_owned();
    let mut relative_hot = settings_dto();
    relative_hot.repo_hot = Some("~/hot".to_owned());
    let mut relative_file = settings_dto();
    relative_file.password = PasswordDto::File {
        path: "password.txt".into(),
    };
    // A relative password file is read before the caps are checked, so
    // it is refused first, even beside a location over its cap.
    let mut relative_file_long_location = settings_dto();
    relative_file_long_location.password = PasswordDto::File {
        path: "password.txt".into(),
    };
    relative_file_long_location.repository =
        format!("/{}", "v".repeat(domain::secrets::MAX_LOCATION_BYTES));
    let mut relative_root = settings_dto();
    relative_root.repository = "opendal:fs".to_owned();
    relative_root.options = [("root".to_owned(), "backups".to_owned())].into();
    let mut relative_cert = settings_dto();
    relative_cert.repository = "rest:https://nas.example/repo".to_owned();
    relative_cert.options_cold = [("http-cacert".to_owned(), "ca.pem".to_owned())].into();
    for (settings, field) in [
        (relative, "repository"),
        (relative_hot, "repo-hot"),
        (relative_file, "password"),
        (relative_file_long_location, "password"),
        (relative_root, "options.root"),
        (relative_cert, "options-cold.http-cacert"),
    ] {
        let refused = Err(RepositoriesError::PathRelative {
            field: field.to_owned(),
        });
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        assert_eq!(
            service
                .test(api::TestRepositoryDto {
                    settings: settings.clone()
                })
                .await
                .map(|_| ()),
            refused,
            "{field}"
        );
        assert_eq!(
            service
                .connect(connect("demo", settings.clone()))
                .await
                .map(|_| ()),
            refused,
            "{field}"
        );
        assert_eq!(
            service.init(init(Some("demo"), settings)).await.map(|_| ()),
            refused,
            "{field}"
        );
        assert_eq!(jobs(&engine), 0, "a relative {field} ran a job");
        assert!(saved(&*config).await.repositories().is_empty());
    }
}

#[tokio::test]
async fn an_initialization_under_an_alias_already_used_runs_no_job() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the first connection");
    let jobs_before = jobs(&engine);
    let mut other = settings_dto();
    other.repository = "/srv/other".to_owned();
    assert_eq!(
        service.init(init(Some("demo"), other)).await,
        Err(RepositoriesError::AliasTaken {
            alias: "demo".to_owned()
        })
    );
    assert_eq!(jobs(&engine), jobs_before, "a repository was created");
    assert_eq!(saved(&*config).await.repositories().len(), 1);
}
