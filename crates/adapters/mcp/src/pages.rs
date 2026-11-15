//! The read tools' arguments, the pages they return, and how a list is cut
//! into pages (§4.6). A page holds at most [`MAX_PAGE`] items, so no result
//! grows with the size of what it lists; its `next` names where the page
//! after it starts.

use api::{
    ErrorCode, ErrorDto, MsgArg, OperationSummaryDto, OperationWarningDto, PlanSummaryDto,
    RepositoryDto, SnapshotDto,
};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How many items a page lists when the call names no `limit`.
pub const DEFAULT_PAGE: u64 = 50;

/// The most items a page lists: a larger `limit` is clamped to it, never
/// refused. The history's largest page.
pub const MAX_PAGE: u64 = 200;

/// The most warnings `get_operation` lists: an operation keeps up to ten
/// thousand, which no client reads in one result.
pub const MAX_OPERATION_WARNINGS: usize = 100;

/// `limit` as a page's length: [`DEFAULT_PAGE`] when not given, and from 1
/// to [`MAX_PAGE`].
pub fn page_length(limit: Option<u64>) -> u64 {
    limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE)
}

/// The page of `items` after the item whose key is `cursor`, or from the
/// first when there is no cursor, at most [`page_length`] long; and the
/// cursor of the page after it, the key of its last item, when more follow.
///
/// # Errors
///
/// `invalid-cursor`: no item has the key `cursor`.
pub fn page<T>(
    items: Vec<T>,
    key: impl Fn(&T) -> &str,
    cursor: Option<String>,
    limit: Option<u64>,
) -> Result<(Vec<T>, Option<String>), ErrorDto> {
    let start = match cursor {
        None => 0,
        Some(cursor) => match items.iter().position(|item| key(item) == cursor) {
            Some(index) => index.saturating_add(1),
            None => {
                return Err(ErrorDto::new(
                    ErrorCode::InvalidCursor,
                    [("cursor", MsgArg::Text(cursor))],
                    None,
                ));
            }
        },
    };
    let length = usize::try_from(page_length(limit)).unwrap_or(usize::MAX);
    let mut rest = items.into_iter().skip(start);
    let listed: Vec<T> = rest.by_ref().take(length).collect();
    let next = match (rest.next(), listed.last()) {
        (Some(_), Some(last)) => Some(key(last).to_owned()),
        _ => None,
    };
    Ok((listed, next))
}

/// The arguments of `list_plans` and `list_repositories`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListArguments {
    /// How many to list, from 1 to 200; 50 when not given. A larger number
    /// lists 200.
    pub limit: Option<u64>,
    /// The `next` of the page before, to list the page after it.
    pub cursor: Option<String>,
}

/// The arguments of `list_snapshots`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SnapshotsArguments {
    /// The repository, by its alias, as `list_repositories` names it.
    pub repository: String,
    /// How many to list, from 1 to 200; 50 when not given. A larger number
    /// lists 200.
    pub limit: Option<u64>,
    /// The `next` of the page before, to list the page after it.
    pub cursor: Option<String>,
}

/// The arguments of `list_operations`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct OperationsArguments {
    /// Only this plan's operations, by its ID.
    pub plan: Option<String>,
    /// How many to list, from 1 to 200; 50 when not given. A larger number
    /// lists 200.
    pub limit: Option<u64>,
    /// The `next` of the page before, to list the page after it.
    pub cursor: Option<String>,
}

/// The arguments of `get_operation`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct OperationArguments {
    /// The operation's ID, as `list_operations` names it.
    pub id: String,
}

/// One page of the configured plans, in the order they were written.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct PlansPage {
    pub plans: Vec<PlanSummaryDto>,
    /// The `cursor` of the page after this one; `null` on the last page.
    pub next: Option<String>,
}

/// One page of the configured repositories, in the order of their aliases.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct RepositoriesPage {
    pub repositories: Vec<RepositoryDto>,
    /// The `cursor` of the page after this one; `null` on the last page.
    pub next: Option<String>,
}

/// One page of a repository's snapshots, newest first.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotsPage {
    pub snapshots: Vec<SnapshotDto>,
    /// The `cursor` of the page after this one; `null` on the last page.
    pub next: Option<String>,
}

/// One operation: its summary, with its failure when it failed, and the
/// first [`MAX_OPERATION_WARNINGS`] warnings it recorded.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct Operation {
    pub operation: OperationSummaryDto,
    /// The first 100 warnings, in the order they were recorded.
    pub warnings: Vec<OperationWarningDto>,
    /// How many more warnings it recorded than `warnings` lists.
    pub unlisted_warnings: u64,
}

impl Operation {
    /// `operation`, with the first [`MAX_OPERATION_WARNINGS`] of `warnings`.
    pub fn listing(operation: OperationSummaryDto, mut warnings: Vec<OperationWarningDto>) -> Self {
        let unlisted = warnings.len().saturating_sub(MAX_OPERATION_WARNINGS);
        warnings.truncate(MAX_OPERATION_WARNINGS);
        Self {
            operation,
            warnings,
            unlisted_warnings: u64::try_from(unlisted).unwrap_or(u64::MAX),
        }
    }
}
