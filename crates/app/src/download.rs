//! The download use case (technical design §10.4): one entry of a snapshot
//! handed to the browser that asked, a file as itself and a folder as one
//! archive. Every download is confirmed by step-up first, as a restore is;
//! the confirmed download waits behind a ticket its browser fetches once,
//! within a minute, and is then streamed a piece at a time. At most
//! [`DOWNLOADS_AT_ONCE`] run at once. A read: no I7 slot and never queued,
//! but the operation log keeps each download the engine found, as a
//! restore's, so the history says who read what and how it ended.

use std::net::IpAddr;
use std::sync::Arc;

use domain::browse::FolderPath;
use domain::execution::FailureReason;
use domain::restore::download::{ArchiveFormat, DownloadShape, DownloadTicket};
use domain::{OperationId, RepositoryAlias, SnapshotId};
use futures::future::BoxFuture;
use futures::stream::BoxStream;

pub use domain::restore::download::DOWNLOADS_AT_ONCE;

use crate::access::Access;
use crate::maintenance::MaintenanceError;
use crate::ports::{Clock, ConfigError, ConfigStore, Engine, OperationStore, Random};
use crate::secrets::{ResolveError, ResolveRepository};

/// One download a person asked for, as it reached the agent. The
/// confirmation is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone)]
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
    /// otherwise.
    pub confirmation: String,
}

impl std::fmt::Debug for DownloadAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadAsk")
            .field("alias", &self.alias)
            .field("snapshot", &self.snapshot)
            .field("entry", &self.entry)
            .field("format", &self.format)
            .field("address", &self.address)
            .field("confirmation", &"[hidden]")
            .finish()
    }
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
    _private: (),
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
        let _ = (engine, clock, config, resolve, random, store);
        todo!()
    }

    /// With sign-in on: step-up asks for the admin password, checked by
    /// `access` in the client's sign-in waits.
    pub fn with_access(self, access: Arc<Access>) -> Self {
        let _ = access;
        todo!()
    }

    /// Confirms `ask` by step-up, takes a download's turn, starts the
    /// engine's job and waits until it says what it found; the download
    /// then waits for its browser under the ticket answered.
    pub async fn start(
        &self,
        op: OperationId,
        ask: DownloadAsk,
    ) -> Result<DownloadTicket, DownloadError> {
        let _ = (op, ask);
        todo!()
    }

    /// The download waiting under `ticket`, taken so no one fetches it
    /// again. Downloads whose minute ran out are given up first, and the
    /// log records them cancelled.
    pub async fn fetch(&self, ticket: &DownloadTicket) -> Result<Download, DownloadError> {
        let _ = ticket;
        todo!()
    }
}
