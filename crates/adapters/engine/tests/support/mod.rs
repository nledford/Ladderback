//! Shared helpers of the engine's integration tests: scratch repositories
//! made with `rustic_core` itself, and a `WorkerEngine` on the real worker.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app::ports::{
    BackupJob, BackupSummary, CheckJob, Engine, EngineEvent, EngineResult, Job, PruneJob,
    RepositoryAccess, RunningJob,
};
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::secrets::SecretValue;
use domain::{AgentId, OperationId, PlanId, SignedDuration, Uuid};
use engine::WorkerEngine;
use futures::StreamExt;
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{
    ConfigOptions, Credentials, KeyOptions, OpenStatus, Repository, RepositoryBackends,
    RepositoryOptions,
};
use tempfile::TempDir;

pub const PASSWORD: &str = "correct horse battery staple";

/// A repository somewhere under a scratch directory, local or through OpenDAL.
#[derive(Clone)]
pub struct Location {
    pub repository: String,
    pub options: BTreeMap<String, String>,
}

impl Location {
    pub fn local(dir: &Path) -> Self {
        Self {
            repository: dir.to_string_lossy().into_owned(),
            options: BTreeMap::new(),
        }
    }

    /// OpenDAL's `fs` service, rooted at `dir`. With `atomic_write_dir`
    /// OpenDAL writes a file there and renames it into place; without it, it
    /// writes in place, and a kill leaves half a file under its final name.
    pub fn opendal_fs(dir: &Path, atomic_write_dir: Option<&Path>) -> Self {
        let mut options = BTreeMap::from([("root".to_owned(), dir.to_string_lossy().into_owned())]);
        if let Some(atomic) = atomic_write_dir {
            options.insert(
                "atomic_write_dir".to_owned(),
                atomic.to_string_lossy().into_owned(),
            );
        }
        Self {
            repository: "opendal:fs".to_owned(),
            options,
        }
    }

    pub fn access(&self, password: &str) -> RepositoryAccess {
        RepositoryAccess {
            repository: self.repository.clone(),
            password: SecretValue::new(password),
            options: self.options.clone(),
            id: None,
        }
    }

    fn backends(&self) -> RepositoryBackends {
        BackendOptions::default()
            .repository(self.repository.clone())
            .options(self.options.clone())
            .to_backends()
            .unwrap()
    }

    pub fn init(&self) -> Repository<OpenStatus> {
        Repository::new(
            &RepositoryOptions::default().no_cache(true),
            &self.backends(),
        )
        .unwrap()
        .init(
            &Credentials::password(PASSWORD),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .unwrap()
    }

    pub fn open(&self) -> Repository<OpenStatus> {
        Repository::new(
            &RepositoryOptions::default().no_cache(true),
            &self.backends(),
        )
        .unwrap()
        .open(&Credentials::password(PASSWORD))
        .unwrap()
    }

    pub fn snapshots(&self) -> Vec<SnapshotFile> {
        self.open().get_all_snapshots().unwrap()
    }
}

/// Runs `f` on a thread outside the test's tokio runtime. `rustic_core`'s
/// OpenDAL backend blocks on a runtime of its own, which tokio refuses to
/// start from a thread already inside one.
pub fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

/// A scratch directory with a local repository and a source folder in it.
pub struct Scratch {
    pub dir: TempDir,
}

impl Scratch {
    pub fn new() -> Self {
        let scratch = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(scratch.source()).unwrap();
        scratch
    }

    pub fn repo_dir(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    pub fn source(&self) -> PathBuf {
        self.dir.path().join("source")
    }

    pub fn local(&self) -> Location {
        Location::local(&self.repo_dir())
    }

    /// OpenDAL's `fs` service on the scratch repository, writing through a
    /// directory of its own so that a write is all or nothing.
    /// `STEADING_KILL_IN_PLACE=1` leaves that out, to see what a kill does
    /// to a repository that is written in place.
    pub fn opendal_fs(&self) -> Location {
        let atomic = std::env::var_os("STEADING_KILL_IN_PLACE")
            .is_none()
            .then(|| self.dir.path().join("atomic"));
        if let Some(atomic) = &atomic {
            fs::create_dir_all(atomic).unwrap();
        }
        Location::opendal_fs(&self.repo_dir(), atomic.as_deref())
    }
}

pub fn write_file(path: impl AsRef<Path>, contents: &[u8]) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A worker engine on this crate's own worker binary, with timings short
/// enough for tests.
pub fn engine() -> WorkerEngine {
    WorkerEngine::new(env!("CARGO_BIN_EXE_engine-worker"), Vec::<OsString>::new())
        .with_stop_grace(Duration::from_secs(5))
        .with_watchdog_interval(Duration::from_millis(50))
}

pub fn op() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(1))
}

pub fn plan_tags(plan: &str) -> PlanTags {
    PlanTags::new(
        PlanId::parse(plan).unwrap(),
        AgentId::from_uuid(Uuid::from_u128(7)),
    )
}

pub fn backup_job(location: &Location, source: &Path, plan: &str, budget: SignedDuration) -> Job {
    Job::Backup(BackupJob {
        repository: location.access(PASSWORD),
        sources: vec![source.to_string_lossy().into_owned()],
        tags: plan_tags(plan),
        extra_tags: vec![UserTag::new("extra").unwrap()],
        label: Some("home".to_owned()),
        deadline_budget: budget,
    })
}

pub fn prune_job(location: &Location) -> Job {
    Job::Prune(PruneJob {
        repository: location.access(PASSWORD),
        keep_delete: KeepDelete::MIN,
    })
}

pub fn check_job(location: &Location, read_data: bool) -> Job {
    Job::Check(CheckJob {
        repository: location.access(PASSWORD),
        read_data,
    })
}

/// Runs a job to its end and returns every event.
pub async fn run_to_end(engine: &WorkerEngine, job: Job) -> Vec<EngineEvent> {
    let RunningJob {
        mut events,
        control,
    } = engine.run(op(), job).await.unwrap();
    let mut all = Vec::new();
    while let Some(event) = events.next().await {
        all.push(event);
    }
    drop(control);
    all
}

pub fn succeeded(events: &[EngineEvent]) -> &EngineResult {
    match events.last() {
        Some(EngineEvent::Succeeded(result)) => result,
        other => panic!("the job did not succeed: {other:?}\nall events: {events:?}"),
    }
}

pub fn backed_up(events: &[EngineEvent]) -> &BackupSummary {
    match succeeded(events) {
        EngineResult::Backup(summary) => summary,
        other => panic!("not a backup result: {other:?}"),
    }
}

/// SplitMix64: a small seeded generator, so a failing run can be repeated.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// Incompressible bytes: deduplication and compression can't shrink them.
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(len + 8);
        while bytes.len() < len {
            bytes.extend_from_slice(&self.next().to_le_bytes());
        }
        bytes.truncate(len);
        bytes
    }
}
