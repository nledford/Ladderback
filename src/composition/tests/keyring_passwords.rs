use api::{ErrorCode, MsgArg, PasswordDto, RepositorySettingsDto};
use app::fakes::{FakeSecrets, InMemoryConfigStore};

use super::service_support::*;
use super::*;

/// [`service`], keeping passwords in `secrets`, which it also reads them
/// from.
fn service_keeping(
    engine: &Arc<Scripted>,
    config: Arc<dyn ConfigStore>,
    secrets: Arc<FakeSecrets>,
) -> RepositoryService {
    RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(secrets.clone()),
        config,
        Arc::new(app::fakes::InMemoryOperationStore::new()),
        Arc::default(),
    )
    .with_keyring(secrets)
}

#[tokio::test]
async fn a_password_kept_in_the_keyring_is_saved_as_a_reference_to_its_entry() {
    use domain::secrets::{KeyringEntry, SecretRef};
    let entry = KeyringEntry::parse("ladderback/demo").expect("an entry");
    for creates in [false, true] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let secrets = Arc::new(FakeSecrets::new());
        let service = service_keeping(&engine, config.clone(), secrets.clone());
        if creates {
            service
                .init(api::InitRepositoryDto {
                    keep_in_keyring: true,
                    ..init(Some("demo"), settings_dto())
                })
                .await
                .map(|_| ())
        } else {
            service
                .connect(api::ConnectRepositoryDto {
                    keep_in_keyring: true,
                    ..connect("demo", settings_dto())
                })
                .await
                .map(|_| ())
        }
        .unwrap_or_else(|e| panic!("creates {creates}: {e:?}"));
        let config = saved(&*config).await;
        assert_eq!(
            config
                .repository(&alias("demo"))
                .map(|r| &r.settings.password),
            Some(&SecretRef::Keyring(entry.clone())),
            "creates {creates}"
        );
        assert_eq!(
            secrets.keyring_entry(&entry).as_deref(),
            Some("demo-password"),
            "creates {creates}"
        );
    }
}

/// The keyring keeps a password typed for a repository saved under a
/// name: a request that asks for it with no name, or with a password
/// that is already a reference, is one the form never sends.
#[tokio::test]
async fn keeping_the_password_in_the_keyring_needs_a_name_and_a_typed_password() {
    let referenced = RepositorySettingsDto {
        password: PasswordDto::Env {
            name: "DEMO_PASSWORD".to_owned(),
        },
        ..settings_dto()
    };
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let secrets = Arc::new(FakeSecrets::new());
    let service = service_keeping(&engine, config.clone(), secrets.clone());
    assert_eq!(
        service
            .init(api::InitRepositoryDto {
                keep_in_keyring: true,
                ..init(None, settings_dto())
            })
            .await
            .map(|_| ()),
        Err(RepositoriesError::InvalidBody),
        "no name"
    );
    assert_eq!(
        service
            .init(api::InitRepositoryDto {
                keep_in_keyring: true,
                ..init(Some("demo"), referenced.clone())
            })
            .await
            .map(|_| ()),
        Err(RepositoriesError::InvalidBody),
        "a creation with a reference"
    );
    assert_eq!(
        service
            .connect(api::ConnectRepositoryDto {
                keep_in_keyring: true,
                ..connect("demo", referenced)
            })
            .await
            .map(|_| ()),
        Err(RepositoriesError::InvalidBody),
        "a connection with a reference"
    );
    assert_eq!(jobs(&engine), 0);
    assert!(saved(&*config).await.repositories().is_empty());
}

/// The resolver reads at most `MAX_SECRET_BYTES` from a keyring entry,
/// so a longer typed password is refused rather than kept where no
/// backup could read it back.
#[tokio::test]
async fn a_password_kept_in_the_keyring_is_one_the_resolver_can_read_back() {
    use domain::secrets::{KeyringEntry, MAX_SECRET_BYTES};
    let entry = KeyringEntry::parse("ladderback/demo").expect("an entry");
    let typed = |length: usize| RepositorySettingsDto {
        password: PasswordDto::Inline {
            value: "p".repeat(length),
        },
        ..settings_dto()
    };
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let secrets = Arc::new(FakeSecrets::new());
    let service = service_keeping(&engine, config.clone(), secrets.clone());
    assert_eq!(
        service
            .connect(api::ConnectRepositoryDto {
                keep_in_keyring: true,
                ..connect("demo", typed(MAX_SECRET_BYTES + 1))
            })
            .await
            .map(|_| ()),
        Err(RepositoriesError::InvalidBody)
    );
    assert_eq!(secrets.keyring_entry(&entry), None);
    assert_eq!(jobs(&engine), 0);
    service
        .connect(api::ConnectRepositoryDto {
            keep_in_keyring: true,
            ..connect("demo", typed(MAX_SECRET_BYTES))
        })
        .await
        .expect("a password as long as the limit is kept");
    assert_eq!(
        secrets.keyring_entry(&entry).map(|kept| kept.len()),
        Some(MAX_SECRET_BYTES)
    );
}

/// A keyring that refuses every password with its error, and has nothing
/// to remove.
struct Refusing(KeyringError);

#[async_trait]
impl Keyring for Refusing {
    async fn keep(
        &self,
        _entry: &domain::secrets::KeyringEntry,
        _password: &domain::secrets::SecretValue,
    ) -> Result<app::ports::Kept, KeyringError> {
        Err(self.0.clone())
    }

    async fn remove(&self, _entry: &domain::secrets::KeyringEntry) -> Result<(), KeyringError> {
        Ok(())
    }

    async fn activation(&self) -> app::ports::KeyringActivation {
        app::ports::KeyringActivation::OnDemand
    }
}

/// Each refusal names the entry and nothing else; what the platform said
/// comes masked of the typed password (I4). A keyring that never answers
/// is one that couldn't keep it.
#[tokio::test]
async fn a_password_the_keyring_could_not_keep_is_refused_naming_its_entry() {
    let said = "the keyring said demo-password".to_owned();
    for (error, code, detail) in [
        (
            KeyringError::Unavailable {
                detail: said.clone(),
            },
            ErrorCode::KeyringUnavailable,
            true,
        ),
        (
            KeyringError::Unwritable {
                detail: said.clone(),
            },
            ErrorCode::KeyringUnwritable,
            true,
        ),
        (
            KeyringError::TimedOut { seconds: 5 },
            ErrorCode::KeyringUnwritable,
            false,
        ),
        (KeyringError::Taken, ErrorCode::KeyringEntryTaken, false),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let answer = service(&engine, config.clone())
            .with_keyring(Arc::new(Refusing(error.clone())))
            .connect(api::ConnectRepositoryDto {
                keep_in_keyring: true,
                ..connect("demo", settings_dto())
            })
            .await;
        let Err(RepositoriesError::SecretRefused(refusal)) = answer else {
            panic!("{error:?} was not refused as a secret: {answer:?}");
        };
        assert_eq!(refusal.code, code, "{error:?}");
        assert_eq!(
            refusal.args,
            vec![("entry", MsgArg::Text("ladderback/demo".to_owned()))],
            "{error:?}"
        );
        assert_eq!(refusal.detail.is_some(), detail, "{error:?}");
        if let Some(shown) = &refusal.detail {
            assert!(!shown.contains("demo-password"), "{error:?}: {shown}");
            assert!(shown.contains("the keyring said"), "{error:?}: {shown}");
        }
        assert!(saved(&*config).await.repositories().is_empty(), "{error:?}");
    }
}

#[tokio::test]
async fn the_other_password_sources_become_their_references() {
    use domain::secrets::{KeyringEntry, SecretRef, SecretValue};
    for (dto, expected) in [
        (
            PasswordDto::Inline {
                value: "typed".into(),
            },
            SecretRef::Inline(SecretValue::new("typed")),
        ),
        (
            PasswordDto::Keyring {
                entry: "ladderback/nas".into(),
            },
            SecretRef::Keyring(KeyringEntry::parse("ladderback/nas").expect("an entry")),
        ),
        (
            PasswordDto::Env {
                name: "DEMO_PASSWORD".into(),
            },
            SecretRef::Env("DEMO_PASSWORD".into()),
        ),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let mut settings = settings_dto();
        settings.password = dto.clone();
        // Only a connection saves the reference, and testing it first
        // resolves it: the fake knows this variable and this entry.
        let secrets = FakeSecrets::new();
        secrets.set_env("DEMO_PASSWORD", "demo-password");
        secrets.set_keyring_entry(
            &KeyringEntry::parse("ladderback/nas").expect("an entry"),
            "demo-password",
        );
        let service = RepositoryService::new(
            clock(),
            engine.clone(),
            Arc::new(app::fakes::FakeNetworkLink::new()),
            ResolveRepository::new(Arc::new(secrets)),
            config.clone(),
            Arc::new(app::fakes::InMemoryOperationStore::new()),
            Arc::default(),
        );
        service
            .connect(connect("demo", settings))
            .await
            .unwrap_or_else(|e| panic!("{dto:?}: {e:?}"));
        let config = saved(&*config).await;
        let kept = &config.repository(&alias("demo")).expect("saved").settings;
        assert_eq!(kept.password, expected, "{dto:?}");
    }
}
