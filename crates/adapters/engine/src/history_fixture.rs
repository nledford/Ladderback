//! A real rustic repository built from a declared snapshot history, for
//! screenshots, demos and the scenarios that need history (#1122). Test
//! tooling: it is compiled only with the `history-fixture` feature, which
//! neither the daemon's own build nor a release build turns on
//! (`just architecture` checks both).
//!
//! A declaration (a TOML file under `tests/fixtures/history/`, which its
//! callers parse into a [`History`]) lists snapshots: the plan that took
//! each, its time, and how many files of how many bytes it holds. Each one
//! is a real backup through `rustic_core`, of a folder of procedural files
//! the builder writes, tagged as the plan's own backup tags it and saved at
//! the declared time, so the daemon, the `rustic` CLI and the scenarios read
//! it as they read any other repository. There is no fake repository and no
//! second adapter to drift from rustic.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::execution::plan_tags::PlanTags;
use domain::planning::Hostname;
use domain::{AgentId, PlanId};
use jiff::Zoned;
use rustic_backend::BackendOptions;
use rustic_core::repofile::{RusticTime, SnapshotFile};
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, IndexedIdsStatus, KeyOptions, ParentOptions,
    PathList, Repository, RepositoryOptions, SnapshotOptions,
};
use serde::Deserialize;

use crate::worker::convert::snapshot_tags;

/// What every snapshot the builder saves names as the program that made it.
const PROGRAM: &str = concat!("ladderback-history-fixture ", env!("CARGO_PKG_VERSION"));

/// The most files one snapshot may declare: a guard against a typo that
/// would fill the disk, far above what a demo needs.
const MAX_FILES: u32 = 100_000;

/// The most bytes one snapshot may declare, for the same reason: 10 GB.
const MAX_BYTES: u64 = 10_000_000_000;

/// The most bytes a whole declaration may declare, its snapshots' sizes
/// added up: 100 GB. A run writes again only what changed, so the builder
/// writes far less, but a typo in a series' runs can't fill the disk.
const MAX_TOTAL_BYTES: u64 = 100_000_000_000;

/// The files of one folder: the builder spreads a snapshot's files over
/// folders of this many.
const FILES_PER_FOLDER: u32 = 100;

/// The most runs one series may declare.
const MAX_RUNS: u32 = 10_000;

/// A repository's history, as its declaration states it: single snapshots,
/// and series of runs at a regular interval. They are built oldest first,
/// whatever order they are declared in.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// The host every snapshot records: a neutral name such as `demo-host`.
    pub host: String,
    #[serde(rename = "snapshot", default)]
    pub snapshots: Vec<Declared>,
    #[serde(default)]
    pub series: Vec<Series>,
}

/// A plan's runs at a regular interval, such as a nightly backup over
/// months.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Series {
    pub plan: String,
    /// The first run's time, read as [`Declared::time`] is.
    pub first: String,
    /// The time between two runs, as jiff reads a span: `"1 day"`,
    /// `"1 week"`, `"6h"`.
    pub every: String,
    /// How many runs there were.
    pub runs: u32,
    /// What the plan's folder held at each run.
    pub files: Files,
    /// The runs, counted from 1, that failed (see [`Declared::failed`]).
    #[serde(default)]
    pub failed: Vec<u32>,
}

/// One declared snapshot.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    /// The plan whose backup took it.
    pub plan: String,
    /// When it was taken, as rustic reads a snapshot's time: a date, a time
    /// of day and an offset (`2026-08-01T02:00:00+02:00`), UTC when none is
    /// given.
    pub time: String,
    /// What the plan's folder held.
    pub files: Files,
    /// The run failed before its snapshot was saved: the data it wrote stays
    /// in the repository, unused, for a prune to reclaim, as a run that
    /// fails after writing packs leaves it, and no snapshot lists it.
    #[serde(default)]
    pub failed: bool,
}

/// How many files a snapshot's folder held, and their size in all.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Files {
    pub count: u32,
    pub bytes: Bytes,
}

/// A size: a whole number of bytes, or one with a unit (`"40 MB"`,
/// `"512 KiB"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "SizeText")]
pub struct Bytes(pub u64);

#[derive(Deserialize)]
#[serde(untagged)]
enum SizeText {
    Number(u64),
    Text(String),
}

impl TryFrom<SizeText> for Bytes {
    type Error = FixtureError;

    fn try_from(text: SizeText) -> Result<Self, Self::Error> {
        match text {
            SizeText::Number(bytes) => Ok(Self(bytes)),
            SizeText::Text(text) => parse_size(&text).map(Self),
        }
    }
}

/// `text` as a number and a unit: B, kB, MB and GB count in thousands, KiB,
/// MiB and GiB in 1024s.
fn parse_size(text: &str) -> Result<u64, FixtureError> {
    let refused = || FixtureError(format!("{text:?} is not a size such as \"40 MB\""));
    let trimmed = text.trim();
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at_checked(split).ok_or_else(refused)?;
    let number: u64 = number.parse().map_err(|_| refused())?;
    let scale: u64 = match unit.trim() {
        "" | "B" => 1,
        "kB" | "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        _ => return Err(refused()),
    };
    number.checked_mul(scale).ok_or_else(refused)
}

/// Where a history is built, and as whom.
#[derive(Clone, Debug)]
pub struct Target {
    /// The repository's folder.
    pub repository: PathBuf,
    /// Whether the builder initializes a new repository there, or adds the
    /// history to the one already there.
    pub start: Start,
    /// The repository's password: a canary, never a real one.
    pub password: String,
    /// The folder under which each plan's source folder is written, one
    /// folder per plan, named by its ID.
    pub sources: PathBuf,
    /// The agent whose plans took the snapshots: the daemon that is to own
    /// them reads its ID from its data folder.
    pub agent: AgentId,
}

/// A new repository, or one that is already there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Init,
    Existing,
}

/// A snapshot the builder took, oldest first.
#[derive(Clone, Debug)]
pub struct Built {
    pub plan: PlanId,
    pub time: Zoned,
    /// The snapshot's ID, or `None` for a failed run, whose snapshot was
    /// never saved.
    pub id: Option<String>,
}

/// Why a history could not be built: a declaration that is not valid, or
/// rustic's or the file system's own words.
#[derive(Debug)]
pub struct FixtureError(String);

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FixtureError {}

impl From<Box<rustic_core::RusticError>> for FixtureError {
    fn from(error: Box<rustic_core::RusticError>) -> Self {
        Self(error.display_log())
    }
}

impl From<io::Error> for FixtureError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

/// A declared snapshot, checked.
struct Planned {
    plan: PlanId,
    time: Zoned,
    count: u32,
    bytes: u64,
    failed: bool,
}

impl History {
    /// Checks every declaration without building anything: what [`build`]
    /// would refuse, it refuses.
    pub fn check(&self) -> Result<(), FixtureError> {
        self.planned().map(|_| ())
    }

    /// Each declared snapshot, checked, oldest first, or what is wrong with
    /// the first that is not valid, by its number in the declaration.
    fn planned(&self) -> Result<(Hostname, Vec<Planned>), FixtureError> {
        let host = Hostname::parse(&self.host)
            .map_err(|error| FixtureError(format!("the host {:?}: {error}", self.host)))?;
        let numbered = |what: &'static str, index: usize| {
            move |FixtureError(why)| {
                FixtureError(format!("{what} {}: {why}", index.saturating_add(1)))
            }
        };
        let mut planned = Vec::new();
        for (index, declared) in self.snapshots.iter().enumerate() {
            planned.push(declared.planned().map_err(numbered("snapshot", index))?);
        }
        for (index, series) in self.series.iter().enumerate() {
            planned.extend(series.planned().map_err(numbered("series", index))?);
        }
        planned.sort_by_key(|snapshot| snapshot.time.timestamp());
        let total = planned
            .iter()
            .try_fold(0_u64, |sum, snapshot| sum.checked_add(snapshot.bytes))
            .filter(|total| *total <= MAX_TOTAL_BYTES);
        if total.is_none() {
            return Err(FixtureError(format!(
                "the snapshots declare more than the {MAX_TOTAL_BYTES} bytes in all a declaration may"
            )));
        }
        Ok((host, planned))
    }
}

impl Declared {
    fn planned(&self) -> Result<Planned, FixtureError> {
        checked(&self.plan, time(&self.time)?, self.files, self.failed)
    }
}

impl Series {
    fn planned(&self) -> Result<Vec<Planned>, FixtureError> {
        if self.runs > MAX_RUNS {
            return Err(FixtureError(format!(
                "{} runs, more than the {MAX_RUNS} a series may declare",
                self.runs
            )));
        }
        if let Some(run) = self
            .failed
            .iter()
            .find(|run| !(1..=self.runs).contains(*run))
        {
            return Err(FixtureError(format!(
                "the failed run {run} is not one of its runs 1 to {}",
                self.runs
            )));
        }
        let every: jiff::Span = self
            .every
            .parse()
            .map_err(|error| FixtureError(format!("the interval {:?}: {error}", self.every)))?;
        let mut at = time(&self.first)?;
        let mut planned = Vec::new();
        for run in 1..=self.runs {
            let failed = self.failed.contains(&run);
            let next = at
                .checked_add(every)
                .map_err(|error| FixtureError(format!("run {run}: {error}")))?;
            if next.timestamp() <= at.timestamp() {
                return Err(FixtureError(format!(
                    "the interval {:?} doesn't move forward",
                    self.every
                )));
            }
            planned.push(checked(&self.plan, at, self.files, failed)?);
            at = next;
        }
        Ok(planned)
    }
}

/// A snapshot's time, as rustic reads one.
fn time(text: &str) -> Result<Zoned, FixtureError> {
    RusticTime::parse_utc(text).map_err(|error| FixtureError(format!("the time {text:?}: {error}")))
}

fn checked(plan: &str, time: Zoned, files: Files, failed: bool) -> Result<Planned, FixtureError> {
    let plan =
        PlanId::parse(plan).map_err(|error| FixtureError(format!("the plan {plan:?}: {error}")))?;
    let Files { count, bytes } = files;
    if count > MAX_FILES {
        return Err(FixtureError(format!(
            "{count} files, more than the {MAX_FILES} a snapshot may declare"
        )));
    }
    if count == 0 && bytes.0 > 0 {
        return Err(FixtureError(format!(
            "{} bytes in no files: a snapshot with bytes needs a file",
            bytes.0
        )));
    }
    if bytes.0 > MAX_BYTES {
        return Err(FixtureError(format!(
            "{} bytes, more than the {MAX_BYTES} a snapshot may declare",
            bytes.0
        )));
    }
    Ok(Planned {
        plan,
        time,
        count,
        bytes: bytes.0,
        failed,
    })
}

/// Builds `history` into `target`: each declared snapshot, oldest first, as
/// a backup of its plan's folder. Every declaration is checked before
/// anything is written.
pub fn build(history: &History, target: &Target) -> Result<Vec<Built>, FixtureError> {
    let (host, planned) = history.planned()?;
    let repository = open(target)?;
    let mut folders: Vec<(PlanId, PlanFolder)> = Vec::new();
    let mut built = Vec::with_capacity(planned.len());
    for snapshot in planned {
        let at = match folders.iter().position(|(plan, _)| *plan == snapshot.plan) {
            Some(at) => at,
            None => {
                let folder = PlanFolder::new(target.sources.join(snapshot.plan.as_str()));
                folders.push((snapshot.plan.clone(), folder));
                folders.len().saturating_sub(1)
            }
        };
        let Some((_, folder)) = folders.get_mut(at) else {
            return Err(FixtureError("a plan's folder went missing".to_owned()));
        };
        let modified = SystemTime::from(snapshot.time.timestamp());
        folder.fill(&snapshot.plan, snapshot.count, snapshot.bytes, modified)?;
        let saved = back_up(&repository, &host, target.agent, &snapshot, folder)?;
        let id = if snapshot.failed {
            repository.delete_snapshots(&[saved.id])?;
            None
        } else {
            folder.parent = Some(saved.id.to_hex().to_string());
            Some(saved.id.to_hex().to_string())
        };
        built.push(Built {
            plan: snapshot.plan,
            time: snapshot.time,
            id,
        });
    }
    Ok(built)
}

/// The repository at `target`, initialized first when asked, without a
/// cache: the builder never writes to the developer's own rustic cache.
fn open(target: &Target) -> Result<Repository<IndexedIdsStatus>, FixtureError> {
    let location = target.repository.to_str().ok_or_else(|| {
        FixtureError(format!(
            "the repository folder {} is not UTF-8",
            target.repository.display()
        ))
    })?;
    let backends = BackendOptions::default()
        .repository(location)
        .to_backends()?;
    let repository = Repository::new(&RepositoryOptions::default().no_cache(true), &backends)?;
    let credentials = Credentials::password(&target.password);
    let repository = match target.start {
        Start::Init => repository.init(
            &credentials,
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )?,
        Start::Existing => repository.open(&credentials)?,
    };
    Ok(repository.to_indexed_ids()?)
}

/// One backup of the plan's folder, as the plan's own backup tags it, at the
/// declared time, on the plan's last snapshot as its parent.
fn back_up(
    repository: &Repository<IndexedIdsStatus>,
    host: &Hostname,
    agent: AgentId,
    snapshot: &Planned,
    folder: &PlanFolder,
) -> Result<SnapshotFile, FixtureError> {
    let tags = snapshot_tags(&PlanTags::new(snapshot.plan.clone(), agent), &[])
        .map_err(|error| FixtureError(format!("the plan's tags: {error:?}")))?;
    let options = SnapshotOptions::default()
        .tags(vec![tags])
        .host(host.as_str().to_owned())
        .time(snapshot.time.clone());
    let mut file = options.to_snapshot()?;
    PROGRAM.clone_into(&mut file.program_version);
    let mut parents = ParentOptions::default();
    if let Some(parent) = &folder.parent {
        parents = parents.parents(vec![parent.clone()]);
    }
    let paths = PathList::from_iter([folder.root.clone()])
        .sanitize()
        .map_err(|error| FixtureError(format!("the plan's folder: {error}")))?;
    Ok(repository.backup(&BackupOptions::default().parent_opts(parents), &paths, file)?)
}

/// A plan's source folder, as the builder last wrote it.
struct PlanFolder {
    root: PathBuf,
    /// The size of each file it holds now, in the order of their names.
    sizes: Vec<u64>,
    /// How many snapshots of the plan were taken, failed runs too.
    taken: u64,
    /// The plan's last saved snapshot.
    parent: Option<String>,
}

impl PlanFolder {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            sizes: Vec::new(),
            taken: 0,
            parent: None,
        }
    }

    /// Makes the folder hold `count` files of `bytes` in all, and changes one
    /// of them, so that no two of the plan's snapshots hold the same: a file
    /// whose size changes is written again, one past `count` is removed,
    /// and the rest are left as they are, so each backup reads only what
    /// changed, as a backup of a real folder does. A file written is dated
    /// `modified`, the snapshot's time, so a snapshot's files show when they
    /// last changed before it, not when the builder ran.
    fn fill(
        &mut self,
        plan: &PlanId,
        count: u32,
        bytes: u64,
        modified: SystemTime,
    ) -> Result<(), FixtureError> {
        let generation = self.taken;
        self.taken = self.taken.saturating_add(1);
        let sizes = spread(bytes, count);
        let changed = generation.checked_rem(u64::from(count));
        // The folders whose entries came or went, which a real folder would
        // date with the change too.
        let mut entries_changed: Vec<PathBuf> = Vec::new();
        for (index, size) in (0_u32..).zip(&sizes) {
            let was = self.sizes.get(usize::try_from(index).unwrap_or(usize::MAX));
            if was == Some(size) && changed != Some(u64::from(index)) {
                continue;
            }
            let path = file_path(&self.root, index);
            if was.is_none() {
                entries_changed.extend(path.parent().map(Path::to_path_buf));
            }
            let seed = seed(plan, generation, index);
            write_procedural(&path, *size, seed, modified)?;
        }
        for index in count..u32::try_from(self.sizes.len()).unwrap_or(u32::MAX) {
            let path = file_path(&self.root, index);
            fs::remove_file(&path)?;
            entries_changed.extend(path.parent().map(Path::to_path_buf));
        }
        fs::create_dir_all(&self.root)?;
        if self.sizes.is_empty() || !entries_changed.is_empty() {
            entries_changed.push(self.root.clone());
        }
        entries_changed.dedup();
        for folder in entries_changed {
            fs::File::open(&folder)?.set_modified(modified)?;
        }
        self.sizes = sizes;
        Ok(())
    }
}

/// `bytes` spread over `count` files, the first ones a byte bigger when
/// they don't divide evenly.
fn spread(bytes: u64, count: u32) -> Vec<u64> {
    let count_wide = u64::from(count);
    let each = bytes.checked_div(count_wide).unwrap_or(0);
    let bigger = bytes.checked_rem(count_wide).unwrap_or(0);
    (0..count_wide)
        .map(|index| each.saturating_add(u64::from(index < bigger)))
        .collect()
}

/// The file `index` of a plan's folder: neutral names, a hundred to a
/// folder.
fn file_path(root: &Path, index: u32) -> PathBuf {
    root.join(format!("folder-{:03}", index / FILES_PER_FOLDER))
        .join(format!("demo-{index:05}.dat"))
}

/// The seed of a file's contents: its plan, the run that wrote it and its
/// number, so that the plans' files differ and each change is new data.
fn seed(plan: &PlanId, generation: u64, index: u32) -> u64 {
    // FNV-1a over the plan's ID.
    let named = plan
        .as_str()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    named ^ generation.rotate_left(32) ^ u64::from(index)
}

/// Writes `len` bytes drawn from SplitMix64, seeded by `seed`, dated
/// `modified`: data that compresses as little as a real file's, the same
/// for the same seed.
fn write_procedural(path: &Path, len: u64, seed: u64, modified: SystemTime) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut out = io::BufWriter::new(fs::File::create(path)?);
    let mut state = seed;
    let mut left = len;
    while left > 0 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        let chunk = (z ^ (z >> 31)).to_le_bytes();
        let take = left.min(8);
        let part = chunk
            .get(..usize::try_from(take).unwrap_or(8))
            .unwrap_or(&chunk);
        out.write_all(part)?;
        left = left.saturating_sub(take);
    }
    out.flush()?;
    out.get_ref().set_modified(modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_read_with_its_unit() {
        assert_eq!(parse_size("40 MB").unwrap(), 40_000_000);
        assert_eq!(parse_size("512 KiB").unwrap(), 524_288);
        assert_eq!(parse_size("2GiB").unwrap(), 2 << 30);
        assert_eq!(parse_size("7").unwrap(), 7);
        assert_eq!(parse_size("3 kB").unwrap(), 3_000);
    }

    #[test]
    fn a_size_with_no_number_or_an_unknown_unit_is_refused() {
        for text in [
            "MB",
            "40 mb",
            "1.5 MB",
            "-2 B",
            "40 TB",
            "99999999999999999999 GB",
        ] {
            assert!(parse_size(text).is_err(), "{text}");
        }
    }

    #[test]
    fn bytes_are_spread_over_the_files_with_nothing_lost() {
        assert_eq!(spread(10, 3), [4, 3, 3]);
        assert_eq!(spread(9, 3), [3, 3, 3]);
        assert_eq!(spread(2, 3), [1, 1, 0]);
        assert!(spread(5, 0).is_empty());
    }

    #[test]
    fn a_declaration_past_the_limits_or_with_a_bad_plan_is_refused_by_its_number() {
        let declared = |plan: &str, count: u32, bytes: u64| Declared {
            plan: plan.to_owned(),
            time: "2026-08-01T02:00:00Z".to_owned(),
            files: Files {
                count,
                bytes: Bytes(bytes),
            },
            failed: false,
        };
        let history = |snapshot: Declared| History {
            host: "demo-host".to_owned(),
            snapshots: vec![declared("home", 1, 1), snapshot],
            series: Vec::new(),
        };
        for (snapshot, why) in [
            (declared("Home Folder", 1, 1), "the plan"),
            (declared("home", MAX_FILES + 1, 1), "files"),
            (declared("home", 1, MAX_BYTES + 1), "bytes"),
            (declared("home", 0, 1), "bytes in no files"),
        ] {
            let Err(FixtureError(error)) = history(snapshot).planned() else {
                panic!("refused: {why}");
            };
            assert!(error.starts_with("snapshot 2: "), "{error}");
            assert!(error.contains(why), "{error}");
        }
        assert!(
            history(declared("home", MAX_FILES, MAX_BYTES))
                .planned()
                .is_ok()
        );
        assert!(history(declared("home", 0, 0)).planned().is_ok());
    }

    #[test]
    fn a_declaration_whose_sizes_add_up_past_the_limit_is_refused() {
        let mut history = series(1, Vec::new());
        history.series[0].files.bytes = Bytes(MAX_BYTES);
        history.series[0].runs = 10;
        history.snapshots[0].files = Files {
            count: 0,
            bytes: Bytes(0),
        };
        assert!(history.planned().is_ok(), "exactly the limit");
        history.snapshots[0].files = Files {
            count: 1,
            bytes: Bytes(1),
        };
        let Err(FixtureError(error)) = history.planned() else {
            panic!("refused: past the total");
        };
        assert!(error.contains("in all"), "{error}");
    }

    fn series(runs: u32, failed: Vec<u32>) -> History {
        History {
            host: "demo-host".to_owned(),
            snapshots: vec![Declared {
                plan: "photos".to_owned(),
                time: "2026-08-02T12:00:00Z".to_owned(),
                files: Files {
                    count: 1,
                    bytes: Bytes(1),
                },
                failed: false,
            }],
            series: vec![Series {
                plan: "home".to_owned(),
                first: "2026-08-01T02:00:00+02:00".to_owned(),
                every: "1 day".to_owned(),
                runs,
                files: Files {
                    count: 2,
                    bytes: Bytes(10),
                },
                failed,
            }],
        }
    }

    #[test]
    fn a_series_is_its_runs_at_its_interval_merged_oldest_first_with_the_rest() {
        let (_, planned) = series(3, vec![2]).planned().unwrap();
        let seen: Vec<(String, String, bool)> = planned
            .iter()
            .map(|snapshot| {
                (
                    snapshot.plan.as_str().to_owned(),
                    RusticTime::to_string(&snapshot.time),
                    snapshot.failed,
                )
            })
            .collect();
        let at = |plan: &str, time: &str, failed| (plan.to_owned(), time.to_owned(), failed);
        assert_eq!(
            seen,
            [
                at("home", "2026-08-01T02:00:00+02:00", false),
                at("home", "2026-08-02T02:00:00+02:00", true),
                at("photos", "2026-08-02T12:00:00+00:00", false),
                at("home", "2026-08-03T02:00:00+02:00", false),
            ]
        );
    }

    #[test]
    fn a_series_refuses_a_failed_run_it_does_not_have_too_many_runs_and_an_interval_that_stands_still()
     {
        for (history, why) in [
            (series(3, vec![4]), "the failed run 4"),
            (series(3, vec![0]), "the failed run 0"),
            (series(MAX_RUNS + 1, Vec::new()), "runs, more than"),
            (with_interval("0 days"), "doesn't move forward"),
            (with_interval("-1 day"), "doesn't move forward"),
        ] {
            let Err(FixtureError(error)) = history.planned() else {
                panic!("refused: {why}");
            };
            assert!(error.starts_with("series 1: "), "{error}");
            assert!(error.contains(why), "{error}");
        }
    }

    fn with_interval(every: &str) -> History {
        let mut history = series(3, Vec::new());
        every.clone_into(&mut history.series[0].every);
        history
    }
}
