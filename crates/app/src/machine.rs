//! Whether this machine's data folder was copied from another computer
//! (technical design §7.1; §16, decision NN; #296). The machine ID lives in
//! the data folder, so a copy of the folder on another computer (a restored
//! backup of a home folder, a cloned disk or virtual machine) runs under the
//! same ID, and its forget would take the first machine's snapshots for its
//! own (I1, I6). The machine identifier the system gives the computer is
//! recorded beside the ID and compared at each start: a copy keeps backing
//! up and stops maintaining until the person takes a new machine ID.

use std::sync::{Arc, Mutex, PoisonError};

use domain::fleet::IdentifierMatch;
use domain::planning::Maintenance;
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
    /// This computer's identifier couldn't be read, so a copy can't be
    /// told: the machine runs as usual.
    Unread,
}

impl Standing {
    /// Whether the machine runs the maintenance its configuration asks for.
    pub fn maintains(self) -> bool {
        todo!()
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
        todo!()
    }
}

/// Compares the identifier recorded beside the machine ID with this
/// computer's, recording it in a data folder that has none (an upgrade,
/// never a copy). Never refuses the start: a check that can't tell
/// leaves the machine running as usual.
pub fn check_at_start(machine: &dyn ThisMachine) -> Found {
    let _ = (machine, IdentifierMatch::Same);
    todo!()
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
    /// This computer's identifier couldn't be read, or the new ID couldn't
    /// be written: maintenance was given up and the ID stays.
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

    /// Takes the new ID, answering it.
    pub async fn take(&self) -> Result<AgentId, NewIdError> {
        let _ = (
            &self.machine,
            &self.config,
            &self.operations,
            &self.writes,
            Maintenance::None,
            Problem::Violation,
        );
        self.standing.set(Standing::Same);
        todo!()
    }
}
