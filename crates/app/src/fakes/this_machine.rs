use std::sync::{Mutex, PoisonError};

use domain::fleet::MachineIdentifier;
use domain::{AgentId, Uuid};

use crate::ports::{MachineError, ThisMachine};

/// A machine whose system gives the identifier it was told, with its data
/// folder's record in memory.
pub struct FakeThisMachine {
    identifier: Mutex<Option<MachineIdentifier>>,
    recorded: Mutex<Option<MachineIdentifier>>,
    agent: Mutex<Option<AgentId>>,
    renewed: Mutex<u128>,
}

impl FakeThisMachine {
    /// A machine whose system gives `identifier` (none that reads when it
    /// is `None`), with nothing recorded.
    pub fn new(identifier: Option<&str>) -> Self {
        let _ = identifier;
        todo!()
    }

    /// Records `identifier` beside the machine ID, as a data folder made on
    /// another computer, or this one, holds it.
    pub fn with_recorded(self, identifier: &str) -> Self {
        let _ = identifier;
        todo!()
    }

    /// The machine ID the data folder holds: the last one renewed, or
    /// `None` while it holds the one it started with.
    pub fn renewed_id(&self) -> Option<AgentId> {
        *self.agent.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ThisMachine for FakeThisMachine {
    fn identifier(&self) -> Result<MachineIdentifier, MachineError> {
        let _ = (&self.identifier, &self.recorded, &self.renewed, Uuid::nil());
        todo!()
    }

    fn recorded(&self) -> Result<Option<MachineIdentifier>, MachineError> {
        todo!()
    }

    fn record(&self, identifier: &MachineIdentifier) -> Result<(), MachineError> {
        let _ = identifier;
        todo!()
    }

    fn renew(&self, identifier: &MachineIdentifier) -> Result<AgentId, MachineError> {
        let _ = identifier;
        todo!()
    }
}
