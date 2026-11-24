use std::assert_matches;

use super::captured_log::{self, capture};
use std::sync::PoisonError;

use api::{ErrorCode, MsgArg, PasswordDto};
use app::fakes::{FakeSecrets, InMemoryConfigStore};
use domain::RepositoryId;

use super::service_support::*;
use super::*;

#[tokio::test]
async fn a_saved_repository_is_created_from_its_entry_which_records_only_its_new_id() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the entry is saved");
    // The entry as the connection left it, recording the ID it reached.
    let recorded = saved(&*config).await;
    imported_and_found_none(&engine, &service, &*config).await;
    let answer = service.create_saved("demo".to_owned()).await;
    assert_eq!(answer, Ok(api::RepositoryIdDto { id: "a".repeat(64) }));
    {
        let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
        let [_, Job::Identify(looked), Job::Init(access)] = seen.as_slice() else {
            panic!("creating looks at the location, then runs Job::Init, got {seen:?}");
        };
        assert_eq!(looked.repository, "/srv/demo/repo");
        assert_eq!(access.repository, "/srv/demo/repo");
    }
    assert_eq!(
        saved(&*config).await.repositories(),
        recorded.repositories()
    );
}

/// An entry that records an ID was reached once: it is refused as a
/// repository already there, its location redacted, and nothing runs.
#[tokio::test]
async fn a_saved_repository_whose_entry_records_an_id_is_refused_as_existing_and_nothing_runs() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    let mut settings = settings_dto();
    settings.repository = "rest:https://backup:hunter2@nas.example/repo".to_owned();
    service
        .connect(connect("demo", settings))
        .await
        .expect("the entry is saved");
    let before = saved(&*config).await;
    let Err(RepositoriesError::Exists { location }) = service.create_saved("demo".to_owned()).await
    else {
        panic!("an entry that records an ID is refused as existing");
    };
    assert!(!location.contains("hunter2"), "{location}");
    assert!(location.contains("nas.example/repo"), "{location}");
    assert_eq!(jobs(&engine), 1, "only the connection's test ran");
    assert_eq!(saved(&*config).await, before);
}

/// A row that doesn't offer Create is refused it as not offered, not as a
/// repository already there, whatever page sent the request, and nothing
/// runs: nothing on record, or a backup that once succeeded before the
/// latest found none there (decision 53).
#[tokio::test]
async fn a_saved_repository_no_job_found_missing_is_refused_as_not_offered_and_nothing_runs() {
    let histories: [&[Result<(), FailureReason>]; 2] =
        [&[], &[Ok(()), Err(FailureReason::RepositoryNotFound)]];
    for history in histories {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        service
            .connect(connect("demo", settings_dto()))
            .await
            .expect("the entry is saved");
        imported(&*config).await;
        for end in history {
            backed_up(&*service.operations, end.clone()).await;
        }
        let before = saved(&*config).await;
        assert_eq!(
            service.create_saved("demo".to_owned()).await,
            Err(RepositoriesError::CreateNotOffered),
            "{history:?}"
        );
        assert_eq!(jobs(&engine), 1, "only the connection's test ran");
        assert_eq!(saved(&*config).await, before, "{history:?}");
    }
}

/// The row offers Create on what a backup found, wherever it ran; the
/// entry may since name a location whose folder isn't there, as on a
/// drive that isn't plugged in. Create looks first, and is refused as
/// not offered before anything is created (decision 53).
#[tokio::test]
async fn a_saved_repository_whose_folder_is_not_there_is_refused_as_not_offered() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the entry is saved");
    imported_and_found_none(&engine, &service, &*config).await;
    *engine.found.lock().unwrap_or_else(PoisonError::into_inner) =
        Some(Err(FailureReason::EngineFailed {
            detail: "the repository folder is not there".into(),
        }));
    let before = saved(&*config).await;
    assert_eq!(
        service.create_saved("demo".to_owned()).await,
        Err(RepositoriesError::CreateNotOffered)
    );
    assert_eq!(saved(&*config).await, before);
    let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
    assert_matches!(
        seen.as_slice(),
        [_, Job::Identify(_)],
        "only the connection's test and Create's look ran: {seen:?}"
    );
}

/// An operation log that can't be read can't say whether the row offers
/// Create: it is refused with the log's own words, and nothing runs.
#[tokio::test]
async fn creating_while_the_operation_log_cannot_be_read_is_refused_and_nothing_runs() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        config.clone(),
        Arc::new(NoLog),
        Arc::default(),
    );
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the entry is saved");
    imported(&*config).await;
    assert_eq!(
        service.create_saved("demo".to_owned()).await,
        Err(RepositoriesError::Unavailable {
            code: ErrorCode::OperationFailed,
            args: vec![],
            detail: Some("the database is locked".to_owned()),
        })
    );
    assert_eq!(jobs(&engine), 1, "only the connection's test ran");
}

#[tokio::test]
async fn creating_an_unknown_or_unnamed_repository_is_refused_and_nothing_runs() {
    let engine = engine();
    let service = service(&engine, Arc::new(InMemoryConfigStore::new()));
    for alias in ["demo", "Not A Name"] {
        assert_eq!(
            service.create_saved(alias.to_owned()).await,
            Err(RepositoriesError::Unknown {
                alias: alias.to_owned()
            }),
            "{alias}"
        );
    }
    assert_eq!(jobs(&engine), 0);
}

/// Maintenance turned on and off through the route's service: the entry
/// asks for it, a run of the repository's maintenance keeps it, named by
/// its ID as the wire writes one, and a name the configuration doesn't
/// hold is unknown.
#[tokio::test]
async fn maintenance_of_a_saved_repository_is_turned_on_and_off_unless_its_maintenance_runs() {
    use domain::execution::{Operation, OperationKind, Target, Trigger};
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let operations = Arc::new(app::fakes::InMemoryOperationStore::new());
    let service = RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
        config.clone(),
        operations.clone(),
        Arc::default(),
    );
    service
        .connect(connect("demo", settings_dto()))
        .await
        .expect("the entry is saved");
    let maintenance = |config: domain::planning::AgentConfig| {
        config
            .repository(&alias("demo"))
            .map(|entry| entry.maintenance)
    };
    let turn = |maintain_here| {
        service.change_maintainer("demo".to_owned(), api::MaintainerDto { maintain_here })
    };

    assert_eq!(turn(true).await, Ok(()));
    assert_eq!(maintenance(saved(&*config).await), Some(Maintenance::Local));

    let running = OperationId::from_uuid(domain::Uuid::from_u128(0x619));
    let at = domain::Timestamp::from_second(1_800_000_000).expect("a timestamp");
    let target = Target {
        plan: None,
        repository: Some(repository_id()),
    };
    let mut prune = Operation::queue(running, OperationKind::Prune, Trigger::Manual, target, at);
    prune.start(at).expect("a queued operation starts");
    assert_eq!(
        operations.try_start(&prune).await,
        Ok(app::ports::Start::Started)
    );
    assert_eq!(
        turn(false).await,
        Err(RepositoriesError::MaintenanceBusy {
            operation: "00000000-0000-0000-0000-000000000619".to_owned(),
        })
    );
    assert_eq!(maintenance(saved(&*config).await), Some(Maintenance::Local));

    for alias in ["other", "Not A Name"] {
        assert_eq!(
            service
                .change_maintainer(
                    alias.to_owned(),
                    api::MaintainerDto {
                        maintain_here: true
                    }
                )
                .await,
            Err(RepositoriesError::Unknown {
                alias: alias.to_owned()
            }),
            "{alias}"
        );
    }
}

#[tokio::test]
async fn a_saved_repository_already_there_is_refused_with_its_location_redacted() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let service = service(&engine, config.clone());
    let mut settings = settings_dto();
    settings.repository = "rest:https://backup:hunter2@nas.example/repo".to_owned();
    service
        .connect(connect("demo", settings))
        .await
        .expect("the entry is saved");
    imported_and_found_none(&engine, &service, &*config).await;
    *engine
        .outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::RepositoryExists);
    let Err(RepositoriesError::Exists { location }) = service.create_saved("demo".to_owned()).await
    else {
        panic!("an existing repository is refused as one");
    };
    assert!(!location.contains("hunter2"), "{location}");
    assert!(location.contains("nas.example/repo"), "{location}");
}

/// A configuration that can't be read refuses the creation with the
/// configuration's own code: the alias may well be there.
#[tokio::test]
async fn creating_from_a_configuration_that_cannot_be_read_is_refused_with_its_code() {
    let engine = engine();
    let service = service(
        &engine,
        Arc::new(Broken(ConfigError::Unavailable {
            file: "/c/config.toml".into(),
            detail: "permission denied".into(),
        })),
    );
    let Err(RepositoriesError::Unavailable { code, .. }) =
        service.create_saved("demo".to_owned()).await
    else {
        panic!("a configuration that can't be read is refused as one");
    };
    assert_eq!(code, ErrorCode::ConfigUnavailable);
    assert_eq!(jobs(&engine), 0);
}

/// The entry's password that can't be read is refused with the secret's
/// code, as a test of the same settings would be.
#[tokio::test]
async fn a_saved_repository_whose_password_cannot_be_read_is_refused_with_the_secrets_code() {
    let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
    let mut settings = settings_dto();
    settings.password = PasswordDto::Env {
        name: "DEMO_PASSWORD".into(),
    };
    // Saved while the variable was set; created once it no longer is.
    let secrets = FakeSecrets::new();
    secrets.set_env("DEMO_PASSWORD", "demo-password");
    RepositoryService::new(
        clock(),
        engine.clone(),
        Arc::new(app::fakes::FakeNetworkLink::new()),
        ResolveRepository::new(Arc::new(secrets)),
        config.clone(),
        Arc::new(app::fakes::InMemoryOperationStore::new()),
        Arc::default(),
    )
    .connect(connect("demo", settings))
    .await
    .expect("the entry is saved");
    let creating = service(&engine, config.clone());
    imported_and_found_none(&engine, &creating, &*config).await;
    assert_eq!(
        creating.create_saved("demo".to_owned()).await,
        Err(RepositoriesError::SecretRefused(SecretRefusal {
            code: ErrorCode::SecretMissing,
            args: vec![
                ("field", MsgArg::Text("password".to_owned())),
                ("source", MsgArg::Text("DEMO_PASSWORD".to_owned())),
            ],
            detail: None,
        }))
    );
    assert_eq!(jobs(&engine), 1, "only the connection's test ran");
}

/// A job that failed is refused with the code of its failure: one that
/// finds no repository as `repository-not-found`, and any other in the
/// engine's words, as an initialisation of new settings is.
#[tokio::test]
async fn a_saved_repository_whose_job_failed_is_refused_with_the_failures_code() {
    for (reason, refusal) in [
        (
            FailureReason::RepositoryNotFound,
            RepositoriesError::NotFound,
        ),
        (
            FailureReason::EngineFailed {
                detail: "disk full".into(),
            },
            RepositoriesError::OperationFailed {
                detail: "disk full".into(),
            },
        ),
    ] {
        let (engine, config) = (engine(), Arc::new(InMemoryConfigStore::new()));
        let service = service(&engine, config.clone());
        service
            .connect(connect("demo", settings_dto()))
            .await
            .expect("the entry is saved");
        imported_and_found_none(&engine, &service, &*config).await;
        *engine
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Err(reason);
        assert_eq!(service.create_saved("demo".to_owned()).await, Err(refusal));
    }
}

/// An operation log that can't be read: every call is refused.
struct NoLog;

#[async_trait]
impl OperationStore for NoLog {
    async fn save(
        &self,
        _operation: &domain::execution::Operation,
        _result: Option<&app::ports::EngineResult>,
    ) -> Result<(), app::ports::StoreError> {
        Err(no_log())
    }

    async fn try_start(
        &self,
        _operation: &domain::execution::Operation,
    ) -> Result<app::ports::Start, app::ports::StoreError> {
        Err(no_log())
    }

    async fn get(
        &self,
        _id: OperationId,
    ) -> Result<Option<app::ports::OperationDetail>, app::ports::StoreError> {
        Err(no_log())
    }

    async fn list(
        &self,
        _query: &app::ports::HistoryQuery,
    ) -> Result<app::ports::HistoryPage, app::ports::StoreError> {
        Err(no_log())
    }

    async fn ever_succeeded(&self, _plan: &domain::PlanId) -> Result<bool, app::ports::StoreError> {
        Err(no_log())
    }

    async fn running_maintenance(
        &self,
        _repository: Option<&domain::RepositoryId>,
    ) -> Result<Option<OperationId>, app::ports::StoreError> {
        Err(no_log())
    }

    async fn recover(
        &self,
        _reason: &FailureReason,
    ) -> Result<app::ports::Recovered, app::ports::StoreError> {
        Err(no_log())
    }
}

fn no_log() -> app::ports::StoreError {
    app::ports::StoreError::Unavailable {
        detail: "the database is locked".to_owned(),
    }
}

/// The rows of a configuration of `demo`, which the plan `demo-home`
/// backs up to and whose entry records `demo_id`, and `spare`, which no
/// plan does, each at its own location, listed over `operations`: each
/// alias and whether its row offers to create it.
async fn offers_listed(
    operations: Arc<dyn OperationStore>,
    demo_id: Option<RepositoryId>,
) -> Vec<(String, bool)> {
    let config = Arc::new(InMemoryConfigStore::new());
    let entry = |name: &str, id| domain::planning::RepositoryConfig {
        locations: Default::default(),
        settings: domain::planning::RepositorySettings {
            repository: format!("/srv/{name}/repo"),
            repo_hot: None,
            password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        },
        id,
        maintenance: Default::default(),
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let plan = domain::planning::Plan::new(
        domain::PlanId::parse("demo-home").expect("a plan ID"),
        alias("demo"),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    );
    config
        .save(
            &domain::planning::AgentConfig::new(
                Default::default(),
                Default::default(),
                [
                    (alias("demo"), entry("demo", demo_id)),
                    (alias("spare"), entry("spare", None)),
                ]
                .into(),
                vec![plan],
            )
            .expect("a valid configuration"),
        )
        .await
        .expect("the configuration saves");
    let catalog = ConfigCatalog {
        config,
        engine: engine(),
        operations,
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
    listed
        .repositories
        .into_iter()
        .map(|row| (row.alias, row.create_offered))
        .collect()
}

fn rows(offers: [(&str, bool); 2]) -> Vec<(String, bool)> {
    offers
        .into_iter()
        .map(|(alias, offered)| (alias.to_owned(), offered))
        .collect()
}

#[tokio::test]
async fn a_row_offers_to_create_its_repository_only_once_a_job_found_none_and_none_reached_it() {
    let log = Arc::new(app::fakes::InMemoryOperationStore::new());
    assert_eq!(
        offers_listed(log.clone(), None).await,
        rows([("demo", false), ("spare", false)]),
        "nothing on record"
    );
    backed_up(&*log, Err(FailureReason::RepositoryNotFound)).await;
    assert_eq!(
        offers_listed(log.clone(), None).await,
        rows([("demo", true), ("spare", false)]),
        "a backup found none there"
    );
    backed_up(&*log, Ok(())).await;
    assert_eq!(
        offers_listed(log, None).await,
        rows([("demo", false), ("spare", false)]),
        "a backup reached it"
    );
}

#[tokio::test]
async fn a_row_whose_entry_records_a_repository_id_offers_no_create() {
    let log = Arc::new(app::fakes::InMemoryOperationStore::new());
    backed_up(&*log, Err(FailureReason::RepositoryNotFound)).await;
    assert_eq!(
        offers_listed(log, Some(repository_id())).await,
        rows([("demo", false), ("spare", false)])
    );
}

/// The daemon logs that it offers nothing, with the log's failure as a
/// code: the log's own words may hold a stored value nobody checked (I4).
#[tokio::test]
async fn an_operation_log_that_cannot_be_read_offers_nothing_to_create() {
    capture();
    assert_eq!(
        offers_listed(Arc::new(NoLog), None).await,
        rows([("demo", false), ("spare", false)])
    );
    let said: Vec<String> = captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .filter(|line| line.contains("no repository is offered to be created"))
        .collect();
    assert!(
        said.iter().any(|line| line.contains("store-unavailable")),
        "{said:?}"
    );
    assert!(
        said.iter()
            .all(|line| !line.contains("the database is locked")),
        "{said:?}"
    );
}
