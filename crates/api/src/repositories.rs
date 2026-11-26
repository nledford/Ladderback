//! Repositories on the wire: their settings and locations, connecting,
//! initializing and testing one, and its statistics.

use serde::{Deserialize, Serialize};

use crate::WarningDto;

/// The repositories list: what the Repositories page shows and the snapshots
/// page offers to pick from, in the order of their aliases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoriesDto {
    pub repositories: Vec<RepositoryDto>,
}

/// What the Repositories page notes above its list, of the machine rather
/// than of one repository: `keyring-not-activatable` while the keyring runs
/// but can't be started on demand (#923). Left empty when there is nothing
/// to note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryNotesDto {
    pub notes: Vec<WarningDto>,
}

/// One configured repository, as a list shows it: where it is, with any
/// password in the location masked, and where its password comes from.
/// Never the password itself (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryDto {
    pub alias: String,
    /// The location, with the password of a URL in it masked.
    pub repository: String,
    /// The hot half of a hot/cold repository, masked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordSourceDto,
    /// Who the entry asks to maintain the repository (§7.3): this agent,
    /// the hub or nobody from here, so a page can say so in words. A row
    /// that doesn't say asks nobody.
    #[serde(default)]
    pub maintenance: RepositoryMaintenanceDto,
    /// Whether this agent maintains the repository: forgets, prunes and
    /// checks it (I1, §7.3). A row that doesn't say is maintained elsewhere,
    /// so a client offers no maintenance for it.
    #[serde(default)]
    pub maintained_here: bool,
    /// Since when this paired machine's maintenance of the repository is
    /// paused because it holds no lease on it from its hub, as an RFC 3339
    /// time (§7.3, decision 158). Left out when it isn't: a machine that
    /// isn't paired, a lease that holds, or a repository maintained
    /// elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance_paused_since: Option<String>,
    /// Whether the row offers to create the repository where its entry says:
    /// its entry records no repository ID, its latest operation failed with
    /// `repository-not-found`, and none of them ever succeeded, so never
    /// while none is on record (decision 53). Left out when it doesn't.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub create_offered: bool,
    /// What the configuration holds that the user should know about this
    /// repository: one per secret written inline, and one
    /// `location-plain-http-credentials` per location that sends its
    /// credentials where an impostor could read them (decision 80). Left
    /// out when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
    /// Every location of a repository reached at several, in the order
    /// they are tried, the first first (decision 80). Left out for a
    /// repository of one location, which `repository` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocationDto>,
}

/// Who a repository's entry asks to maintain it, as its `maintenance`
/// setting says (§7.3): `local`, this agent; `hub`, the hub; `none`, nobody
/// from here, which an entry without the setting asks too.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryMaintenanceDto {
    Local,
    Hub,
    #[default]
    None,
}

/// One location of a repository reached at several, as the Repositories
/// page lists it: its address with the credentials written in it masked,
/// and its options by name. Never a secret (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationDto {
    /// Its place in the order, from 1.
    pub position: u8,
    /// `repository` as the entry writes it, masked.
    pub repository: String,
    /// `repo-hot`, masked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    /// Its options, set by set, each by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<LocationOptionDto>,
    /// Its rule's alternatives, in the order written, of which any one
    /// holding lets it be tried. Left out for a location with no rule,
    /// which is tried on every network.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<LocationAlternativeDto>,
}

/// One alternative of a location's rule (decision 80).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationAlternativeDto {
    /// The kind of network, as the configuration writes it: `wired` or
    /// `wifi`.
    pub network: String,
    /// The Wi-Fi networks, by name, it holds on alone (`only`); empty when
    /// it lists none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    /// The Wi-Fi networks, by name, it doesn't hold on (`except`); empty
    /// when it lists none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub except: Vec<String>,
}

/// One option of a location: its name, and its value only when the value
/// is references alone (`${LAN_KEY}`), which say where a secret is and
/// never hold it. A value written in the configuration is never shown: it
/// may be the secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationOptionDto {
    /// `options`, `options-hot` or `options-cold`, as the configuration
    /// names the set.
    pub set: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// What became of one location of a repository reached at several: in an
/// operation, as the run's choice saw it; in a Test, as its probe found it
/// (decision 80). Named by its position, never a network's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationTriedDto {
    /// Its place in the order, from 1.
    pub position: u8,
    /// Its address as the entry wrote it then, credentials masked, at most
    /// 256 bytes.
    pub address: String,
    /// `used` (in a Test: it answers and holds the repository),
    /// `not-tried`, `rule-not-met`, `rule-unknown`, `silent`, `refused`,
    /// `not-a-repository`, `other-repository`, `password-not-accepted`,
    /// `secret-unavailable`, or the refusal code of a setting that can't be
    /// used (`template-invalid`, `repository-path-relative`,
    /// `secret-expansion-too-long`, `secret-in-process-arguments`,
    /// `location-too-many-credentials`).
    pub outcome: String,
    /// For a location whose settings didn't resolve: where the secret was
    /// looked for (`env LAN_KEY`), or the setting that can't be used. Never
    /// a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The answer to a Test of a saved repository's locations
/// (`POST /api/v1/repositories/{alias}/test`): each location's outcome, in
/// order, none `not-tried`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LocationsTestDto {
    pub locations: Vec<LocationTriedDto>,
}

/// Where a configured repository's password comes from, without its value:
/// all five sources the configuration takes, the command by its program
/// only, since its arguments may hold the secret itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum PasswordSourceDto {
    /// The configuration file holds the password.
    Inline,
    /// A file holds it.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon.
    Env { name: String },
    /// The output of a program.
    Command { program: String },
}

/// Where a request says the repository's password is: the four sources the
/// API takes. A `command` source is not one of them: it runs a program, and
/// the first route that may run one is the one behind authentication
/// (technical design §10.1). Its `Debug` form never shows a typed password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PasswordDto {
    /// The password itself, as the person typed it.
    Inline { value: String },
    /// A file that holds it, by its absolute path.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon that holds it.
    Env { name: String },
}

impl std::fmt::Debug for PasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { .. } => f.write_str("Inline { value: *** }"),
            Self::File { path } => f.debug_struct("File").field("path", path).finish(),
            Self::Keyring { entry } => f.debug_struct("Keyring").field("entry", entry).finish(),
            Self::Env { name } => f.debug_struct("Env").field("name", name).finish(),
        }
    }
}

/// What a request says about one repository: where it is and how to open it.
/// The field names are the configuration's own, so a repository written in
/// the file and one typed into the page mean the same. Its `Debug` form shows
/// no text a person could have put a secret in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RepositorySettingsDto {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordDto,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_hot: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_cold: std::collections::BTreeMap<String, String>,
}

impl std::fmt::Debug for RepositorySettingsDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A location can carry a password in a URL and an option can be a
        // credential, so only what is not text a person typed is shown.
        f.debug_struct("RepositorySettingsDto")
            .field("repository", &"***")
            .field("repo_hot", &self.repo_hot.as_ref().map(|_| "***"))
            .field("password", &self.password)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .field("options_hot", &self.options_hot.keys().collect::<Vec<_>>())
            .field(
                "options_cold",
                &self.options_cold.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// `POST /api/v1/repositories/_test`: open the repository these settings
/// describe, and save nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TestRepositoryDto {
    pub settings: RepositorySettingsDto,
}

/// `POST /api/v1/repositories`: test the repository, and save it under
/// `alias` once the test passed. `maintain-here` asks this agent to maintain
/// it (`maintenance = "local"`, §7.3); left out or `false`, nobody is asked.
/// `keep-in-keyring` keeps a typed (inline) password in the system keyring
/// entry `ladderback/<alias>` and saves a reference to it (§10.2); left out
/// or `false`, the password is saved as it was sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConnectRepositoryDto {
    pub alias: String,
    pub settings: RepositorySettingsDto,
    #[serde(default)]
    pub maintain_here: bool,
    #[serde(default)]
    pub keep_in_keyring: bool,
}

/// `PUT /api/v1/repositories/{alias}/maintainer`: turn maintenance of a
/// saved repository from this agent on (`maintain-here: true`, its entry
/// then asks for `local`) or off (`none`), §7.3. Answers 204; turning it off
/// while a forget, prune, check or adoption of it runs here is refused with
/// `maintenance-busy` (I1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MaintainerDto {
    pub maintain_here: bool,
}

/// `POST /api/v1/repositories/_init`: create a repository where there is
/// none, and save it under `alias` when one is given. `keep-in-keyring`, as
/// for [`ConnectRepositoryDto`], needs an `alias`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InitRepositoryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub settings: RepositorySettingsDto,
    #[serde(default)]
    pub keep_in_keyring: bool,
}

/// What testing or connecting a repository answers: its ID, as rustic
/// knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryIdDto {
    pub id: String,
}

/// What initializing a repository answers: its ID, and whether it was saved
/// into the configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryInitDto {
    pub id: String,
    pub saved: bool,
}

/// One reading of a repository's stats (§16, decision 104): when it was taken
/// and its figures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StatsReadingDto {
    /// RFC 3339, UTC: when the stats were read.
    pub taken_at: String,
    /// How many snapshots the repository holds.
    pub snapshots: u64,
    /// The sources the snapshots hold, added up, in bytes: a file that ten
    /// snapshots hold counts ten times.
    pub total_size: u64,
    /// What the repository's packs store for them, in bytes, after
    /// deduplication and compression.
    pub stored_size: u64,
    /// The share of the data's raw size compression saved, a whole percent
    /// rounded down; absent when the repository stores nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression_saved_percent: Option<u8>,
}

/// A repository's stats as last read and kept: `latest` is absent when none
/// have been read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryStatsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<StatsReadingDto>,
}

#[cfg(test)]
mod tests;
