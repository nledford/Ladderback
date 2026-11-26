//! The sweep that gives up the downloads nobody fetched within their
//! minute (§10.4, decision 114): the agent's, never a request's (#846).

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicUsize, Ordering};

use app::fakes::{FakeClock, InMemoryAdminPassword, InMemorySessionStore, SeededRandom};
use app::ports::{DumpHeader, HistoryQuery, PageSize};
use domain::restore::download::{ArchiveFormat, DownloadShape};
use http_adapter::{DownloadStart, Downloads as _};

use super::support::*;
use super::*;

/// An engine whose download jobs find their entry, a file of 11 bytes, and
/// then hand nothing over, as while the browser hasn't fetched it. It
/// counts the jobs it ran and those stopped: a job's events dropped.
struct Found {
    jobs: Arc<AtomicUsize>,
    stopped: Arc<AtomicUsize>,
}

/// Counts a job stopped once its events are dropped.
struct Stopping(Arc<AtomicUsize>);

impl Drop for Stopping {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl Engine for Found {
    async fn run(&self, _op: OperationId, _job: Job) -> Result<RunningJob, EngineError> {
        self.jobs.fetch_add(1, Ordering::SeqCst);
        let stopping = Stopping(Arc::clone(&self.stopped));
        let found = EngineEvent::Dumping(DumpHeader {
            shape: DownloadShape::File { size: 11 },
            name: None,
            host: "laptop".to_owned(),
        });
        let events = futures::stream::iter([found])
            .chain(futures::stream::pending())
            .map(move |event| {
                // The guard lives as long as the events do.
                let _ = &stopping;
                event
            });
        Ok(RunningJob {
            events: events.boxed(),
            control: Box::new(Parked),
        })
    }
}

/// A download nobody fetches within its minute is given up by the sweep
/// alone, within 10 s of its minute (§10.4): with no other download asked
/// for and no fetch, time passing its minute stops its job and ends it
/// cancelled in the history (#888).
#[tokio::test]
async fn the_sweep_gives_up_a_download_nobody_fetched_within_its_minute() {
    let (mut runner, _entered, _open_gate, _scratch) = runner_at_a_gate().await;
    let jobs = Arc::new(AtomicUsize::new(0));
    let stopped = Arc::new(AtomicUsize::new(0));
    runner.engine = Arc::new(Found {
        jobs: Arc::clone(&jobs),
        stopped: Arc::clone(&stopped),
    });
    let clock = Arc::new(FakeClock::new(
        domain::Timestamp::from_second(1_800_000_000).expect("a time"),
    ));
    let sign_in = SignIn::Off {
        password: Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        sessions: Arc::new(InMemorySessionStore::new()),
    };
    let service = downloads::DownloadService::new(
        &runner,
        &sign_in,
        Arc::new(SeededRandom::new(7)),
        Arc::clone(&clock) as Arc<dyn Clock>,
    );
    let snapshot = domain::SnapshotId::parse(&"3f2a1b4c".repeat(8)).expect("a snapshot ID");
    let confirmation = domain::access::step_up::StepUpConfirmation::parse(
        domain::access::step_up::short_id(&snapshot).to_owned(),
    )
    .expect("a confirmation");
    service
        .start(
            "demo".to_owned(),
            DownloadStart {
                snapshot,
                entry: FolderPath::parse("home/notes.md").expect("an entry path"),
                format: ArchiveFormat::Tar,
                address: IpAddr::V4(Ipv4Addr::LOCALHOST),
                confirmation,
            },
        )
        .await
        .expect("the download is confirmed");
    let history = runner
        .store
        .list(&HistoryQuery {
            plan: None,
            after: None,
            limit: PageSize::DEFAULT,
        })
        .await
        .expect("the log reads")
        .operations;
    let [download] = history.as_slice() else {
        panic!("one operation in {history:?}");
    };
    assert_eq!(download.status, OperationStatus::Running);

    // Time moves from one of the sweep's wakes to the next, each within
    // 10 s, until the download's minute has run out.
    let ten_seconds = domain::SignedDuration::from_secs(10);
    let mut waited = domain::SignedDuration::ZERO;
    while waited <= domain::SignedDuration::from_secs(60) {
        // Moving time before the sweep waits on the clock would skip its
        // wake.
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while clock.sleepers() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the sweep waits for its next turn");
        let next = clock
            .until_next_sleeper()
            .expect("the sweep waits on the clock");
        assert!(
            next <= ten_seconds,
            "the sweep comes back within 10 s, not {next:#}"
        );
        clock.advance(next);
        waited += next;
    }
    assert_eq!(ended(&runner, download.id).await, OperationStatus::Canceled);
    assert_eq!(
        stopped.load(Ordering::SeqCst),
        1,
        "the download's job was stopped"
    );
    assert_eq!(jobs.load(Ordering::SeqCst), 1, "no other job ran");
}
