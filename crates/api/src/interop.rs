//! Profile import and export (technical design §6.2): the wire side of the
//! profile interop context (§3.1).

use serde::{Deserialize, Serialize};

use crate::WarningDto;

/// What `ladderback import-profile --json` prints when the import saved the
/// profile: the name the repository was saved under, the IDs of the plans
/// added, and what the profile could not carry over or changed on the way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ProfileImportDto {
    pub repository: String,
    pub plans: Vec<String>,
    pub notices: Vec<WarningDto>,
}

/// `POST /api/v1/profiles/preview`: what importing the rustic profile whose
/// TOML is `content` would add under the repository alias `repository`,
/// told without saving anything or reading any file (§4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProfilePreviewDto {
    pub content: String,
    pub repository: String,
}

/// `GET /api/v1/plans/{id}/profile`: the plan as a profile the stock
/// `rustic` reads, and what the export left out of it or warns of (§6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ProfileExportDto {
    pub text: String,
    pub notices: Vec<WarningDto>,
}

/// The query `GET /api/v1/plans/{id}/profile` takes: `commands=left-out`
/// leaves the profile's commands out, as the MCP tool `export_profile` asks
/// (#856). Without it the export writes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ProfileExportQuery {
    pub commands: ProfileCommands,
}

/// What an export does with the commands a profile runs: its
/// `password-command`, the options rustic runs as commands and each hook's
/// command line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileCommands {
    /// Written, as the web UI's and the command line's exports have them.
    #[default]
    Written,
    /// Left out, each named by a `profile-command-left-out` notice.
    LeftOut,
}
