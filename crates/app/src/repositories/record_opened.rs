//! Recording the ID of a saved repository the first time a job opens it
//! (§6.1, decision 57). An entry written by hand or imported from a profile
//! records no repository ID, since Ladderback never opened it; the first job
//! that opens it, of whatever kind, has the daemon record the ID it read.
//! The engine is where every job's open reports back, so the record is made
//! there, once, for every kind of job.

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::planning::NotRecorded;
use domain::{OperationId, RepositoryAlias, RepositoryId};
use futures::StreamExt;
use futures::future::{self, Either};

use crate::ports::{
    ConfigError, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl,
    NotInPlace, RecordError, RepositoryAccess, RunningJob, SavedEntry,
};

/// The daemon's configuration write lock: held across every read-modify-write
/// of the configuration the daemon makes, so two of them can't undo each
/// other.
pub type ConfigWrites = futures::lock::Mutex<()>;

/// What became of the ID of a repository a job opened, whose entry recorded
/// none when the job was made. Nothing here is a secret: an alias and a
/// repository ID name a repository, they don't open it (I4).
#[derive(Clone, Debug, PartialEq)]
pub enum Recording {
    /// The ID is recorded in the entry.
    Recorded {
        alias: RepositoryAlias,
        id: RepositoryId,
    },
    /// The configuration says not to record it; the next open tries again.
    NotRecorded {
        alias: RepositoryAlias,
        id: RepositoryId,
        why: NotRecorded,
    },
    /// The store couldn't add the ID without changing something else in
    /// the configuration, so it wrote nothing; the next open tries again.
    NotInPlace {
        alias: RepositoryAlias,
        id: RepositoryId,
        why: NotInPlace,
    },
    /// Another write to the configuration was under way, a connection that
    /// waits for its repository's answer among them, so nothing was read or
    /// written; the next open tries again. A job never waits for the lock.
    Busy {
        alias: RepositoryAlias,
        id: RepositoryId,
    },
    /// The configuration couldn't be read or written, a read-only file among
    /// them; the next open tries again.
    Failed {
        alias: RepositoryAlias,
        id: RepositoryId,
        error: ConfigError,
    },
}

impl Recording {
    /// Whether nothing but a person's edit of the file changes it: the next
    /// open meets it again, so it is told once a run, not at every job.
    fn lasts(&self) -> bool {
        matches!(
            self,
            Self::NotRecorded {
                why: NotRecorded::SameRepository { .. },
                ..
            } | Self::NotInPlace {
                why: NotInPlace::Form | NotInPlace::ReadBack | NotInPlace::TooLarge,
                ..
            }
        )
    }
}

/// An [`Engine`] that records, in the entry a job was made from, the ID of
/// the repository the job opened, when the entry records none. It does so
/// when the job reports its start, which names the repository it opened, and
/// passes every event on as the engine reported it: a job that opened the
/// repository and then failed has opened it all the same. A record that
/// can't be made changes nothing about the job.
pub struct RecordOpened {
    inner: Arc<dyn Engine>,
    recorder: Arc<Recorder>,
}

/// What every job's record goes through.
struct Recorder {
    config: Arc<dyn ConfigStore>,
    writes: Arc<ConfigWrites>,
    report: Arc<dyn Fn(Recording) + Send + Sync>,
    /// The lasting outcomes told so far.
    told: Mutex<Vec<Recording>>,
}

impl RecordOpened {
    /// Records through `config`, holding `writes`, the lock every other
    /// write of the daemon's configuration holds, and tells `report` what
    /// became of each ID it was given: an ID an earlier job recorded is not
    /// told, and an outcome only a person's edit changes is told once.
    pub fn new(
        inner: Arc<dyn Engine>,
        config: Arc<dyn ConfigStore>,
        writes: Arc<ConfigWrites>,
        report: Arc<dyn Fn(Recording) + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            recorder: Arc::new(Recorder {
                config,
                writes,
                report,
                told: Mutex::new(Vec::new()),
            }),
        }
    }
}

#[async_trait]
impl Engine for RecordOpened {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        // An access names the ID its entry records (`resolve_saved`), so a
        // job that names none was made from an entry that records none. One
        // that names an ID its entry doesn't record was made by an operation
        // that identified the repository first, and that job recorded it.
        let unrecorded = job
            .access()
            .filter(|access| access.id.is_none())
            .and_then(|access| access.entry.clone().map(|saved| (access.clone(), saved)));
        let Some((access, saved)) = unrecorded else {
            return self.inner.run(op, job).await;
        };
        match job {
            // An identify is how the ID is found, and an initialisation
            // makes the repository: each records the ID it opened.
            Job::Identify(_) | Job::Init(_) => self.recording(op, job, saved).await,
            _ => self.identified_first(op, job, access, saved).await,
        }
    }
}

impl RecordOpened {
    /// Runs `job` and records, when it starts, the ID of the repository it
    /// opened in `saved`'s entry. Every event passes on as it came.
    async fn recording(
        &self,
        op: OperationId,
        job: Job,
        saved: SavedEntry,
    ) -> Result<RunningJob, EngineError> {
        let running = self.inner.run(op, job).await?;
        let recorder = Arc::clone(&self.recorder);
        let mut pending = Some(saved);
        let events = running
            .events
            .then(move |event| {
                let opened = match &event {
                    EngineEvent::Started {
                        repository: Some(id),
                        ..
                    } => pending.take().map(|saved| (saved, id.clone())),
                    _ => None,
                };
                match opened {
                    None => Either::Left(future::ready(event)),
                    Some((saved, id)) => {
                        let recorder = Arc::clone(&recorder);
                        Either::Right(async move {
                            recorder.tell(recorder.record(saved, id).await);
                            event
                        })
                    }
                }
            })
            .boxed();
        Ok(RunningJob {
            events,
            control: running.control,
        })
    }

    /// Runs `job`, whose entry records no ID, after an identify of `access`
    /// that records the ID (#421): without it the job would run with no
    /// cache, which the engine keeps by the ID, and its operation would name
    /// no repository. The job's events are its own, none of the identify's.
    /// An identify that fails ends the job with its reason, the one the
    /// job's own open would have given, and the job never runs; one the
    /// engine can't start refuses the job as it would refuse it. Stopping
    /// the job stops whichever of the two runs, and never waits for the
    /// job's start: a job that starts after it was stopped is stopped
    /// before anything it reports is passed on.
    async fn identified_first(
        &self,
        op: OperationId,
        mut job: Job,
        access: RepositoryAccess,
        saved: SavedEntry,
    ) -> Result<RunningJob, EngineError> {
        let identify = self.recording(op, Job::Identify(access), saved).await?;
        let phase = Arc::new(Mutex::new(Phase::Identifying(identify.control)));
        let inner = Arc::clone(&self.inner);
        let handed = Arc::clone(&phase);
        let found = identify.events;
        let events = futures::stream::once(async move {
            let found = identified(found).await;
            let id = {
                let mut phase = handed.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(*phase, Phase::Stopped) {
                    return futures::stream::empty().boxed();
                }
                match found {
                    Ok(id) => {
                        *phase = Phase::Starting;
                        id
                    }
                    Err(reason) => {
                        return futures::stream::iter([EngineEvent::Failed(reason)]).boxed();
                    }
                }
            };
            if let Some(access) = job.access_mut() {
                access.id = Some(id);
            }
            // The phase isn't locked while the job starts: a stop that
            // comes meanwhile returns at once, as nothing may poll this
            // stream again to finish the start.
            let RunningJob { events, control } = match inner.run(op, job).await {
                Ok(running) => running,
                Err(error) => {
                    // A job stopped while it started ends with nothing more.
                    if matches!(
                        *handed.lock().unwrap_or_else(PoisonError::into_inner),
                        Phase::Stopped
                    ) {
                        return futures::stream::empty().boxed();
                    }
                    return futures::stream::iter([EngineEvent::Failed(FailureReason::from(
                        error,
                    ))])
                    .boxed();
                }
            };
            let unwanted = {
                let mut phase = handed.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(*phase, Phase::Stopped) {
                    Some(control)
                } else {
                    *phase = Phase::Running(control);
                    None
                }
            };
            match unwanted {
                Some(mut control) => {
                    control.stop().await;
                    futures::stream::empty().boxed()
                }
                None => events,
            }
        })
        .flatten()
        .boxed();
        Ok(RunningJob {
            events,
            control: Box::new(Handover(phase)),
        })
    }
}

/// Which job of an identified-first run is going. It is never locked across
/// an await, so a stop never waits for the job's stream.
enum Phase {
    Identifying(Box<dyn JobControl>),
    /// The identify found the ID and the engine is starting the job.
    Starting,
    Running(Box<dyn JobControl>),
    Stopped,
}

/// Stops an identified-first run: the identify while it runs, the job once
/// it does, and nothing is started after.
struct Handover(Arc<Mutex<Phase>>);

#[async_trait]
impl JobControl for Handover {
    async fn stop(&mut self) {
        let going = std::mem::replace(
            &mut *self.0.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Stopped,
        );
        if let Phase::Identifying(mut control) | Phase::Running(mut control) = going {
            control.stop().await;
        }
    }
}

/// The ID an identify found, or why it found none.
async fn identified(
    mut events: futures::stream::BoxStream<'static, EngineEvent>,
) -> Result<RepositoryId, FailureReason> {
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Succeeded(EngineResult::Repository { id }) => return Ok(id),
            EngineEvent::Failed(reason) => return Err(reason),
            EngineEvent::Succeeded(_) => break,
            EngineEvent::Started { .. } | EngineEvent::Progress(_) | EngineEvent::Warning(_) => {}
        }
    }
    Err(FailureReason::EngineExited { exit_code: None })
}

impl Recorder {
    /// Records `id` in `saved`'s entry, unless another write of the
    /// configuration holds `writes`: a job never waits for one.
    async fn record(&self, saved: SavedEntry, id: RepositoryId) -> Recording {
        let SavedEntry { alias, entry } = saved;
        let Some(_writing) = self.writes.try_lock() else {
            return Recording::Busy { alias, id };
        };
        match self.config.record_id(&alias, &entry, &id).await {
            Ok(()) => Recording::Recorded { alias, id },
            Err(RecordError::NotRecorded(why)) => Recording::NotRecorded { alias, id, why },
            Err(RecordError::NotInPlace(why)) => Recording::NotInPlace { alias, id, why },
            Err(RecordError::Store(error)) => Recording::Failed { alias, id, error },
        }
    }

    /// Tells `report` about `recording`, unless an earlier job recorded the
    /// ID, or it lasts and was told already.
    fn tell(&self, recording: Recording) {
        if let Recording::NotRecorded {
            why: NotRecorded::Recorded,
            ..
        } = recording
        {
            return;
        }
        if recording.lasts() {
            let mut told = self.told.lock().unwrap_or_else(PoisonError::into_inner);
            if told.contains(&recording) {
                return;
            }
            told.push(recording.clone());
        }
        (self.report)(recording);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use async_trait::async_trait;
    use domain::execution::deadline::KeepDelete;
    use domain::execution::plan_tags::PlanTags;
    use domain::execution::{FailureReason, OperationKind, OperationStatus, Trigger};
    use domain::planning::{
        AgentConfig, Concurrency, Maintenance, NotRecorded, RepositoryConfig, RepositorySettings,
    };
    use domain::secrets::{SecretRef, SecretValue};
    use domain::{
        AgentId, OperationId, PlanId, RepositoryAlias, RepositoryId, SignedDuration, Timestamp,
        Uuid,
    };
    use futures::stream::BoxStream;
    use futures::{FutureExt, StreamExt};

    use super::{ConfigWrites, RecordOpened, Recording};
    use crate::execution::RunOperation;
    use crate::fakes::{FakeClock, InMemoryConfigStore, InMemoryOperationStore};
    use crate::ports::{
        BackupJob, CheckJob, ConfigError, ConfigStore, Engine, EngineError, EngineEvent,
        EngineResult, ForgetJob, Job, JobControl, NotInPlace, PruneJob, RecordError,
        RepositoryAccess, RewriteJob, RunningJob, SavedEntry,
    };
    use crate::queue::RunQueue;

    struct Stop;

    #[async_trait]
    impl JobControl for Stop {
        async fn stop(&mut self) {}
    }

    /// An engine that plays `events` for every job.
    struct Plays(Vec<EngineEvent>);

    #[async_trait]
    impl Engine for Plays {
        async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
            Ok(RunningJob {
                events: futures::stream::iter(self.0.clone()).boxed(),
                control: Box::new(Stop),
            })
        }
    }

    /// The in-memory store, counting the records asked of it, refusing them
    /// when told to.
    #[derive(Default)]
    struct Counted {
        store: InMemoryConfigStore,
        saves: Mutex<usize>,
        read_only: bool,
        not_in_place: Option<NotInPlace>,
    }

    #[async_trait]
    impl ConfigStore for Counted {
        async fn load(&self) -> Result<AgentConfig, ConfigError> {
            self.store.load().await
        }

        async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
            *self.saves.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            self.store.save(config).await
        }

        async fn record_id(
            &self,
            alias: &RepositoryAlias,
            read: &RepositoryConfig,
            id: &RepositoryId,
        ) -> Result<(), RecordError> {
            *self.saves.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            if self.read_only {
                return Err(RecordError::Store(read_only()));
            }
            if let Some(why) = self.not_in_place {
                return Err(RecordError::NotInPlace(why));
            }
            self.store.record_id(alias, read, id).await
        }
    }

    fn read_only() -> ConfigError {
        ConfigError::Unavailable {
            file: "config.toml".to_owned(),
            detail: "Read-only file system".to_owned(),
        }
    }

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).expect("an alias")
    }

    fn opened() -> RepositoryId {
        RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
    }

    fn other() -> RepositoryId {
        RepositoryId::parse(&"08".repeat(32)).expect("a repository ID")
    }

    fn op() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(0x7ec0))
    }

    /// A hot/cold entry at `cold`, recording `id`.
    fn entry(cold: &str, id: Option<RepositoryId>) -> RepositoryConfig {
        RepositoryConfig {
            settings: RepositorySettings {
                repository: cold.to_owned(),
                repo_hot: Some("/srv/demo/hot".to_owned()),
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id,
            maintenance: Maintenance::None,
            group_by: Default::default(),
            hooks: Default::default(),
        }
    }

    async fn holding(entries: Vec<(&str, RepositoryConfig)>) -> Arc<Counted> {
        let store = Arc::new(Counted::default());
        save(&store.store, entries).await;
        store
    }

    async fn save(store: &dyn ConfigStore, entries: Vec<(&str, RepositoryConfig)>) {
        let config = AgentConfig::new(
            Default::default(),
            Default::default(),
            entries
                .into_iter()
                .map(|(name, entry)| (alias(name), entry))
                .collect(),
            Vec::new(),
        )
        .expect("a valid configuration");
        store.save(&config).await.expect("the configuration saves");
    }

    async fn entry_of(store: &Counted, name: &str) -> RepositoryConfig {
        store
            .load()
            .await
            .expect("the configuration loads")
            .repository(&alias(name))
            .cloned()
            .expect("the entry")
    }

    /// The access a job made from the entry `name` of `store` names, as the
    /// use cases make it.
    async fn access_of(store: &Counted, name: &str) -> RepositoryAccess {
        let entry = entry_of(store, name).await;
        RepositoryAccess {
            repository: entry.settings.repository.clone(),
            repo_hot: entry.settings.repo_hot.clone(),
            password: SecretValue::new("demo-password"),
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: entry.id.clone(),
            entry: Some(SavedEntry {
                alias: alias(name),
                entry,
            }),
        }
    }

    fn started(repository: Option<RepositoryId>) -> EngineEvent {
        EngineEvent::Started {
            index_read_at: Timestamp::from_second(1_800_000_000).expect("a time"),
            repository,
        }
    }

    /// A success, as an identify reports it: the ID of the repository it
    /// opened. A job that runs after an identify takes its ID from it.
    fn succeeded() -> EngineEvent {
        EngineEvent::Succeeded(EngineResult::Repository { id: opened() })
    }

    /// Every kind of job that opens a repository, over `access`.
    fn every_kind(access: &RepositoryAccess) -> Vec<Job> {
        let tags = PlanTags::new(
            PlanId::parse("demo-home").expect("a plan ID"),
            AgentId::from_uuid(Uuid::from_u128(1)),
        );
        let prune = PruneJob {
            repository: access.clone(),
            keep_delete: KeepDelete::MIN,
        };
        vec![
            Job::Identify(access.clone()),
            Job::Ls(access.clone()),
            Job::Backup(BackupJob {
                repository: access.clone(),
                sources: vec!["/home/demo".to_owned()],
                tags: tags.clone(),
                extra_tags: Vec::new(),
                label: None,
                host: None,
                filter: Default::default(),
                deadline_budget: SignedDuration::from_hours(1),
            }),
            Job::Check(CheckJob {
                repository: access.clone(),
                read_data: false,
            }),
            Job::Forget(ForgetJob {
                repository: access.clone(),
                ids: Vec::new(),
            }),
            Job::Rewrite(RewriteJob {
                repository: access.clone(),
                ids: Vec::new(),
                tags,
            }),
            Job::PrunePlan(prune.clone()),
            Job::Prune(prune),
        ]
    }

    /// Wraps an engine playing `events` over `store`, reporting into the
    /// returned list.
    fn recording(
        store: &Arc<Counted>,
        writes: &Arc<ConfigWrites>,
        events: Vec<EngineEvent>,
    ) -> (RecordOpened, Arc<Mutex<Vec<Recording>>>) {
        let reports = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&reports);
        let engine = RecordOpened::new(
            Arc::new(Plays(events)),
            store.clone(),
            Arc::clone(writes),
            Arc::new(move |recording| {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(recording);
            }),
        );
        (engine, reports)
    }

    /// Runs `job` to its end, and gives what it reported.
    async fn run(engine: &RecordOpened, job: Job) -> Vec<EngineEvent> {
        engine
            .run(op(), job)
            .await
            .expect("the engine takes the job")
            .events
            .collect()
            .await
    }

    fn taken(reports: &Mutex<Vec<Recording>>) -> Vec<Recording> {
        std::mem::take(&mut *reports.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn saves(store: &Counted) -> usize {
        *store.saves.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An identify and an initialisation are the jobs that record the ID
    /// they opened: every other job of an entry that records no ID runs
    /// after an identify, which records it.
    #[tokio::test]
    async fn an_identify_or_an_initialisation_records_the_id_of_the_repository_it_opened() {
        let writes = Arc::new(ConfigWrites::new(()));
        let events = vec![started(Some(opened())), succeeded()];
        let unrecorded = entry("/srv/demo/cold", None);
        let template = holding(vec![("demo", unrecorded.clone())]).await;
        let access = access_of(&template, "demo").await;
        for job in [Job::Identify(access.clone()), Job::Init(access)] {
            let kind = job.kind();
            let store = holding(vec![("demo", unrecorded.clone())]).await;
            let (engine, reports) = recording(&store, &writes, events.clone());
            assert_eq!(
                run(&engine, job).await,
                events,
                "{kind:?}: the events pass on"
            );
            let mut recorded = unrecorded.clone();
            recorded.id = Some(opened());
            assert_eq!(entry_of(&store, "demo").await, recorded, "{kind:?}");
            assert_eq!(
                taken(&reports),
                [Recording::Recorded {
                    alias: alias("demo"),
                    id: opened(),
                }],
                "{kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_identify_or_an_initialisation_that_failed_after_it_opened_the_repository_records_it()
     {
        let writes = Arc::new(ConfigWrites::new(()));
        let failed = EngineEvent::Failed(FailureReason::EngineFailed {
            detail: "the repository could not be read".to_owned(),
        });
        let template = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&template, "demo").await;
        for job in [Job::Identify(access.clone()), Job::Init(access)] {
            let kind = job.kind();
            let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
            let (engine, _) = recording(
                &store,
                &writes,
                vec![started(Some(opened())), failed.clone()],
            );
            run(&engine, job).await;
            assert_eq!(
                entry_of(&store, "demo").await.id,
                Some(opened()),
                "{kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_entry_that_records_an_id_is_never_written() {
        let writes = Arc::new(ConfigWrites::new(()));
        for reported in [opened(), other()] {
            let store = holding(vec![("demo", entry("/srv/demo/cold", Some(opened())))]).await;
            let (engine, reports) =
                recording(&store, &writes, vec![started(Some(reported)), succeeded()]);
            run(&engine, Job::Ls(access_of(&store, "demo").await)).await;
            assert_eq!(saves(&store), 0);
            assert_eq!(entry_of(&store, "demo").await.id, Some(opened()));
            assert_eq!(taken(&reports), []);
        }
    }

    /// A maintenance operation that identifies the repository first records
    /// its ID from that job; the jobs it runs after it name the ID, over the
    /// entry as it was read, and don't record it again.
    #[tokio::test]
    async fn a_job_that_names_the_id_its_operation_identified_records_nothing() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let mut identified = access_of(&store, "demo").await;
        identified.id = Some(opened());
        let (engine, reports) =
            recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(&engine, Job::Ls(identified)).await;
        assert_eq!(saves(&store), 0);
        assert_eq!(entry_of(&store, "demo").await.id, None);
        assert_eq!(taken(&reports), []);
    }

    #[tokio::test]
    async fn an_entry_edited_after_the_job_read_it_records_nothing_until_the_next_open() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let read = access_of(&store, "demo").await;
        let mut edited = entry("/srv/demo/cold", None);
        edited.maintenance = Maintenance::Local;
        save(&store.store, vec![("demo", edited.clone())]).await;
        let (engine, reports) =
            recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(&engine, Job::Ls(read)).await;
        assert_eq!(entry_of(&store, "demo").await, edited);
        assert_eq!(
            taken(&reports),
            [Recording::NotRecorded {
                alias: alias("demo"),
                id: opened(),
                why: NotRecorded::Changed,
            }]
        );
        run(&engine, Job::Ls(access_of(&store, "demo").await)).await;
        assert_eq!(entry_of(&store, "demo").await.id, Some(opened()));
    }

    #[tokio::test]
    async fn an_entry_moved_after_the_job_read_it_records_nothing() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let read = access_of(&store, "demo").await;
        let mut moved = entry("/srv/demo/cold", None);
        moved.settings.repo_hot = Some("/srv/elsewhere/hot".to_owned());
        save(&store.store, vec![("demo", moved.clone())]).await;
        let (engine, _) = recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(
            &engine,
            Job::Check(CheckJob {
                repository: read,
                read_data: false,
            }),
        )
        .await;
        assert_eq!(entry_of(&store, "demo").await, moved);
    }

    #[tokio::test]
    async fn an_id_another_entry_records_is_not_recorded_and_the_configuration_still_loads() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![
            ("demo", entry("/srv/demo/cold", None)),
            ("mounted", entry("/Volumes/demo/cold", None)),
        ])
        .await;
        let (engine, reports) =
            recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(&engine, Job::Ls(access_of(&store, "demo").await)).await;
        taken(&reports);
        run(&engine, Job::Ls(access_of(&store, "mounted").await)).await;
        assert_eq!(
            taken(&reports),
            [Recording::NotRecorded {
                alias: alias("mounted"),
                id: opened(),
                why: NotRecorded::SameRepository {
                    other: alias("demo")
                },
            }]
        );
        assert_eq!(entry_of(&store, "mounted").await.id, None);
        assert_eq!(entry_of(&store, "demo").await.id, Some(opened()));
        run(&engine, Job::Ls(access_of(&store, "mounted").await)).await;
        assert_eq!(taken(&reports), [], "told once, at the first open");
    }

    /// The store can't add the ID alone until a person edits the file, so
    /// every open meets the same refusal: it is told once for each entry,
    /// and every open still tries.
    #[tokio::test]
    async fn a_refusal_that_lasts_until_the_file_is_edited_is_told_once_for_each_entry() {
        let writes = Arc::new(ConfigWrites::new(()));
        for why in [NotInPlace::Form, NotInPlace::ReadBack, NotInPlace::TooLarge] {
            let store = Arc::new(Counted {
                not_in_place: Some(why),
                ..Counted::default()
            });
            save(
                &store.store,
                vec![
                    ("demo", entry("/srv/demo/cold", None)),
                    ("usb", entry("/media/usb/cold", None)),
                ],
            )
            .await;
            let (engine, reports) =
                recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
            for name in ["demo", "demo", "usb", "demo", "usb"] {
                run(&engine, Job::Ls(access_of(&store, name).await)).await;
            }
            assert_eq!(
                taken(&reports),
                ["demo", "usb"].map(|name| Recording::NotInPlace {
                    alias: alias(name),
                    id: opened(),
                    why,
                }),
                "{why:?}"
            );
            assert_eq!(saves(&store), 5, "{why:?}: every open tries again");
        }
    }

    /// Two jobs made from the entry before either recorded the ID: the
    /// second finds the ID it opened recorded already, and tells nothing.
    #[tokio::test]
    async fn a_job_that_finds_the_id_it_opened_recorded_already_tells_nothing() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let first = access_of(&store, "demo").await;
        let second = first.clone();
        let (engine, reports) =
            recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(&engine, Job::Ls(first)).await;
        assert_eq!(
            taken(&reports),
            [Recording::Recorded {
                alias: alias("demo"),
                id: opened(),
            }]
        );
        run(&engine, Job::Ls(second)).await;
        assert_eq!(taken(&reports), []);
        assert_eq!(entry_of(&store, "demo").await.id, Some(opened()));
    }

    #[tokio::test]
    async fn while_the_configuration_is_being_written_nothing_is_recorded_and_the_job_goes_on() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let events = vec![started(Some(opened())), succeeded()];
        let (engine, reports) = recording(&store, &writes, events.clone());
        let held = writes.lock().await;
        for _ in 0..2 {
            let played = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                run(&engine, Job::Ls(access_of(&store, "demo").await)),
            )
            .await
            .expect("the job never waits for the lock");
            assert_eq!(played, events);
        }
        assert_eq!(saves(&store), 0);
        assert_eq!(
            taken(&reports),
            [(); 2].map(|()| Recording::Busy {
                alias: alias("demo"),
                id: opened(),
            }),
            "told at every open: the lock is let go soon"
        );
        drop(held);
        run(&engine, Job::Ls(access_of(&store, "demo").await)).await;
        assert_eq!(entry_of(&store, "demo").await.id, Some(opened()));
    }

    #[tokio::test]
    async fn a_configuration_that_cannot_be_written_fails_the_record_not_the_job() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = Arc::new(Counted {
            read_only: true,
            ..Counted::default()
        });
        save(&store.store, vec![("demo", entry("/srv/demo/cold", None))]).await;
        let events = vec![started(Some(opened())), succeeded()];
        let (engine, reports) = recording(&store, &writes, events.clone());
        assert_eq!(
            run(&engine, Job::Ls(access_of(&store, "demo").await)).await,
            events
        );
        assert_eq!(
            taken(&reports),
            [Recording::Failed {
                alias: alias("demo"),
                id: opened(),
                error: read_only(),
            }]
        );
        assert!(
            writes.try_lock().is_some(),
            "the lock is let go after a failed write"
        );
    }

    #[tokio::test]
    async fn an_id_the_store_cannot_add_alone_is_not_recorded_and_the_job_goes_on() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = Arc::new(Counted {
            not_in_place: Some(NotInPlace::ReadBack),
            ..Counted::default()
        });
        save(&store.store, vec![("demo", entry("/srv/demo/cold", None))]).await;
        let events = vec![started(Some(opened())), succeeded()];
        let (engine, reports) = recording(&store, &writes, events.clone());
        assert_eq!(
            run(&engine, Job::Ls(access_of(&store, "demo").await)).await,
            events
        );
        assert_eq!(
            taken(&reports),
            [Recording::NotInPlace {
                alias: alias("demo"),
                id: opened(),
                why: NotInPlace::ReadBack,
            }]
        );
        assert_eq!(entry_of(&store, "demo").await.id, None);
        assert!(writes.try_lock().is_some(), "the lock is let go");
    }

    #[tokio::test]
    async fn a_job_no_entry_was_read_for_or_that_opened_nothing_records_nothing() {
        let writes = Arc::new(ConfigWrites::new(()));
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let mut unsaved = access_of(&store, "demo").await;
        unsaved.entry = None;
        let (engine, reports) =
            recording(&store, &writes, vec![started(Some(opened())), succeeded()]);
        run(&engine, Job::Identify(unsaved)).await;
        let (nothing_opened, _) = recording(&store, &writes, vec![started(None), succeeded()]);
        run(&nothing_opened, Job::Init(access_of(&store, "demo").await)).await;
        assert_eq!(saves(&store), 0);
        assert_eq!(taken(&reports), []);
    }

    /// What [`Keeps`] finds when it is asked to identify the repository.
    #[derive(Clone)]
    enum Finds {
        /// The repository [`opened`].
        Repository,
        /// Nothing: it fails with this reason before it starts, as a worker
        /// that can't open the repository does.
        Fails(FailureReason),
        /// It opens the repository and ends without a result.
        Nothing,
        /// It can't be started at all.
        Refused(EngineError),
        /// It opens the repository and runs until it is stopped.
        Hangs,
    }

    /// A job's control that ends its job's stream when stopped, and notes
    /// which kind of job it stopped. While it lives, so does its job's
    /// worker: dropping it kills the worker, as dropping a real job does.
    struct Halts {
        kind: OperationKind,
        halt: Option<futures::channel::oneshot::Sender<()>>,
        stopped: Arc<Mutex<Vec<OperationKind>>>,
        live: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl JobControl for Halts {
        async fn stop(&mut self) {
            if let Some(halt) = self.halt.take() {
                let _ = halt.send(());
                self.stopped
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(self.kind);
            }
        }
    }

    impl Drop for Halts {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// An engine that keeps the jobs it is given. An identify finds what
    /// `finds` says; every other job opens [`opened`] and succeeds, or, when
    /// `job_hangs`, runs until it is stopped. A job other than an identify
    /// is started only once the test lets it go, after [`Keeps::hold_start`],
    /// and refused when the test sends an error instead.
    struct Keeps {
        finds: Finds,
        job_hangs: bool,
        held: Mutex<Option<futures::channel::oneshot::Receiver<Result<(), EngineError>>>>,
        seen: Mutex<Vec<Job>>,
        stopped: Arc<Mutex<Vec<OperationKind>>>,
        live: Arc<AtomicUsize>,
    }

    impl Keeps {
        fn seen(&self) -> Vec<Job> {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        fn stopped(&self) -> Vec<OperationKind> {
            self.stopped
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        /// How many of the jobs it started still have a worker.
        fn live(&self) -> usize {
            self.live.load(Ordering::SeqCst)
        }

        /// Holds the start of the next job other than an identify until the
        /// returned sender sends, or is dropped. The start is refused with
        /// the error it sends.
        fn hold_start(&self) -> futures::channel::oneshot::Sender<Result<(), EngineError>> {
            let (go, held) = futures::channel::oneshot::channel();
            *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Some(held);
            go
        }
    }

    #[async_trait]
    impl Engine for Keeps {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            let kind = job.kind();
            let identify = matches!(job, Job::Identify(_));
            // The worker exists from the start of the job, so a start that
            // is dropped while it is held drops its worker too.
            let (halt, halted) = futures::channel::oneshot::channel::<()>();
            self.live.fetch_add(1, Ordering::SeqCst);
            let control = Box::new(Halts {
                kind,
                halt: Some(halt),
                stopped: Arc::clone(&self.stopped),
                live: Arc::clone(&self.live),
            });
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            if !identify {
                let held = self
                    .held
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(held) = held
                    && let Ok(Err(error)) = held.await
                {
                    return Err(error);
                }
            }
            let (events, hangs) = match (identify, &self.finds) {
                (true, Finds::Refused(error)) => return Err(error.clone()),
                (true, Finds::Repository) => (vec![started(Some(opened())), succeeded()], false),
                (true, Finds::Fails(reason)) => (vec![EngineEvent::Failed(reason.clone())], false),
                (true, Finds::Nothing) => (vec![started(Some(opened()))], false),
                (true, Finds::Hangs) => (vec![started(Some(opened()))], true),
                (false, _) if self.job_hangs => (vec![started(Some(opened()))], true),
                (false, _) => (
                    vec![
                        started(Some(opened())),
                        EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
                    ],
                    false,
                ),
            };
            let played = futures::stream::iter(events);
            let events = if hangs {
                played
                    .chain(futures::stream::once(halted).filter_map(|_| async { None }))
                    .boxed()
            } else {
                played.boxed()
            };
            Ok(RunningJob { events, control })
        }
    }

    fn keeping(
        store: &Arc<Counted>,
        finds: Finds,
        job_hangs: bool,
    ) -> (RecordOpened, Arc<Keeps>, Arc<Mutex<Vec<Recording>>>) {
        let keeps = Arc::new(Keeps {
            finds,
            job_hangs,
            held: Mutex::new(None),
            seen: Mutex::new(Vec::new()),
            stopped: Arc::new(Mutex::new(Vec::new())),
            live: Arc::new(AtomicUsize::new(0)),
        });
        let reports = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&reports);
        let engine = RecordOpened::new(
            Arc::clone(&keeps) as Arc<dyn Engine>,
            store.clone(),
            Arc::new(ConfigWrites::new(())),
            Arc::new(move |recording| {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(recording);
            }),
        );
        (engine, keeps, reports)
    }

    fn backup(access: &RepositoryAccess) -> Job {
        every_kind(access)
            .into_iter()
            .find(|job| matches!(job, Job::Backup(_)))
            .expect("a backup")
    }

    /// A job whose entry records no ID would open the repository without
    /// knowing it: with no cache, which the engine keeps by the ID, and in
    /// an operation that names no repository. It runs after an identify,
    /// with the ID the identify found, which is recorded from that identify.
    /// An identify, and an initialisation that makes the repository, run as
    /// they are.
    #[tokio::test]
    async fn a_job_of_an_entry_that_records_no_id_runs_after_an_identify_with_the_id_it_found() {
        let unrecorded = entry("/srv/demo/cold", None);
        let template = holding(vec![("demo", unrecorded.clone())]).await;
        let access = access_of(&template, "demo").await;
        let mut identified = access.clone();
        identified.id = Some(opened());
        let mut recorded = unrecorded.clone();
        recorded.id = Some(opened());
        for (job, named) in every_kind(&access)
            .into_iter()
            .zip(every_kind(&identified))
            .chain([(Job::Init(access.clone()), Job::Init(access.clone()))])
        {
            let kind = job.kind();
            let store = holding(vec![("demo", unrecorded.clone())]).await;
            let (engine, keeps, reports) = keeping(&store, Finds::Repository, false);
            let events = run(&engine, job.clone()).await;
            if matches!(job, Job::Identify(_) | Job::Init(_)) {
                assert_eq!(keeps.seen(), [job], "{kind:?} runs alone");
                continue;
            }
            assert_eq!(
                keeps.seen(),
                [Job::Identify(access.clone()), named],
                "{kind:?}: an identify, then the job with the ID it found"
            );
            assert_eq!(
                events,
                [
                    started(Some(opened())),
                    EngineEvent::Succeeded(EngineResult::Fake { steps_completed: 1 }),
                ],
                "{kind:?}: the job's own events, and none of the identify's"
            );
            assert_eq!(entry_of(&store, "demo").await, recorded, "{kind:?}");
            assert_eq!(
                taken(&reports),
                [Recording::Recorded {
                    alias: alias("demo"),
                    id: opened(),
                }],
                "{kind:?}: recorded once"
            );
        }
    }

    /// The identify opens the repository as the job would have: when it
    /// can't, the job ends with the reason the identify gave, which is the
    /// one the job's own open would have given, and the job never runs
    /// without the ID.
    #[tokio::test]
    async fn a_job_whose_identify_fails_ends_as_the_identify_did_and_never_runs() {
        let wrong_password = FailureReason::EngineFailed {
            detail: "the password is wrong".to_owned(),
        };
        for (finds, reason) in [
            (
                Finds::Fails(FailureReason::RepositoryNotFound),
                FailureReason::RepositoryNotFound,
            ),
            (Finds::Fails(wrong_password.clone()), wrong_password),
            (
                Finds::Nothing,
                FailureReason::EngineExited { exit_code: None },
            ),
        ] {
            let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
            let access = access_of(&store, "demo").await;
            let (engine, keeps, _) = keeping(&store, finds, false);
            assert_eq!(
                run(&engine, backup(&access)).await,
                [EngineEvent::Failed(reason.clone())],
                "{reason:?}"
            );
            assert_eq!(keeps.seen(), [Job::Identify(access)], "{reason:?}");
        }
    }

    #[tokio::test]
    async fn a_job_whose_identify_the_engine_cannot_start_is_refused_as_it_would_be() {
        let refused = EngineError::Unavailable {
            detail: "no such file or directory".to_owned(),
        };
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Refused(refused.clone()), false);
        let Err(error) = engine.run(op(), backup(&access)).await else {
            panic!("the job was started");
        };
        assert_eq!(error, refused);
        assert_eq!(keeps.seen(), [Job::Identify(access)]);
    }

    /// The bound every stop, every stream's end and every run's end is held
    /// to here: one that waits on something nothing will do fails its test
    /// rather than hanging it.
    const BOUND: std::time::Duration = std::time::Duration::from_secs(5);

    /// Stops a job, failing when the stop doesn't return within [`BOUND`].
    async fn stop_within_bound(control: &mut Box<dyn JobControl>) {
        tokio::time::timeout(BOUND, control.stop())
            .await
            .expect("the stop returns");
    }

    /// What a stopped job's stream still reports, failing when it doesn't
    /// end within [`BOUND`].
    async fn rest(events: BoxStream<'static, EngineEvent>) -> Vec<EngineEvent> {
        tokio::time::timeout(BOUND, events.collect())
            .await
            .expect("the stream ends once the job is stopped")
    }

    /// A stop before anything read the job's events, its identify running:
    /// the identify is stopped, and the job never starts.
    #[tokio::test]
    async fn stopping_a_job_before_its_events_are_read_stops_the_identify_and_starts_nothing() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Hangs, false);
        let RunningJob {
            events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        stop_within_bound(&mut control).await;
        assert_eq!(rest(events).await, []);
        drop(control);
        assert_eq!(keeps.seen(), [Job::Identify(access)]);
        assert_eq!(keeps.stopped(), [OperationKind::Identify]);
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A stop while the job's stream waits for its identify.
    #[tokio::test]
    async fn stopping_a_job_while_it_identifies_stops_the_identify_and_starts_nothing() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Hangs, false);
        let RunningJob {
            mut events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        assert_eq!(events.next().now_or_never(), None, "the identify runs");
        stop_within_bound(&mut control).await;
        assert_eq!(rest(events).await, []);
        drop(control);
        assert_eq!(keeps.seen(), [Job::Identify(access)]);
        assert_eq!(keeps.stopped(), [OperationKind::Identify]);
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A stop once the identify has found the ID, before the job's stream
    /// has read it and so before the job is started: the job never starts.
    #[tokio::test]
    async fn stopping_a_job_whose_identify_found_the_id_starts_nothing() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Repository, true);
        let RunningJob {
            events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        stop_within_bound(&mut control).await;
        assert_eq!(rest(events).await, []);
        drop(control);
        assert_eq!(keeps.seen(), [Job::Identify(access)]);
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A stop while the engine is starting the job, after its identify: the
    /// stop doesn't wait for the start, and the job it started is stopped
    /// before anything it reports is passed on.
    #[tokio::test]
    async fn stopping_a_job_while_it_starts_returns_at_once_and_stops_the_job_it_started() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let mut identified = access.clone();
        identified.id = Some(opened());
        let (engine, keeps, _) = keeping(&store, Finds::Repository, true);
        let go = keeps.hold_start();
        let RunningJob {
            mut events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        assert_eq!(events.next().now_or_never(), None, "the job is starting");
        assert_eq!(
            keeps.seen(),
            [Job::Identify(access.clone()), backup(&identified)]
        );
        assert_eq!(keeps.live(), 1, "the job's worker is starting");
        stop_within_bound(&mut control).await;
        let _ = go.send(Ok(()));
        assert_eq!(rest(events).await, [], "nothing of the job is passed on");
        drop(control);
        assert_eq!(keeps.stopped(), [OperationKind::Backup]);
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A stop while the engine is starting the job, after its identify, and
    /// the engine then refuses the job: the job was stopped, so its refusal
    /// is not passed on either.
    #[tokio::test]
    async fn stopping_a_job_whose_start_then_fails_passes_nothing_on() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Repository, true);
        let go = keeps.hold_start();
        let RunningJob {
            mut events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        assert_eq!(events.next().now_or_never(), None, "the job is starting");
        stop_within_bound(&mut control).await;
        let _ = go.send(Err(EngineError::Unavailable {
            detail: "the worker couldn't be started".to_owned(),
        }));
        assert_eq!(rest(events).await, [], "nothing of the job is passed on");
        drop(control);
        assert_eq!(keeps.stopped(), [], "no job was started to stop");
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A stop once the job runs, after its identify.
    #[tokio::test]
    async fn stopping_a_job_after_its_identify_stops_the_job() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let mut identified = access.clone();
        identified.id = Some(opened());
        let (engine, keeps, _) = keeping(&store, Finds::Repository, true);
        let RunningJob {
            mut events,
            mut control,
        } = engine
            .run(op(), backup(&access))
            .await
            .expect("the engine takes the job");
        assert_eq!(events.next().await, Some(started(Some(opened()))));
        stop_within_bound(&mut control).await;
        assert_eq!(rest(events).await, []);
        drop(control);
        assert_eq!(
            keeps.seen(),
            [Job::Identify(access.clone()), backup(&identified)]
        );
        assert_eq!(keeps.stopped(), [OperationKind::Backup]);
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }

    /// A cancel that comes while the engine is starting a backup's job,
    /// after its identify, ends the operation cancelled at once and gives
    /// its place in the queue to the next run, with no worker left running:
    /// a shutdown, which cancels the runs and waits for them, ends too.
    #[tokio::test]
    async fn cancelling_a_backup_while_its_job_starts_ends_it_and_frees_its_place() {
        let store = holding(vec![("demo", entry("/srv/demo/cold", None))]).await;
        let access = access_of(&store, "demo").await;
        let (engine, keeps, _) = keeping(&store, Finds::Repository, true);
        let _held = keeps.hold_start();
        let queue = RunQueue::new();
        let place = queue.push(
            &PlanId::parse("demo-home").expect("a plan ID"),
            Concurrency::default(),
        );
        let mut next = queue.push(
            &PlanId::parse("demo-work").expect("a plan ID"),
            Concurrency::default(),
        );
        let use_case = RunOperation::new(
            Arc::new(engine),
            Arc::new(FakeClock::new(
                Timestamp::from_second(1_800_000_000).expect("a time"),
            )),
            Arc::new(InMemoryOperationStore::new()),
        );
        let queued = use_case.queue(op(), Trigger::Manual, backup(&access)).await;
        let (cancel, cancelled) = futures::channel::oneshot::channel::<()>();
        let running = tokio::spawn(async move {
            use_case
                .run_in_turn(queued, place, cancelled.map(|_| ()))
                .await
        });
        tokio::time::timeout(BOUND, async {
            while keeps.seen().len() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the job is being started");
        assert_eq!(keeps.live(), 1, "the job's worker is starting");
        cancel.send(()).expect("the run waits for a cancel");
        let finished = tokio::time::timeout(BOUND, running)
            .await
            .expect("the run ends once cancelled")
            .expect("the run ends without a panic");
        assert_eq!(finished.operation.status(), &OperationStatus::Cancelled);
        tokio::time::timeout(BOUND, next.turn())
            .await
            .expect("the next run's turn comes");
        assert_eq!(keeps.live(), 0, "no worker is left running");
    }
}
