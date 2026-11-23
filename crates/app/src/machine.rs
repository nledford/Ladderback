//! Whether this machine's data folder was copied from another computer
//! (technical design §7.1; §16, decision 144; #296), or its hub refused it
//! as a second computer under its machine ID (decision 150; #1462). The
//! machine ID lives in the data folder, so a copy of the folder on another
//! computer (a restored backup of a home folder, a cloned disk or virtual
//! machine) runs under the same ID, and its forget would take the first
//! machine's snapshots for its own (I1, I6). The machine identifier the
//! system gives the computer is recorded beside the ID and compared at each
//! start: a copy keeps backing up and stops maintaining until the person
//! takes a new machine ID. A copy made before the identifier was kept
//! recorded its own computer's, so only its hub's signed refusal tells it,
//! and the person may take a new machine ID on it too.

use std::sync::{Arc, Mutex, PoisonError};

use domain::fleet::IdentifierMatch;
use domain::maintenance::Ownership;
use domain::{AgentId, OperationId};

use crate::ports::{
    ConfigError, ConfigStore, MachineError, OperationStore, Problem, StoreError, ThisMachine,
};
use crate::repositories::ConfigWrites;

/// Where this machine stands, as the check at start found it and as a new
/// machine ID taken since leaves it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standing {
    /// The data folder is on the computer it was made on, or was made
    /// before the identifier was kept and has it recorded now.
    #[default]
    Same,
    /// The data folder came from another computer: backups run, and
    /// maintenance is refused.
    Copied,
    /// A new machine ID was taken for the copy. It is used from the next
    /// start; until then maintenance stays refused, since this run still
    /// tags and selects snapshots under the old ID.
    NewIdTaken,
    /// The hub this machine paired with refused it, signed, as a second
    /// computer under its machine ID (`identity-conflict`): backups run, and
    /// the person may take a new machine ID. It may be the original
    /// computer, so it maintains as before; its hub grants it nothing while
    /// it refuses it. Its hub taking it again sets it back to [`Self::Same`].
    RefusedByHub,
    /// This computer's identifier couldn't be read, so a copy can't be
    /// told: the machine runs as usual.
    Unread,
}

impl Standing {
    /// Whether the machine runs the maintenance its configuration asks for.
    pub fn maintains(self) -> bool {
        matches!(self, Self::Same | Self::Unread)
    }
}

/// The standing every part of a running daemon reads: set once at start,
/// and by [`TakeNewId`].
#[derive(Debug, Default)]
pub struct MachineStanding(Mutex<Standing>);

impl MachineStanding {
    pub fn new(standing: Standing) -> Self {
        Self(Mutex::new(standing))
    }

    pub fn get(&self) -> Standing {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, standing: Standing) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = standing;
    }

    /// The hub this machine paired with refused it, signed, as a second
    /// computer under its machine ID.
    pub fn hub_refused(&self) {
        todo!()
    }

    /// The hub this machine paired with took it.
    pub fn hub_took(&self) {
        todo!()
    }

    /// Whether the machine runs the maintenance its configuration asks for.
    pub fn maintains(&self) -> bool {
        self.get().maintains()
    }
}

/// What the check at start found, for the daemon's log: the system's own
/// words where something couldn't be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// The recorded identifier is this computer's.
    Same,
    /// None was recorded, and this computer's is now.
    Recorded,
    /// None was recorded, and this computer's couldn't be: the next start
    /// tries again.
    RecordFailed(MachineError),
    /// The recorded identifier is another computer's.
    Copied,
    /// This computer's identifier, or the one recorded, couldn't be read.
    Unread(MachineError),
}

impl Found {
    /// Where the machine stands after the check.
    pub fn standing(&self) -> Standing {
        match self {
            Self::Same | Self::Recorded | Self::RecordFailed(_) => Standing::Same,
            Self::Copied => Standing::Copied,
            Self::Unread(_) => Standing::Unread,
        }
    }
}

/// Compares the identifier recorded beside the machine ID with this
/// computer's, recording it in a data folder that has none (an upgrade,
/// never a copy). Never refuses the start: a check that can't tell
/// leaves the machine running as usual.
pub fn check_at_start(machine: &dyn ThisMachine) -> Found {
    let current = match machine.identifier() {
        Ok(current) => current,
        Err(error) => return Found::Unread(error),
    };
    let recorded = match machine.recorded() {
        Ok(recorded) => recorded,
        Err(error) => return Found::Unread(error),
    };
    match IdentifierMatch::of(recorded.as_ref(), Some(&current)) {
        IdentifierMatch::Same | IdentifierMatch::Unread => Found::Same,
        IdentifierMatch::Differs => Found::Copied,
        IdentifierMatch::Unrecorded => match machine.record(&current) {
            Ok(()) => Found::Recorded,
            Err(error) => Found::RecordFailed(error),
        },
    }
}

/// Why a new machine ID wasn't taken. A typed code, never a sentence.
#[derive(Debug, PartialEq)]
pub enum NewIdError {
    /// The data folder isn't a copy, or a new ID was taken already: the
    /// ID stays.
    NotCopied,
    /// The configuration couldn't be read or written: nothing changed.
    Config(ConfigError),
    /// A maintenance operation, `running`, runs here on a repository this
    /// machine maintains, so it keeps maintaining it until that one ends
    /// (I1): nothing changed.
    Busy { running: OperationId },
    /// The operation log, which says what maintenance runs, couldn't be
    /// read: nothing changed.
    Log(StoreError),
    /// This computer's identifier couldn't be read, and nothing changed; or
    /// the new ID couldn't be written whole, once maintenance was given up:
    /// the ID stays, or a new one was written without this computer's
    /// identifier beside it, which the next start finds a copy still.
    Machine(MachineError),
}

/// "This is a new computer: take a new ID", for a copied data folder: the
/// machine gives up maintaining every repository it maintained, so it and
/// the computer it was copied from never both maintain one (I1), then takes
/// a new machine ID and records this computer's identifier beside it. Its
/// plans stay; its backups carry the new ID from the next start, and the
/// snapshots under the old ID stay the other computer's (I6).
pub struct TakeNewId {
    machine: Arc<dyn ThisMachine>,
    standing: Arc<MachineStanding>,
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    writes: Arc<ConfigWrites>,
}

impl TakeNewId {
    pub fn new(
        machine: Arc<dyn ThisMachine>,
        standing: Arc<MachineStanding>,
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        writes: Arc<ConfigWrites>,
    ) -> Self {
        Self {
            machine,
            standing,
            config,
            operations,
            writes,
        }
    }

    /// Where the machine stands now.
    pub fn standing(&self) -> Standing {
        self.standing.get()
    }

    /// Takes the new ID, answering it. Under the configuration's write lock,
    /// so two requests can't both take one and no change of who maintains a
    /// repository runs between the two writes. A maintenance operation still
    /// running on a repository it maintains refuses the whole take, as
    /// turning that maintenance off would be ([`Ownership::give_up`], I1).
    #[expect(
        clippy::result_large_err,
        reason = "the configuration store's own error, which the route maps to its code"
    )]
    pub async fn take(&self) -> Result<AgentId, NewIdError> {
        let _writing = self.writes.lock().await;
        if self.standing.get() != Standing::Copied {
            return Err(NewIdError::NotCopied);
        }
        // Read before anything is written: a computer whose identifier can't
        // be read now keeps its ID and its maintenance as they are.
        let identifier = self.machine.identifier().map_err(NewIdError::Machine)?;
        let loaded = self.config.load().await.map_err(NewIdError::Config)?;
        let mut repositories = loaded.repositories().clone();
        let mut gave_up = false;
        for entry in repositories.values_mut() {
            if Ownership::owned_here(entry.maintenance) {
                // An entry that records no ID can't tell its maintenance
                // apart from another repository's: any running here keeps it.
                let running = self
                    .operations
                    .running_maintenance(entry.id.as_ref())
                    .await
                    .map_err(NewIdError::Log)?;
                entry.maintenance =
                    Ownership::give_up(running).map_err(|running| NewIdError::Busy { running })?;
                gave_up = true;
            }
        }
        // Maintenance is given up first: a new ID that then can't be written
        // leaves the copy maintaining nothing, the safe direction (I1).
        if gave_up {
            let changed = loaded
                .replacing(
                    loaded.agent().clone(),
                    loaded.hooks().clone(),
                    repositories,
                    loaded.plans().to_vec(),
                )
                .map_err(|violation| {
                    NewIdError::Config(ConfigError::Invalid {
                        file: String::new(),
                        key: None,
                        at: None,
                        problem: Problem::Violation(violation),
                        detail: String::new(),
                    })
                })?;
            self.config
                .save(&changed)
                .await
                .map_err(NewIdError::Config)?;
        }
        let id = self
            .machine
            .renew(&identifier)
            .map_err(NewIdError::Machine)?;
        self.standing.set(Standing::NewIdTaken);
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
