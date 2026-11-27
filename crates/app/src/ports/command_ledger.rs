use async_trait::async_trait;
use domain::Timestamp;
use domain::fleet::{CommandId, HubRun};

use super::StoreError;

/// Where a paired machine keeps the IDs of the commands it took from its
/// hub and the backups the hub started on it (§16, decisions 120 and 176),
/// in its database, so a command replayed after a restart is refused and
/// the hub's runs stay counted against its limit.
#[async_trait]
pub trait CommandLedger: Send + Sync {
    /// Remembers `id`, taken at `now` on the machine's clock, unless it
    /// remembers it already. Each ID taken
    /// [`COMMANDS_REMEMBERED_FOR`](domain::fleet::COMMANDS_REMEMBERED_FOR)
    /// or longer before `now` is forgotten first; when
    /// [`MAX_COMMANDS_REMEMBERED`](domain::fleet::MAX_COMMANDS_REMEMBERED)
    /// remain, `id` isn't remembered.
    async fn remember(&self, id: CommandId, now: Timestamp) -> Result<Remembered, StoreError>;

    /// Keeps `run`, a backup the hub started. Each run an hour or longer
    /// before it is forgotten.
    async fn ran(&self, run: &HubRun) -> Result<(), StoreError>;

    /// The backups the hub started that are kept, oldest first.
    async fn runs(&self) -> Result<Vec<HubRun>, StoreError>;
}

/// What [`CommandLedger::remember`] made of a command's ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remembered {
    /// Taken for the first time: it is remembered now.
    New,
    /// Taken before: the command is a replay.
    Seen,
    /// Not remembered: the ledger is full until the oldest ID it holds,
    /// taken at `oldest`, is forgotten.
    Full { oldest: Timestamp },
}
