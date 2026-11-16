use super::support::*;
use super::*;

/// An engine for a repository that holds no snapshots, as [`Empty`], that
/// also tells which repository ID each job names as the one it must open.
/// Like the real engine, it identifies a repository as the ID the job
/// named, when the job names one.
struct Named(tokio::sync::mpsc::UnboundedSender<(String, Option<domain::RepositoryId>)>);

#[async_trait]
impl Engine for Named {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let named = job.target().repository;
        let _ = self.0.send((format!("{:?}", job.kind()), named.clone()));
        match (&job, named) {
            (Job::Identify(_), Some(id)) => Ok(RunningJob {
                events: futures::stream::iter([EngineEvent::Succeeded(EngineResult::Repository {
                    id,
                })])
                .boxed(),
                control: Box::new(Parked),
            }),
            _ => Empty.run(op, job).await,
        }
    }
}

/// A repository ID the configuration recorded goes with every job that
/// opens the repository: the listing, the reads behind the history view
/// and a backup. The engine refuses a location whose repository has
/// another ID (§7.3), so a location that now holds another repository is
/// never read or written under the entry's name.
#[tokio::test]
async fn every_job_names_the_repository_id_the_configuration_recorded() {
    let (mut runner, _entered, open_gate, scratch) = runner_at_a_gate().await;
    let (seen, mut jobs) = tokio::sync::mpsc::unbounded_channel();
    runner.engine = Arc::new(Named(seen));
    let recorded = "07".repeat(32);
    let location = scratch.path().join("repo").display().to_string();
    let source = scratch.path().join("source").display().to_string();
    let (config_dir, _) = own_folders(scratch.path());
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "schema = 1\n\n\
                 [repositories.demo]\nrepository = {location:?}\n\
                 repository-id = {recorded:?}\npassword = \"demo-password\"\n\n\
                 [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [{source:?}]\n"
        ),
    )
    .expect("the config file");
    let expected = Some(domain::RepositoryId::parse(&recorded).expect("a repository ID"));
    let catalog = ConfigCatalog {
        config: Arc::clone(&runner.config),
        engine: Arc::clone(&runner.engine),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    catalog
        .snapshots("demo".to_owned())
        .await
        .expect("the listing");
    runner.groups("demo".to_owned()).await.expect("the groups");
    open_gate.send(true).expect("the gate opens");
    runner
        .start_backup("home".to_owned(), Trigger::Manual)
        .await
        .expect("the plan starts");
    let mut kinds = Vec::new();
    while !kinds.iter().any(|kind| kind == "Backup") {
        let (kind, id) = tokio::time::timeout(std::time::Duration::from_secs(5), jobs.recv())
            .await
            .expect("the engine is asked")
            .expect("a job");
        assert_eq!(id, expected, "the {kind} job");
        kinds.push(kind);
    }
    assert!(kinds.contains(&"Ls".to_owned()), "the jobs: {kinds:?}");
}

/// An engine for a repository that holds no snapshots, as [`Empty`], that
/// also tells which configuration entry each job was made from.
struct Saved(tokio::sync::mpsc::UnboundedSender<(String, Option<app::ports::SavedEntry>)>);

#[async_trait]
impl Engine for Saved {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let entry = job.access().and_then(|access| access.entry.clone());
        let _ = self.0.send((format!("{:?}", job.kind()), entry));
        Empty.run(op, job).await
    }
}

/// Every job that opens a saved repository carries the entry it was made
/// from, as it was read: the listing, the reads behind the history view
/// and a backup. The daemon records in that entry the ID of the
/// repository the job opened, when it records none (§6.1, decision 57).
#[tokio::test]
async fn every_job_carries_the_entry_it_was_made_from() {
    let (mut runner, _entered, open_gate, scratch) = runner_at_a_gate().await;
    let (seen, mut jobs) = tokio::sync::mpsc::unbounded_channel();
    runner.engine = Arc::new(Saved(seen));
    let location = scratch.path().join("repo").display().to_string();
    let source = scratch.path().join("source").display().to_string();
    let (config_dir, _) = own_folders(scratch.path());
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "schema = 1\n\n\
                 [repositories.demo]\nrepository = {location:?}\npassword = \"demo-password\"\n\
                 maintenance = \"local\"\n\n\
                 [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [{source:?}]\n"
        ),
    )
    .expect("the config file");
    let alias = RepositoryAlias::parse("demo").expect("an alias");
    let expected = Some(app::ports::SavedEntry {
        entry: runner
            .config
            .load()
            .await
            .expect("the configuration")
            .repository(&alias)
            .cloned()
            .expect("the entry"),
        alias,
    });
    let catalog = ConfigCatalog {
        config: Arc::clone(&runner.config),
        engine: Arc::clone(&runner.engine),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    catalog
        .snapshots("demo".to_owned())
        .await
        .expect("the listing");
    runner.groups("demo".to_owned()).await.expect("the groups");
    open_gate.send(true).expect("the gate opens");
    runner
        .start_backup("home".to_owned(), Trigger::Manual)
        .await
        .expect("the plan starts");
    let mut kinds = Vec::new();
    while !kinds.iter().any(|kind| kind == "Backup") {
        let (kind, entry) = tokio::time::timeout(std::time::Duration::from_secs(5), jobs.recv())
            .await
            .expect("the engine is asked")
            .expect("a job");
        assert_eq!(entry, expected, "the {kind} job");
        kinds.push(kind);
    }
    assert!(kinds.contains(&"Ls".to_owned()), "the jobs: {kinds:?}");
}

/// An engine for a repository that holds no snapshots, whose every job
/// opens the repository `.0` and says so when it starts.
struct Opens(domain::RepositoryId);

#[async_trait]
impl Engine for Opens {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let result = match job {
            Job::Identify(_) => EngineResult::Repository { id: self.0.clone() },
            _ => EngineResult::Snapshots(Vec::new()),
        };
        Ok(RunningJob {
            events: futures::stream::iter([
                EngineEvent::Started {
                    index_read_at: domain::Timestamp::from_second(1_800_000_000).expect("a time"),
                    repository: Some(self.0.clone()),
                },
                EngineEvent::Succeeded(result),
            ])
            .boxed(),
            control: Box::new(Parked),
        })
    }
}

/// The daemon's engine records, through the configuration file, the ID
/// of the repository a job opened in the entry the job was made from:
/// the file gains that one line, and every other byte is as written
/// (§6.1, decision 57).
#[tokio::test]
async fn a_job_that_opens_an_entry_without_an_id_records_it_in_the_file() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let (config_dir, _) = own_folders(scratch.path());
    let file = config_dir.join("config.toml");
    let location = scratch.path().join("repo").display().to_string();
    let source = scratch.path().join("source").display().to_string();
    let entry = format!(
        "schema = 1\n\n\
             [repositories.demo]\nrepository = {location:?}\npassword = \"demo-password\"\n\
             maintenance = \"local\"\n"
    );
    let plans =
        format!("\n[[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [{source:?}]\n");
    std::fs::write(&file, format!("{entry}{plans}")).expect("the config file");
    let id = "07".repeat(32);
    let config: Arc<dyn ConfigStore> =
        Arc::new(FileConfigStore::new(file.clone(), Arc::new(SystemClock)));
    let told = Arc::new(std::sync::Mutex::new(Vec::new()));
    let report = {
        let told = Arc::clone(&told);
        move |recording: Recording| {
            told.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(recording);
        }
    };
    let catalog = ConfigCatalog {
        config: Arc::clone(&config),
        engine: Arc::new(app::repositories::RecordOpened::new(
            Arc::new(Opens(
                domain::RepositoryId::parse(&id).expect("a repository ID"),
            )),
            config,
            Arc::new(app::repositories::ConfigWrites::default()),
            Arc::new(report),
        )),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    catalog
        .snapshots("demo".to_owned())
        .await
        .expect("the listing");
    assert_eq!(
        std::fs::read_to_string(&file).expect("the config file"),
        format!("{entry}repository-id = {id:?}\n{plans}")
    );
    assert_eq!(
        *told
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![Recording::Recorded {
            alias: RepositoryAlias::parse("demo").expect("an alias"),
            id: domain::RepositoryId::parse(&id).expect("a repository ID"),
        }]
    );
}

/// A start that carries a selection is held to it (I6): what a forget
/// would remove is read again, and a repository that no longer matches
/// what the review named refuses before anything is removed.
#[tokio::test]
async fn a_forget_held_to_a_selection_the_repository_no_longer_matches_is_refused() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(Empty);
    let reviewed = runner
        .preview("demo".to_owned(), http_adapter::MaintenanceRequest::Forget)
        .await
        .expect("a preview of the empty repository");
    let api::MaintenancePreviewDto::Forget { selection, .. } = reviewed else {
        panic!("a forget is previewed as a forget");
    };
    let other = if selection == "0123456789abcdef" {
        "fedcba9876543210"
    } else {
        "0123456789abcdef"
    };
    assert_eq!(
        runner
            .start_maintenance(
                "demo".to_owned(),
                http_adapter::MaintenanceRequest::Forget,
                Some(other.to_owned()),
            )
            .await,
        Err(MaintenanceRefusal::SelectionChanged)
    );
}
