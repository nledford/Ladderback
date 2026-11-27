use super::captured_log::{self, capture};

use super::service_support::*;
use super::*;

#[tokio::test]
async fn a_configuration_that_cannot_be_used_is_unavailable_with_its_code() {
    for (error, code, args, detail) in unusable_configurations() {
        let engine = engine();
        let service = service(&engine, Arc::new(Broken(error.clone())));
        let expected = Err(RepositoriesError::Unavailable {
            code,
            args,
            detail: detail.map(str::to_owned),
        });
        assert_eq!(
            service
                .connect(connect("demo", settings_dto()))
                .await
                .map(|_| ()),
            expected.clone().map(|_: ()| ()),
            "{error:?}"
        );
        assert_eq!(
            service
                .init(init(Some("demo"), settings_dto()))
                .await
                .map(|_| ()),
            expected.map(|_: ()| ()),
            "{error:?}"
        );
    }
}

/// The lists of plans and repositories refuse a configuration they can't
/// read as a save does (#347): a mistake names its file, its line and its
/// setting.
#[tokio::test]
async fn the_lists_refuse_a_configuration_that_cannot_be_used_as_a_save_does() {
    for (error, code, args, detail) in unusable_configurations() {
        let catalog = ConfigCatalog {
            config: Arc::new(Broken(error.clone())),
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
        let expected = CatalogError::Unavailable {
            code,
            args,
            detail: detail.map(str::to_owned),
        };
        assert_eq!(
            catalog.plans().await.map(|_| ()),
            Err(expected.clone()),
            "{error:?}"
        );
        assert_eq!(
            catalog.repositories().await.map(|_| ()),
            Err(expected),
            "{error:?}"
        );
    }
}

/// A listing of a repository's snapshots and the start of a backup read
/// the configuration first, and refuse one they can't use as a save does
/// (#347): the engine is never asked.
#[tokio::test]
async fn a_listing_and_a_backup_refuse_a_configuration_that_cannot_be_used_as_a_save_does() {
    for (error, code, args, detail) in unusable_configurations() {
        let (scripted, config) = (engine(), Arc::new(Broken(error.clone())));
        let catalog = ConfigCatalog {
            config: config.clone(),
            engine: scripted.clone(),
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
        let runner = PlanRunner {
            config,
            resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
            agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
            engine: scripted.clone(),
            locations: None,
            retention: Arc::new(engine::RusticRetention),
            store: Arc::new(app::fakes::InMemoryOperationStore::new()),
            registry: Arc::new(CancelRegistry::new()),
            hooks: Arc::new(ProcessHookRunner::new()),
            events: Arc::new(BroadcastEventBus::new()),
            queue: Arc::new(RunQueue::new()),
            maintenance_queue: Arc::new(app::queue::MaintenanceQueue::new()),
            ends: app::retry::run_ends().0,
            runs: crate::composition::runs::Runs::default(),
            readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
            full_disk_access: None,
            machine: Arc::default(),
            leases: None,
            running: Arc::default(),
            left_out: KeyFilesLeftOut::default(),
            writes: Arc::default(),
        };
        let detail = detail.map(str::to_owned);
        assert_eq!(
            catalog.snapshots("demo".to_owned()).await.map(|_| ()),
            Err(SnapshotsError::Unavailable {
                code,
                args: args.clone(),
                detail: detail.clone(),
            }),
            "{error:?}"
        );
        assert_eq!(
            runner
                .start_backup("home".to_owned(), Trigger::Manual)
                .await,
            Err(StartError::Unavailable { code, args, detail }),
            "{error:?}"
        );
        assert_eq!(jobs(&scripted), 0, "{error:?}");
    }
}

/// A scheduled run over a configuration the daemon can't use doesn't
/// start: nothing is queued and the engine is never asked, and the log
/// names the plan and the configuration's code (#347), never the store's
/// own words, which may quote a line of the file (I4).
#[tokio::test]
async fn a_scheduled_run_over_a_configuration_that_cannot_be_used_queues_nothing_and_logs_its_code()
{
    capture();
    let plan = domain::PlanId::try_from("home".to_owned()).expect("a plan ID");
    for (error, code, _, detail) in unusable_configurations() {
        let scripted = engine();
        let store = Arc::new(app::fakes::InMemoryOperationStore::new());
        let runner = PlanRunner {
            config: Arc::new(Broken(error.clone())),
            resolve: Arc::new(ResolveRepository::new(Arc::new(SystemSecrets::default()))),
            agent_id: AgentId::from_uuid(Uuid::from_u128(1)),
            engine: scripted.clone(),
            locations: None,
            retention: Arc::new(engine::RusticRetention),
            store: store.clone(),
            registry: Arc::new(CancelRegistry::new()),
            hooks: Arc::new(ProcessHookRunner::new()),
            events: Arc::new(BroadcastEventBus::new()),
            queue: Arc::new(RunQueue::new()),
            maintenance_queue: Arc::new(app::queue::MaintenanceQueue::new()),
            ends: app::retry::run_ends().0,
            runs: crate::composition::runs::Runs::default(),
            readings: Arc::new(app::fakes::InMemoryStatsStore::new()),
            full_disk_access: None,
            machine: Arc::default(),
            leases: None,
            running: Arc::default(),
            left_out: KeyFilesLeftOut::default(),
            writes: Arc::default(),
        };

        let place = runner
            .queue
            .push(&plan, domain::planning::Concurrency::default());
        PlanRuns::start(&runner, &plan, Trigger::Schedule, place, None).await;

        assert_eq!(jobs(&scripted), 0, "{error:?}");
        let history = store
            .list(&app::ports::HistoryQuery::default())
            .await
            .expect("the log reads");
        assert!(history.operations.is_empty(), "{error:?}: {history:?}");
        let said = captured_log::lines();
        // The code as the page and the API name it, not its Rust name.
        let wire = serde_json::to_value(code).expect("a code is JSON");
        let wire = wire.as_str().expect("a code is a string");
        let line = said
            .iter()
            .find(|(level, line)| {
                *level == log::Level::Warn
                    && line.contains("the scheduled run of plan home didn't start")
                    && line.contains(&format!("({wire})"))
            })
            .map(|(_, line)| line);
        let line = line.unwrap_or_else(|| panic!("{error:?}: no warning names {code:?}: {said:?}"));
        if let Some(detail) = detail {
            assert!(
                !line.contains(detail),
                "{error:?}: the log quotes the store: {line}"
            );
        }
    }
}
