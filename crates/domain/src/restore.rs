//! The Restore context (technical design §3.1, §10.4): what a restore may
//! overwrite, who may ask for one, and the folder a restore into a new folder
//! makes.

use jiff::civil::DateTime;

use crate::SnapshotId;
use crate::access::step_up::short_id;
use crate::browse::FolderPath;

/// Where a restore writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// A folder made for this restore under the agent's restore folder, so
    /// no file that was there is touched (the default).
    NewFolder,
    /// Over the original paths: it adds and overwrites, and never deletes.
    InPlace,
}

/// Where a restore was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreOrigin {
    /// The agent's own web UI.
    LocalUi,
    /// The hub, on behalf of a person signed in there.
    Hub,
}

/// The agent's settings that decide which restores it takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RestorePolicy {
    /// The agent's `allow-hub-restore`.
    pub allow_hub_restore: bool,
}

/// Why a restore was refused before anything ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreRefused {
    /// In place is offered only in the agent's own web UI.
    InPlaceFromHub,
    /// The agent doesn't set `allow-hub-restore`.
    HubNotAllowed,
}

/// One restore a person asked for (the Restore aggregate's root): one entry
/// of one snapshot, where it goes and where it was asked for. Only
/// [`Self::admit`] makes one, so every request holds the policy's rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreRequest {
    snapshot: SnapshotId,
    entry: FolderPath,
    destination: Destination,
    origin: RestoreOrigin,
}

impl RestoreRequest {
    /// The request for `entry` of `snapshot` into `destination`, asked for
    /// from `origin`, if `policy` takes it.
    pub fn admit(
        policy: RestorePolicy,
        snapshot: SnapshotId,
        entry: FolderPath,
        destination: Destination,
        origin: RestoreOrigin,
    ) -> Result<Self, RestoreRefused> {
        if origin == RestoreOrigin::Hub {
            if destination == Destination::InPlace {
                return Err(RestoreRefused::InPlaceFromHub);
            }
            if !policy.allow_hub_restore {
                return Err(RestoreRefused::HubNotAllowed);
            }
        }
        Ok(Self {
            snapshot,
            entry,
            destination,
            origin,
        })
    }

    pub fn snapshot(&self) -> &SnapshotId {
        &self.snapshot
    }

    /// The entry restored, by its keys from the snapshot's top.
    pub fn entry(&self) -> &FolderPath {
        &self.entry
    }

    pub fn destination(&self) -> Destination {
        self.destination
    }

    pub fn origin(&self) -> RestoreOrigin {
        self.origin
    }
}

/// The most characters of the host a new folder's name keeps.
const MAX_HOST_CHARS: usize = 64;

/// The name of the folder a restore into a new folder makes:
/// `<host>-<short ID>-<YYYYMMDD-HHMMSS>`, the time on the agent's clock in
/// its own time zone. The host is text the repository holds, so it is made a
/// safe name of one folder: never empty, `.` or `..`, no separator.
pub fn restore_folder_name(host: &str, snapshot: &SnapshotId, at: DateTime) -> String {
    // Leading dots would hide the folder, or make it `.` or `..`; any other
    // character but a letter, a digit, `.`, `-` and `_` (a separator, a
    // space, a control or a direction override) becomes `_`.
    let mut safe: String = host
        .trim_start_matches('.')
        .chars()
        .take(MAX_HOST_CHARS)
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        safe.push_str("host");
    }
    format!(
        "{safe}-{}-{}",
        short_id(snapshot),
        at.strftime("%Y%m%d-%H%M%S")
    )
}

#[cfg(test)]
mod tests;
