//! The download use case (technical design §10.4): one entry of a snapshot
//! handed to the browser that asked, a file as itself and a folder as one
//! archive. Every download is confirmed by step-up first, as a restore is;
//! the confirmed download waits behind a ticket its browser fetches once,
//! within a minute, and is then streamed a piece at a time. At most
//! [`DOWNLOADS_AT_ONCE`] run at once. A read: no I7 slot and never queued,
//! but the operation log keeps each download the engine found, as a
//! restore's, so the history says who read what and how it ended.

use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::access::step_up::{StepUpConfirmation, StepUpFor};
use domain::browse::{DisplayName, FolderPath};
use domain::execution::deadline::backup_budget;
use domain::execution::{FailureReason, Operation, OperationKind, Trigger};
use domain::restore::download::{
    ArchiveFormat, DownloadShape, DownloadTicket, PendingDownloads, download_file_name,
};
use domain::{OperationId, RepositoryAlias, SnapshotId};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};

pub use domain::restore::download::DOWNLOADS_AT_ONCE;

use crate::access::{Access, StepUpRefused, confirm_step_up};
use crate::locations::ruled_positions;
use crate::log_write::save_within_bound;
use crate::maintenance::{MaintenanceError, engine_error, within_read_timeout};
use crate::ports::{
    Clock, ConfigError, ConfigStore, DumpAnswer, DumpHeader, DumpJob, Engine, EngineError,
    EngineEvent, EngineResult, Job, OperationStore, Random, RunningJob,
};
use crate::secrets::{ResolveError, ResolveRepository, ResolvedRepository};

/// One download a person asked for, as it reached the agent.
#[derive(Debug)]
pub struct DownloadAsk {
    pub alias: RepositoryAlias,
    pub snapshot: SnapshotId,
    /// The entry, by its keys from the snapshot's top.
    pub entry: FolderPath,
    /// The archive a folder is handed over as.
    pub format: ArchiveFormat,
    /// The address the request came from, whose sign-in waits a wrong
    /// step-up password counts against.
    pub address: IpAddr,
    /// The admin password with sign-in on, the snapshot's short ID
    /// otherwise. Never shown (I4).
    pub confirmation: StepUpConfirmation,
}

/// Why a download wasn't confirmed or handed over. A typed code, never a
/// sentence (§8.4).
#[derive(Debug)]
pub enum DownloadError {
    /// The step-up confirmation wasn't the admin password, or the
    /// snapshot's ID.
    StepUpFailed,
    /// The address's wait after failed sign-ins has `seconds` left; the
    /// password wasn't checked.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked: the platform's own text.
    StepUpUnavailable(String),
    /// [`DOWNLOADS_AT_ONCE`] downloads are under way.
    Busy,
    /// No repository is configured under the alias.
    UnknownRepository,
    /// The repository holds no snapshot by that ID.
    SnapshotUnknown,
    /// The snapshot holds no entry at that path.
    EntryUnknown,
    /// No download waits under the ticket: it was fetched already, its
    /// minute ran out, or it never was.
    TicketUnknown,
    /// The platform gave no randomness for a ticket.
    RandomUnavailable,
    /// The configuration couldn't be read.
    Config(Box<ConfigError>),
    /// The repository's secrets couldn't be resolved.
    Resolve(ResolveError),
    /// Reading the repository failed before its first piece.
    Read(MaintenanceError),
}

/// A download handed over: the name to save it under, what it is, and its
/// pieces. Dropping the pieces stops the engine's job at once.
pub struct Download {
    pub file_name: String,
    pub shape: DownloadShape,
    /// The pieces, each at most `PIECE_BYTES`; an error ends a download
    /// the engine couldn't finish, so the browser never takes a part for
    /// the whole.
    pub pieces: BoxStream<'static, Result<Vec<u8>, FailureReason>>,
    /// Writes how the download ended to the operation log once the pieces
    /// end or are dropped: succeeded, failed, or cancelled when the reader
    /// went away. The caller runs it beside the pieces, to its end.
    pub recorded: BoxFuture<'static, ()>,
}

/// Confirms downloads and hands them over.
pub struct Downloads {
    engine: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    config: Arc<dyn ConfigStore>,
    resolve: ResolveRepository,
    random: Arc<dyn Random>,
    store: Arc<dyn OperationStore>,
    access: Option<Arc<Access>>,
    /// How many downloads hold a turn: confirmed and waiting, or handed
    /// over and not yet ended.
    under_way: Arc<AtomicUsize>,
    pending: Mutex<PendingDownloads<Confirmed>>,
}

impl Downloads {
    /// Downloads with sign-in off: step-up asks for the snapshot's short ID.
    /// Each one the engine found is recorded in `store`.
    pub fn new(
        engine: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        config: Arc<dyn ConfigStore>,
        resolve: ResolveRepository,
        random: Arc<dyn Random>,
        store: Arc<dyn OperationStore>,
    ) -> Self {
        Self {
            engine,
            clock,
            config,
            resolve,
            random,
            store,
            access: None,
            under_way: Arc::default(),
            pending: Mutex::default(),
        }
    }

    /// With sign-in on: step-up asks for the admin password, checked by
    /// `access` in the client's sign-in waits.
    pub fn with_access(self, access: Arc<Access>) -> Self {
        Self {
            access: Some(access),
            ..self
        }
    }

    /// Confirms `ask` by step-up, takes a download's turn, starts the
    /// engine's job and waits until it says what it found; the download
    /// then waits for its browser under the ticket answered. The caller
    /// runs it to its end, on a task of its own, as a restore's start: it
    /// writes the log, and gives up the downloads whose minute ran out.
    pub async fn start(
        &self,
        op: OperationId,
        ask: DownloadAsk,
    ) -> Result<DownloadTicket, DownloadError> {
        confirm_step_up(
            self.access.as_deref(),
            ask.address,
            &ask.confirmation,
            StepUpFor::Snapshot(&ask.snapshot),
        )
        .await
        .map_err(|refused| match refused {
            StepUpRefused::Failed => DownloadError::StepUpFailed,
            StepUpRefused::Wait { seconds } => DownloadError::StepUpWait { seconds },
            StepUpRefused::Unavailable(text) => DownloadError::StepUpUnavailable(text),
        })?;
        // Downloads whose minute ran out give their turns back first. This
        // one takes its own before their rows are written, so a slow log
        // can't hand the turns it freed to one asked for after it (#888).
        let cancelled = self.stop_expired();
        let turn = Turn::take(&self.under_way);
        self.write_cancelled(&cancelled).await;
        let turn = turn.ok_or(DownloadError::Busy)?;
        let config = self
            .config
            .load()
            .await
            .map_err(|error| DownloadError::Config(Box::new(error)))?;
        let repository = config
            .repositories()
            .get(&ask.alias)
            .ok_or(DownloadError::UnknownRepository)?;
        let ResolvedRepository { access, warnings } = self
            .resolve
            .resolve_saved(&ask.alias, repository)
            .await
            .map_err(DownloadError::Resolve)?;
        let ticket = self
            .random
            .token()
            .map(DownloadTicket::from_bytes)
            .map_err(|_| DownloadError::RandomUnavailable)?;
        let job = Job::Dump(DumpJob {
            repository: access,
            snapshot: ask.snapshot.clone(),
            entry: ask.entry,
            format: ask.format,
            deadline_budget: backup_budget(None, None),
        });
        let ruled = ruled_positions(&job);
        let mut operation = Operation::queue(
            op,
            OperationKind::Download,
            Trigger::Manual,
            job.target(),
            self.clock.now(),
        )
        .with_repository_alias(ask.alias);
        if let Some(entry) = job.entry() {
            operation = operation.with_entry(entry);
        }
        let (header, running) = self.found(op, job, &mut operation, &ruled).await?;
        if operation.started_at().is_none() {
            let _ = operation.start(self.clock.now());
        }
        for warning in &warnings {
            let _ = operation.record_warning(warning.warning());
        }
        // The log keeps the download from here on; a write it refuses
        // doesn't stop it, as it doesn't stop a backup (§9).
        let _ = save_within_bound(&*self.clock, &*self.store, &operation, None).await;
        let confirmed = Confirmed {
            file_name: download_file_name(
                header.name.as_ref().map(DisplayName::as_str),
                &header.host,
                &ask.snapshot,
                header.shape,
            ),
            shape: header.shape,
            handing: Handing {
                running,
                operation,
                ruled,
                clock: self.clock.clone(),
                ended: None,
                left: match header.shape {
                    DownloadShape::File { size } => Some(size),
                    DownloadShape::Archive(_) => None,
                },
                _turn: turn,
            },
        };
        let held = lock(&self.pending).hold(ticket.clone(), self.clock.now(), confirmed);
        match held {
            Ok(()) => Ok(ticket),
            // Unreachable while every waiting download holds one of the
            // DOWNLOADS_AT_ONCE turns, fewer than MAX_PENDING; kept so a
            // change to either still logs the refused download cancelled.
            Err(confirmed) => {
                self.write_cancelled(&[self.stop(confirmed)]).await;
                Err(DownloadError::Busy)
            }
        }
    }

    /// The download waiting under `ticket`, taken so no one fetches it
    /// again; one whose minute ran out is unknown. It waits for nothing, so
    /// a browser that hangs up can't cut it short: giving up the downloads
    /// whose minute ran out is [`give_up_expired`](Self::give_up_expired)'s,
    /// run by the agent, never by the request (#846).
    pub fn fetch(&self, ticket: &DownloadTicket) -> Result<Download, DownloadError> {
        let confirmed = lock(&self.pending)
            .take(ticket, self.clock.now())
            .ok_or(DownloadError::TicketUnknown)?;
        let Confirmed {
            file_name,
            shape,
            mut handing,
        } = confirmed;
        let (ended, end) = oneshot::channel();
        handing.ended = Some(ended);
        let (clock, store) = (self.clock.clone(), self.store.clone());
        let recorded = async move {
            // Dropped unsent only when the pieces were never polled to an
            // end and never dropped: nothing to write then.
            if let Ok((operation, result)) = end.await {
                let _ = save_within_bound(&*clock, &*store, &operation, result.as_ref()).await;
            }
        }
        .boxed();
        Ok(Download {
            file_name,
            shape,
            pieces: stream::unfold(handing, Handing::next_piece).boxed(),
            recorded,
        })
    }

    /// Gives up every confirmed download whose minute ran out: its job
    /// stops, its turn is free again and the log records it cancelled. All
    /// of them stop before the first is written, so a slow log holds no
    /// worker or turn (#888). The caller runs it to its end, on a task of
    /// its own: one dropped halfway leaves the downloads it took running in
    /// the log.
    pub async fn give_up_expired(&self) {
        let cancelled = self.stop_expired();
        self.write_cancelled(&cancelled).await;
    }

    /// Stops every confirmed download whose minute ran out and frees its
    /// turn, and returns their records, cancelled, for the log.
    fn stop_expired(&self) -> Vec<Operation> {
        let expired = lock(&self.pending).expire(self.clock.now());
        expired
            .into_iter()
            .map(|confirmed| self.stop(confirmed))
            .collect()
    }

    /// Writes `cancelled` to the log, one after another.
    async fn write_cancelled(&self, cancelled: &[Operation]) {
        for operation in cancelled {
            let _ = save_within_bound(&*self.clock, &*self.store, operation, None).await;
        }
    }

    /// Stops `confirmed`'s job and frees its turn, and returns its record,
    /// cancelled from now, for the log.
    fn stop(&self, confirmed: Confirmed) -> Operation {
        let mut operation = confirmed.handing.operation.clone();
        drop(confirmed);
        let _ = operation.cancel(self.clock.now());
        operation
    }

    /// Runs `job` until the engine says what it found at its entry, within
    /// a read's time, applying what it tells before to `operation`.
    async fn found(
        &self,
        op: OperationId,
        job: Job,
        operation: &mut Operation,
        ruled: &[u8],
    ) -> Result<(DumpHeader, RunningJob), DownloadError> {
        let found = within_read_timeout(&*self.clock, async {
            let RunningJob {
                mut events,
                control,
            } = self.engine.run(op, job).await.map_err(engine_error)?;
            while let Some(event) = events.next().await {
                let at = self.clock.now();
                match event {
                    EngineEvent::Dumping(header) => {
                        return Ok(Ok((header, RunningJob { events, control })));
                    }
                    EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::SnapshotUnknown)) => {
                        return Ok(Err(DownloadError::SnapshotUnknown));
                    }
                    EngineEvent::Succeeded(EngineResult::Dump(DumpAnswer::EntryUnknown)) => {
                        return Ok(Err(DownloadError::EntryUnknown));
                    }
                    // A refusal after an identify is answered as a refusal before it.
                    EngineEvent::Failed(reason) => {
                        return Err(EngineError::try_from(reason)
                            .map_or_else(MaintenanceError::Failed, engine_error));
                    }
                    EngineEvent::Started { repository, .. } => {
                        let _ = operation.start_on(at, repository.as_ref());
                    }
                    EngineEvent::Warning(warning) => {
                        let _ = operation.record_warning(warning);
                    }
                    EngineEvent::Located(record) => {
                        let _ = operation
                            .record_locations(record, |position| ruled.contains(&position));
                    }
                    EngineEvent::Progress(_) => {}
                    // Nothing else comes before what the dump found.
                    EngineEvent::Succeeded(_) | EngineEvent::Piece(_) => break,
                }
            }
            Err(MaintenanceError::Failed(FailureReason::EngineExited {
                exit_code: None,
            }))
        })
        .await;
        found.map_err(DownloadError::Read)?
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A download's turn, one of [`DOWNLOADS_AT_ONCE`]: given back when it is
/// dropped.
struct Turn {
    under_way: Arc<AtomicUsize>,
}

impl Turn {
    /// A turn, unless every one is taken.
    fn take(under_way: &Arc<AtomicUsize>) -> Option<Self> {
        under_way
            .try_update(Ordering::AcqRel, Ordering::Acquire, |taken| {
                (taken < DOWNLOADS_AT_ONCE).then_some(taken + 1)
            })
            .ok()?;
        Some(Self {
            under_way: under_way.clone(),
        })
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.under_way.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A download the engine found, waiting for its browser.
struct Confirmed {
    file_name: String,
    shape: DownloadShape,
    handing: Handing,
}

/// What hands a download's pieces over: the engine's job, past what it
/// found, and the download's record. Dropped before the job ended, it stops
/// the job and tells the record cancelled.
struct Handing {
    running: RunningJob,
    operation: Operation,
    ruled: Vec<u8>,
    clock: Arc<dyn Clock>,
    /// Where the record goes once the download ends; none until it is
    /// handed over, and once it has ended.
    ended: Option<oneshot::Sender<(Operation, Option<EngineResult>)>>,
    /// The bytes of a file not yet handed over; none for an archive, whose
    /// size isn't known ahead.
    left: Option<u64>,
    _turn: Turn,
}

/// What a dump's next event comes to, once what only goes to the record
/// is applied.
enum Step {
    Piece(Vec<u8>),
    Done(EngineResult),
    Failed(FailureReason),
}

impl Handing {
    /// The next piece, or the download's end, told to its record. A file
    /// goes with its size, and a reader that has that many bytes asks for
    /// nothing more (HTTP drops the body unread then): the piece that
    /// completes a file is handed over only once the job's end is known.
    async fn next_piece(mut self) -> Option<(Result<Vec<u8>, FailureReason>, Self)> {
        self.ended.as_ref()?;
        match self.next_step().await {
            Step::Piece(bytes) => {
                let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                self.left = self.left.map(|left| left.saturating_sub(length));
                if self.left == Some(0) {
                    match self.next_step().await {
                        Step::Done(result) => self.succeed(result),
                        Step::Failed(reason) => return Some(self.fail(reason)),
                        // A byte beyond the file's size: not the file it said.
                        Step::Piece(_) => {
                            let reason = FailureReason::EngineExited { exit_code: None };
                            return Some(self.fail(reason));
                        }
                    }
                }
                Some((Ok(bytes), self))
            }
            Step::Done(result) => {
                self.succeed(result);
                None
            }
            Step::Failed(reason) => Some(self.fail(reason)),
        }
    }

    /// The job's next piece or end, applying what it tells before to the
    /// record.
    async fn next_step(&mut self) -> Step {
        loop {
            match self.running.events.next().await {
                Some(EngineEvent::Piece(bytes)) => return Step::Piece(bytes),
                Some(EngineEvent::Succeeded(
                    result @ EngineResult::Dump(DumpAnswer::Done { .. }),
                )) => return Step::Done(result),
                Some(EngineEvent::Warning(warning)) => {
                    let _ = self.operation.record_warning(warning);
                }
                Some(EngineEvent::Located(record)) => {
                    let ruled = &self.ruled;
                    let _ = self
                        .operation
                        .record_locations(record, |position| ruled.contains(&position));
                }
                Some(
                    EngineEvent::Progress(_)
                    | EngineEvent::Started { .. }
                    | EngineEvent::Dumping(_),
                ) => {}
                Some(EngineEvent::Failed(reason)) => return Step::Failed(reason),
                // A dump that ends otherwise didn't hand everything over.
                Some(EngineEvent::Succeeded(_)) | None => {
                    return Step::Failed(FailureReason::EngineExited { exit_code: None });
                }
            }
        }
    }

    fn succeed(&mut self, result: EngineResult) {
        let _ = self.operation.succeed(self.clock.now());
        self.end(Some(result));
    }

    fn fail(mut self, reason: FailureReason) -> (Result<Vec<u8>, FailureReason>, Self) {
        let _ = self.operation.fail(reason.clone(), self.clock.now());
        self.end(None);
        (Err(reason), self)
    }

    fn end(&mut self, result: Option<EngineResult>) {
        if let Some(ended) = self.ended.take() {
            let _ = ended.send((self.operation.clone(), result));
        }
    }
}

impl Drop for Handing {
    fn drop(&mut self) {
        // Handed over and gone before its end: the reader went away.
        if self.ended.is_some() {
            let _ = self.operation.cancel(self.clock.now());
            self.end(None);
        }
    }
}
