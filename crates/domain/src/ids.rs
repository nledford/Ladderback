use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies one operation (shared kernel). Generated outside the domain
/// (UUIDv7 from the `Random` port), never here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(Uuid);

impl OperationId {
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}
