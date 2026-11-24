use app::fakes::InMemoryConfigStore;

use super::service_support::*;
use super::*;

#[tokio::test]
async fn the_list_shows_where_each_password_comes_from_and_never_the_password() {
    use domain::secrets::{CommandLine, KeyringEntry};
    let config = Arc::new(InMemoryConfigStore::new());
    let sources = [
        (
            "inline",
            SecretRef::Inline(SecretValue::new("typed-secret")),
            api::PasswordSourceDto::Inline,
        ),
        (
            "file",
            SecretRef::File {
                path: "/run/pw".to_owned(),
                read: FileRead::FirstLine,
            },
            api::PasswordSourceDto::File {
                path: "/run/pw".to_owned(),
            },
        ),
        (
            "keyring",
            SecretRef::Keyring(KeyringEntry::parse("ladderback/nas").expect("an entry")),
            api::PasswordSourceDto::Keyring {
                entry: "ladderback/nas".to_owned(),
            },
        ),
        (
            "env",
            SecretRef::Env("PW".to_owned()),
            api::PasswordSourceDto::Env {
                name: "PW".to_owned(),
            },
        ),
        (
            "command",
            SecretRef::Command(
                CommandLine::parse("pass show --secret=hunter2 nas").expect("a command"),
            ),
            api::PasswordSourceDto::Command {
                program: "pass".to_owned(),
            },
        ),
    ];
    let mut repositories = std::collections::BTreeMap::new();
    for (name, password, _) in &sources {
        repositories.insert(
            alias(name),
            domain::planning::RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings: RepositorySettings {
                    // One location each: two entries at one location
                    // are one repository under two names (§7.3).
                    repository: format!("rest:https://u:hunter2@nas.example/repo/{name}"),
                    repo_hot: Some("rest:https://u:hunter3@hot.example/repo".to_owned()),
                    password: password.clone(),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                },
                maintenance: Default::default(),
                group_by: Default::default(),
                hooks: Default::default(),
            },
        );
    }
    config
        .save(
            &domain::planning::AgentConfig::new(
                Default::default(),
                Default::default(),
                repositories,
                Vec::new(),
            )
            .expect("a valid configuration"),
        )
        .await
        .expect("the configuration saves");
    let catalog = ConfigCatalog {
        config,
        engine: engine(),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        leases: None,
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    let listed = catalog.repositories().await.expect("the list");
    // In the order of the aliases, one row each.
    let mut expected: Vec<_> = sources
        .iter()
        .map(|(name, _, shown)| (*name, shown))
        .collect();
    expected.sort_by_key(|(name, _)| *name);
    assert_eq!(listed.repositories.len(), expected.len());
    for (row, (name, shown)) in listed.repositories.iter().zip(expected) {
        assert_eq!(row.alias, name);
        assert_eq!(&row.password, shown, "{name}");
    }
    let wire = serde_json::to_string(&listed).expect("the list serialises");
    for secret in ["typed-secret", "hunter2", "hunter3"] {
        assert!(!wire.contains(secret), "{secret} in {wire}");
    }
    let first = &listed.repositories[0];
    assert!(first.repository.contains("nas.example/repo"), "{first:?}");
    assert!(
        first
            .repo_hot
            .as_deref()
            .is_some_and(|hot| hot.contains("hot.example/repo")),
        "{first:?}"
    );
}

#[test]
fn a_location_is_shown_without_the_password_in_it() {
    let mut settings = domain::planning::RepositorySettings {
        repository: "rest:https://backup:hunter2@nas.example/repo".to_owned(),
        repo_hot: Some("rest:https://backup:hunter3@hot.example/repo".to_owned()),
        password: domain::secrets::SecretRef::Inline(domain::secrets::SecretValue::new("typed")),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    let shown = redacted_location(&settings, &settings.repository);
    assert!(
        !shown.contains("hunter2") && shown.contains("nas.example"),
        "{shown}"
    );
    let hot = settings.repo_hot.clone().expect("a hot half");
    let shown = redacted_location(&settings, &hot);
    assert!(
        !shown.contains("hunter3") && shown.contains("hot.example"),
        "{shown}"
    );
    // The typed password is not a needle: a location that happens to
    // hold the same text is still a location.
    settings.repository = "/srv/typed/repo".to_owned();
    assert_eq!(
        redacted_location(&settings, &settings.repository),
        "/srv/typed/repo"
    );
}
