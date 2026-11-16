use std::assert_matches;

use super::support::*;
use super::*;

/// How a [`ReadsStats`] engine answers a job.
type StatsAnswer = fn() -> Result<EngineEvent, EngineError>;

/// An engine that reads a repository's stats as the function says, and
/// answers every job so.
struct ReadsStats(StatsAnswer);

#[async_trait]
impl Engine for ReadsStats {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        let event = (self.0)()?;
        Ok(RunningJob {
            events: futures::stream::iter([event]).boxed(),
            control: Box::new(Parked),
        })
    }
}

/// The figures [`ReadsStats`] reads: two snapshots of 2.3 MB each, which
/// the repository stores in 7 % of their raw size.
fn two_snapshots() -> Result<EngineEvent, EngineError> {
    Ok(EngineEvent::Succeeded(EngineResult::Stats(
        domain::maintenance::RepositoryStats {
            snapshots: 2,
            total_size: 4_600_005,
            stored_size: 300_542,
            raw_size: 4_300_005,
        },
    )))
}

/// The stats read through the daemon are kept, and from then on told as
/// the repository's last read: before any read, none are.
#[tokio::test]
async fn the_stats_read_are_kept_and_told_as_last_read() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(ReadsStats(two_snapshots));
    assert_eq!(
        runner.stats("demo".to_owned()).await,
        Ok(api::RepositoryStatsDto { latest: None })
    );

    let read = runner
        .read_stats("demo".to_owned())
        .await
        .expect("the stats are read");

    assert_eq!(
        (
            read.snapshots,
            read.total_size,
            read.stored_size,
            read.compression_saved_percent
        ),
        (2, 4_600_005, 300_542, Some(93))
    );
    assert!(
        read.taken_at.parse::<domain::Timestamp>().is_ok(),
        "taken at a time: {}",
        read.taken_at
    );
    assert_eq!(
        runner.stats("demo".to_owned()).await,
        Ok(api::RepositoryStatsDto { latest: Some(read) })
    );
}

/// A read the engine can't start, or one that fails, is refused as the
/// other maintenance reads are, and keeps nothing.
#[tokio::test]
async fn a_stats_read_that_fails_is_refused_and_keeps_nothing() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    let cases: [(StatsAnswer, MaintenanceRefusal); 2] = [
        (
            || {
                Err(EngineError::Unavailable {
                    detail: "no worker".to_owned(),
                })
            },
            MaintenanceRefusal::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: Some("no worker".to_owned()),
            },
        ),
        (
            || {
                Ok(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "an index file can't be read".to_owned(),
                }))
            },
            MaintenanceRefusal::OperationFailed {
                detail: "an index file can't be read".to_owned(),
            },
        ),
    ];
    for (read, refused) in cases {
        runner.engine = Arc::new(ReadsStats(read));
        assert_eq!(runner.read_stats("demo".to_owned()).await, Err(refused));
        assert_eq!(
            runner.stats("demo".to_owned()).await,
            Ok(api::RepositoryStatsDto { latest: None })
        );
    }
}

/// A history that refuses every reading and every look.
struct RefusingReadings;

#[async_trait]
impl app::ports::StatsStore for RefusingReadings {
    async fn record(
        &self,
        _reading: &domain::maintenance::StatsReading,
    ) -> Result<(), app::ports::StoreError> {
        Err(refused_reading())
    }

    async fn latest(
        &self,
        _alias: &RepositoryAlias,
    ) -> Result<Option<domain::maintenance::StatsReading>, app::ports::StoreError> {
        Err(refused_reading())
    }

    async fn history(
        &self,
        _alias: &RepositoryAlias,
    ) -> Result<Vec<domain::maintenance::StatsReading>, app::ports::StoreError> {
        Err(refused_reading())
    }
}

fn refused_reading() -> app::ports::StoreError {
    app::ports::StoreError::Unavailable {
        detail: "the disk is full".to_owned(),
    }
}

/// A history that can't keep the reading, or tell the last one, is
/// refused in its own words: the stats read aren't answered as kept.
#[tokio::test]
async fn stats_the_history_refuses_are_refused_in_its_words() {
    let (mut runner, _entered, _gate, _scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(ReadsStats(two_snapshots));
    runner.readings = Arc::new(RefusingReadings);
    let refused = MaintenanceRefusal::Unavailable {
        code: ErrorCode::OperationFailed,
        args: Vec::new(),
        detail: Some("the disk is full".to_owned()),
    };
    assert_eq!(
        runner.read_stats("demo".to_owned()).await,
        Err(refused.clone())
    );
    assert_eq!(runner.stats("demo".to_owned()).await, Err(refused));
}

/// One repository under two names never reaches the history view (I6,
/// §7.3): a file with a second entry at the first one's location is not a
/// configuration, so neither name's groups are read, and no plan of the
/// one can show as orphaned history adoptable into the other. The refusal
/// is the mistake's, as a save's is (#347).
#[tokio::test]
async fn a_repository_under_two_names_never_reaches_the_history_view() {
    let (mut runner, _entered, _gate, scratch) = runner_at_a_gate().await;
    runner.engine = Arc::new(Empty);
    let location = scratch.path().join("repo").display().to_string();
    let (config_dir, _) = own_folders(scratch.path());
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            "schema = 1\n\n\
                 [repositories.demo]\nrepository = {location:?}\npassword = \"demo-password\"\n\
                 maintenance = \"local\"\n\n\
                 [repositories.other]\nrepository = {location:?}\npassword = \"demo-password\"\n\n\
                 [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n"
        ),
    )
    .expect("the config file");
    for alias in ["demo", "other"] {
        let refused = runner.groups(alias.to_owned()).await;
        assert_matches!(
            refused,
            Err(MaintenanceRefusal::Unavailable {
                code: ErrorCode::ConfigInvalid,
                ..
            }),
            "the groups of {alias}: {refused:?}"
        );
    }
}
