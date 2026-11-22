use std::sync::{Mutex, PoisonError};

use domain::fleet::MachineIdentifier;
use domain::{AgentId, Uuid};

use crate::ports::{MachineError, ThisMachine};

/// A machine whose system gives the identifier it was told, with its data
/// folder's record in memory.
pub struct FakeThisMachine {
    identifier: Option<MachineIdentifier>,
    recorded: Mutex<Option<MachineIdentifier>>,
    /// The machine ID the last renewal gave, and how many there were.
    renewed: Mutex<(Option<AgentId>, u128)>,
}

impl FakeThisMachine {
    /// A machine whose system gives `identifier` (none that reads when it
    /// is `None`), with nothing recorded.
    pub fn new(identifier: Option<&str>) -> Self {
        Self {
            identifier: identifier.map(parsed),
            recorded: Mutex::new(None),
            renewed: Mutex::new((None, 0)),
        }
    }

    /// Records `identifier` beside the machine ID, as a data folder made on
    /// another computer, or this one, holds it.
    pub fn with_recorded(self, identifier: &str) -> Self {
        *self.recorded.lock().unwrap_or_else(PoisonError::into_inner) = Some(parsed(identifier));
        self
    }

    /// The machine ID the data folder holds: the last one renewed, or
    /// `None` while it holds the one it started with.
    pub fn renewed_id(&self) -> Option<AgentId> {
        self.renewed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }
}

impl ThisMachine for FakeThisMachine {
    fn identifier(&self) -> Result<MachineIdentifier, MachineError> {
        self.identifier.clone().ok_or_else(|| MachineError {
            detail: "the fake machine gives no identifier".to_owned(),
        })
    }

    fn recorded(&self) -> Result<Option<MachineIdentifier>, MachineError> {
        Ok(self
            .recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn record(&self, identifier: &MachineIdentifier) -> Result<(), MachineError> {
        *self.recorded.lock().unwrap_or_else(PoisonError::into_inner) = Some(identifier.clone());
        Ok(())
    }

    fn renew(&self, identifier: &MachineIdentifier) -> Result<AgentId, MachineError> {
        let id = {
            let mut renewed = self.renewed.lock().unwrap_or_else(PoisonError::into_inner);
            renewed.1 = renewed.1.saturating_add(1);
            let id = AgentId::from_uuid(Uuid::from_u128(
                0x018f_0000_0000_7000_8000_0000_0000_0000_u128.saturating_add(renewed.1),
            ));
            renewed.0 = Some(id);
            id
        };
        self.record(identifier)?;
        Ok(id)
    }
}

fn parsed(text: &str) -> MachineIdentifier {
    #[expect(
        clippy::expect_used,
        reason = "a fake's identifier is a test's literal, and a test that gives a bad one fails at once"
    )]
    MachineIdentifier::parse(text).expect("a machine identifier")
}
