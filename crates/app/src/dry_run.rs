//! The plan dry run (§16, decision 89, point 13): an AI agent's draft plan
//! walked through its source filter, counting the files and bytes it would
//! back up and what each exclude removes on its own, never naming a path.
//! It runs no hook, resolves no secret and opens no repository.
//!
//! Everything an AI agent controls is bounded: a dry run reads only within
//! the dry-run allowlist, runs one at a time outside the backup
//! concurrency limit, doesn't start while a backup holds a place in the
//! queue and stops to give way when one takes a place, stops each walk
//! after [`DryRunLimits::entries`] entries and all its walks after
//! [`DryRunLimits::time`], and starts at most
//! [`MAX_DRY_RUNS_PER_HOUR`](domain::planning::MAX_DRY_RUNS_PER_HOUR) an
//! hour for each proposer. The allowlist is set by a person, here, under
//! the configuration write lock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use domain::planning::{
    AgentConfig, DryRunAllowlist, DryRunRecord, ExcludeSetting, Plan, Proposer, Source,
    SourceFilter, is_file_name, is_within,
};
use domain::{SignedDuration, Timestamp};

use crate::ports::{
    Clock, ConfigError, ConfigStore, GiveWay, NormalizeSources, WalkError, WalkLimits, WalkSources,
    WalkStop, Walked,
};
use crate::proposals::same_proposer;
use crate::repositories::ConfigWrites;
use crate::schedule::ConfigChanged;

/// How far a dry run reads: each walk at most `entries` entries, all its
/// walks together at most `time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DryRunLimits {
    pub entries: u64,
    pub time: SignedDuration,
}

impl DryRunLimits {
    /// The daemon's: 200,000 entries a walk, 20 seconds in all, short
    /// enough that an MCP client's call doesn't time out first.
    pub const DAEMON: Self = Self {
        entries: 200_000,
        time: SignedDuration::from_secs(20),
    };
}

/// What a dry run counted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanDryRun {
    /// The files the plan would back up, and their bytes.
    pub files: u64,
    pub bytes: u64,
    /// The entries it couldn't read.
    pub unreadable: u64,
    /// What each exclude removes on its own, in the order of
    /// [`SourceFilter::excludes`]; those the dry run didn't reach are left
    /// out.
    pub excludes: Vec<ExcludeTally>,
    /// Why it stopped before the end, if it did: its counts are then what
    /// it read until then.
    pub stopped: Option<WalkStop>,
    /// The warnings the engine raised in all its walks, left out of the
    /// daemon's log since they can name a path.
    pub warnings: u64,
}

/// What one exclude removes on its own: the files and bytes of the plan's
/// sources with no exclude, less those with this one alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludeTally {
    pub exclude: ExcludeSetting,
    pub files: u64,
    pub bytes: u64,
}

/// Why a dry run didn't run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DryRunError {
    /// Another dry run is running.
    Running,
    /// A backup holds a place in the queue: the dry run gives way.
    BackupGoing,
    /// The proposer started its limit of dry runs in the last hour; it may
    /// again from `until`.
    TooSoon { until: Timestamp },
    /// A path the plan reads (its `setting`'s entry `index`, from 0) isn't
    /// within the allowlist once resolved, or can't be resolved.
    NotAllowed { setting: &'static str, index: usize },
    /// For the plan's `setting`, the walk would read files in the folders
    /// above its sources, outside the allowlist
    /// ([`SourceFilter::reads_above`]).
    ReadsAbove { setting: &'static str },
    /// The engine can't apply the plan's source filter.
    Filter,
    /// The walks would take secret key files in the data folder that no
    /// glob can name: refused as a backup of the plan is (§16, decision
    /// 120), as a dry run predicts what the backup does.
    KeyFilesNotLeftOut,
    /// The configuration couldn't be read.
    Config(Box<ConfigError>),
    /// The walk failed; `detail` is the engine's own message.
    Walk { detail: String },
}

/// The globs every walk of the plan's `sources` leaves out after the
/// plan's own, as each backup leaves out the secret key files in the data
/// folder (§16, decision 120): after every glob of the plan's own, its
/// glob files' included, and never tallied as one of its excludes. `None`
/// when the walks would take key files no glob can name, which refuses
/// the dry run as it refuses the backup.
pub type LeftOut = Arc<dyn Fn(&[Source]) -> Option<Vec<String>> + Send + Sync>;

/// The dry runs of the daemon.
pub struct PlanDryRuns {
    config: Arc<dyn ConfigStore>,
    sources: Arc<dyn NormalizeSources>,
    walk: Arc<dyn WalkSources>,
    clock: Arc<dyn Clock>,
    /// Whether a backup holds a place in the queue, running or waiting.
    backups: GiveWay,
    limits: DryRunLimits,
    running: Arc<AtomicBool>,
    /// What every walk leaves out after the plan's own globs: the secret
    /// key files in the data folder (§16, decision 120).
    left_out: LeftOut,
    records: Mutex<Vec<(Proposer, DryRunRecord)>>,
}

/// Lets go of the one dry run's turn when dropped: once the dry run and
/// every walk still holding its give-way have let go, so a caller that
/// hangs up mid-walk doesn't free the turn while the walk reads on.
struct Turn(Arc<AtomicBool>);

impl Drop for Turn {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Asks the dry run's walks to give way when its caller lets go of it.
struct StopWhenDropped(Arc<AtomicBool>);

impl Drop for StopWhenDropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl PlanDryRuns {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        sources: Arc<dyn NormalizeSources>,
        walk: Arc<dyn WalkSources>,
        clock: Arc<dyn Clock>,
        backups: GiveWay,
        limits: DryRunLimits,
        left_out: LeftOut,
    ) -> Self {
        Self {
            config,
            sources,
            walk,
            clock,
            backups,
            limits,
            running: Arc::new(AtomicBool::new(false)),
            left_out,
            records: Mutex::new(Vec::new()),
        }
    }

    /// Dry-runs `plan` for `asker`.
    pub async fn run(&self, asker: &Proposer, plan: &Plan) -> Result<PlanDryRun, DryRunError> {
        if self.running.swap(true, Ordering::AcqRel) {
            return Err(DryRunError::Running);
        }
        let turn = Turn(Arc::clone(&self.running));
        if (self.backups)() {
            return Err(DryRunError::BackupGoing);
        }
        self.start(asker)?;
        let started = self.clock.monotonic();
        let config = self
            .config
            .load()
            .await
            .map_err(|error| DryRunError::Config(Box::new(error)))?;
        let sources = self.allowed(&config, plan).await?;
        let left_out = (self.left_out)(&sources).ok_or(DryRunError::KeyFilesNotLeftOut)?;
        let dropped = Arc::new(AtomicBool::new(false));
        let _stop = StopWhenDropped(Arc::clone(&dropped));
        let backups = Arc::clone(&self.backups);
        let give_way: GiveWay = Arc::new(move || {
            let _turn = &turn;
            dropped.load(Ordering::Acquire) || backups()
        });
        let walks = Walks {
            sources: &sources,
            left_out: &left_out,
            started,
            give_way: &give_way,
        };
        self.count(&walks, &plan.filter).await
    }

    /// Records a dry run of `asker`'s, unless it must wait.
    fn start(&self, asker: &Proposer) -> Result<(), DryRunError> {
        let now = self.clock.now();
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        let at = match records
            .iter()
            .position(|(proposer, _)| same_proposer(proposer, asker))
        {
            Some(at) => at,
            None => {
                records.push((asker.clone(), DryRunRecord::default()));
                records.len().saturating_sub(1)
            }
        };
        match records.get_mut(at) {
            Some((_, record)) => record
                .start(now)
                .map_err(|until| DryRunError::TooSoon { until }),
            None => Ok(()),
        }
    }

    /// The plan's sources resolved, once its filter reads nothing above
    /// them, each is within the allowlist resolved, and so is each glob
    /// file it reads; each marker it names must be a file name alone.
    async fn allowed(&self, config: &AgentConfig, plan: &Plan) -> Result<Vec<Source>, DryRunError> {
        if let Some(setting) = plan.filter.reads_above() {
            return Err(DryRunError::ReadsAbove { setting });
        }
        let mut folders = Vec::new();
        for folder in config.agent().dry_run_allowlist.folders() {
            if let Ok(folder) = Source::parse(folder.as_str())
                && let Some(resolved) = self.resolved(&folder).await
            {
                folders.push(resolved);
            }
        }
        let allowed = |path: &Source| {
            folders
                .iter()
                .any(|folder| is_within(path.as_str(), folder.as_str()))
        };
        let mut sources = Vec::with_capacity(plan.sources.len());
        for (index, source) in plan.sources.iter().enumerate() {
            match self.resolved(source).await {
                Some(resolved) if allowed(&resolved) => sources.push(resolved),
                _ => {
                    return Err(DryRunError::NotAllowed {
                        setting: "sources",
                        index,
                    });
                }
            }
        }
        let files = [
            ("glob-files", &plan.filter.glob_files),
            ("iglob-files", &plan.filter.iglob_files),
        ];
        for (setting, list) in files {
            for (index, file) in list.iter().enumerate() {
                let resolved = match Source::parse(file.as_str()) {
                    Ok(file) => self.resolved(&file).await,
                    Err(_) => None,
                };
                if !resolved.as_ref().is_some_and(allowed) {
                    return Err(DryRunError::NotAllowed { setting, index });
                }
            }
        }
        let markers = &plan.filter.exclude_if_present;
        if let Some(index) = markers.iter().position(|name| !is_file_name(name)) {
            return Err(DryRunError::NotAllowed {
                setting: "exclude-if-present",
                index,
            });
        }
        Ok(sources)
    }

    /// `path` as a backup walks it, or none when it can't be resolved.
    async fn resolved(&self, path: &Source) -> Option<Source> {
        self.sources
            .normalized(std::slice::from_ref(path))
            .await
            .ok()
            .and_then(|mut one| one.pop())
    }

    /// Walks the sources through `filter`, then with no exclude and with
    /// each exclude alone, until a walk stops early, all in the time
    /// left since the dry run started.
    async fn count(
        &self,
        walks: &Walks<'_>,
        filter: &SourceFilter,
    ) -> Result<PlanDryRun, DryRunError> {
        let planned = self.walked(walks, filter).await?;
        let mut dry_run = PlanDryRun {
            files: planned.files,
            bytes: planned.bytes,
            unreadable: planned.unreadable,
            excludes: Vec::new(),
            stopped: planned.stopped,
            warnings: planned.warnings,
        };
        let excludes = filter.excludes();
        if dry_run.stopped.is_some() || excludes.is_empty() {
            return Ok(dry_run);
        }
        let all = self.walked(walks, &SourceFilter::default()).await?;
        dry_run.warnings = dry_run.warnings.saturating_add(all.warnings);
        if all.stopped.is_some() {
            dry_run.stopped = all.stopped;
            return Ok(dry_run);
        }
        for (exclude, alone) in excludes {
            let with = self.walked(walks, &alone).await?;
            dry_run.warnings = dry_run.warnings.saturating_add(with.warnings);
            if with.stopped.is_some() {
                dry_run.stopped = with.stopped;
                break;
            }
            dry_run.excludes.push(ExcludeTally {
                exclude,
                files: all.files.saturating_sub(with.files),
                bytes: all.bytes.saturating_sub(with.bytes),
            });
        }
        Ok(dry_run)
    }

    /// One walk, in the time left since the dry run started, leaving out
    /// what every backup leaves out besides `filter`; none left stops it
    /// before it starts.
    async fn walked(
        &self,
        walks: &Walks<'_>,
        filter: &SourceFilter,
    ) -> Result<Walked, DryRunError> {
        let spent = self.clock.monotonic().saturating_sub(walks.started);
        let time = self.limits.time.saturating_sub(spent);
        if !time.is_positive() {
            return Ok(Walked {
                stopped: Some(WalkStop::Time),
                ..Walked::default()
            });
        }
        let limits = WalkLimits {
            entries: self.limits.entries,
            time,
        };
        self.walk
            .walk(
                walks.sources,
                filter,
                walks.left_out,
                limits,
                Arc::clone(walks.give_way),
            )
            .await
            .map_err(|error| match error {
                WalkError::Filter => DryRunError::Filter,
                WalkError::Engine { detail } => DryRunError::Walk { detail },
            })
    }
}

/// What each walk of one dry run shares: the plan's sources, resolved,
/// the globs left out after the plan's own, when the dry run started and
/// when to give way.
struct Walks<'a> {
    sources: &'a [Source],
    left_out: &'a [String],
    started: SignedDuration,
    give_way: &'a GiveWay,
}

/// Reads and sets the dry-run allowlist, a person's setting.
pub struct DryRunAllowlists {
    config: Arc<dyn ConfigStore>,
    writes: Arc<ConfigWrites>,
    changed: ConfigChanged,
}

#[expect(
    clippy::result_large_err,
    reason = "the configuration store's own error, which the routes map to its code"
)]
impl DryRunAllowlists {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        writes: Arc<ConfigWrites>,
        changed: ConfigChanged,
    ) -> Self {
        Self {
            config,
            writes,
            changed,
        }
    }

    /// The allowlist the configuration holds.
    pub async fn allowlist(&self) -> Result<DryRunAllowlist, ConfigError> {
        Ok(self.config.load().await?.agent().dry_run_allowlist.clone())
    }

    /// Saves `allowlist` as the configuration's, holding the write lock.
    pub async fn set(&self, allowlist: DryRunAllowlist) -> Result<(), ConfigError> {
        let _writing = self.writes.lock().await;
        let loaded = self.config.load().await?;
        self.config
            .save(&loaded.with_dry_run_allowlist(allowlist))
            .await?;
        self.changed.tell();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
