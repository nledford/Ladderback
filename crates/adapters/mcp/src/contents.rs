//! The tools that show what snapshots hold (§4.6, #953): one page of a
//! snapshot's folder, and a search of a repository's snapshots for entries
//! by name (§16, decision 127). Both read the repository and change nothing;
//! restoring and downloading stay with the person (§16, decision 20).

use api::{ErrorCode, ErrorDto, MsgArg};
use domain::browse::FolderPath;
use domain::find::{NamePattern, SearchCaps};
use domain::{PlanId, SnapshotId};
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

use crate::arguments::invalid;

/// The arguments of `list_snapshot_folder`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct FolderArguments {
    /// The repository, by its alias, as `list_repositories` names it.
    pub repository: String,
    /// The snapshot, by its full ID, as `list_snapshots` gives it.
    pub snapshot: String,
    /// The folder, by the names of the folders that lead to it from the
    /// snapshot's top, joined by `/` (each a `name` a listing gave); the
    /// snapshot's top when not given.
    pub path: Option<String>,
    /// How many entries to list, a whole number from 1 to 200; 50 when not
    /// given. A larger number lists 200, and 0 lists 1.
    pub limit: Option<u64>,
    /// The `next` of the page before, to list the page after it.
    pub from: Option<u64>,
}

/// The snapshot and the folder `list_snapshot_folder` was given, parsed:
/// an ID no snapshot could have is as unknown as one the repository doesn't
/// hold, and a path no folder could have as one the snapshot doesn't, as
/// the API tells both.
pub fn folder_of(arguments: &FolderArguments) -> Result<(SnapshotId, FolderPath), ErrorDto> {
    let snapshot = SnapshotId::parse(&arguments.snapshot).map_err(|_| {
        ErrorDto::new(
            ErrorCode::SnapshotUnknown,
            [("snapshot", MsgArg::Text(arguments.snapshot.clone()))],
            None,
        )
    })?;
    let path = arguments.path.as_deref().unwrap_or_default();
    let folder = FolderPath::parse(path).map_err(|_| {
        ErrorDto::new(
            ErrorCode::FolderUnknown,
            [("path", MsgArg::Text(path.to_owned()))],
            None,
        )
    })?;
    Ok((snapshot, folder))
}

/// The arguments of `find_in_snapshots`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct FindArguments {
    /// The repository, by its alias, as `list_repositories` names it.
    pub repository: String,
    /// The name to look for: a file's or a folder's name, never a path with
    /// `/`. `*` stands for any run of characters and `?` for any one;
    /// letters match in either case. At most 255 characters.
    pub pattern: String,
    /// Only the snapshots of this plan, by its ID: those whose
    /// `ladderback-plan:` tag names it.
    pub plan: Option<String>,
    /// How many of the newest snapshots to search, a whole number from 1 to
    /// 100; 20 when not given. A larger number searches 100, and 0 one.
    pub snapshots: Option<u64>,
    /// How many matches to return, a whole number from 1 to 500; 100 when
    /// not given. A larger number returns 500, and 0 one.
    pub matches: Option<u64>,
}

/// What `find_in_snapshots` searches for, parsed where it enters: the
/// pattern, the plan and the caps, each cap lowered to its most.
pub struct Search {
    pub repository: String,
    pub pattern: NamePattern,
    pub plan: Option<PlanId>,
    pub caps: SearchCaps,
}

impl TryFrom<FindArguments> for Search {
    type Error = ErrorDto;

    /// # Errors
    ///
    /// `invalid-arguments`: a pattern that is empty, too long or holds a
    /// `/`, or a plan ID no plan could have.
    fn try_from(arguments: FindArguments) -> Result<Self, ErrorDto> {
        let pattern = NamePattern::parse(&arguments.pattern).map_err(|_| invalid("pattern"))?;
        let plan = arguments
            .plan
            .as_deref()
            .map(PlanId::parse)
            .transpose()
            .map_err(|_| invalid("plan"))?;
        Ok(Self {
            repository: arguments.repository,
            pattern,
            plan,
            caps: SearchCaps::clamped(arguments.snapshots, arguments.matches),
        })
    }
}
