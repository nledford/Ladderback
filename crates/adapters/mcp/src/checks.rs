//! The tools that check and preview, changing nothing (§4.6):
//! `test_repository` tries a repository's locations, `preview_forget` tells
//! what a forget would let go, `preview_import` what importing a profile
//! would add, and `export_profile` writes a plan out as a profile. Their
//! arguments and the one result of their own.

use api::{ErrorCode, ErrorDto, ForgetPlanDto, LeftAloneDto, MaintenancePreviewDto, WarningDto};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The arguments of `test_repository` and `preview_forget`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct RepositoryArguments {
    /// The repository's alias, as `list_repositories` names it.
    pub repository: String,
}

/// The arguments of `preview_import`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ImportArguments {
    /// The rustic profile's TOML itself, never a path: no file is read, and
    /// the profiles it uses are named in a notice.
    pub content: String,
    /// The alias the profile's repository would be saved under.
    pub repository: String,
}

/// The arguments of `export_profile`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ExportArguments {
    /// The plan's ID, as `list_plans` names it.
    pub plan: String,
}

/// What `preview_forget` returns: which snapshots of each plan its
/// retention lets go, what is left alone, and what the review warns of.
/// Without the preview's `selection`, which only a forget's start takes,
/// and MCP starts none (§4.6).
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
pub struct ForgetPreview {
    pub plans: Vec<ForgetPlanDto>,
    pub left_alone: LeftAloneDto,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
}

impl ForgetPreview {
    /// The forget preview `preview` holds.
    ///
    /// # Errors
    ///
    /// `daemon-unreachable`: `preview` is of another kind, an answer no
    /// Ladderback gives to a forget's preview.
    pub fn of(preview: MaintenancePreviewDto) -> Result<Self, ErrorDto> {
        match preview {
            MaintenancePreviewDto::Forget {
                plans,
                left_alone,
                selection: _,
                warnings,
            } => Ok(Self {
                plans,
                left_alone,
                warnings,
            }),
            _ => Err(ErrorDto::new(ErrorCode::DaemonUnreachable, [], None)),
        }
    }
}
