use serde::{Deserialize, Serialize};

use crate::{SnapshotId, Timestamp};

/// What Steading knows about one rustic snapshot (shared kernel). Read from
/// the repository; never created or edited except through an operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub id: SnapshotId,
    pub time: Timestamp,
    pub host: String,
    pub label: String,
    pub paths: Vec<String>,
    pub tags: Vec<String>,
}
