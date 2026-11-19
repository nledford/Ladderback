//! Wire types shared by the daemon, the hub, the worker and the web UI: the
//! published language (technical design §3.1). Builds for `wasm32`.
//!
//! Errors and warnings cross every boundary as codes with typed arguments,
//! never as sentences; the presentation layer renders them (§8.4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use serde::{Deserialize, Serialize};

mod codes;
mod events;
mod execution;
mod interop;
mod planning;
mod snapshots;
pub mod sync;
#[cfg(test)]
mod tests;

pub use codes::*;
pub use events::*;
pub use execution::*;
pub use interop::*;
pub use planning::*;
pub use snapshots::*;

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
    /// Whether this agent maintains the repository: forgets, prunes and
    /// checks it (I1, §7.3). A row that doesn't say is maintained elsewhere,
    /// so a client offers no maintenance for it.
    #[serde(default)]
    pub maintained_here: bool,
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

/// The header that marks a request the web UI sent on its own, such as a
/// re-read after an event or a poll, rather than one a person caused
/// (§10.1). With sign-in on, a marked request never uses its session: only
/// what a person does keeps one from ending unused. Its value is ignored.
/// It can only shorten a session's life, so the daemon trusts it as sent.
pub const BACKGROUND_HEADER: &str = "x-ladderback-background";

/// `GET /api/v1/session` and a sign-in's answer (§10.1): whether sign-in is
/// on, and whether the request's session is live. A live session's answer
/// carries the CSRF token its changed requests send as `x-csrf-token`, how
/// long the session lasts unused (`idle-seconds`, left out for a session
/// signed in with Keep me signed in on this device, which has no idle
/// limit), and how long it has left however it is used (`ends-in-seconds`,
/// left out when nothing but its idle limit, a sign-out or a new admin
/// password ends it): the web UI warns before the first ends it (components
/// §4.5). A signed-out answer says whether the sign-in form offers Keep me
/// signed in on this device (`keep-signed-in-offered`), which it doesn't
/// under Stay signed in for Until I close the browser. `exposure` is
/// `network` when the daemon listens beyond loopback, and left out on
/// loopback, as before it existed: with sign-in off, the web UI then warns
/// on every page. With sign-in off, `admin-password` says an admin password
/// `ladderback set-password` has kept since the daemon started, looked for as
/// the route is read: sign-in turns on only at the next start (decision 70),
/// and it is left out while none is kept. The build the daemon runs
/// (`version`, `commit`, `dirty`, `release`) is told to a page that may see
/// the pages, with sign-in off or a live session, and left out for a page
/// that has yet to sign in. A request sending a `read` API token is answered
/// `signed-out` with the build, without `keep-signed-in-offered` or a CSRF
/// token, whatever session cookie it also sends (decision 95). With the
/// build, and only with it, comes the `available-update` the agent's release
/// check found, when there is one (decision 107).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionDto {
    pub sign_in: SignInStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_in_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "ExposureDto::is_loopback")]
    pub exposure: ExposureDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_password: Option<AdminPasswordDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_signed_in_offered: Option<bool>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_update: Option<AvailableUpdateDto>,
}

/// A newer release of Ladderback than the one the daemon runs, which the
/// header shows (decision 107): its version, and the address of its notes,
/// built from the version, never from what the release feed answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AvailableUpdateDto {
    pub version: String,
    pub release_notes: String,
}

/// What `GET /api/v1/restart` answers, and `POST` once it has taken a
/// request (decision 94): whether Settings offers a restart, which it does
/// only when a service manager runs the daemon and starts it again once it
/// exits; whether a restart asked for once the running operations finish is
/// pending, neither come nor cancelled, whatever runs (it stays pending
/// until the daemon stops); and how many operations are running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestartDto {
    pub offered: bool,
    pub pending: bool,
    pub running: u32,
}

/// The body of `POST /api/v1/restart`: when the daemon restarts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestartRequestDto {
    pub when: RestartWhenDto,
}

/// When a restart happens: once no operation is running, starting no queued
/// one meanwhile (`finished`); or at once, stopping the running operations
/// in order, as any stop does (`now`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartWhenDto {
    Finished,
    Now,
}

/// What `GET /api/v1/full-disk-access` answers, and `POST …/check` once the
/// probe has run again (§16, decision 115): what the agent's Full Disk Access
/// probe found last, and whether this browser may ask the agent to open
/// System Settings' Full Disk Access pane, which it may only from the
/// agent's own Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FullDiskAccessDto {
    pub state: FullDiskAccessStateDto,
    pub opens_settings: bool,
}

/// What the Full Disk Access probe found: `not-applicable` where the agent
/// isn't on macOS, which has no such grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FullDiskAccessStateDto {
    Granted,
    Missing,
    Unknown,
    NotApplicable,
}

/// What `GET /api/v1/status` answers: the status summary (§3.1, §4.6). Each
/// plan, in the order the configuration writes them; the operations queued
/// or running among the newest 200, newest first; the repositories
/// that couldn't be read at their last attempt, in the order of their
/// aliases; and what the Full Disk Access probe found. Plans and
/// repositories are named by their IDs and aliases, never by an address or
/// anything else a secret could be in (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct StatusDto {
    pub plans: Vec<PlanStatusDto>,
    /// Queued or running, as each one's `status` says.
    pub running: Vec<OperationSummaryDto>,
    pub unreadable_repositories: Vec<UnreadableRepositoryDto>,
    pub full_disk_access: FullDiskAccessStateDto,
}

/// One plan in the status summary: the plans list's entry, its last
/// operation, its last successful backup, however long ago,
/// and whether it is overdue (§3.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct PlanStatusDto {
    #[serde(flatten)]
    pub plan: PlanSummaryDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<OperationSummaryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success: Option<LastSuccessDto>,
    pub overdue: bool,
}

/// A plan's last successful backup: its operation's ID, when it finished,
/// and how many whole seconds ago that was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LastSuccessDto {
    pub operation: String,
    pub finished_at: String,
    pub ago_seconds: u64,
}

/// A repository, by its alias, that couldn't be read at its last attempt,
/// and the operation that couldn't read it, its `failure` saying why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct UnreadableRepositoryDto {
    pub repository: String,
    pub operation: OperationSummaryDto,
}

/// The build a program came from (§3.1, decision 90): its version, the
/// commit it was built from (`null` when the build had no git to ask), whether
/// the tree had uncommitted changes, and whether the commit is the one its
/// version's release tag names. Separate fields, so no reader parses the
/// shown form ([`BuildDto::display`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct BuildDto {
    /// The workspace version, `0.3.0`.
    pub version: String,
    /// The full commit hash.
    pub commit: Option<String>,
    /// Whether tracked files differed from the commit.
    pub dirty: bool,
    /// Whether the commit is the one the tag `v<version>` names.
    pub release: bool,
}

/// How many of a commit's hex digits the shown form keeps, as `git`'s short
/// hash does.
const SHORT_COMMIT: usize = 7;

impl BuildDto {
    /// The build as people read it, a SemVer version with build metadata:
    /// `0.3.0` for a clean build of its release tag or when the commit is
    /// unknown; otherwise the version and the short commit, `0.3.0+3f937cf`,
    /// with `.dirty` after it when the tree had changes.
    pub fn display(&self) -> String {
        match &self.commit {
            Some(commit) if self.dirty || !self.release => {
                let short: String = commit.chars().take(SHORT_COMMIT).collect();
                let dirty = if self.dirty { ".dirty" } else { "" };
                format!("{}+{short}{dirty}", self.version)
            }
            _ => self.version.clone(),
        }
    }
}

/// The recent log (§3.1, #618): the newest lines of the daemon log, newest
/// first, as `GET /api/v1/log` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct RecentLogDto {
    pub lines: Vec<LogLineDto>,
}

/// One line of the daemon log, as the log wrote it, its secrets masked (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LogLineDto {
    /// When it was written: RFC 3339 in UTC, to the millisecond
    /// (`2026-11-15T09:00:01.250Z`), as the log writes it.
    pub at: String,
    pub level: LogLevelDto,
    /// The part of Ladderback, or the crate, that wrote it
    /// (`ladderback::scheduler`).
    pub source: String,
    pub message: String,
}

/// How much a line of the daemon log matters, most severe first, so that
/// `Error < Warn`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LogLevelDto {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Where a request stands with sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SignInStateDto {
    /// No admin password is set: there is nothing to sign in to.
    Off,
    /// Sign-in is on and the request carries no live session.
    SignedOut,
    /// The request's session is live.
    SignedIn,
}

/// With sign-in off, the admin password kept since the daemon started
/// (§10.1, decision 70), which only the next start turns sign-in on for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdminPasswordDto {
    /// A hash the daemon can check: a restart turns sign-in on.
    Set,
    /// A file the daemon can't read or check: a restart would refuse to start
    /// until `ladderback set-password` writes it anew.
    Unreadable,
}

/// Who can reach the daemon at the address it listens on (§10.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExposureDto {
    /// This machine only: a loopback address.
    #[default]
    Loopback,
    /// The network: any other address.
    Network,
}

impl ExposureDto {
    /// Whether this is loopback, which the session's answer leaves out.
    pub fn is_loopback(&self) -> bool {
        *self == Self::Loopback
    }
}

/// `POST /api/v1/session`: the password, as typed, and whether to **Keep me
/// signed in on this device** (`keep-signed-in`, false when left out). The
/// password is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInDto {
    pub password: String,
    #[serde(default)]
    pub keep_signed_in: bool,
}

impl std::fmt::Debug for SignInDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SignInDto {{ password: [hidden], keep_signed_in: {} }}",
            self.keep_signed_in
        )
    }
}

/// `POST /api/v1/admin-password`: the new admin password, as typed, and, with
/// sign-in on, the current one, which a change needs (§10.1, decision 70).
/// With sign-in off the current one is not asked for, and not read. Never
/// logged or shown (I4): its `Debug` hides both.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SetAdminPasswordDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
    pub new_password: String,
}

impl std::fmt::Debug for SetAdminPasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = if self.current_password.is_some() {
            "Some([hidden])"
        } else {
            "None"
        };
        write!(
            f,
            "SetAdminPasswordDto {{ current_password: {current}, new_password: [hidden] }}"
        )
    }
}

/// The session length, as **Stay signed in for** names it (§10.1): how long
/// every session lasts after its sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionLengthDto {
    #[serde(rename = "until-browser-closes")]
    UntilBrowserCloses,
    #[serde(rename = "7-days")]
    SevenDays,
    #[serde(rename = "30-days")]
    ThirtyDays,
    #[serde(rename = "90-days")]
    NinetyDays,
    #[serde(rename = "1-year")]
    OneYear,
    #[serde(rename = "until-sign-out")]
    UntilSignOut,
}

/// `GET` and `PUT /api/v1/settings/sign-in`: the sign-in settings, which
/// only a session reads or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInSettingsDto {
    pub session_length: SessionLengthDto,
}

/// A scope of an API token (§10.1, decision 93), as the token routes name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiTokenScopeDto {
    Read,
    Run,
    Preview,
    Propose,
}

/// One API token as the token routes list it: who it is, what it may do,
/// when it was created and last used. Never its secret (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokenDto {
    pub id: String,
    pub label: String,
    pub scopes: Vec<ApiTokenScopeDto>,
    pub created_at: String,
    /// `null` for a token never used.
    #[serde(default)]
    pub last_used_at: Option<String>,
}

/// `GET /api/v1/tokens`: the tokens kept, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokensDto {
    pub tokens: Vec<ApiTokenDto>,
}

/// `POST /api/v1/tokens`: the label and scopes of the token to create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CreateApiTokenDto {
    pub label: String,
    pub scopes: Vec<ApiTokenScopeDto>,
}

/// What creating a token answers: the token as it is listed, and its whole
/// text, `ladderback_<id>_<secret>`, shown this once. Its `Debug` hides the
/// secret (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreatedApiTokenDto {
    pub token: ApiTokenDto,
    pub secret: String,
}

impl std::fmt::Debug for CreatedApiTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedApiTokenDto")
            .field("token", &self.token)
            .field("secret", &"[hidden]")
            .finish()
    }
}

/// What initialising a repository answers: its ID, and whether it was saved
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

/// How many snapshots one group of [`SnapshotGroupsDto`] lists. An
/// input-derived size, capped (§13.5): a repository can hold thousands of
/// snapshots of one plan, and a page that lists them all is no use to the
/// person reading it. The `count` next to a capped list is the real total.
pub const MAX_GROUP_SNAPSHOTS: usize = 500;

/// How many snapshots one list of a [`MaintenancePreviewDto`] names: a
/// review is read in one sitting, so it names fewer than a group's page.
pub const MAX_PREVIEW_SNAPSHOTS: usize = 50;

/// One plan of this agent on a repository, and the snapshots it owns, newest
/// first, at most [`MAX_GROUP_SNAPSHOTS`] of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlanGroupDto {
    pub plan: String,
    /// How many snapshots the plan owns, listed or not.
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
}

/// Snapshots a `ladderback-` tag owns that no plan of this agent does:
/// another agent's plan, or a plan this configuration no longer has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OrphanedGroupDto {
    /// The `ladderback-agent:` tag's text, `null` when the snapshots carry
    /// none, or two that disagree.
    pub agent: Option<String>,
    /// The `ladderback-plan:` tag's text, `null` when the snapshots carry
    /// none, or two that disagree.
    pub plan: Option<String>,
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
    /// The plans of this repository the group can be adopted into: all of
    /// them, since which one takes the history is the person's choice.
    pub adopt_into: Vec<String>,
}

/// Untagged snapshots, grouped by host, label and paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct UnmanagedGroupDto {
    pub hostname: String,
    /// Empty when the snapshots carry no label.
    pub label: String,
    pub paths: Vec<String>,
    pub count: u64,
    pub snapshots: Vec<SnapshotDto>,
    /// The plans whose host, label and sources equal the group's: none when
    /// no plan would adopt it, one when it is clear which does, several when
    /// it is ambiguous.
    pub adopt_into: Vec<String>,
}

/// One repository's snapshots grouped by who owns them (technical design
/// §7.3): what the Snapshots page shows beside its maintenance buttons.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotGroupsDto {
    /// Every plan of this agent on the repository, in the configuration's
    /// order, the ones with no snapshots too.
    pub plans: Vec<PlanGroupDto>,
    pub orphaned: Vec<OrphanedGroupDto>,
    pub unmanaged: Vec<UnmanagedGroupDto>,
    /// What the page warns of the repository: the other hosts it holds
    /// snapshots from, when it is maintained here (§7.3). Left out when
    /// there is nothing to warn of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
}

/// How many of a list's snapshots are shown, and which: how many `snapshots`
/// there are in all, and the first `max` of them as `dto` shows them. Only
/// those are built, so a group of thousands costs no more than a page of it
/// (an input-derived size, capped, §13.5). A snapshot `dto` can't show is
/// left out of the list and still counted.
fn listed<T>(
    snapshots: &[T],
    max: usize,
    dto: impl FnMut(&T) -> Option<SnapshotDto>,
) -> (u64, Vec<SnapshotDto>) {
    (
        u64::try_from(snapshots.len()).unwrap_or(u64::MAX),
        snapshots.iter().take(max).filter_map(dto).collect(),
    )
}

impl PlanGroupDto {
    /// The group of `plan`, which owns `snapshots`, newest first: `count` is
    /// all of them and at most [`MAX_GROUP_SNAPSHOTS`] are listed. `dto`
    /// shows one snapshot, or `None` for one the wire can't carry.
    pub fn new<T>(
        plan: String,
        snapshots: &[T],
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            plan,
            count,
            snapshots,
        }
    }
}

impl OrphanedGroupDto {
    /// The orphaned group of `agent` and `plan`, which holds `snapshots`,
    /// newest first, and may be adopted into `adopt_into`: `count` is all of
    /// the snapshots and at most [`MAX_GROUP_SNAPSHOTS`] are listed. `dto`
    /// shows one snapshot, or `None` for one the wire can't carry.
    pub fn new<T>(
        agent: Option<String>,
        plan: Option<String>,
        snapshots: &[T],
        adopt_into: Vec<String>,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            agent,
            plan,
            count,
            snapshots,
            adopt_into,
        }
    }
}

impl UnmanagedGroupDto {
    /// The unmanaged group of `hostname`, `label` and `paths`, which holds
    /// `snapshots`, newest first, and may be adopted into `adopt_into`:
    /// `count` is all of the snapshots and at most [`MAX_GROUP_SNAPSHOTS`]
    /// are listed. `dto` shows one snapshot, or `None` for one the wire
    /// can't carry.
    pub fn new<T>(
        hostname: String,
        label: String,
        paths: Vec<String>,
        snapshots: &[T],
        adopt_into: Vec<String>,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, snapshots) = listed(snapshots, MAX_GROUP_SNAPSHOTS, dto);
        Self {
            hostname,
            label,
            paths,
            count,
            snapshots,
            adopt_into,
        }
    }
}

/// What a forget run would do to one plan's history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ForgetPlanDto {
    pub plan: String,
    /// How many snapshots the plan owns.
    pub total: u64,
    /// How many of them the plan's retention lets go.
    pub remove: u64,
    /// The ones it lets go, newest first, at most [`MAX_PREVIEW_SNAPSHOTS`].
    pub listed: Vec<SnapshotDto>,
}

impl ForgetPlanDto {
    /// What a forget would do to `plan`, which owns `total` snapshots and
    /// whose policy lets `remove` go, newest first: at most
    /// [`MAX_PREVIEW_SNAPSHOTS`] are listed. `dto` shows one snapshot, or
    /// `None` for one the wire can't carry.
    pub fn new<T>(
        plan: String,
        total: u64,
        remove: &[T],
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (remove, listed) = listed(remove, MAX_PREVIEW_SNAPSHOTS, dto);
        Self {
            plan,
            total,
            remove,
            listed,
        }
    }
}

/// How many groups and snapshots a forget leaves alone of one kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct TallyDto {
    pub groups: u64,
    pub snapshots: u64,
}

/// The history a forget leaves alone (I6): groups no plan of this agent
/// owns, which only an adoption or an explicit deletion touches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct LeftAloneDto {
    pub orphaned: TallyDto,
    pub unmanaged: TallyDto,
}

/// What a maintenance run would do, told before it starts: a read, so it
/// takes no maintenance slot and writes no log row (§7.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum MaintenancePreviewDto {
    /// Which snapshots of each plan the plan's retention lets go, and what
    /// is left alone. `selection` names that selection: a start that carries
    /// it is refused when the repository has changed since.
    Forget {
        plans: Vec<ForgetPlanDto>,
        left_alone: LeftAloneDto,
        selection: String,
        /// What the review warns of: the other hosts the repository holds
        /// snapshots from, when it is maintained here (§7.3). Left out when
        /// there is nothing to warn of.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// The snapshots an adoption would rewrite into the plan `into`.
    Adopt {
        into: String,
        /// How many snapshots the adoption rewrites.
        count: u64,
        /// The first of them, newest first, at most
        /// [`MAX_PREVIEW_SNAPSHOTS`].
        listed: Vec<SnapshotDto>,
        selection: String,
        /// As a forget's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// What a prune would do, from a dry run of the engine's own plan. Its
    /// figures are estimates: the plan is computed again when the prune
    /// runs.
    Prune {
        packs_marked: u64,
        packs_deleted: u64,
        packs_recovered: u64,
        bytes_deleted: u64,
        /// How long a pack stays marked before a later prune deletes it
        /// (I2).
        keep_delete_hours: u64,
        /// As a forget's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WarningDto>,
    },
    /// A check changes nothing; it holds the repository while it runs.
    Check { read_data: bool },
}

impl MaintenancePreviewDto {
    /// The preview of an adoption into `into` of `snapshots`, newest first:
    /// `count` is all of them and at most [`MAX_PREVIEW_SNAPSHOTS`] are
    /// listed. `dto` shows one snapshot, or `None` for one the wire can't
    /// carry. It warns of nothing until [`warned`](Self::warned) says.
    pub fn adopt<T>(
        into: String,
        snapshots: &[T],
        selection: String,
        dto: impl FnMut(&T) -> Option<SnapshotDto>,
    ) -> Self {
        let (count, listed) = listed(snapshots, MAX_PREVIEW_SNAPSHOTS, dto);
        Self::Adopt {
            into,
            count,
            listed,
            selection,
            warnings: Vec::new(),
        }
    }

    /// The same preview, warning of `warnings`. A check's warns of nothing:
    /// it only reads.
    #[must_use]
    pub fn warned(mut self, warned: Vec<WarningDto>) -> Self {
        match &mut self {
            Self::Forget { warnings, .. }
            | Self::Adopt { warnings, .. }
            | Self::Prune { warnings, .. } => *warnings = warned,
            Self::Check { .. } => {}
        }
        self
    }

    /// What the review warns of.
    pub fn warnings(&self) -> &[WarningDto] {
        match self {
            Self::Forget { warnings, .. }
            | Self::Adopt { warnings, .. }
            | Self::Prune { warnings, .. } => warnings,
            Self::Check { .. } => &[],
        }
    }
}

/// How a request names the group an adoption rewrites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "from", from = "AdoptionFromWire")]
pub enum AdoptionFromDto {
    /// The orphaned group these two tags name.
    Orphaned {
        /// The `ladderback-agent:` tag's text.
        agent: String,
        /// The `ladderback-plan:` tag's text.
        plan: String,
    },
    /// The unmanaged group whose host, label and paths equal the
    /// destination plan's host, label and sources.
    Unmanaged,
}

/// What [`AdoptionFromDto`] is read from. Serde's internally tagged unit
/// variant skips every key it doesn't know, so the read goes through a
/// struct variant that refuses them: the wire is the same, and a key the
/// body doesn't know is an invalid body.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "from", deny_unknown_fields)]
enum AdoptionFromWire {
    Orphaned { agent: String, plan: String },
    Unmanaged {},
}

impl From<AdoptionFromWire> for AdoptionFromDto {
    fn from(wire: AdoptionFromWire) -> Self {
        match wire {
            AdoptionFromWire::Orphaned { agent, plan } => Self::Orphaned { agent, plan },
            AdoptionFromWire::Unmanaged {} => Self::Unmanaged,
        }
    }
}

/// The body of a maintenance preview and of a maintenance start: the kind,
/// for an adoption its destination plan and the group it rewrites, and, on a
/// start, the selection a preview named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MaintenanceRequestDto {
    /// `forget`, `prune`, `check` or `adopt`.
    pub kind: String,
    /// The plan an adoption's history becomes part of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    /// The group an adoption rewrites: another owner's tagged history, or
    /// the unmanaged history that matches the destination plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AdoptionFromDto>,
    /// The selection a preview named, for a forget or an adoption. Absent,
    /// the run selects as it finds the repository; present, it is refused
    /// when what it would select is not what the preview showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
}

/// Where a restore writes (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestoreDestinationDto {
    /// A folder made for this restore under the agent's restore folder.
    NewFolder,
    /// Over the original paths: adds and overwrites, never deletes.
    InPlace,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/restore/preview`:
/// the entry whose restore in place a dry run is to tell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestorePreviewRequestDto {
    /// The entry's path in the snapshot, its names joined by `/`; empty for
    /// the snapshot's top.
    pub path: String,
}

/// What restoring an entry in place would do, from a dry run that wrote
/// nothing (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestorePreviewDto {
    /// Files it would add, where nothing is.
    pub added: u64,
    /// Files it would overwrite, which differ from the snapshot's.
    pub overwritten: u64,
    /// Files already as the snapshot holds them.
    pub unchanged: u64,
    /// Entries the original holds and the snapshot doesn't: they stay. A
    /// folder of them counts once.
    pub kept: u64,
    /// Bytes it would write.
    pub bytes: u64,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/restore`: the
/// entry to restore, where to, and the step-up confirmation: the admin
/// password with sign-in on, the snapshot's short ID otherwise. The
/// confirmation is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RestoreRequestDto {
    /// The entry's path in the snapshot, as [`RestorePreviewRequestDto`]
    /// gives it.
    pub path: String,
    pub destination: RestoreDestinationDto,
    pub confirmation: String,
}

impl std::fmt::Debug for RestoreRequestDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreRequestDto")
            .field("path", &self.path)
            .field("destination", &self.destination)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// What `POST …/snapshots/{snapshot}/restore` answers once the restore
/// started: its operation's ID and, for a restore into a new folder, that
/// folder, so the person is told where its files are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestoreStartedDto {
    pub operation: String,
    /// The folder made for the restore; absent when it restores over the
    /// original paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// The archive a downloaded folder is handed over as (§10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveFormatDto {
    Tar,
    Zip,
}

/// `POST /api/v1/repositories/{alias}/snapshots/{snapshot}/download`: the
/// entry to download, the archive a folder goes as, and the step-up
/// confirmation, as a restore asks for it. The confirmation is never
/// logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DownloadRequestDto {
    /// The entry's path in the snapshot, as [`RestorePreviewRequestDto`]
    /// gives it; empty for the snapshot's top.
    pub path: String,
    /// The archive a folder is handed over as; a file goes as itself.
    pub format: ArchiveFormatDto,
    pub confirmation: String,
}

impl std::fmt::Debug for DownloadRequestDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadRequestDto")
            .field("path", &self.path)
            .field("format", &self.format)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// What `POST …/snapshots/{snapshot}/download` answers once the download
/// is confirmed: the ticket its browser fetches it by, once, within a
/// minute, at `GET /api/v1/downloads/{ticket}`. A capability: never logged
/// or shown, and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DownloadStartedDto {
    pub ticket: String,
}

impl std::fmt::Debug for DownloadStartedDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadStartedDto")
            .field("ticket", &"[hidden]")
            .finish()
    }
}
