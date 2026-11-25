use std::path::Path;

use async_trait::async_trait;
use domain::RepositoryAlias;
use domain::execution::plan_tags::PlanTags;
use domain::planning::{AgentSettings, Hooks, Plan, RepositoryConfig};

use super::Position;

/// Reads a rustic profile (technical design §6.2): the file and the
/// profiles it uses, as the stock `rustic` CLI would merge them, turned into
/// Ladderback's own types. rustic's profile format stays behind this port.
///
/// A profile is input nobody vetted: the port reads a bounded file, and
/// refuses what Ladderback could only guess at. It reads nothing but files
/// (the profile, the profiles it uses) and never opens a repository or runs a
/// hook. A profile given as text it reads without touching a file at all.
///
/// Contract (checked by `contract::profiles`):
/// - a profile it cannot read is a [`ProfileError`] and nothing else;
/// - every plan it returns backs up into `repository`;
/// - no value it returns holds a `${` that was not made by the port itself:
///   what the profile wrote is escaped, so it stays text (§10.2);
/// - a profile read from text reads no profile it uses: each is a
///   [`ProfileNotice::UsedProfileNotRead`].
#[async_trait]
pub trait ProfileReader: Send + Sync {
    /// The repository, plans and hooks of the profile in `file`. Every plan
    /// is for the repository `repository`, which is the alias the caller
    /// will save the profile's repository under.
    async fn read(
        &self,
        file: &Path,
        repository: &RepositoryAlias,
    ) -> Result<ImportedProfile, ProfileError>;

    /// As [`read`](Self::read), of the profile whose TOML is `text`, which
    /// a refusal names `name` as it would a file. Every profile it uses is
    /// a [`ProfileNotice::UsedProfileNotRead`], never looked for: no file is
    /// read (§4.6).
    fn read_text(
        &self,
        name: &str,
        text: &str,
        repository: &RepositoryAlias,
    ) -> Result<ImportedProfile, ProfileError>;
}

/// Writes a plan as a rustic profile the stock `rustic` CLI accepts.
///
/// Contract (checked by `contract::profiles`):
/// - the text holds only keys rustic knows, and no secret as text;
/// - what Ladderback can't say in rustic's words is a [`ProfileNotice`] or a
///   [`ExportRefusal`], never silently dropped;
/// - with [`ExportCommands::LeftOut`], the text holds no `password-command`,
///   no option rustic runs as a command and no hook, and a
///   [`ProfileNotice::CommandLeftOut`] names each key left out.
pub trait ProfileWriter: Send + Sync {
    fn render(&self, export: &PlanExport) -> Result<RenderedProfile, ExportRefusal>;
}

/// What a profile gave: the repository and the plans for it, the hooks that
/// wrapped every job of the profile, and what it couldn't carry over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedProfile {
    pub repository: RepositoryConfig,
    pub plans: Vec<Plan>,
    /// The profile's `[global.hooks]`: they wrap every job of the agent.
    pub hooks: Hooks,
    pub notices: Vec<ProfileNotice>,
}

/// Something the import or export did that the person should know. A typed
/// code, never a sentence (technical design §8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileNotice {
    /// A key or section Ladderback does not carry over, by its path in the
    /// profile (`copy`, `backup.group-by`, `repository.key-file`).
    KeyIgnored { key: String },
    /// `[forget] prune = true`: the repository asks for local maintenance.
    Prune,
    /// `[global.env]` was left out.
    EnvIgnored,
    /// Backup-level hooks now run once per plan.
    HooksPerPlan,
    /// A profile named by `use-profiles` was not found.
    UsedProfileMissing { name: String },
    /// A profile read from text names a profile in `use-profiles`, which
    /// was not read: reading from text reads no file.
    UsedProfileNotRead { name: String },
    /// A secret written in the profile was kept as text in the config.
    InlineSecret { field: String },
    /// A location written in the profile holds a password, which was kept
    /// as text in the config; `field` is the key the location is at.
    /// `rclone` says the location is an rclone one, whose credentials belong
    /// in rclone's own configuration rather than in a reference.
    LocationSecret { field: String, rclone: bool },
    /// A glob of the plan `plan` can never match (#510): `glob` as written,
    /// and `absolute`, the same glob written from the root under the source
    /// it is for as the backup walks it, its links followed; none when no
    /// one source is (see [`crate::globs::NeverMatching`]).
    GlobNeverMatches {
        plan: String,
        glob: String,
        absolute: Option<String>,
    },
    /// One of Ladderback's reserved tags was dropped from a plan.
    TagDropped { tag: String },
    /// An export left an option out because it is a reference; `variable` is
    /// the environment variable to set instead.
    OptionLeftOut { option: String, variable: String },
    /// An export wrote a plan that keeps everything: the profile has no
    /// `keep-*` option, and rustic's `forget` refuses to run on one.
    RetentionEmpty,
    /// An import added its plans to the entry `repository` the configuration
    /// held at the profile's location, which kept its own `setting`: the
    /// profile set it otherwise. `setting` is the entry's setting as the
    /// configuration names it: `password`, `maintenance`, `group-by`,
    /// `options`, `options-hot`, `options-cold` or `hooks`.
    RepositorySettingKept { repository: String, setting: String },
    /// An export wrote a profile that reads the repository's password from
    /// the keyring with `program`, which isn't on the `PATH` it was exported
    /// under: rustic can't read the password on this machine. A warning, not
    /// a refusal: the profile may run on another machine, and nothing in it
    /// is a secret.
    KeyringCommandMissing { program: String },
    /// An export wrote a repository reached at several locations at its
    /// first: rustic has no fallback, so the `further` locations after it
    /// were left out (decision 80).
    LocationsNotExported { further: usize },
    /// An export left out the command at `key`, its path in the profile
    /// (`repository.password-command`, `repository.options.rclone-command`,
    /// `global.hooks.run-before`), because it was asked to leave every
    /// command out ([`ExportCommands::LeftOut`]).
    CommandLeftOut { key: String },
    /// An import made a command runnable: `key` is where the configuration
    /// keeps it (`hooks.run-before`, `repositories.nas.password-command`,
    /// `repositories.nas.options.rclone-command`, `plans.home.hooks.run-after`)
    /// and `program` the program it starts. One for each command, in the
    /// order of [`Imported::commands`](crate::interop::Imported::commands).
    CommandImported { key: String, program: String },
    /// An import left the profile's `[global.hooks]` out: the configuration
    /// already had plans or repositories, around whose jobs they would have
    /// run too.
    GlobalHooksLeftOut,
    /// An import added the plan `plan` with no schedule, as every rustic
    /// profile has none: it runs only when it is started (#1610).
    PlanUnscheduled { plan: String },
}

/// Why a profile couldn't be read. `file` is the path as the user gave it, or
/// the path of the profile it uses that is at fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The file (or a profile it uses) couldn't be read.
    Unreadable { file: String },
    /// The file is longer than `limit` bytes, so it isn't read.
    TooLarge { file: String, limit: u64 },
    /// The file is not a profile rustic accepts. `key` is the setting at
    /// fault and `at` where it is, when the reader can tell; `detail` holds
    /// only what the parser said, never a value.
    Invalid {
        file: String,
        key: Option<String>,
        at: Option<Position>,
        problem: ProfileProblem,
        detail: String,
    },
    /// The profile says something Ladderback could only guess at, at `key`:
    /// a rule that keeps particular snapshots, a plan with no password
    /// source, a source that is not absolute. `at` is where it is written,
    /// when something is.
    Unsupported {
        file: String,
        key: String,
        at: Option<Position>,
    },
}

/// What is wrong with a profile that is not valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileProblem {
    /// The text is not TOML.
    Syntax,
    /// rustic has no such key, and refuses the profile for it.
    UnknownKey,
    /// The value is of another type than the key takes.
    WrongType,
    /// The value is one rustic's or Ladderback's types don't read: a host
    /// name, a hook's command, a number of snapshots to keep.
    BadValue,
    /// rustic accepts the key but refuses it where it is: `name` or
    /// `sources` in `[backup]`, `snapshots` in a snapshot entry.
    NotHere,
    /// A setting the profile needs, such as a snapshot entry's `sources` or
    /// the repository, is not there.
    Missing,
    /// A name in `use-profiles` that is not a plain file name.
    UsedName,
    /// Profiles that use each other, or one that uses itself.
    UsedCycle,
    /// Profiles that use profiles that use profiles, deeper than the reader
    /// follows.
    UsedTooDeep,
    /// More profiles used, all told, than the reader reads.
    UsedTooMany,
    /// More snapshot entries than the reader takes.
    TooManyEntries,
    /// The plans the profile makes would hold more items (sources, globs,
    /// tags, hooks) than the reader takes, the settings of `[backup]` being
    /// copied into each.
    TooManyItems,
    /// More different keys left out than the reader keeps a note of.
    TooManyKeys,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ProfileError {}

/// What an export is made from: one plan, the repository it backs up into,
/// and the agent's settings that go into the profile with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanExport {
    pub plan: Plan,
    pub repository: RepositoryConfig,
    /// The agent's hooks, which become `[global.hooks]`.
    pub agent_hooks: Hooks,
    /// The agent's settings: its hostname is the plan's snapshot host when
    /// the plan names none ([`Plan::snapshot_host`]).
    pub agent: AgentSettings,
    /// The two tags that make a snapshot this plan's, and that the profile's
    /// forget is scoped by.
    pub tags: PlanTags,
    /// Whether the commands the profile runs are written.
    pub commands: ExportCommands,
}

/// What an export does with the commands a profile runs: its
/// `password-command`, the options rustic runs as commands (`rclone-command`,
/// `post-create-command`, `post-delete-command`) and each hook's command
/// line. A person wrote them, and
/// one may carry a token (a health-check URL in a hook), so an export handed
/// to an AI agent leaves them out (#856).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportCommands {
    /// Every command is written, as the person's own export has it.
    #[default]
    Written,
    /// No command is written, and a notice names each one left out.
    LeftOut,
}

/// A profile as text, and what the writer left out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedProfile {
    pub text: String,
    pub notices: Vec<ProfileNotice>,
}

/// Why a plan can't be written as a profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportRefusal {
    /// The repository's password is written in the config, and a profile
    /// would have to repeat it.
    InlineSecret,
    /// The value at `key` is a reference rustic can't resolve, and there is
    /// no variable to set instead.
    Reference { key: String },
    /// The location at `key` (`repository` or `repo-hot`) holds a password
    /// in its URL, which a profile would have to repeat as text.
    UrlPassword { key: String },
    /// A tag holds a comma, which rustic reads as two tags.
    TagComma { tag: String },
}

impl std::fmt::Display for ExportRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ExportRefusal {}
