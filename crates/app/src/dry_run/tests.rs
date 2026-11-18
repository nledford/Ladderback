use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use domain::access::TOKEN_BYTES;
use domain::access::api_token::{ApiTokenId, ApiTokenLabel};
use domain::planning::{
    AgentSettings, DRY_RUN_WINDOW, DryRunFolder, GlobFile, Hooks, MAX_DRY_RUNS_PER_HOUR,
};
use domain::{PlanId, RepositoryAlias};
use futures::FutureExt;
use tokio::sync::Notify;

use super::*;
use crate::fakes::{FakeClock, InMemoryConfigStore, InMemoryTree};
use crate::ports::SourcesError;
use crate::schedule::config_changes;

fn epoch() -> Timestamp {
    Timestamp::constant(1_800_000_000, 0)
}

fn source(text: &str) -> Source {
    Source::parse(text).expect("a source")
}

/// Files under `/srv/demo`: `a.txt` (3 bytes), `docs/b.txt` (5),
/// `docs/c.txt` (7) and `cache/x` (11).
fn tree() -> InMemoryTree {
    let folder = |path: &str| (path.to_owned(), None);
    let file = |path: &str, size| (path.to_owned(), Some(size));
    InMemoryTree::new([
        folder("/srv/demo"),
        file("/srv/demo/a.txt", 3),
        folder("/srv/demo/cache"),
        file("/srv/demo/cache/x", 11),
        folder("/srv/demo/docs"),
        file("/srv/demo/docs/b.txt", 5),
        file("/srv/demo/docs/c.txt", 7),
        file("/srv/demo-2/y", 13),
        file("/etc/z", 17),
    ])
}

/// Resolves `/home/demo/link` to `/srv/demo/docs` and `/srv/demo/escape`
/// to `/etc`, as links on the disk would; `/missing` can't be resolved;
/// anything else is as written.
struct Links;

#[async_trait]
impl NormalizeSources for Links {
    async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
        sources
            .iter()
            .map(|path| match path.as_str() {
                "/home/demo/link" => Ok(source("/srv/demo/docs")),
                "/srv/demo/escape" => Ok(source("/etc")),
                text if text.starts_with("/missing") => Err(SourcesError::Unreadable {
                    detail: "not there".to_owned(),
                }),
                _ => Ok(path.clone()),
            })
            .collect()
    }
}

fn plan(sources: &[&str], globs: &[&str]) -> Plan {
    let mut plan = Plan::new(
        PlanId::parse("draft").expect("an ID"),
        RepositoryAlias::parse("nas").expect("an alias"),
        sources.iter().map(|text| source(text)).collect(),
    );
    plan.filter.globs = globs.iter().map(|glob| (*glob).to_owned()).collect();
    plan
}

/// A configuration whose allowlist is `folders`.
async fn config(folders: &[&str]) -> Arc<InMemoryConfigStore> {
    let allowlist = DryRunAllowlist::new(
        folders
            .iter()
            .map(|folder| DryRunFolder::parse(folder).expect("a folder"))
            .collect(),
    )
    .expect("an allowlist");
    let settings = AgentSettings {
        dry_run_allowlist: allowlist,
        ..AgentSettings::default()
    };
    let config = AgentConfig::new(settings, Hooks::default(), BTreeMap::new(), Vec::new())
        .expect("a configuration");
    let store = Arc::new(InMemoryConfigStore::new());
    store.save(&config).await.expect("saved");
    store
}

struct Setup {
    walk: Arc<dyn WalkSources>,
    clock: Arc<FakeClock>,
    backups: Arc<AtomicBool>,
    limits: DryRunLimits,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            walk: Arc::new(tree()),
            clock: Arc::new(FakeClock::new(epoch())),
            backups: Arc::new(AtomicBool::new(false)),
            limits: DryRunLimits::DAEMON,
        }
    }
}

impl Setup {
    async fn dry_runs(&self, folders: &[&str]) -> PlanDryRuns {
        let backups = Arc::clone(&self.backups);
        PlanDryRuns::new(
            config(folders).await,
            Arc::new(Links),
            Arc::clone(&self.walk),
            self.clock.clone(),
            Arc::new(move || backups.load(Ordering::Acquire)),
            self.limits,
        )
    }
}

fn token(seed: u8) -> Proposer {
    Proposer::Token {
        id: ApiTokenId::from_bytes(&[seed; TOKEN_BYTES]),
        label: ApiTokenLabel::new(format!("demo-agent-{seed}")).expect("a label"),
    }
}

fn tally(setting: &'static str, index: usize, files: u64, bytes: u64) -> ExcludeTally {
    ExcludeTally {
        exclude: ExcludeSetting {
            setting,
            index: Some(index),
        },
        files,
        bytes,
    }
}

#[tokio::test]
async fn a_dry_run_counts_the_plan_and_what_each_exclude_removes_alone() {
    let dry_runs = Setup::default().dry_runs(&["/srv/demo"]).await;
    let counted = dry_runs
        .run(
            &Proposer::NoToken,
            &plan(&["/srv/demo"], &["!docs", "!nothing-here"]),
        )
        .await;
    assert_eq!(
        counted,
        Ok(PlanDryRun {
            files: 2,
            bytes: 14,
            unreadable: 0,
            excludes: vec![tally("globs", 0, 2, 12), tally("globs", 1, 0, 0)],
            stopped: None,
            warnings: 0,
        })
    );
}

#[tokio::test]
async fn a_source_reached_through_a_link_into_the_allowlist_is_walked_there() {
    let dry_runs = Setup::default().dry_runs(&["/srv/demo"]).await;
    let counted = dry_runs
        .run(&Proposer::NoToken, &plan(&["/home/demo/link"], &[]))
        .await
        .expect("a dry run");
    assert_eq!((counted.files, counted.bytes), (2, 12));
}

#[tokio::test]
async fn a_path_outside_the_allowlist_once_resolved_is_refused_by_its_place() {
    let dry_runs = Setup::default().dry_runs(&["/srv/demo/"]).await;
    let refused = |setting, index| Err(DryRunError::NotAllowed { setting, index });
    for (sources, at) in [
        (&["/srv/demo/docs", "/srv/demo-2"][..], 1),
        (&["/srv/demo/escape"][..], 0),
        (&["/srv/demo", "/missing/x"][..], 1),
        (&["/srv"][..], 0),
    ] {
        assert_eq!(
            dry_runs.run(&Proposer::NoToken, &plan(sources, &[])).await,
            refused("sources", at),
            "{sources:?}"
        );
    }
    let mut reads_a_glob_file = plan(&["/srv/demo"], &[]);
    reads_a_glob_file.filter.iglob_files = vec![
        GlobFile::parse("/srv/demo/globs").expect("a glob file"),
        GlobFile::parse("/etc/passwd").expect("a glob file"),
    ];
    assert_eq!(
        dry_runs.run(&Proposer::NoToken, &reads_a_glob_file).await,
        refused("iglob-files", 1)
    );
}

#[tokio::test]
async fn with_no_folder_allowed_every_dry_run_is_refused() {
    let dry_runs = Setup::default().dry_runs(&[]).await;
    assert_eq!(
        dry_runs
            .run(&Proposer::NoToken, &plan(&["/srv/demo"], &[]))
            .await,
        Err(DryRunError::NotAllowed {
            setting: "sources",
            index: 0
        })
    );
}

#[tokio::test]
async fn a_glob_file_outside_or_a_marker_that_is_not_a_name_alone_is_refused() {
    let dry_runs = Setup::default().dry_runs(&["/srv/demo"]).await;
    let mut globs = plan(&["/srv/demo"], &[]);
    globs.filter.glob_files =
        vec![domain::planning::GlobFile::parse("/srv/other/globs").expect("a path")];
    assert_eq!(
        dry_runs.run(&token(1), &globs).await,
        Err(DryRunError::NotAllowed {
            setting: "glob-files",
            index: 0,
        })
    );
    let mut markers = plan(&["/srv/demo"], &[]);
    markers.filter.exclude_if_present = vec![".nobackup".to_owned(), "../outside".to_owned()];
    assert_eq!(
        dry_runs.run(&token(1), &markers).await,
        Err(DryRunError::NotAllowed {
            setting: "exclude-if-present",
            index: 1,
        })
    );
}

/// rustic's walker reads ignore files in every folder above the sources
/// for these settings, outside the allowlist: refused before any walk.
#[tokio::test]
async fn a_filter_that_reads_above_its_sources_is_refused_before_anything_is_read() {
    let walks = Arc::new(Counted::default());
    let setup = Setup {
        walk: walks.clone(),
        ..Setup::default()
    };
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let reads_above = [
        (
            "custom-ignorefiles",
            SourceFilter {
                custom_ignorefiles: vec![".ignore".to_owned()],
                ..SourceFilter::default()
            },
        ),
        (
            "git-ignore",
            SourceFilter {
                git_ignore: true,
                ..SourceFilter::default()
            },
        ),
        (
            "no-require-git",
            SourceFilter {
                no_require_git: true,
                ..SourceFilter::default()
            },
        ),
    ];
    for (setting, filter) in reads_above {
        let mut draft = plan(&["/srv/demo"], &[]);
        draft.filter = filter;
        assert_eq!(
            dry_runs.run(&token(1), &draft).await,
            Err(DryRunError::ReadsAbove { setting })
        );
    }
    assert_eq!(walks.walks.load(Ordering::Acquire), 0, "nothing was walked");
}

/// The tree, each walk counted and raising one warning.
#[derive(Default)]
struct Counted {
    walks: std::sync::atomic::AtomicU64,
}

#[async_trait]
impl WalkSources for Counted {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        left_out: &[String],
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        self.walks.fetch_add(1, Ordering::AcqRel);
        let walked = tree()
            .walk(sources, filter, left_out, limits, give_way)
            .await?;
        Ok(Walked {
            warnings: 1,
            ..walked
        })
    }
}

#[tokio::test]
async fn the_warnings_of_every_walk_of_a_dry_run_are_counted_together() {
    let walks = Arc::new(Counted::default());
    let setup = Setup {
        walk: walks.clone(),
        ..Setup::default()
    };
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let counted = dry_runs
        .run(
            &Proposer::NoToken,
            &plan(&["/srv/demo"], &["!docs", "!cache"]),
        )
        .await
        .expect("a dry run");
    // The plan, then none of its excludes, then each of the two alone.
    assert_eq!(walks.walks.load(Ordering::Acquire), 4);
    assert_eq!(counted.warnings, 4);
}

/// The glob that leaves out [`WithKeyFile`]'s key file.
const KEY_GLOB: &str = "!/srv/demo/hub-key*";

/// The tree with a key file, `/srv/demo/hub-key` (17 bytes), which a walk
/// leaves out only when it is given [`KEY_GLOB`] alone to leave out after
/// the filter; any other filter is the tree's to apply.
struct WithKeyFile;

#[async_trait]
impl WalkSources for WithKeyFile {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        left_out: &[String],
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        if left_out == [KEY_GLOB] {
            return tree().walk(sources, filter, &[], limits, give_way).await;
        }
        let file = |path: &str, size| (path.to_owned(), Some(size));
        let folder = |path: &str| (path.to_owned(), None);
        InMemoryTree::new([
            folder("/srv/demo"),
            file("/srv/demo/a.txt", 3),
            file("/srv/demo/hub-key", 17),
            folder("/srv/demo/cache"),
            file("/srv/demo/cache/x", 11),
            folder("/srv/demo/docs"),
            file("/srv/demo/docs/b.txt", 5),
            file("/srv/demo/docs/c.txt", 7),
        ])
        .walk(sources, filter, left_out, limits, give_way)
        .await
    }
}

/// A dry run counts what a backup takes, so it leaves out the key files
/// each backup does (§16, decision 120), in every walk, and its tally of
/// what each exclude removes counts the plan's own alone.
#[tokio::test]
async fn a_dry_run_leaves_out_what_every_backup_leaves_out() {
    let setup = Setup {
        walk: Arc::new(WithKeyFile),
        ..Setup::default()
    };
    let dry_runs = setup
        .dry_runs(&["/srv/demo"])
        .await
        .with_left_out(vec![KEY_GLOB.to_owned()]);
    let counted = dry_runs
        .run(&Proposer::NoToken, &plan(&["/srv/demo"], &["!docs"]))
        .await;
    assert_eq!(
        counted,
        Ok(PlanDryRun {
            files: 2,
            bytes: 14,
            unreadable: 0,
            excludes: vec![tally("globs", 0, 2, 12)],
            stopped: None,
            warnings: 0,
        })
    );
}

/// How long a test waits on a held walk or on a dry run: both run in
/// memory on a fake clock and answer at once, so one that waits on what
/// never comes fails the test here instead of hanging it.
const ANSWERED_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// `work`'s output, failing the test when it doesn't come within
/// [`ANSWERED_WITHIN`].
async fn within<T>(what: &str, work: impl Future<Output = T>) -> T {
    tokio::time::timeout(ANSWERED_WITHIN, work)
        .await
        .unwrap_or_else(|_| panic!("{what} didn't come within {ANSWERED_WITHIN:?}"))
}

/// A walk that waits, once it has started, until it is let go.
#[derive(Default)]
struct Held {
    started: Notify,
    go: Notify,
}

#[async_trait]
impl WalkSources for Held {
    async fn walk(
        &self,
        _: &[Source],
        _: &SourceFilter,
        _: &[String],
        _: WalkLimits,
        _: GiveWay,
    ) -> Result<Walked, WalkError> {
        self.started.notify_one();
        self.go.notified().await;
        Ok(Walked::default())
    }
}

#[tokio::test]
async fn a_dry_run_while_another_runs_is_refused_and_the_next_one_runs() {
    let held = Arc::new(Held::default());
    let setup = Setup {
        walk: held.clone(),
        ..Setup::default()
    };
    let dry_runs = Arc::new(setup.dry_runs(&["/srv/demo"]).await);
    let first = tokio::spawn({
        let dry_runs = Arc::clone(&dry_runs);
        async move {
            dry_runs
                .run(&Proposer::NoToken, &plan(&["/srv/demo"], &[]))
                .await
        }
    });
    within("the first walk", held.started.notified()).await;
    assert_eq!(
        within(
            "the second dry run",
            dry_runs.run(&token(1), &plan(&["/srv/demo"], &[]))
        )
        .await,
        Err(DryRunError::Running)
    );
    held.go.notify_one();
    assert_eq!(
        within("the first dry run", first).await.expect("joined"),
        Ok(PlanDryRun::default())
    );
    // Let go before it starts: the permit waits for it.
    held.go.notify_one();
    assert_eq!(
        within(
            "the next dry run",
            dry_runs.run(&token(1), &plan(&["/srv/demo"], &[]))
        )
        .await,
        Ok(PlanDryRun::default())
    );
}

/// A walk that, as the engine's blocking walk does, keeps its give-way
/// after its caller's future is dropped, until the test takes it.
#[derive(Default)]
struct Orphaned {
    started: Notify,
    kept: std::sync::Mutex<Option<GiveWay>>,
}

#[async_trait]
impl WalkSources for Orphaned {
    async fn walk(
        &self,
        _: &[Source],
        _: &SourceFilter,
        _: &[String],
        _: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        *self.kept.lock().expect("unpoisoned") = Some(give_way);
        self.started.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_dry_run_dropped_mid_walk_keeps_its_turn_until_its_walk_lets_go_and_asks_it_to_stop() {
    let orphaned = Arc::new(Orphaned::default());
    let setup = Setup {
        walk: orphaned.clone(),
        ..Setup::default()
    };
    let dry_runs = Arc::new(setup.dry_runs(&["/srv/demo"]).await);
    let run = |asker: Proposer| {
        let dry_runs = Arc::clone(&dry_runs);
        tokio::spawn(async move { dry_runs.run(&asker, &plan(&["/srv/demo"], &[])).await })
    };
    let first = run(Proposer::NoToken);
    within("the first walk", orphaned.started.notified()).await;
    first.abort();
    assert!(
        within("the abort", first).await.is_err(),
        "the caller hung up"
    );
    let give_way = orphaned
        .kept
        .lock()
        .expect("unpoisoned")
        .take()
        .expect("the walk keeps its give-way");
    assert!(give_way(), "the walk is asked to stop");
    assert_eq!(
        within(
            "the second dry run",
            dry_runs.run(&token(1), &plan(&["/srv/demo"], &[]))
        )
        .await,
        Err(DryRunError::Running),
        "the turn is the walk's while it reads on"
    );
    drop(give_way);
    let next = run(token(1));
    within("the next walk", orphaned.started.notified()).await;
    next.abort();
}

#[tokio::test]
async fn no_dry_run_starts_while_a_backup_holds_a_place() {
    let setup = Setup::default();
    setup.backups.store(true, Ordering::Release);
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    assert_eq!(
        dry_runs
            .run(&Proposer::NoToken, &plan(&["/srv/demo"], &[]))
            .await,
        Err(DryRunError::BackupGoing)
    );
}

/// The tree, a backup taking a place in the queue as its walk starts.
struct BackupArrives {
    backups: Arc<AtomicBool>,
}

#[async_trait]
impl WalkSources for BackupArrives {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        left_out: &[String],
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        self.backups.store(true, Ordering::Release);
        tree()
            .walk(sources, filter, left_out, limits, give_way)
            .await
    }
}

#[tokio::test]
async fn a_dry_run_gives_way_to_a_backup_that_takes_a_place() {
    let mut setup = Setup::default();
    setup.walk = Arc::new(BackupArrives {
        backups: Arc::clone(&setup.backups),
    });
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let counted = dry_runs
        .run(&Proposer::NoToken, &plan(&["/srv/demo"], &["!docs"]))
        .await
        .expect("a dry run");
    assert_eq!(
        (counted.files, counted.excludes, counted.stopped),
        (0, Vec::new(), Some(WalkStop::GaveWay))
    );
}

#[tokio::test]
async fn a_proposer_starts_its_hourly_limit_of_dry_runs_and_others_still_run() {
    let setup = Setup::default();
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let draft = plan(&["/srv/demo"], &[]);
    for _ in 0..MAX_DRY_RUNS_PER_HOUR {
        assert!(dry_runs.run(&token(1), &draft).await.is_ok());
    }
    let until = epoch() + DRY_RUN_WINDOW;
    assert_eq!(
        dry_runs.run(&token(1), &draft).await,
        Err(DryRunError::TooSoon { until })
    );
    // The same token under another label is the same proposer.
    let relabelled = Proposer::Token {
        id: ApiTokenId::from_bytes(&[1; TOKEN_BYTES]),
        label: ApiTokenLabel::new("renamed".to_owned()).expect("a label"),
    };
    assert_eq!(
        dry_runs.run(&relabelled, &draft).await,
        Err(DryRunError::TooSoon { until })
    );
    assert!(dry_runs.run(&token(2), &draft).await.is_ok());
    assert!(dry_runs.run(&Proposer::NoToken, &draft).await.is_ok());
    setup.clock.advance(DRY_RUN_WINDOW);
    assert!(dry_runs.run(&token(1), &draft).await.is_ok());
}

/// With sign-in off no client presents a token, so every client is the one
/// proposer: one hourly limit across them all (decision 89, point 13).
#[tokio::test]
async fn without_a_token_every_client_shares_one_hourly_limit() {
    let setup = Setup::default();
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let draft = plan(&["/srv/demo"], &[]);
    for _ in 0..MAX_DRY_RUNS_PER_HOUR {
        assert!(dry_runs.run(&Proposer::NoToken, &draft).await.is_ok());
    }
    assert_eq!(
        dry_runs.run(&Proposer::NoToken, &draft).await,
        Err(DryRunError::TooSoon {
            until: epoch() + DRY_RUN_WINDOW
        })
    );
    assert!(dry_runs.run(&token(1), &draft).await.is_ok());
}

#[tokio::test]
async fn a_walk_that_reads_its_limit_of_entries_stops_the_dry_run_there() {
    let setup = Setup {
        limits: DryRunLimits {
            entries: 6,
            ..DryRunLimits::DAEMON
        },
        ..Setup::default()
    };
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    // Six entries below `/srv/demo`: the plan's walk reads them all; with
    // a seventh the walk stops, and no exclude is counted.
    let whole = dry_runs
        .run(&Proposer::NoToken, &plan(&["/srv/demo"], &["!docs"]))
        .await
        .expect("a dry run");
    assert_eq!(
        (whole.files, whole.stopped, whole.excludes.len()),
        (2, None, 1)
    );
    let short = Setup {
        limits: DryRunLimits {
            entries: 5,
            ..DryRunLimits::DAEMON
        },
        ..Setup::default()
    };
    let counted = short
        .dry_runs(&["/srv/demo"])
        .await
        .run(&Proposer::NoToken, &plan(&["/srv/demo"], &["!docs"]))
        .await
        .expect("a dry run");
    assert_eq!(
        (counted.stopped, counted.excludes),
        (Some(WalkStop::Entries), Vec::new())
    );
}

/// The tree, each walk taking 8 seconds of the clock, and the time each
/// was given.
struct Slow {
    clock: Arc<FakeClock>,
    given: std::sync::Mutex<Vec<SignedDuration>>,
}

#[async_trait]
impl WalkSources for Slow {
    async fn walk(
        &self,
        sources: &[Source],
        filter: &SourceFilter,
        left_out: &[String],
        limits: WalkLimits,
        give_way: GiveWay,
    ) -> Result<Walked, WalkError> {
        self.given
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(limits.time);
        self.clock.advance(SignedDuration::from_secs(8));
        tree()
            .walk(sources, filter, left_out, limits, give_way)
            .await
    }
}

#[tokio::test]
async fn the_walks_of_one_dry_run_share_its_time_and_it_stops_when_none_is_left() {
    let clock = Arc::new(FakeClock::new(epoch()));
    let slow = Arc::new(Slow {
        clock: clock.clone(),
        given: std::sync::Mutex::new(Vec::new()),
    });
    let setup = Setup {
        walk: slow.clone(),
        clock,
        ..Setup::default()
    };
    let dry_runs = setup.dry_runs(&["/srv/demo"]).await;
    let counted = dry_runs
        .run(
            &Proposer::NoToken,
            &plan(&["/srv/demo"], &["!docs", "!cache"]),
        )
        .await
        .expect("a dry run");
    assert_eq!(
        (counted.files, counted.stopped, counted.excludes),
        (1, Some(WalkStop::Time), vec![tally("globs", 0, 2, 12)])
    );
    let seconds = |s| SignedDuration::from_secs(s);
    assert_eq!(
        *slow.given.lock().unwrap_or_else(PoisonError::into_inner),
        [seconds(20), seconds(12), seconds(4)]
    );
}

#[tokio::test]
async fn a_filter_the_engine_can_t_apply_is_its_own_refusal() {
    let dry_runs = Setup::default().dry_runs(&["/srv/demo"]).await;
    assert_eq!(
        dry_runs
            .run(&Proposer::NoToken, &plan(&["/srv/demo"], &["docs/**/[x"]))
            .await,
        Err(DryRunError::Filter)
    );
}

#[tokio::test]
async fn setting_the_allowlist_saves_it_and_tells_the_scheduler() {
    let store = config(&[]).await;
    let (changed, changes) = config_changes();
    let allowlists = DryRunAllowlists::new(store.clone(), Arc::default(), changed);
    let allowlist = DryRunAllowlist::new(vec![DryRunFolder::parse("/srv/demo").expect("a folder")])
        .expect("an allowlist");
    allowlists.set(allowlist.clone()).await.expect("saved");
    assert_eq!(allowlists.allowlist().await, Ok(allowlist));
    assert!(changes.next().now_or_never().is_some());
}
