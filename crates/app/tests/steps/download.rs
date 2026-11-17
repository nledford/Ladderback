//! Steps for the untagged scenarios of `features/restore/download.feature`:
//! the download use case with a scripted engine and the scenario's fake
//! clock. The snapshot, the sign-in and the engine's record of jobs are the
//! restore scenarios' (`restore.rs`), whose steps set them up.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use app::access::Access;
use app::download::{Download, DownloadAsk, DownloadError, Downloads};
use app::fakes::{
    FakeClock, FakeSecrets, InMemoryAdminPassword, InMemoryApiTokenStore, InMemoryOperationStore,
    InMemorySessionStore, SeededRandom,
};
use app::ports::{
    Clock, DumpAnswer, DumpHeader, DumpJob, Engine, EngineError, EngineEvent, EngineResult,
    HistoryPage, HistoryQuery, Job, JobControl, OperationDetail, OperationStore, Recovered,
    RunningJob, Start, StoreError,
};
use app::secrets::ResolveRepository;
use async_trait::async_trait;
use cucumber::{given, then, when};
use domain::browse::{DisplayName, FolderPath};
use domain::execution::{FailureReason, Operation, OperationKind, OperationStatus, Trigger};
use domain::restore::download::{ArchiveFormat, DownloadShape, DownloadTicket};
use domain::{OperationId, PlanId, RepositoryId, SignedDuration, SnapshotId, Uuid};
use futures::{FutureExt, StreamExt, stream};
use tokio::task::JoinHandle;

use super::Agent;

/// The scripted snapshot's one file and what it holds.
const NOTES: &str = "home/docs/notes.md";
const NOTES_BYTES: &[u8] = b"hello world";
/// Files the engine doesn't end as it said it would.
const TORN: &str = "home/docs/torn.md";
const GROWN: &str = "home/docs/grown.md";

/// What the download scenarios confirmed and were handed.
#[derive(Default)]
pub struct DownloadWorld {
    /// The use case, made at the first download so its turns and tickets
    /// last the scenario.
    downloads: Option<Arc<Downloads>>,
    /// How each confirmation answered, oldest first.
    confirmed: Vec<Result<DownloadTicket, DownloadError>>,
    /// The download handed over last, or why it wasn't.
    fetched: Option<Result<Download, DownloadError>>,
    /// Every piece handed over, joined.
    bytes: Vec<u8>,
    /// The download ended in an error, short of its end.
    ended_short: bool,
    /// How many of the engine's download jobs were stopped, or dropped,
    /// before their end.
    stopped: Arc<AtomicUsize>,
    runs: u128,
    /// The writes of how each fetched download ended, run beside it.
    recording: Vec<JoinHandle<()>>,
    /// Confirmations running beside the scenario, oldest first, while the
    /// history is slow to write.
    asking: Vec<JoinHandle<Result<DownloadTicket, DownloadError>>>,
    /// While it holds `true`, the operation log answers no write: one is
    /// under way, and answers once it is let go.
    log_held: Arc<tokio::sync::watch::Sender<bool>>,
    /// The clock the downloads read when time moves only as the scenario
    /// says; the scenario's own, which jumps to each sleeper's deadline,
    /// otherwise.
    clock: Option<Arc<FakeClock>>,
}

/// The scenario's operation log, which can be held mid-write, as a log on
/// a busy disk is.
struct SlowLog {
    inner: Arc<InMemoryOperationStore>,
    held: Arc<tokio::sync::watch::Sender<bool>>,
}

#[async_trait]
impl OperationStore for SlowLog {
    async fn save(
        &self,
        operation: &Operation,
        result: Option<&EngineResult>,
    ) -> Result<(), StoreError> {
        // A held write answers once the log is let go, as a busy disk
        // catches up.
        let _ = self.held.subscribe().wait_for(|held| !held).await;
        self.inner.save(operation, result).await
    }

    async fn try_start(&self, operation: &Operation) -> Result<Start, StoreError> {
        self.inner.try_start(operation).await
    }

    async fn running_maintenance(
        &self,
        repository: Option<&RepositoryId>,
    ) -> Result<Option<OperationId>, StoreError> {
        self.inner.running_maintenance(repository).await
    }

    async fn get(&self, id: OperationId) -> Result<Option<OperationDetail>, StoreError> {
        self.inner.get(id).await
    }

    async fn list(&self, query: &HistoryQuery) -> Result<HistoryPage, StoreError> {
        self.inner.list(query).await
    }

    async fn ever_succeeded(&self, plan: &PlanId) -> Result<bool, StoreError> {
        self.inner.ever_succeeded(plan).await
    }

    async fn recover(&self, reason: &FailureReason) -> Result<Recovered, StoreError> {
        self.inner.recover(reason).await
    }
}

/// Counts a job stopped, by its control or by dropping its events before
/// they ended.
struct Stopping {
    stopped: Arc<AtomicUsize>,
    ended: bool,
}

impl Drop for Stopping {
    fn drop(&mut self) {
        if !self.ended {
            self.stopped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

struct Control {
    stopped: Arc<AtomicUsize>,
}

#[async_trait]
impl JobControl for Control {
    async fn stop(&mut self) {
        self.stopped.fetch_add(1, Ordering::SeqCst);
    }
}

/// Lists nothing and hands over the scripted snapshot's entries: the file
/// as itself in one piece, a folder as an archive in two.
struct Scripted {
    snapshot: SnapshotId,
    host: String,
    jobs: Arc<Mutex<Vec<Job>>>,
    stopped: Arc<AtomicUsize>,
    clock: Arc<dyn Clock>,
}

impl Scripted {
    fn events(&self, job: &DumpJob) -> Vec<EngineEvent> {
        let started = EngineEvent::Started {
            index_read_at: self.clock.now(),
            repository: None,
        };
        if job.snapshot != self.snapshot {
            return vec![
                started,
                EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::SnapshotUnknown)),
            ];
        }
        let entry = job.entry.to_string();
        let name = job
            .entry
            .names()
            .last()
            .map(|name| DisplayName::new(name.as_str()));
        let header = |shape| {
            EngineEvent::Dumping(DumpHeader {
                shape,
                name: name.clone(),
                host: self.host.clone(),
            })
        };
        match entry.as_str() {
            NOTES => vec![
                started,
                header(DownloadShape::File {
                    size: NOTES_BYTES.len() as u64,
                }),
                EngineEvent::Piece(NOTES_BYTES.to_vec()),
                EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::Done {
                    bytes: NOTES_BYTES.len() as u64,
                })),
            ],
            // A file the engine fails once it has handed every byte over.
            TORN => vec![
                started,
                header(DownloadShape::File {
                    size: NOTES_BYTES.len() as u64,
                }),
                EngineEvent::Piece(NOTES_BYTES.to_vec()),
                EngineEvent::Failed(FailureReason::EngineExited { exit_code: Some(1) }),
            ],
            // A file the engine hands more of over than its size.
            GROWN => vec![
                started,
                header(DownloadShape::File { size: 5 }),
                EngineEvent::Piece(b"hello".to_vec()),
                EngineEvent::Piece(b" world".to_vec()),
                EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::Done { bytes: 11 })),
            ],
            "" | "home" | "home/docs" | "home/photos" => vec![
                started,
                header(DownloadShape::Archive(job.format)),
                EngineEvent::Piece(b"first piece".to_vec()),
                EngineEvent::Piece(b"second piece".to_vec()),
                EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::Done { bytes: 23 })),
            ],
            _ => vec![
                started,
                EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::EntryUnknown)),
            ],
        }
    }
}

#[async_trait]
impl Engine for Scripted {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(job.clone());
        let Job::Dump(dump) = job else {
            panic!("a download asks for no {:?} job", job.kind());
        };
        let events = self.events(&dump);
        let guard = Stopping {
            stopped: self.stopped.clone(),
            ended: false,
        };
        // The guard lives as long as the events: dropped before the last
        // one was taken, the job counts as stopped.
        let events = stream::unfold(
            (events.into_iter(), guard),
            |(mut events, mut guard)| async move {
                let event = events.next()?;
                if events.len() == 0 {
                    guard.ended = true;
                }
                Some((event, (events, guard)))
            },
        );
        Ok(RunningJob {
            events: events.boxed(),
            control: Box::new(Control {
                stopped: self.stopped.clone(),
            }),
        })
    }
}

fn format(text: &str) -> ArchiveFormat {
    match text {
        "tar" => ArchiveFormat::Tar,
        "zip" => ArchiveFormat::Zip,
        other => panic!("no archive format {other:?}"),
    }
}

fn path(text: &str) -> FolderPath {
    FolderPath::parse(text).expect("an entry path")
}

impl Agent {
    /// The clock the downloads read: the one the scenario moves by hand,
    /// when it has one.
    fn download_clock(&self) -> Arc<FakeClock> {
        self.download
            .clock
            .clone()
            .unwrap_or_else(|| self.clock.clone())
    }

    /// The use case, made the first time it is asked for.
    async fn downloads(&mut self) -> Arc<Downloads> {
        if let Some(downloads) = &self.download.downloads {
            return downloads.clone();
        }
        let (snapshot, host) = self
            .restore
            .snapshot
            .clone()
            .expect("the scenario's snapshot");
        let clock = self.download_clock();
        let engine: Arc<dyn Engine> = Arc::new(Scripted {
            snapshot,
            host,
            jobs: self.restore.jobs.clone(),
            stopped: self.download.stopped.clone(),
            clock: clock.clone(),
        });
        let mut downloads = Downloads::new(
            engine,
            clock.clone(),
            self.restore.config().await,
            ResolveRepository::new(Arc::new(FakeSecrets::default())),
            Arc::new(SeededRandom::new(7)),
            Arc::new(SlowLog {
                inner: self.restore.log.clone(),
                held: self.download.log_held.clone(),
            }),
        );
        if let Some(password) = &self.restore.password {
            downloads = downloads.with_access(Arc::new(Access::new(
                Arc::new(InMemoryAdminPassword::with(password)),
                Arc::new(SeededRandom::new(1)),
                clock,
                Arc::new(InMemoryApiTokenStore::new()),
                Arc::new(InMemorySessionStore::new()),
            )));
        }
        let downloads = Arc::new(downloads);
        self.download.downloads = Some(downloads.clone());
        downloads
    }

    /// What the person asks for when they confirm a download of `entry`
    /// as `format`, under a new operation's ID.
    fn download_ask(
        &mut self,
        entry: FolderPath,
        format: ArchiveFormat,
        confirmation: &str,
    ) -> (OperationId, DownloadAsk) {
        self.download.runs += 1;
        let op = OperationId::from_uuid(Uuid::from_u128(
            0x018f_0000_0000_7000_8000_0000_0000_7000 | self.download.runs,
        ));
        let ask = DownloadAsk {
            alias: domain::RepositoryAlias::parse("demo").expect("an alias"),
            snapshot: self
                .restore
                .snapshot
                .clone()
                .expect("the scenario's snapshot")
                .0,
            entry,
            format,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            confirmation: domain::access::step_up::StepUpConfirmation::parse(
                confirmation.to_owned(),
            )
            .expect("a confirmation"),
        };
        (op, ask)
    }

    /// Confirms a download of `entry` as `format`, keeping how it answered.
    async fn confirm_download(
        &mut self,
        entry: FolderPath,
        format: ArchiveFormat,
        confirmation: &str,
    ) {
        let downloads = self.downloads().await;
        let (op, ask) = self.download_ask(entry, format, confirmation);
        let confirmed = downloads.start(op, ask).await;
        self.download.confirmed.push(confirmed);
    }

    /// Confirms a download of `entry` beside the scenario, as a browser's
    /// request runs while the history is slow to write, and waits until it
    /// has gone as far as the log lets it: answered, or held on a write.
    async fn start_confirming(&mut self, entry: FolderPath, confirmation: &str) {
        let downloads = self.downloads().await;
        let (op, ask) = self.download_ask(entry, ArchiveFormat::Tar, confirmation);
        let log = self.download.log_held.clone();
        // Each write the log holds waits on a receiver of its own.
        let held = log.receiver_count();
        let asking = tokio::spawn(async move { downloads.start(op, ask).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !asking.is_finished() && log.receiver_count() == held {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("the confirmation goes as far as the held log lets it");
        self.download.asking.push(asking);
    }

    /// The ticket the confirmation at `index` answered.
    fn ticket(&self, index: usize) -> DownloadTicket {
        match self.download.confirmed.get(index) {
            Some(Ok(ticket)) => ticket.clone(),
            other => panic!("the download was confirmed: {other:?}"),
        }
    }

    /// Fetches the download confirmed at `index`, keeping it unread; the
    /// write of how it ends runs beside it, as the agent runs it.
    async fn fetch_download(&mut self, index: usize) {
        let downloads = self.downloads().await;
        let ticket = self.ticket(index);
        let mut fetched = downloads.fetch(&ticket);
        if let Ok(download) = fetched.as_mut() {
            let recorded = std::mem::replace(&mut download.recorded, async {}.boxed());
            self.download.recording.push(tokio::spawn(recorded));
        }
        self.download.fetched = Some(fetched);
    }

    /// Waits until every fetched download's end is written to the log.
    async fn recorded(&mut self) {
        for recording in self.download.recording.drain(..) {
            tokio::time::timeout(std::time::Duration::from_secs(5), recording)
                .await
                .expect("the download's end is written once its pieces end or go")
                .expect("the write ran");
        }
    }

    /// Reads the download fetched last to its end.
    async fn read_download(&mut self) {
        let Some(Ok(download)) = self.download.fetched.as_mut() else {
            panic!(
                "a download was handed over: {:?}",
                self.download.fetched.as_ref().map(|f| f.as_ref().err())
            );
        };
        let mut bytes = Vec::new();
        while let Some(piece) = download.pieces.next().await {
            bytes.extend(piece.expect("every piece is handed over"));
        }
        self.download.bytes = bytes;
    }

    /// Confirms a download of `entry`, then fetches it and reads it.
    async fn download_entry(
        &mut self,
        entry: FolderPath,
        format: ArchiveFormat,
        confirmation: &str,
    ) {
        self.confirm_download(entry, format, confirmation).await;
        if matches!(self.download.confirmed.last(), Some(Ok(_))) {
            let last = self.download.confirmed.len() - 1;
            self.fetch_download(last).await;
            self.read_download().await;
        }
    }

    fn download_refusal(&self) -> &DownloadError {
        match self
            .download
            .confirmed
            .last()
            .expect("a download was asked for")
        {
            Err(error) => error,
            Ok(ticket) => panic!("the download was confirmed: {ticket:?}"),
        }
    }

    fn fetch_refusal(&self) -> &DownloadError {
        match self
            .download
            .fetched
            .as_ref()
            .expect("a download was fetched")
        {
            Err(error) => error,
            Ok(download) => panic!("the download was handed over as {:?}", download.file_name),
        }
    }

    fn handed(&self) -> &Download {
        match self.download.fetched.as_ref() {
            Some(Ok(download)) => download,
            Some(Err(error)) => panic!("the download wasn't handed over: {error:?}"),
            None => panic!(
                "the download wasn't handed over: {:?}",
                self.download.confirmed.last()
            ),
        }
    }

    fn dump_job(&self) -> DumpJob {
        self.restore
            .jobs()
            .into_iter()
            .rev()
            .find_map(|job| match job {
                Job::Dump(job) => Some(job),
                _ => None,
            })
            .expect("the engine was given a dump")
    }
}

#[when(expr = "the person downloads {string} of the snapshot, confirming with {string}")]
async fn downloads_file(agent: &mut Agent, entry: String, confirmation: String) {
    agent
        .download_entry(path(&entry), ArchiveFormat::Tar, &confirmation)
        .await;
}

#[when(expr = "the person downloads {string} of the snapshot as {word}, confirming with {string}")]
async fn downloads_as(agent: &mut Agent, entry: String, as_format: String, confirmation: String) {
    agent
        .download_entry(path(&entry), format(&as_format), &confirmation)
        .await;
}

#[when(expr = "the person downloads the top of the snapshot as {word}, confirming with {string}")]
async fn downloads_top(agent: &mut Agent, as_format: String, confirmation: String) {
    agent
        .download_entry(FolderPath::top(), format(&as_format), &confirmation)
        .await;
}

#[when(
    expr = "the person confirms downloads of {string} and {string} of the snapshot as {word}, confirming with {string}"
)]
async fn confirms_two(
    agent: &mut Agent,
    first: String,
    second: String,
    as_format: String,
    confirmation: String,
) {
    for entry in [first, second] {
        agent
            .confirm_download(path(&entry), format(&as_format), &confirmation)
            .await;
        assert!(
            matches!(agent.download.confirmed.last(), Some(Ok(_))),
            "{entry} was confirmed: {:?}",
            agent.download.confirmed.last()
        );
    }
}

#[when("the first download has been handed over to its end")]
async fn first_to_its_end(agent: &mut Agent) {
    agent.fetch_download(0).await;
    agent.read_download().await;
    // Handed over and read: let it go, as the browser's request ends.
    agent.download.fetched = None;
}

#[when("the browser asks for the same download again")]
async fn asks_again(agent: &mut Agent) {
    let last = agent.download.confirmed.len() - 1;
    agent.fetch_download(last).await;
}

#[when(
    expr = "the person confirms a download of {string} of the snapshot, confirming with {string}"
)]
async fn confirms_one(agent: &mut Agent, entry: String, confirmation: String) {
    agent
        .confirm_download(path(&entry), ArchiveFormat::Tar, &confirmation)
        .await;
}

/// The scenario's clock jumps to each sleeper's deadline, so a write the
/// log holds would be given up at once, at its 20 s bound: a history that
/// can be slow to write keeps the downloads on a clock that moves only
/// when the scenario says time passes.
#[given("the history can be slow to write")]
fn history_can_be_slow(agent: &mut Agent) {
    assert!(
        agent.download.downloads.is_none(),
        "the clock is chosen before the first download"
    );
    agent.download.clock = Some(Arc::new(FakeClock::new(agent.clock.now())));
}

#[when(expr = "{int} seconds pass before the browser fetches it")]
async fn seconds_pass(agent: &mut Agent, seconds: i64) {
    agent
        .download_clock()
        .advance(SignedDuration::from_secs(seconds));
    let last = agent.download.confirmed.len() - 1;
    agent.fetch_download(last).await;
}

#[when(
    expr = "{int} seconds pass before the browser fetches it and hangs up while the history is written"
)]
async fn hangs_up_while_written(agent: &mut Agent, seconds: i64) {
    let downloads = agent.downloads().await;
    let ticket = agent.ticket(agent.download.confirmed.len() - 1);
    agent
        .download_clock()
        .advance(SignedDuration::from_secs(seconds));
    // The browser's request runs while the log is busy, then hangs up.
    // Fetching waits for nothing (#846); were it to wait again, poll it
    // once here (`now_or_never`) and drop it, never `.await` it: the held
    // log doesn't answer until it is let go.
    agent.download.log_held.send_replace(true);
    let fetched = downloads.fetch(&ticket);
    agent.download.log_held.send_replace(false);
    agent.download.fetched = Some(fetched);
}

#[when("the agent gives up the downloads whose minute ran out")]
async fn gives_up_expired(agent: &mut Agent) {
    agent.downloads().await.give_up_expired().await;
}

#[when(expr = "{int} seconds pass while the history is slow to write")]
async fn pass_while_slow(agent: &mut Agent, seconds: i64) {
    agent
        .download_clock()
        .advance(SignedDuration::from_secs(seconds));
    agent.download.log_held.send_replace(true);
}

#[when("the agent starts giving up the downloads whose minute ran out")]
async fn starts_giving_up(agent: &mut Agent) {
    let downloads = agent.downloads().await;
    let mut giving_up = async move { downloads.give_up_expired().await }.boxed();
    // Taken as far as it goes before the held log answers; the rest runs
    // beside the scenario, as the agent's sweep does, and the history's
    // steps wait for it.
    if futures::poll!(giving_up.as_mut()).is_pending() {
        agent.download.recording.push(tokio::spawn(giving_up));
    }
}

#[when("the history takes its writes again")]
fn log_let_go(agent: &mut Agent) {
    agent.download.log_held.send_replace(false);
}

#[when(
    expr = "the person asks for downloads of {string}, {string} and {string} of the snapshot, one after another, confirming with {string}"
)]
async fn asks_for_three(
    agent: &mut Agent,
    first: String,
    second: String,
    third: String,
    confirmation: String,
) {
    for entry in [first, second, third] {
        agent.start_confirming(path(&entry), &confirmation).await;
    }
}

#[when(
    expr = "the person downloads {string} of the snapshot as {word}, confirming with {string}, and the browser goes away after the first piece"
)]
async fn goes_away(agent: &mut Agent, entry: String, as_format: String, confirmation: String) {
    agent
        .confirm_download(path(&entry), format(&as_format), &confirmation)
        .await;
    let last = agent.download.confirmed.len() - 1;
    agent.fetch_download(last).await;
    let Some(Ok(download)) = agent.download.fetched.as_mut() else {
        panic!("the download was handed over");
    };
    let first = download.pieces.next().await;
    assert!(matches!(first, Some(Ok(_))), "a first piece: {first:?}");
    // The browser goes: its request, and the download with it, is dropped.
    agent.download.fetched = None;
}

#[when(
    expr = "the person downloads {string} of the snapshot, confirming with {string}, and the browser stops reading at the file's last byte"
)]
async fn stops_at_last_byte(agent: &mut Agent, entry: String, confirmation: String) {
    agent
        .confirm_download(path(&entry), ArchiveFormat::Tar, &confirmation)
        .await;
    let last = agent.download.confirmed.len() - 1;
    agent.fetch_download(last).await;
    let Some(Ok(download)) = agent.download.fetched.as_mut() else {
        panic!("the download was handed over");
    };
    let DownloadShape::File { size } = download.shape else {
        panic!("a file: {:?}", download.shape);
    };
    let mut read = 0;
    while read < size {
        let piece = download.pieces.next().await;
        let Some(Ok(bytes)) = piece else {
            panic!("a piece before the file's last byte: {piece:?}");
        };
        read += u64::try_from(bytes.len()).expect("a piece's length");
    }
    // As an HTTP server does once a body's length is out: the rest of the
    // download is never asked for.
    agent.download.fetched = None;
}

#[when(
    expr = "the person downloads {string} of the snapshot, confirming with {string}, and the browser reads it as far as it goes"
)]
async fn reads_as_far_as_it_goes(agent: &mut Agent, entry: String, confirmation: String) {
    agent
        .confirm_download(path(&entry), ArchiveFormat::Tar, &confirmation)
        .await;
    let last = agent.download.confirmed.len() - 1;
    agent.fetch_download(last).await;
    let Some(Ok(download)) = agent.download.fetched.as_mut() else {
        panic!("the download was handed over");
    };
    let mut bytes = Vec::new();
    let mut ended_short = false;
    while let Some(piece) = download.pieces.next().await {
        match piece {
            Ok(piece) => bytes.extend(piece),
            Err(_) => ended_short = true,
        }
    }
    agent.download.bytes = bytes;
    agent.download.ended_short = ended_short;
}

#[then("the download ended short of the file's last byte")]
fn ended_short(agent: &mut Agent) {
    let DownloadShape::File { size } = agent.handed().shape else {
        panic!("a file: {:?}", agent.handed().shape);
    };
    assert!(agent.download.ended_short, "the download ended in an error");
    let read = u64::try_from(agent.download.bytes.len()).expect("a length");
    assert!(read < size, "{read} of the file's {size} bytes handed over");
}

#[then(expr = "the engine reads {string} of the snapshot for a download")]
fn engine_reads(agent: &mut Agent, entry: String) {
    let job = agent.dump_job();
    assert_eq!(job.entry, path(&entry));
    assert_eq!(
        Some(&job.snapshot),
        agent.restore.snapshot.as_ref().map(|(id, _)| id)
    );
}

#[then(expr = "the engine reads {string} of the snapshot for a download as {word}")]
fn engine_reads_as(agent: &mut Agent, entry: String, as_format: String) {
    let job = agent.dump_job();
    assert_eq!(job.entry, path(&entry));
    assert_eq!(job.format, format(&as_format));
}

#[then(expr = "the download is the file {string} of {int} bytes")]
fn is_the_file(agent: &mut Agent, name: String, size: u64) {
    let download = agent.handed();
    assert_eq!(download.file_name, name);
    assert_eq!(download.shape, DownloadShape::File { size });
}

#[then(expr = "the download hands over {string}")]
fn hands_over(agent: &mut Agent, text: String) {
    let _ = agent.handed();
    assert_eq!(agent.download.bytes, text.as_bytes());
}

#[then(expr = "the download is the archive {string}")]
fn is_the_archive(agent: &mut Agent, name: String) {
    let download = agent.handed();
    assert_eq!(download.file_name, name);
    assert!(
        matches!(download.shape, DownloadShape::Archive(_)),
        "{:?}",
        download.shape
    );
    assert_eq!(agent.download.bytes, b"first piecesecond piece");
}

#[then("the download is refused because the step-up confirmation failed")]
fn refused_step_up(agent: &mut Agent) {
    let refusal = agent.download_refusal();
    assert!(
        matches!(refusal, DownloadError::StepUpFailed),
        "{refusal:?}"
    );
}

#[then("the download is refused while the sign-in wait lasts")]
fn refused_wait(agent: &mut Agent) {
    let refusal = agent.download_refusal();
    assert!(
        matches!(refusal, DownloadError::StepUpWait { seconds } if *seconds > 0),
        "{refusal:?}"
    );
}

#[then("the download is refused because the snapshot holds no such entry")]
fn refused_unknown_entry(agent: &mut Agent) {
    let refusal = agent.download_refusal();
    assert!(
        matches!(refusal, DownloadError::EntryUnknown),
        "{refusal:?}"
    );
}

#[then("the download is refused because two downloads are under way")]
fn refused_busy(agent: &mut Agent) {
    let refusal = agent.download_refusal();
    assert!(matches!(refusal, DownloadError::Busy), "{refusal:?}");
}

#[then(
    "the first of the three is confirmed, and one of the other two is refused because two downloads are under way"
)]
async fn first_of_three_confirmed(agent: &mut Agent) {
    let mut answers = Vec::new();
    for asking in agent.download.asking.drain(..) {
        let answer = tokio::time::timeout(std::time::Duration::from_secs(5), asking)
            .await
            .expect("the confirmation answers once the history takes its writes")
            .expect("the confirmation ran");
        answers.push(answer);
    }
    assert!(
        answers.first().is_some_and(Result::is_ok),
        "the download that gave the expired turns back took one: {answers:?}"
    );
    let refused = answers
        .iter()
        .filter(|answer| matches!(answer, Err(DownloadError::Busy)))
        .count();
    assert_eq!((answers.len(), refused), (3, 1), "{answers:?}");
}

#[then("the second hand-over is refused because the download is unknown")]
fn second_refused(agent: &mut Agent) {
    let refusal = agent.fetch_refusal();
    assert!(
        matches!(refusal, DownloadError::TicketUnknown),
        "{refusal:?}"
    );
}

#[then("the hand-over is refused because the download is unknown")]
fn hand_over_refused(agent: &mut Agent) {
    let refusal = agent.fetch_refusal();
    assert!(
        matches!(refusal, DownloadError::TicketUnknown),
        "{refusal:?}"
    );
}

#[then("the engine's download job was stopped")]
fn job_stopped(agent: &mut Agent) {
    assert!(
        agent.download.stopped.load(Ordering::SeqCst) >= 1,
        "the download's job was stopped before its end"
    );
}

#[then("the engine's jobs of both downloads were stopped before the history wrote either")]
async fn both_stopped_before_written(agent: &mut Agent) {
    assert_eq!(
        agent.download.stopped.load(Ordering::SeqCst),
        2,
        "both downloads' jobs were stopped while the history's first write was held"
    );
    let history = agent.restore.history().await;
    assert_eq!(history.len(), 2, "{history:?}");
    assert!(
        history
            .iter()
            .all(|record| record.status == OperationStatus::Running),
        "the history wrote neither end yet: {history:?}"
    );
}

#[then(
    regex = r"^the history shows the download, started by hand, as (succeeded|cancelled|failed)$"
)]
async fn history_shows_download(agent: &mut Agent, ended: String) {
    // A download read to its end, or dropped, lets its write finish.
    agent.download.fetched = None;
    agent.recorded().await;
    let history = agent.restore.history().await;
    let [record] = history.as_slice() else {
        panic!("one operation in {history:?}");
    };
    assert_eq!(record.kind, OperationKind::Download);
    assert_eq!(record.trigger, Trigger::Manual);
    match ended.as_str() {
        "succeeded" => assert_eq!(record.status, OperationStatus::Succeeded),
        "cancelled" => assert_eq!(record.status, OperationStatus::Cancelled),
        _ => assert!(
            matches!(record.status, OperationStatus::Failed { .. }),
            "{:?}",
            record.status
        ),
    }
}

#[then("the history shows both downloads, started by hand, as cancelled")]
async fn history_shows_both_cancelled(agent: &mut Agent) {
    agent.recorded().await;
    let history = agent.restore.history().await;
    assert_eq!(history.len(), 2, "{history:?}");
    for record in &history {
        assert_eq!(record.kind, OperationKind::Download);
        assert_eq!(record.trigger, Trigger::Manual);
        assert_eq!(record.status, OperationStatus::Cancelled, "{history:?}");
    }
}

#[then("a new download can start at once")]
async fn new_can_start(agent: &mut Agent) {
    // Both turns: the one the gone reader held is free again.
    for _ in 0..2 {
        agent
            .confirm_download(path(NOTES), ArchiveFormat::Tar, &short(agent))
            .await;
        let confirmed = agent.download.confirmed.last();
        assert!(matches!(confirmed, Some(Ok(_))), "{confirmed:?}");
    }
}

/// The scenario's snapshot's short ID, which confirms with sign-in off.
fn short(agent: &Agent) -> String {
    let (snapshot, _) = agent.restore.snapshot.as_ref().expect("a snapshot");
    domain::access::step_up::short_id(snapshot).to_owned()
}
