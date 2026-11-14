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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Why something failed. Closed: a code this version doesn't know, say from
/// a newer agent, reads as [`ErrorCode::Unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The worker couldn't read the job it was given.
    InvalidJobSpec,
    /// The engine reported a failure; the detail carries its own message.
    OperationFailed,
    /// The job's deadline passed before it finished, so the worker stopped it.
    DeadlinePassed,
    /// The engine couldn't be started, for example because its program is
    /// missing, or it refused the job. The detail carries the platform's or
    /// the adapter's own message.
    EngineUnavailable,
    /// The engine stopped without reporting a result. The argument
    /// `exit_code` is its exit status, left out when none is known: a signal
    /// ended it, its status couldn't be read, or the daemon never learned it.
    EngineExited,
    /// The engine reported events out of order. The argument `transition`
    /// names the step the operation refused.
    ProtocolViolated,
    /// The engine's process didn't start the operation in time, so it never
    /// ran: it didn't take its job, or took it and sent nothing.
    EngineStartTimedOut,
    /// The engine stopped reporting, but its process stayed alive and was
    /// killed.
    EngineExitTimedOut,
    /// The daemon stopped while the operation was queued or running (a crash,
    /// a kill, a power cut), so the operation never reached its end and what
    /// it did is unknown. The next start ended it as failed.
    DaemonStopped,
    /// The request's `Host` header names neither a loopback address nor a
    /// configured host name (a DNS-rebinding defense, §10.1).
    HostNotAllowed,
    /// The request's `Origin` header isn't the UI's own origin (§10.1).
    OriginNotAllowed,
    /// A mutating request didn't use `Content-Type: application/json` (§10.1).
    JsonRequired,
    /// The configuration file couldn't be read or written (§6.1). The detail
    /// carries the operating system's own message.
    ConfigUnavailable,
    /// The configuration file is larger than any configuration needs, so it
    /// wasn't read.
    ConfigTooLarge,
    /// The configuration file isn't a valid configuration: a key it shouldn't
    /// have, a value out of range, a plan that names a repository it doesn't
    /// have. The detail carries the key and what is wrong with it.
    ConfigInvalid,
    /// The configuration file doesn't say which version of the format it is
    /// in.
    ConfigSchemaMissing,
    /// The configuration file is in a version of the format newer than this
    /// build reads.
    ConfigSchemaNewer,
    /// A repository already exists where one was to be initialised, so
    /// nothing was written. The argument `location` is where it is.
    RepositoryExists,
    /// No repository is at the location: the engine found an empty folder
    /// on this machine there, so nothing was opened or written.
    RepositoryNotFound,
    /// The server the repository is on answered and refused the credentials
    /// it was sent (HTTP 401 or 403), so nothing was opened or written.
    CredentialsRefused,
    /// A repository reached at several locations had none that could be
    /// used (decision 80): each was silent, refused, held no repository or
    /// another one, or a secret of it was unavailable. The operation names
    /// each location and why, by position.
    NoLocationUsable,
    /// No location's rule held when the job started (decision 80). The
    /// operation names each location and why, by position.
    NoLocationRuleHolds,
    /// The repository's password opened nothing at any location tried
    /// (decision 80).
    PasswordNotAccepted,
    /// A saved repository's Create was refused because its row doesn't offer
    /// it: no job found its location empty since nothing reached it, so
    /// nothing says the location is reachable and holds no repository
    /// (decision 53). Nothing was run.
    CreateNotOffered,
    /// A repository didn't answer a test, a connection or an initialisation
    /// in time, so the engine's worker was stopped and nothing was saved.
    RepositoryTimedOut,
    /// The configuration already holds a repository under the alias. The
    /// argument `alias` names it.
    RepositoryAliasTaken,
    /// The configuration already holds the repository, by its location or
    /// its repository ID, under another alias: a repository has one alias
    /// (§7.3). The argument `alias` names the one it has.
    RepositoryAlreadyConfigured,
    /// The location of a configured repository now holds another repository
    /// than the one whose ID the configuration recorded (§7.3), so the job
    /// read and wrote nothing there.
    RepositoryReplaced,
    /// A repository was to be saved under a name that is not an alias: a
    /// name is up to 64 lower-case letters, digits and hyphens, starting
    /// with a letter or a digit.
    RepositoryAliasInvalid,
    /// A repository's secret has nothing in its source: the variable isn't
    /// set, the file or keyring entry isn't there, the command printed
    /// nothing. The arguments name the setting (`field`) and the source
    /// (`source`), never a value.
    SecretMissing,
    /// A repository's secret has a source that couldn't be reached: a file
    /// that can't be read, a program that can't be started, a keyring that
    /// can't be opened. As `SecretMissing`; the platform's words, redacted,
    /// are the `detail`.
    SecretUnreadable,
    /// A repository's secret is in a keyring entry, and the machine has no
    /// keyring service that can be asked for it: on Linux no Secret Service
    /// runs, there is no session bus to reach one by (a server, a system
    /// service), or the service says its keyring is locked. As
    /// `SecretMissing`; the platform's words, redacted, are the `detail`.
    SecretKeyringUnavailable,
    /// A repository's secret isn't text: its source holds bytes that are not
    /// UTF-8. As `SecretMissing`.
    SecretNotText,
    /// A repository's secret is more than `limit` bytes. As `SecretMissing`,
    /// and `limit`.
    SecretTooLarge,
    /// A command that gives a repository's secret ran and failed. As
    /// `SecretMissing`; its first line of stderr, redacted, is the `detail`.
    SecretCommandFailed,
    /// A command that gives a repository's secret succeeded, but something
    /// it started kept its output open before the secret's line ended, so it
    /// gave no whole secret. As `SecretMissing`; nothing it printed is shown.
    SecretCommandOutputOpen,
    /// A command that gives a repository's secret ran for longer than it
    /// may. As `SecretMissing`, and `seconds`.
    SecretCommandTimedOut,
    /// A repository's setting becomes an argument of a process and holds a
    /// reference, so the secret it names would show in the process list. The
    /// argument `field` names the setting.
    SecretInProcessArguments,
    /// A repository's setting, once its references are in, is longer than a
    /// secret may be. The arguments are `field` and `limit`.
    SecretExpansionTooLong,
    /// A repository's secrets, each counted once, come to more than a job
    /// masks: `limit` bytes (§10.2), passed at the setting `field`: one of
    /// the repository's, or the proxy variable of the daemon's environment
    /// (such as `HTTPS_PROXY`) when the engine counted its credentials with
    /// the job's. The engine leaves `field` out when the job's own secrets
    /// passed it. Neither argument is a secret or tells one.
    SecretsTooLargeToMask,
    /// A repository's setting is not a valid template: a `${` that is not a
    /// reference. The argument `field` names the setting, and `at`, when the
    /// fault has a place, the byte of the setting it starts at.
    TemplateInvalid,
    /// A repository's location is an rclone connection string with more
    /// credentials written in it than a job takes: each is a secret to mask
    /// in every text the job shows (I4). The argument `field` names the
    /// setting (`repository` or `repo-hot`).
    LocationTooManyCredentials,
    /// A repository's setting names a file, a folder or a program on this
    /// machine by a relative path, which would be read from wherever the
    /// daemon was started. The argument `field` names the setting
    /// (`repository`, `repo-hot`, `password`, `options-cold.http-cacert`).
    RepositoryPathRelative,
    /// The configuration holds no plan by that name. The argument `plan`
    /// names it.
    PlanUnknown,
    /// The configuration holds no repository by that name. The argument
    /// `alias` names it.
    RepositoryUnknown,
    /// The request names an operation the log has none of. The argument
    /// `operation` names it.
    OperationUnknown,
    /// The request asked to cancel an operation that is not running, so
    /// there is nothing to stop. The argument `operation` names it.
    OperationNotRunning,
    /// A history request's cursor is not an operation's ID. The argument
    /// `cursor` is what was sent.
    InvalidCursor,
    /// A request's body is not the JSON the route takes.
    InvalidBody,
    /// A request's body is larger than the most its route class takes
    /// (§10.1), so the route never ran. The argument `limit` is that most.
    BodyTooLarge,
    /// Another maintenance operation already runs on the repository (I7):
    /// nothing ran. The argument `operation` names the one that holds it.
    MaintenanceBusy,
    /// This agent doesn't maintain the repository: its entry doesn't ask for
    /// `local` (I1, §7.3), so nothing was asked of it. The argument `alias`
    /// names it.
    MaintenanceNotOwner,
    /// The plan's retention policy is set and would keep nothing, so the
    /// forget was refused before anything was removed. The argument `plan`
    /// names it.
    RetentionKeepsNothing,
    /// The request asks for a maintenance kind this version doesn't run.
    /// The argument `kind` is what was sent.
    MaintenanceKindUnknown,
    /// The group an adoption named matches no plan of this agent: nothing
    /// was written.
    AdoptionNoMatch,
    /// More than one plan matches the group an adoption named. The argument
    /// `plans` names them all.
    AdoptionAmbiguous,
    /// A rustic profile couldn't be read: the file isn't there, or isn't
    /// readable. The argument `file` is the path as it was given, or the path
    /// of the profile it uses that is at fault.
    ProfileUnreadable,
    /// A rustic profile is larger than any profile needs, so it wasn't read.
    /// The argument `file` names it.
    ProfileTooLarge,
    /// A file isn't a profile rustic accepts. The argument `file` names it;
    /// `key` is the setting at fault and `line` where it is written, each
    /// left out when none is known. The detail says what is wrong.
    ProfileInvalid,
    /// A profile says something Ladderback can't honour, so nothing was
    /// imported. The argument `key` is the setting, `file` the profile and
    /// `line` where it is written, left out when none is known.
    ProfileUnsupported,
    /// The configuration, or the profile itself, already has a plan with this
    /// ID, so nothing was imported. The argument `plan` names it.
    PlanIdTaken,
    /// A plan can't be exported as a profile because its repository's
    /// password is written in the configuration: a profile would have to
    /// repeat it as text. Nothing was written.
    InlineSecret,
    /// A plan can't be exported as a profile because the value at `key` of
    /// the repository is a reference rustic can't resolve, and there is no
    /// variable to set instead. Nothing was written. The argument `key` is the
    /// setting.
    ProfileExportReference,
    /// A plan can't be exported as a profile because the repository's
    /// location holds a password in its URL, which a profile would have to
    /// repeat as text. Nothing was written. The argument `key` is the
    /// setting (`repository` or `repo-hot`), never its value.
    ProfileExportUrlPassword,
    /// A plan can't be exported as a profile because one of its tags holds a
    /// comma, which rustic reads as two tags. Nothing was written. The
    /// argument `tag` is the tag.
    ProfileExportTagComma,
    /// A command of the command line has no folder to keep the configuration
    /// in: the platform names no home folder and no configuration folder is
    /// set. Nothing was done.
    ConfigFolderUnknown,
    /// A command of the command line could not start the async runtime it
    /// works on, so it did nothing. The detail is the operating system's own
    /// message.
    RuntimeUnavailable,
    /// The snapshots a forget or an adoption would act on are no longer the
    /// ones its preview named: the repository changed after the review, so
    /// nothing was done (I6).
    MaintenanceSelectionChanged,
    /// The repository did not answer a read in time, so its history, or a
    /// preview of a maintenance run, could not be shown.
    RepositoryReadTimedOut,
    /// Sign-in is on and the request carries no live session (§10.1): it
    /// never reached its route. The session may have ended: unused for 12
    /// hours, 30 days old, signed out, or started under an admin password
    /// that has since been set anew.
    SignInRequired,
    /// The password given at sign-in is not the admin password.
    SignInFailed,
    /// A sign-in came too soon after failed ones from the same address, so
    /// the password was not checked (§10.1). The argument `seconds` is how
    /// long is left to wait.
    SignInWait,
    /// A changed request of a session didn't carry that session's CSRF
    /// token (§10.1), so it never reached its route.
    CsrfTokenInvalid,
    /// The admin password could not be checked, or no session token could
    /// be drawn, so nobody was signed in. The detail is the platform's own
    /// message, never the password or its hash.
    SignInUnavailable,
    #[serde(other)]
    Unknown,
}

/// A problem with one item that doesn't fail the operation. Closed like [`ErrorCode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    SourceUnreadable,
    /// A location of a repository reached at several was skipped where it
    /// shouldn't have been (decision 80). The argument `position` is the
    /// location's, from 1, and `outcome` why: `other-repository`,
    /// `secret-unavailable`, a setting's refusal code (`template-invalid`,
    /// `repository-path-relative`, `secret-expansion-too-long`,
    /// `secret-in-process-arguments`, `location-too-many-credentials`),
    /// `silent`, `refused`, `not-a-repository` or `password-not-accepted`.
    /// Never an address or a network's name.
    LocationSkipped,
    /// A location of a repository sends its credentials where whoever
    /// answers at its address can read them (decision 80, §10.6). The
    /// arguments are `position`, the location's, from 1; `cause`:
    /// `plain-http`, `unchecked-certificate`, `plain-endpoint` or
    /// `plain-ftp`; `rule`: `network` when the location has a rule, which
    /// a network made to look like the person's own passes too, or `none`;
    /// and `locations`: `one` for a repository of one location, whose text
    /// names no position, or `several`. Never the address or a credential.
    LocationPlainHttpCredentials,
    /// Every location of a repository has a rule (decision 80): a scheduled
    /// run is skipped on a network where none holds, and any other run fails
    /// there. The argument `locations` is `one` for a repository of one
    /// location, which the page lists without its locations, or `several`.
    LocationEveryRuled,
    /// A repository's last location has a rule and the plan `plan`, which
    /// has a schedule, doesn't retry a failed run (decision 80): a run that
    /// finds no location usable fails until the next one. `locations` as
    /// for `location-every-ruled`.
    LocationLastRuledNoRetry,
    /// The rule of the location at `position`, from 1, lists Wi-Fi networks
    /// by name, and this machine couldn't read the name of the one it is on
    /// when it last tried (decision 80): that part of the rule doesn't hold
    /// here. `locations` as for `location-every-ruled`.
    LocationRuleNameUnreadable,
    /// An export wrote the repository's first location only: rustic has no
    /// fallback. The argument `count` is how many locations it left out.
    ProfileLocationsNotExported,
    /// An import left out a setting of the profile that Ladderback has no
    /// place for. The argument `key` is its path in the profile.
    ProfileKeyIgnored,
    /// The profile prunes after its forget, and the repository now prunes
    /// from one place.
    ProfilePrune,
    /// The profile's `[global.env]` was left out.
    ProfileEnvIgnored,
    /// Hooks set for every snapshot entry of the profile now run once per
    /// plan.
    ProfileHooksPerPlan,
    /// A profile named by `use-profiles` wasn't found. The argument `name` is
    /// the name as the profile wrote it.
    ProfileUsedMissing,
    /// A secret written in the profile was kept as text in the configuration.
    /// The argument `field` names it.
    InlineSecret,
    /// A repository's configuration holds a secret as text: its password, or
    /// an option whose name says it is a credential. Said when the
    /// configuration loads and on the repository's row of the list. The
    /// argument `field` names where it is, as the configuration names it;
    /// the optional `position`, from 2, is the further location that holds
    /// it, absent for the first. Never the secret itself (I4).
    ConfigInlineSecret,
    /// A repository's location holds a credential as text: a password in the
    /// userinfo of its URL, or a credential of its rclone connection string.
    /// Said by the import, when the configuration loads and on the
    /// repository's row of the list. The argument `field` names the setting
    /// that holds the location, as the profile or the configuration names it
    /// (`repository.repository`, `repo-hot`); `kind` is `rclone` for an rclone
    /// location, whose credential belongs in rclone's own configuration, and
    /// `url` for any other, where a `${…}` reference keeps it out; the
    /// optional `position`, from 2, is the further location the setting is
    /// in, absent for the first. Never the credential, nor the location it
    /// is in (I4).
    LocationSecret,
    /// A glob of a plan can never match (#510): it has a slash in the middle,
    /// so rustic reads it from the root of the file system, and written from
    /// a source it reaches none. Said by the import, when the configuration
    /// loads and on the plan's row of the list. The arguments: `plan`, the
    /// plan's ID; `glob`, the glob as written; and `absolute`, the glob
    /// written from the root under the source it is for as the backup walks
    /// it, its links followed: the plan's only source, the one the glob
    /// names, or of several the one that holds its folders. It is left out
    /// when no one source is, and the message then says where to write it.
    GlobNeverMatches,
    /// A tag of Ladderback's own, found on a snapshot entry, was dropped. The
    /// argument `tag` is the tag.
    ProfileTagDropped,
    /// An export left an option out, because it is a reference. The argument
    /// `option` names it and `variable` is the environment variable that
    /// sets it.
    ProfileOptionLeftOut,
    /// An export wrote a plan that keeps everything, which rustic's forget
    /// refuses to run on until a `keep-*` option is set.
    ProfileRetentionEmpty,
    /// An import added its plans to a repository the configuration already
    /// held at the profile's location, and kept one of the entry's settings,
    /// which the profile set otherwise. The argument `repository` is the
    /// entry's alias and `setting` the setting, as the configuration names
    /// it: `password`, `maintenance`, `group-by`, `options`, `options-hot`,
    /// `options-cold` or `hooks`.
    ProfileSettingKept,
    /// An export wrote a profile that reads the repository's password from
    /// the keyring with a program that isn't on the `PATH` it was exported
    /// under, so rustic can't read the password on this machine. The
    /// argument `program` is the program: `secret-tool`, or `security` on
    /// macOS.
    ProfileKeyringCommandMissing,
    /// A notification of an event wasn't delivered to its channel (§11). It
    /// is not tried again. The arguments: `event`, the event's name
    /// (`backup.failed`); `rule` and `channel`, their names in the
    /// configuration; and `reason`, why: `secret-unavailable`,
    /// `invalid-address`, `unreachable`, `timed-out` or `rejected`. Never the
    /// channel's URL, its token or what it answered (I4).
    NotificationUndelivered,
    HookWarning,
    /// A forget or an adoption ended before its last engine job; the jobs
    /// that finished stay done.
    MaintenancePartial,
    /// A check found something to tidy, such as a pack no index lists; unless
    /// the check also found an error, no snapshot's data depends on it. The
    /// warning's detail is rustic's own words, which name it.
    CheckWarning,
    /// A check found an error: damage in the repository, or a part of it, or
    /// of the agent's cache of it, that could not be read or did not match,
    /// so a snapshot may not restore whole. The warning's detail is rustic's
    /// own words.
    CheckError,
    /// A repository maintained here holds snapshots from other hosts, any of
    /// which may maintain it too, which can't be detected without a hub (I1,
    /// §7.3). Said on its groups and the review of a forget, an adoption or
    /// a prune. The argument `count` is how many other hosts there are and
    /// `hosts` names the first five of them, separated by `, `.
    MaintenanceOtherHosts,
    #[serde(other)]
    Unknown,
}

/// The step a running operation is in (technical design §3.1, §4.4; §16,
/// decision 82), on the wire as its kebab-case name: `reading-index`. Closed
/// like [`WarningCode`]: a page that reads a code it doesn't know has
/// `Unknown`, which it shows as it shows no step. The daemon never sends
/// `Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStep {
    ReadingIndex,
    GettingSnapshots,
    BackingUp,
    ListingPacks,
    CheckingCache,
    CheckingHotRepository,
    CheckingTrees,
    ReadingPackData,
    FindingUsedData,
    MarkingUnusedPacks,
    Repacking,
    RebuildingIndex,
    DeletingUnusedData,
    DeletingSnapshots,
    SavingSnapshots,
    #[serde(other)]
    Unknown,
}

/// What a progress bar counts (technical design §4.4; §16, decision 83), on
/// the wire as its kebab-case name: bytes, which a page shows as sizes, or
/// things such as files or packs, which it shows as plain numbers. Closed
/// like [`OperationStep`]: a page that reads a unit it doesn't know has
/// `Unknown`, which it shows as a plain number. The daemon never sends
/// `Unknown`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressUnit {
    #[default]
    Count,
    Bytes,
    #[serde(other)]
    Unknown,
}

/// A typed message argument. Identifiers, ports, years and hostnames are
/// `Text`, so they are never formatted as numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub enum MsgArg {
    Count(u64),
    Bytes(u64),
    DurationMs(u64),
    /// An instant, as milliseconds since the Unix epoch. The zone, when the
    /// sender has one that matters (a plan's own zone, the agent's zone, or the
    /// browser's own, which the web UI always passes), is how the instant is
    /// read; without one it is read as UTC, on every host, and never as the
    /// viewer's zone.
    Timestamp {
        utc_ms: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<IanaZone>,
    },
    Text(String),
}

/// An IANA time-zone name such as `Europe/Berlin`. Checked for syntax only:
/// whether the zone exists is for whoever resolves it. The syntax rules out
/// anything that could walk a file path (`..`, a leading `/`, a backslash).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IanaZone(String);

/// The text is not shaped like an IANA time-zone name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidZone;

impl std::fmt::Display for InvalidZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not an IANA time-zone name")
    }
}

impl std::error::Error for InvalidZone {}

impl IanaZone {
    /// The longest accepted name; the longest in the database has 32 characters.
    pub const MAX_LEN: usize = 64;

    /// A zone name, if `name` has the shape of one: up to [`MAX_LEN`](Self::MAX_LEN)
    /// characters of `A-Z`, `a-z`, `0-9`, `_`, `+` and `-`, in non-empty
    /// segments joined by `/`.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidZone> {
        let name = name.into();
        let shaped = !name.is_empty()
            && name.len() <= Self::MAX_LEN
            && name.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
            });
        if shaped {
            Ok(Self(name))
        } else {
            Err(InvalidZone)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for IanaZone {
    type Error = InvalidZone;

    fn try_from(name: String) -> Result<Self, InvalidZone> {
        Self::new(name)
    }
}

impl From<IanaZone> for String {
    fn from(zone: IanaZone) -> String {
        zone.0
    }
}

/// What kind of value a message argument carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArgKind {
    Count,
    Bytes,
    DurationMs,
    Timestamp,
    Text,
}

/// One argument a code's message takes: the name its catalog message uses
/// for it, and what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    /// Whether the sender may leave it out, as the store does with the exit
    /// status of an engine when none is known. The renderer then supplies
    /// the text `none`, so the message can choose its wording (§8.4). An
    /// optional argument is always text.
    pub optional: bool,
}

impl ErrorCode {
    /// The arguments this code's message takes. A producer sends exactly
    /// these, except that it may leave out an optional one, and the catalog
    /// tests check that the message reads exactly these. A new code lists its
    /// own here, so the compiler asks for them.
    pub const fn args(self) -> &'static [ArgSpec] {
        match self {
            Self::EngineExited => &[ArgSpec {
                name: "exit_code",
                kind: ArgKind::Text,
                optional: true,
            }],
            Self::ProtocolViolated => &[ArgSpec {
                name: "transition",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ConfigUnavailable | Self::ConfigSchemaMissing => &[ArgSpec {
                name: "file",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::SignInWait => &[ArgSpec {
                name: "seconds",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::BodyTooLarge => &[ArgSpec {
                name: "limit",
                kind: ArgKind::Bytes,
                optional: false,
            }],
            Self::ConfigTooLarge => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ],
            Self::ConfigInvalid => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::ConfigSchemaNewer => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "found",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "supported",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::RepositoryExists => &[ArgSpec {
                name: "location",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RepositoryNotFound
            | Self::CredentialsRefused
            | Self::CreateNotOffered
            | Self::NoLocationUsable
            | Self::NoLocationRuleHolds
            | Self::PasswordNotAccepted => &[],
            Self::RepositoryAliasTaken | Self::RepositoryAlreadyConfigured => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RepositoryAliasInvalid => &[],
            Self::SecretMissing
            | Self::SecretUnreadable
            | Self::SecretKeyringUnavailable
            | Self::SecretNotText
            | Self::SecretCommandFailed
            | Self::SecretCommandOutputOpen => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::SecretTooLarge => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ],
            Self::SecretCommandTimedOut => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "seconds",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::SecretInProcessArguments
            | Self::LocationTooManyCredentials
            | Self::RepositoryPathRelative => &[ArgSpec {
                name: "field",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::SecretsTooLargeToMask => &[
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::SecretExpansionTooLong => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ],
            Self::TemplateInvalid => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "at",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::PlanUnknown => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RepositoryUnknown => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::OperationUnknown | Self::OperationNotRunning => &[ArgSpec {
                name: "operation",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InvalidCursor => &[ArgSpec {
                name: "cursor",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::MaintenanceBusy => &[ArgSpec {
                name: "operation",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::MaintenanceNotOwner => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RetentionKeepsNothing => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::MaintenanceKindUnknown => &[ArgSpec {
                name: "kind",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::AdoptionNoMatch => &[],
            Self::MaintenanceSelectionChanged | Self::RepositoryReadTimedOut => &[],
            Self::AdoptionAmbiguous => &[ArgSpec {
                name: "plans",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileUnreadable => &[ArgSpec {
                name: "file",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileTooLarge => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ],
            Self::ProfileInvalid => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "key",
                    kind: ArgKind::Text,
                    optional: true,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::ProfileUnsupported => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "key",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::PlanIdTaken => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileExportReference => &[ArgSpec {
                name: "key",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileExportUrlPassword => &[ArgSpec {
                name: "key",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileExportTagComma => &[ArgSpec {
                name: "tag",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InvalidBody
            | Self::InvalidJobSpec
            | Self::OperationFailed
            | Self::DeadlinePassed
            | Self::EngineUnavailable
            | Self::EngineStartTimedOut
            | Self::EngineExitTimedOut
            | Self::RepositoryTimedOut
            | Self::RepositoryReplaced
            | Self::DaemonStopped
            | Self::HostNotAllowed
            | Self::OriginNotAllowed
            | Self::JsonRequired
            | Self::SignInRequired
            | Self::SignInFailed
            | Self::CsrfTokenInvalid
            | Self::SignInUnavailable
            | Self::InlineSecret
            | Self::ConfigFolderUnknown
            | Self::RuntimeUnavailable
            | Self::Unknown => &[],
        }
    }
}

impl WarningCode {
    /// The arguments this code's message takes (see [`ErrorCode::args`]).
    pub const fn args(self) -> &'static [ArgSpec] {
        match self {
            Self::HookWarning => &[
                ArgSpec {
                    name: "phase",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "exit_code",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::MaintenanceOtherHosts => &[
                ArgSpec {
                    name: "count",
                    kind: ArgKind::Count,
                    optional: false,
                },
                ArgSpec {
                    name: "hosts",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::LocationSkipped => &[
                ArgSpec {
                    name: "position",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "outcome",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::LocationEveryRuled => &[ArgSpec {
                name: "locations",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::LocationLastRuledNoRetry => &[
                ArgSpec {
                    name: "plan",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "locations",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::LocationRuleNameUnreadable => &[
                ArgSpec {
                    name: "position",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "locations",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::LocationPlainHttpCredentials => &[
                ArgSpec {
                    name: "position",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "cause",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "rule",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "locations",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::ProfileLocationsNotExported => &[ArgSpec {
                name: "count",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::MaintenancePartial => &[
                ArgSpec {
                    name: "what",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "done",
                    kind: ArgKind::Count,
                    optional: false,
                },
                ArgSpec {
                    name: "total",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::ProfileKeyIgnored => &[ArgSpec {
                name: "key",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileUsedMissing => &[ArgSpec {
                name: "name",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InlineSecret => &[ArgSpec {
                name: "field",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ConfigInlineSecret => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "position",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::LocationSecret => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "kind",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "position",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::GlobNeverMatches => &[
                ArgSpec {
                    name: "plan",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "glob",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "absolute",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::ProfileTagDropped => &[ArgSpec {
                name: "tag",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileOptionLeftOut => &[
                ArgSpec {
                    name: "option",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "variable",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::ProfileSettingKept => &[
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "setting",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::ProfileKeyringCommandMissing => &[ArgSpec {
                name: "program",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::NotificationUndelivered => &[
                ArgSpec {
                    name: "event",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "rule",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "channel",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "reason",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::SourceUnreadable
            | Self::CheckWarning
            | Self::CheckError
            | Self::ProfilePrune
            | Self::ProfileEnvIgnored
            | Self::ProfileHooksPerPlan
            | Self::ProfileRetentionEmpty
            | Self::Unknown => &[],
        }
    }
}

/// A typed error, as a refusal carries it: over the HTTP API, and in the
/// CLI's `--json` output (§8.4, §12.1). The code; the arguments its message
/// takes, by name, for the reader to render; and, when the sender has words
/// of its own to show, the `detail`, such as the engine's message as it came.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ErrorDto {
    pub code: ErrorCode,
    /// Left out when the code takes none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, MsgArg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ErrorDto {
    /// An error of `code`, with its arguments named as [`ErrorCode::args`]
    /// names them and the sender's own words, if it has some.
    pub fn new(
        code: ErrorCode,
        args: impl IntoIterator<Item = (&'static str, MsgArg)>,
        detail: Option<String>,
    ) -> Self {
        Self {
            code,
            args: named(args),
            detail,
        }
    }

    /// The error as an `operation-failed` event tells it
    /// ([`UiEvent::OperationFailed`]): its arguments by position, in the
    /// order [`ErrorCode::args`] declares them, up to the first it lacks,
    /// then its detail as one more text, when it has one.
    pub fn positional(&self) -> Vec<MsgArg> {
        self.code
            .args()
            .iter()
            .map_while(|spec| self.args.get(spec.name).cloned())
            .chain(self.detail.clone().map(MsgArg::Text))
            .collect()
    }

    /// The error an `operation-failed` event of `code` tells with `args`, as
    /// [`Self::positional`] writes them: each argument named as the code
    /// declares them, in order, and the first text after those as the
    /// detail.
    pub fn from_positional(code: ErrorCode, args: &[MsgArg]) -> Self {
        let mut rest = args.iter();
        let named = code
            .args()
            .iter()
            .zip(rest.by_ref())
            .map(|(spec, arg)| (spec.name.to_owned(), arg.clone()))
            .collect();
        let detail = rest.find_map(|arg| match arg {
            MsgArg::Text(text) => Some(text.clone()),
            _ => None,
        });
        Self {
            code,
            args: named,
            detail,
        }
    }
}

/// A typed warning: the code and the arguments its message takes, never the
/// sentence (§8.4) and never a secret (I4). `import-profile --json` reports
/// its notices as these, and the row of a configured repository carries its
/// own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WarningDto {
    pub code: WarningCode,
    /// Left out when the code takes none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, MsgArg>,
}

impl WarningDto {
    /// A warning of `code`, with its arguments named as
    /// [`WarningCode::args`] names them.
    pub fn new(code: WarningCode, args: impl IntoIterator<Item = (&'static str, MsgArg)>) -> Self {
        Self {
            code,
            args: named(args),
        }
    }

    /// The warning that a repository maintained here holds snapshots from
    /// `count` other hosts, of which it names `hosts`, in order: the first
    /// five (§7.3).
    pub fn other_hosts(count: usize, hosts: &[String]) -> Self {
        Self::new(
            WarningCode::MaintenanceOtherHosts,
            [
                (
                    "count",
                    MsgArg::Count(u64::try_from(count).unwrap_or(u64::MAX)),
                ),
                ("hosts", MsgArg::Text(hosts.join(", "))),
            ],
        )
    }
}

/// A message's arguments, keyed by the names its code declares them under.
fn named(args: impl IntoIterator<Item = (&'static str, MsgArg)>) -> BTreeMap<String, MsgArg> {
    args.into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

/// What `ladderback import-profile --json` prints when the import saved the
/// profile: the name the repository was saved under, the IDs of the plans
/// added, and what the profile could not carry over or changed on the way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProfileImportDto {
    pub repository: String,
    pub plans: Vec<String>,
    pub notices: Vec<WarningDto>,
}

/// What a command prints in `--json` mode when it refused: the typed error,
/// under the key `error`, which no document of a command that did what it
/// was asked has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FailureDto {
    pub error: ErrorDto,
}

#[cfg(test)]
mod typed_dto_tests {
    use serde_json::{from_str, json, to_string, to_value};

    use super::{
        ErrorCode, ErrorDto, FailureDto, MsgArg, ProfileImportDto, WarningCode, WarningDto,
    };

    fn text(value: &str) -> MsgArg {
        MsgArg::Text(value.to_owned())
    }

    #[test]
    fn an_error_carries_its_code_its_arguments_by_name_and_its_detail() {
        let error = ErrorDto::new(
            ErrorCode::ProfileInvalid,
            [("file", text("nas.toml")), ("line", text("7"))],
            Some("unknown field".to_owned()),
        );
        assert_eq!(
            to_value(&error).unwrap(),
            json!({
                "code": "profile-invalid",
                "args": {
                    "file": {"type": "text", "value": "nas.toml"},
                    "line": {"type": "text", "value": "7"},
                },
                "detail": "unknown field",
            })
        );
    }

    #[test]
    fn an_error_with_no_arguments_and_no_detail_is_its_code_alone() {
        let error = ErrorDto::new(ErrorCode::InlineSecret, [], None);
        assert_eq!(to_string(&error).unwrap(), r#"{"code":"inline-secret"}"#);
    }

    #[test]
    fn a_warning_carries_its_code_and_its_arguments_by_name() {
        let warning = WarningDto::new(
            WarningCode::ProfileOptionLeftOut,
            [
                ("option", text("password")),
                ("variable", text("RUSTIC_PASSWORD")),
            ],
        );
        assert_eq!(
            to_value(&warning).unwrap(),
            json!({
                "code": "profile-option-left-out",
                "args": {
                    "option": {"type": "text", "value": "password"},
                    "variable": {"type": "text", "value": "RUSTIC_PASSWORD"},
                },
            })
        );
    }

    #[test]
    fn a_warning_with_no_arguments_is_its_code_alone() {
        let warning = WarningDto::new(WarningCode::ProfilePrune, []);
        assert_eq!(to_string(&warning).unwrap(), r#"{"code":"profile-prune"}"#);
    }

    #[test]
    fn an_import_is_its_repository_its_plans_and_its_notices() {
        let import = ProfileImportDto {
            repository: "nas".to_owned(),
            plans: vec!["docs".to_owned(), "home".to_owned()],
            notices: vec![WarningDto::new(
                WarningCode::ProfileKeyIgnored,
                [("key", text("copy"))],
            )],
        };
        assert_eq!(
            to_value(&import).unwrap(),
            json!({
                "repository": "nas",
                "plans": ["docs", "home"],
                "notices": [
                    {
                        "code": "profile-key-ignored",
                        "args": {"key": {"type": "text", "value": "copy"}},
                    },
                ],
            })
        );
    }

    #[test]
    fn an_import_that_added_no_plan_and_noted_nothing_still_says_so() {
        let import = ProfileImportDto {
            repository: "nas".to_owned(),
            plans: Vec::new(),
            notices: Vec::new(),
        };
        assert_eq!(
            to_string(&import).unwrap(),
            r#"{"repository":"nas","plans":[],"notices":[]}"#
        );
    }

    #[test]
    fn a_failure_is_the_typed_error_under_the_one_key_error() {
        let failure = FailureDto {
            error: ErrorDto::new(ErrorCode::PlanIdTaken, [("plan", text("home"))], None),
        };
        assert_eq!(
            to_string(&failure).unwrap(),
            r#"{"error":{"code":"plan-id-taken","args":{"plan":{"type":"text","value":"home"}}}}"#
        );
    }

    #[test]
    fn every_document_reads_back_as_it_was_written() {
        let error = ErrorDto::new(
            ErrorCode::ProfileTooLarge,
            [
                ("file", text("nas.toml")),
                ("limit", MsgArg::Bytes(1_048_576)),
            ],
            Some("a detail".to_owned()),
        );
        let warning = WarningDto::new(
            WarningCode::ProfileTagDropped,
            [("tag", text("ladderback-plan:home"))],
        );
        let import = ProfileImportDto {
            repository: "nas".to_owned(),
            plans: vec!["home".to_owned()],
            notices: vec![warning.clone()],
        };
        let failure = FailureDto {
            error: error.clone(),
        };
        assert_eq!(
            from_str::<ErrorDto>(&to_string(&error).unwrap()).unwrap(),
            error
        );
        assert_eq!(
            from_str::<WarningDto>(&to_string(&warning).unwrap()).unwrap(),
            warning
        );
        assert_eq!(
            from_str::<ProfileImportDto>(&to_string(&import).unwrap()).unwrap(),
            import
        );
        assert_eq!(
            from_str::<FailureDto>(&to_string(&failure).unwrap()).unwrap(),
            failure
        );
    }

    #[test]
    fn a_document_that_leaves_out_the_arguments_and_the_detail_reads_as_having_none() {
        assert_eq!(
            from_str::<ErrorDto>(r#"{"code":"inline-secret"}"#).unwrap(),
            ErrorDto::new(ErrorCode::InlineSecret, [], None)
        );
        assert_eq!(
            from_str::<WarningDto>(r#"{"code":"profile-prune"}"#).unwrap(),
            WarningDto::new(WarningCode::ProfilePrune, [])
        );
    }

    /// An event tells a failure's arguments by position: those the code
    /// declares, in its order, then the engine's own words. Read back, each
    /// has its name again and the words are the detail.
    #[test]
    fn a_failure_told_by_position_reads_back_by_name_with_its_detail() {
        let cases = [
            ErrorDto::new(
                ErrorCode::OperationFailed,
                [],
                Some("no key opens the repository".to_owned()),
            ),
            ErrorDto::new(ErrorCode::EngineExited, [("exit_code", text("3"))], None),
            ErrorDto::new(ErrorCode::EngineExited, [], None),
            ErrorDto::new(
                ErrorCode::SecretsTooLargeToMask,
                [
                    ("limit", MsgArg::Bytes(262_144)),
                    ("field", text("HTTPS_PROXY")),
                ],
                None,
            ),
            ErrorDto::new(
                ErrorCode::SecretsTooLargeToMask,
                [("limit", MsgArg::Bytes(262_144))],
                None,
            ),
        ];
        for error in cases {
            let told = error.positional();
            assert_eq!(
                ErrorDto::from_positional(error.code, &told),
                error,
                "{told:?}"
            );
        }
    }

    #[test]
    fn a_failure_is_told_in_the_order_its_code_declares_then_the_engines_words() {
        let error = ErrorDto::new(
            ErrorCode::SecretsTooLargeToMask,
            [
                ("field", text("HTTPS_PROXY")),
                ("limit", MsgArg::Bytes(262_144)),
            ],
            None,
        );
        assert_eq!(
            error.positional(),
            [MsgArg::Bytes(262_144), text("HTTPS_PROXY")],
            "the declared order, not the names'"
        );
        let failed = ErrorDto::new(ErrorCode::OperationFailed, [], Some("said".to_owned()));
        assert_eq!(failed.positional(), [text("said")]);
        // An argument the code doesn't declare is not told, and one it
        // declares that the error lacks ends the arguments told.
        let odd = ErrorDto::new(
            ErrorCode::SecretsTooLargeToMask,
            [("field", text("HTTPS_PROXY")), ("other", text("x"))],
            Some("said".to_owned()),
        );
        assert_eq!(odd.positional(), [text("said")]);
    }

    #[test]
    fn a_failure_read_by_position_takes_the_first_text_after_the_declared_ones_as_its_detail() {
        assert_eq!(
            ErrorDto::from_positional(
                ErrorCode::EngineExited,
                &[text("3"), MsgArg::Count(7), text("said"), text("more")],
            ),
            ErrorDto::new(
                ErrorCode::EngineExited,
                [("exit_code", text("3"))],
                Some("said".to_owned())
            )
        );
        assert_eq!(
            ErrorDto::from_positional(ErrorCode::OperationFailed, &[]),
            ErrorDto::new(ErrorCode::OperationFailed, [], None)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_travel_as_kebab_case_strings() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidJobSpec).unwrap(),
            r#""invalid-job-spec""#
        );
        assert_eq!(
            serde_json::to_string(&WarningCode::SourceUnreadable).unwrap(),
            r#""source-unreadable""#
        );
    }

    #[test]
    fn other_hosts_have_their_own_code_counted_and_named() {
        assert_eq!(
            serde_json::to_string(&WarningCode::MaintenanceOtherHosts).unwrap(),
            r#""maintenance-other-hosts""#
        );
        let spec = |name, kind| ArgSpec {
            name,
            kind,
            optional: false,
        };
        assert_eq!(
            WarningCode::MaintenanceOtherHosts.args(),
            &[spec("count", ArgKind::Count), spec("hosts", ArgKind::Text)]
        );
        let named = ["demo-desktop".to_owned(), "demo-nas".to_owned()];
        let warning = WarningDto::other_hosts(7, &named);
        assert_eq!(warning.code, WarningCode::MaintenanceOtherHosts);
        assert_eq!(
            warning.args,
            BTreeMap::from([
                ("count".to_owned(), MsgArg::Count(7)),
                (
                    "hosts".to_owned(),
                    MsgArg::Text("demo-desktop, demo-nas".to_owned())
                ),
            ])
        );
    }

    #[test]
    fn a_partial_maintenance_run_has_its_own_code_and_declares_its_arguments() {
        assert_eq!(
            serde_json::to_string(&WarningCode::MaintenancePartial).unwrap(),
            r#""maintenance-partial""#
        );
        let spec = |name, kind| ArgSpec {
            name,
            kind,
            optional: false,
        };
        assert_eq!(
            WarningCode::MaintenancePartial.args(),
            &[
                spec("what", ArgKind::Text),
                spec("done", ArgKind::Count),
                spec("total", ArgKind::Count),
            ]
        );
    }

    #[test]
    fn a_passed_deadline_travels_as_its_own_code() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::DeadlinePassed).unwrap(),
            r#""deadline-passed""#
        );
        let read: ErrorCode = serde_json::from_str(r#""deadline-passed""#).unwrap();
        assert_eq!(read, ErrorCode::DeadlinePassed);
    }

    #[test]
    fn a_code_from_a_newer_peer_reads_as_unknown() {
        let error: ErrorCode = serde_json::from_str(r#""quota-exceeded""#).unwrap();
        assert_eq!(error, ErrorCode::Unknown);
        let warning: WarningCode = serde_json::from_str(r#""xattr-unreadable""#).unwrap();
        assert_eq!(warning, WarningCode::Unknown);
    }

    #[test]
    fn message_arguments_keep_their_type() {
        for arg in [
            MsgArg::Count(3),
            MsgArg::Bytes(1 << 30),
            MsgArg::DurationMs(90_000),
            MsgArg::Text("9898".to_owned()),
        ] {
            let json = serde_json::to_string(&arg).unwrap();
            assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), arg);
        }
    }

    #[test]
    fn a_repository_row_carries_its_warnings_as_a_code_and_arguments_and_leaves_none_out() {
        let row = RepositoryDto {
            alias: "demo".to_owned(),
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: PasswordSourceDto::Inline,
            maintained_here: true,
            create_offered: true,
            warnings: vec![WarningDto {
                code: WarningCode::ConfigInlineSecret,
                args: [("field".to_owned(), MsgArg::Text("password".to_owned()))].into(),
            }],
            locations: Vec::new(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert_eq!(
            json,
            r#"{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":true,"create-offered":true,"warnings":[{"code":"config-inline-secret","args":{"field":{"type":"text","value":"password"}}}]}"#
        );
        assert_eq!(serde_json::from_str::<RepositoryDto>(&json).unwrap(), row);
        let bare = r#"{"alias":"demo","repository":"/x","password":{"source":"inline"}}"#;
        assert!(
            serde_json::from_str::<RepositoryDto>(bare)
                .unwrap()
                .warnings
                .is_empty()
        );
    }

    #[test]
    fn a_zone_name_with_the_shape_of_one_is_accepted_up_to_the_length_cap() {
        let longest = "a".repeat(IanaZone::MAX_LEN);
        for name in [
            "UTC",
            "Europe/Berlin",
            "America/Argentina/Buenos_Aires",
            "America/Port-au-Prince",
            "Etc/GMT+5",
            longest.as_str(),
        ] {
            let zone = IanaZone::new(name).unwrap_or_else(|_| panic!("{name} is a valid shape"));
            assert_eq!(zone.as_str(), name);
        }
    }

    #[test]
    fn a_zone_name_that_could_walk_a_path_or_overflow_is_rejected() {
        let too_long = "a".repeat(IanaZone::MAX_LEN + 1);
        for name in [
            "",
            ".",
            "..",
            "../../etc/passwd",
            "/etc/localtime",
            "Europe/../Berlin",
            "Europe//Berlin",
            "Europe/Berlin/",
            "Europe\\Berlin",
            "Europe/Ber lin",
            "Europe/Berlin\n",
            "Europe/Zürich",
            too_long.as_str(),
        ] {
            assert_eq!(IanaZone::new(name), Err(InvalidZone), "{name:?}");
        }
    }

    #[test]
    fn a_timestamp_travels_as_utc_plus_an_optional_zone() {
        let zoned = MsgArg::Timestamp {
            utc_ms: 1_794_144_600_000,
            zone: Some(IanaZone::new("Europe/Berlin").unwrap()),
        };
        let json = serde_json::to_string(&zoned).unwrap();
        assert_eq!(
            json,
            r#"{"type":"timestamp","value":{"utc_ms":1794144600000,"zone":"Europe/Berlin"}}"#
        );
        assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), zoned);

        // Before the epoch, and without a zone: the field is left out, and a
        // peer that writes `null` instead is read the same way.
        let unzoned = MsgArg::Timestamp {
            utc_ms: -1,
            zone: None,
        };
        let json = serde_json::to_string(&unzoned).unwrap();
        assert_eq!(json, r#"{"type":"timestamp","value":{"utc_ms":-1}}"#);
        assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), unzoned);
        let explicit_null = r#"{"type":"timestamp","value":{"utc_ms":-1,"zone":null}}"#;
        assert_eq!(
            serde_json::from_str::<MsgArg>(explicit_null).unwrap(),
            unzoned
        );
    }

    #[test]
    fn a_timestamp_naming_a_hostile_zone_is_rejected_when_read() {
        let hostile = r#"{"type":"timestamp","value":{"utc_ms":0,"zone":"../../etc/passwd"}}"#;
        assert!(serde_json::from_str::<MsgArg>(hostile).is_err());
    }

    #[test]
    fn a_code_declares_the_arguments_its_producers_send() {
        let spec = |name, kind, optional| ArgSpec {
            name,
            kind,
            optional,
        };
        assert_eq!(
            ErrorCode::EngineExited.args(),
            &[spec("exit_code", ArgKind::Text, true)]
        );
        assert_eq!(
            ErrorCode::ProtocolViolated.args(),
            &[spec("transition", ArgKind::Text, false)]
        );
        let named = [
            spec("field", ArgKind::Text, false),
            spec("source", ArgKind::Text, false),
        ];
        for code in [
            ErrorCode::SecretMissing,
            ErrorCode::SecretUnreadable,
            ErrorCode::SecretKeyringUnavailable,
            ErrorCode::SecretNotText,
            ErrorCode::SecretCommandFailed,
            ErrorCode::SecretCommandOutputOpen,
        ] {
            assert_eq!(code.args(), &named, "{code:?}");
        }
        assert_eq!(
            ErrorCode::SecretTooLarge.args(),
            &[named[0], named[1], spec("limit", ArgKind::Bytes, false)]
        );
        assert_eq!(
            ErrorCode::SecretCommandTimedOut.args(),
            &[named[0], named[1], spec("seconds", ArgKind::Count, false)]
        );
        for code in [
            ErrorCode::RepositoryAliasTaken,
            ErrorCode::RepositoryAlreadyConfigured,
        ] {
            assert_eq!(
                code.args(),
                &[spec("alias", ArgKind::Text, false)],
                "{code:?}"
            );
        }
        for code in [
            ErrorCode::SecretInProcessArguments,
            ErrorCode::LocationTooManyCredentials,
            ErrorCode::RepositoryPathRelative,
        ] {
            assert_eq!(code.args(), &[named[0]], "{code:?}");
        }
        assert_eq!(
            ErrorCode::SecretExpansionTooLong.args(),
            &[named[0], spec("limit", ArgKind::Bytes, false)]
        );
        assert_eq!(
            ErrorCode::SecretsTooLargeToMask.args(),
            &[
                spec("limit", ArgKind::Bytes, false),
                spec("field", ArgKind::Text, true)
            ]
        );
        assert_eq!(
            ErrorCode::TemplateInvalid.args(),
            &[named[0], spec("at", ArgKind::Text, true)]
        );
        for code in [
            ErrorCode::InvalidJobSpec,
            ErrorCode::OperationFailed,
            ErrorCode::DeadlinePassed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::RepositoryTimedOut,
            ErrorCode::RepositoryReplaced,
            ErrorCode::CredentialsRefused,
            ErrorCode::DaemonStopped,
            ErrorCode::HostNotAllowed,
            ErrorCode::OriginNotAllowed,
            ErrorCode::JsonRequired,
            ErrorCode::SignInRequired,
            ErrorCode::SignInFailed,
            ErrorCode::CsrfTokenInvalid,
            ErrorCode::SignInUnavailable,
            ErrorCode::ConfigFolderUnknown,
            ErrorCode::RuntimeUnavailable,
            ErrorCode::Unknown,
        ] {
            assert!(code.args().is_empty(), "{code:?}");
        }
        for code in [WarningCode::SourceUnreadable, WarningCode::Unknown] {
            assert!(code.args().is_empty(), "{code:?}");
        }
    }

    #[test]
    fn a_sign_in_wait_names_the_seconds_left() {
        assert_eq!(
            ErrorCode::SignInWait.args(),
            &[ArgSpec {
                name: "seconds",
                kind: ArgKind::Count,
                optional: false,
            }]
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::CsrfTokenInvalid).unwrap(),
            r#""csrf-token-invalid""#
        );
    }

    #[test]
    fn a_session_answer_carries_its_csrf_token_and_limits_only_when_signed_in() {
        let signed_in = SessionDto {
            sign_in: SignInStateDto::SignedIn,
            csrf_token: Some("ab".to_owned()),
            idle_seconds: Some(43_200),
            ends_in_seconds: Some(60),
            exposure: ExposureDto::Loopback,
            build: None,
        };
        assert_eq!(
            serde_json::to_value(&signed_in).unwrap(),
            serde_json::json!({
                "sign-in": "signed-in",
                "csrf-token": "ab",
                "idle-seconds": 43_200,
                "ends-in-seconds": 60
            })
        );
        let off = SessionDto {
            sign_in: SignInStateDto::Off,
            csrf_token: None,
            idle_seconds: None,
            ends_in_seconds: None,
            exposure: ExposureDto::Loopback,
            build: None,
        };
        assert_eq!(
            serde_json::to_value(&off).unwrap(),
            serde_json::json!({"sign-in": "off"})
        );
        // Loopback is left out, as before the field existed; the network is
        // said, and an answer without it reads as loopback.
        let on_the_network = SessionDto {
            exposure: ExposureDto::Network,
            ..off
        };
        assert_eq!(
            serde_json::to_value(&on_the_network).unwrap(),
            serde_json::json!({"sign-in": "off", "exposure": "network"})
        );
        let read: SessionDto =
            serde_json::from_value(serde_json::json!({"sign-in": "off"})).unwrap();
        assert_eq!(read.exposure, ExposureDto::Loopback);
    }

    fn build(commit: Option<&str>, dirty: bool, release: bool) -> BuildDto {
        BuildDto {
            version: "0.3.0".to_owned(),
            commit: commit.map(str::to_owned),
            dirty,
            release,
        }
    }

    const COMMIT: &str = "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn a_clean_build_of_its_release_tag_shows_the_version_alone() {
        assert_eq!(build(Some(COMMIT), false, true).display(), "0.3.0");
    }

    #[test]
    fn a_build_between_releases_shows_its_short_commit() {
        assert_eq!(build(Some(COMMIT), false, false).display(), "0.3.0+3f937cf");
    }

    #[test]
    fn a_build_with_changes_says_so_even_at_its_release_tag() {
        assert_eq!(
            build(Some(COMMIT), true, false).display(),
            "0.3.0+3f937cf.dirty"
        );
        assert_eq!(
            build(Some(COMMIT), true, true).display(),
            "0.3.0+3f937cf.dirty"
        );
    }

    #[test]
    fn a_build_with_no_known_commit_shows_the_version_alone() {
        assert_eq!(build(None, false, false).display(), "0.3.0");
        assert_eq!(build(None, true, false).display(), "0.3.0");
    }

    #[test]
    fn a_commit_shorter_than_a_short_hash_is_shown_whole() {
        assert_eq!(build(Some("3f93"), false, false).display(), "0.3.0+3f93");
    }

    #[test]
    fn a_session_answer_names_the_build_in_fields_of_its_own() {
        let off = SessionDto {
            sign_in: SignInStateDto::Off,
            csrf_token: None,
            idle_seconds: None,
            ends_in_seconds: None,
            exposure: ExposureDto::Loopback,
            build: Some(build(Some(COMMIT), true, false)),
        };
        let written = serde_json::json!({
            "sign-in": "off",
            "version": "0.3.0",
            "commit": COMMIT,
            "dirty": true,
            "release": false
        });
        assert_eq!(serde_json::to_value(&off).unwrap(), written);
        assert_eq!(serde_json::from_value::<SessionDto>(written).unwrap(), off);
        // A build with no known commit says so with `null`.
        let unknown = SessionDto {
            build: Some(build(None, false, false)),
            ..off.clone()
        };
        let written = serde_json::json!({
            "sign-in": "off",
            "version": "0.3.0",
            "commit": null,
            "dirty": false,
            "release": false
        });
        assert_eq!(serde_json::to_value(&unknown).unwrap(), written);
        assert_eq!(
            serde_json::from_value::<SessionDto>(written).unwrap(),
            unknown
        );
        // An answer that doesn't name the build reads as none.
        let read: SessionDto =
            serde_json::from_value(serde_json::json!({"sign-in": "signed-out"})).unwrap();
        assert_eq!(read.build, None);
    }

    #[test]
    fn a_stopped_daemons_code_is_spelled_daemon_stopped() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::DaemonStopped).unwrap(),
            r#""daemon-stopped""#
        );
    }

    #[test]
    fn a_body_too_large_declares_the_limit_it_names() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::BodyTooLarge).unwrap(),
            r#""body-too-large""#
        );
        assert_eq!(
            ErrorCode::BodyTooLarge.args(),
            &[ArgSpec {
                name: "limit",
                kind: ArgKind::Bytes,
                optional: false,
            }]
        );
    }

    #[test]
    fn config_codes_declare_the_arguments_their_producers_send() {
        let text = |name, optional| ArgSpec {
            name,
            kind: ArgKind::Text,
            optional,
        };
        assert_eq!(ErrorCode::ConfigUnavailable.args(), &[text("file", false)]);
        assert_eq!(
            ErrorCode::ConfigTooLarge.args(),
            &[
                text("file", false),
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ]
        );
        assert_eq!(
            ErrorCode::ConfigInvalid.args(),
            &[text("file", false), text("line", true)]
        );
        assert_eq!(
            ErrorCode::ConfigSchemaMissing.args(),
            &[text("file", false)]
        );
        assert_eq!(
            ErrorCode::ConfigSchemaNewer.args(),
            &[
                text("file", false),
                text("found", false),
                text("supported", false),
            ]
        );
    }

    #[test]
    fn profile_codes_declare_the_arguments_their_producers_send() {
        let text = |name, optional| ArgSpec {
            name,
            kind: ArgKind::Text,
            optional,
        };
        assert_eq!(ErrorCode::ProfileUnreadable.args(), &[text("file", false)]);
        assert_eq!(
            ErrorCode::ProfileExportReference.args(),
            &[text("key", false)]
        );
        assert_eq!(
            ErrorCode::ProfileExportUrlPassword.args(),
            &[text("key", false)]
        );
        assert_eq!(
            ErrorCode::ProfileExportTagComma.args(),
            &[text("tag", false)]
        );
        assert!(ErrorCode::InlineSecret.args().is_empty());
        assert_eq!(
            ErrorCode::ProfileInvalid.args(),
            &[text("file", false), text("key", true), text("line", true)]
        );
        assert_eq!(
            ErrorCode::ProfileUnsupported.args(),
            &[text("file", false), text("key", false), text("line", true)]
        );
        assert_eq!(ErrorCode::PlanIdTaken.args(), &[text("plan", false)]);
        assert_eq!(
            WarningCode::ProfileOptionLeftOut.args(),
            &[text("option", false), text("variable", false)]
        );
        assert_eq!(
            WarningCode::ConfigInlineSecret.args(),
            &[text("field", false), text("position", true)]
        );
        assert_eq!(
            WarningCode::LocationSecret.args(),
            &[
                text("field", false),
                text("kind", false),
                text("position", true)
            ]
        );
        // A repository of one location is told about itself, never about
        // "location 1" (#473).
        assert_eq!(
            WarningCode::LocationPlainHttpCredentials.args(),
            &[
                text("position", false),
                text("cause", false),
                text("rule", false),
                text("locations", false)
            ]
        );
        assert_eq!(
            WarningCode::LocationEveryRuled.args(),
            &[text("locations", false)]
        );
        assert_eq!(
            WarningCode::LocationLastRuledNoRetry.args(),
            &[text("plan", false), text("locations", false)]
        );
        assert_eq!(
            WarningCode::LocationRuleNameUnreadable.args(),
            &[text("position", false), text("locations", false)]
        );
        assert_eq!(
            WarningCode::GlobNeverMatches.args(),
            &[
                text("plan", false),
                text("glob", false),
                text("absolute", true)
            ]
        );
        assert_eq!(
            WarningCode::ProfileKeyringCommandMissing.args(),
            &[text("program", false)]
        );
        for code in [
            WarningCode::ProfilePrune,
            WarningCode::ProfileEnvIgnored,
            WarningCode::ProfileHooksPerPlan,
            WarningCode::ProfileRetentionEmpty,
        ] {
            assert!(code.args().is_empty(), "{code:?}");
        }
    }

    #[test]
    fn the_codes_of_a_command_that_cannot_start_are_named_on_the_wire() {
        for (code, wire) in [
            (ErrorCode::ConfigFolderUnknown, r#""config-folder-unknown""#),
            (ErrorCode::RuntimeUnavailable, r#""runtime-unavailable""#),
        ] {
            assert_eq!(serde_json::to_string(&code).unwrap(), wire);
            assert_eq!(serde_json::from_str::<ErrorCode>(wire).unwrap(), code);
        }
    }
}

/// How many paths one listed snapshot may carry. An input-derived size,
/// capped (§13.5): a snapshot lists at most a handful of roots.
pub const MAX_SNAPSHOT_PATHS: usize = 64;

/// How many tags one listed snapshot may carry. An input-derived size,
/// capped (§13.5) like the paths.
pub const MAX_SNAPSHOT_TAGS: usize = 64;

/// One snapshot of a repository's list, as the daemon tells the web UI:
/// what a list view needs — id, when it was taken (RFC 3339) and by a clock
/// at which offset, which host, its label, tags and paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDto {
    /// 64 hex digits, as rustic writes a snapshot's ID.
    pub id: String,
    /// RFC 3339, UTC.
    pub time: String,
    /// The offset of the clock that took the snapshot: with `time`, the
    /// snapshot time (§3.1), whose day is the one a calendar retention rule
    /// counts the snapshot on (§16, decision 56). Absent from a daemon older
    /// than the field, which said only the instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<ClockOffset>,
    pub hostname: String,
    /// Empty when the snapshot carries no label.
    pub label: String,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    /// The snapshot has more paths than `paths` lists, or one of them is cut
    /// short: a list shows at most [`MAX_SNAPSHOT_PATHS`] of them, each of a
    /// bounded length, and says so here instead of leaving them out silently.
    #[serde(default)]
    pub paths_truncated: bool,
    /// The same for `tags`, which a list shows at most [`MAX_SNAPSHOT_TAGS`]
    /// of.
    #[serde(default)]
    pub tags_truncated: bool,
}

/// The offset from UTC of the clock that took a snapshot. On the wire it is
/// written as RFC 3339 writes an offset, `-06:00` or `+00:00`, with seconds
/// (`+00:19:32`) only when it has some, and read back only in that form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ClockOffset(jiff::tz::Offset);

/// The text is not an offset as RFC 3339 writes one, or is beyond the range
/// of an offset (±25:59:59).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOffset;

impl std::fmt::Display for InvalidOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not an offset from UTC")
    }
}

impl std::error::Error for InvalidOffset {}

impl ClockOffset {
    pub const fn new(offset: jiff::tz::Offset) -> Self {
        Self(offset)
    }

    pub const fn get(self) -> jiff::tz::Offset {
        self.0
    }
}

impl std::str::FromStr for ClockOffset {
    type Err = InvalidOffset;

    /// `±HH:MM` or `±HH:MM:SS`, nothing else: no `Z`, no zone name, no
    /// space.
    fn from_str(text: &str) -> Result<Self, InvalidOffset> {
        let bytes = text.as_bytes();
        let sign = match bytes.first() {
            Some(b'+') => 1,
            Some(b'-') => -1,
            _ => return Err(InvalidOffset),
        };
        // Two digits at `at`, at most `max`.
        let pair = |at: usize, max: i32| -> Result<i32, InvalidOffset> {
            match (bytes.get(at), bytes.get(at + 1)) {
                (Some(&high), Some(&low)) if high.is_ascii_digit() && low.is_ascii_digit() => {
                    let value = i32::from(high - b'0') * 10 + i32::from(low - b'0');
                    if value <= max {
                        Ok(value)
                    } else {
                        Err(InvalidOffset)
                    }
                }
                _ => Err(InvalidOffset),
            }
        };
        let seconds = match bytes.len() {
            6 | 9 if bytes.get(3) == Some(&b':') => {
                let mut seconds = pair(1, 25)? * 3600 + pair(4, 59)? * 60;
                if bytes.len() == 9 {
                    if bytes.get(6) != Some(&b':') {
                        return Err(InvalidOffset);
                    }
                    seconds += pair(7, 59)?;
                }
                seconds
            }
            _ => return Err(InvalidOffset),
        };
        jiff::tz::Offset::from_seconds(sign * seconds)
            .map(Self)
            .map_err(|_| InvalidOffset)
    }
}

impl std::fmt::Display for ClockOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seconds = self.0.seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let magnitude = seconds.unsigned_abs();
        let (hours, minutes, rest) = (magnitude / 3600, magnitude / 60 % 60, magnitude % 60);
        write!(f, "{sign}{hours:02}:{minutes:02}")?;
        if rest != 0 {
            write!(f, ":{rest:02}")?;
        }
        Ok(())
    }
}

impl TryFrom<String> for ClockOffset {
    type Error = InvalidOffset;

    fn try_from(text: String) -> Result<Self, InvalidOffset> {
        text.parse()
    }
}

impl From<ClockOffset> for String {
    fn from(offset: ClockOffset) -> String {
        offset.to_string()
    }
}

/// Why a snapshot DTO can't be: an ID that isn't 64 hex digits, a time that
/// isn't RFC 3339, or more paths than [`MAX_SNAPSHOT_PATHS`] or tags than
/// [`MAX_SNAPSHOT_TAGS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidSnapshot {
    Id,
    Time,
    Paths,
    Tags,
}

impl std::fmt::Display for InvalidSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidSnapshot {}

impl SnapshotDto {
    /// Builds the DTO from what the engine reported, refusing an ID that
    /// isn't 64 hex digits, a time that isn't RFC 3339, and more paths than
    /// [`MAX_SNAPSHOT_PATHS`] or tags than [`MAX_SNAPSHOT_TAGS`] (input-derived
    /// sizes, capped). The truncation flags start false: the engine's list
    /// says which snapshots it cut, and [`SnapshotDto::truncated`] carries that
    /// over.
    pub fn new(
        id: String,
        time: String,
        hostname: String,
        label: String,
        tags: Vec<String>,
        paths: Vec<String>,
    ) -> Result<Self, InvalidSnapshot> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InvalidSnapshot::Id);
        }
        if time.parse::<jiff::Timestamp>().is_err() {
            return Err(InvalidSnapshot::Time);
        }
        if paths.len() > MAX_SNAPSHOT_PATHS {
            return Err(InvalidSnapshot::Paths);
        }
        if tags.len() > MAX_SNAPSHOT_TAGS {
            return Err(InvalidSnapshot::Tags);
        }
        Ok(Self {
            id,
            time,
            offset: None,
            hostname,
            label,
            tags,
            paths,
            paths_truncated: false,
            tags_truncated: false,
        })
    }

    /// The snapshot with what the engine reported it cut: some of its paths,
    /// some of its tags.
    #[must_use]
    pub fn truncated(mut self, paths: bool, tags: bool) -> Self {
        self.paths_truncated = paths;
        self.tags_truncated = tags;
        self
    }

    /// The snapshot with the offset of the clock that took it, which the
    /// engine read from the snapshot file.
    #[must_use]
    pub fn taken_at_offset(mut self, offset: jiff::tz::Offset) -> Self {
        self.offset = Some(ClockOffset::new(offset));
        self
    }
}

/// One live update the daemon pushes to a connected page over the events
/// WebSocket (technical design §4.4): what an operation reported, or that an
/// agent came or went. Identifiers are text, counts are numbers, and times
/// are RFC 3339 strings. The daemon forwards an operation's progress and
/// warning events unchanged: coalescing is the worker's, once (§4.1). How an
/// operation started and how it ended are announced by the run that recorded
/// them, once the history has each (§4.4): a page that reads the history on
/// hearing either finds it there. A read that is no operation tells nothing:
/// no start, end, progress or warning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UiEvent {
    /// An operation left the queue and is running, and the history says so.
    /// `started_at` is the history's start time.
    OperationStarted {
        operation: String,
        repository: Option<String>,
        started_at: String,
    },
    /// A running operation moved forward: in `step`, `current` of `total`,
    /// when it says, counted in `unit`. What a page is shown is one progress
    /// bar, the one the engine began last and still has open (§4.4), so its
    /// numbers are that step's. `step` is `null` when the engine names no
    /// step this build knows, and is left out by a daemon older than the
    /// field; `unit` is left out by a daemon older than its field, and then
    /// reads as a count. Progress is transient: it is never stored, and a
    /// subscriber whose buffer fills loses the oldest of these first.
    OperationProgress {
        operation: String,
        #[serde(default)]
        step: Option<OperationStep>,
        #[serde(default)]
        unit: ProgressUnit,
        current: u64,
        total: Option<u64>,
    },
    /// A problem with one item that doesn't fail the operation.
    OperationWarning {
        operation: String,
        code: WarningCode,
        args: Vec<MsgArg>,
    },
    /// The operation ended well. The run that recorded it announces this once,
    /// after its ending hooks and its last write to the operation log, so a
    /// page that reads the history on hearing it finds the end there, unless
    /// the log lost that write. A cancelled operation has no such event.
    OperationSucceeded {
        operation: String,
        finished_at: String,
    },
    /// The operation ended badly: announced like [`Self::OperationSucceeded`],
    /// for a failure the run found out itself as well as the engine's own.
    /// `args` are the failure's arguments by position, then the engine's own
    /// words ([`ErrorDto::positional`]).
    OperationFailed {
        operation: String,
        code: ErrorCode,
        args: Vec<MsgArg>,
        finished_at: String,
    },
    /// A plan's backup failed, and the agent retries it (`backup.retrying`,
    /// §5, §11): `operation` is the run that failed, `attempt` which of the
    /// plan's `attempts` retries this is, and `at` when it runs. Told once,
    /// when the retry is planned.
    BackupRetrying {
        operation: String,
        plan: String,
        attempt: u32,
        attempts: u32,
        at: String,
    },
    /// An agent joined the fleet.
    AgentOnline { agent: String, at: String },
    /// An agent left the fleet.
    AgentOffline { agent: String, at: String },
}

#[cfg(test)]
mod snapshot_tests {
    use super::{
        ClockOffset, InvalidOffset, InvalidSnapshot, MAX_SNAPSHOT_PATHS, MAX_SNAPSHOT_TAGS,
        SnapshotDto,
    };
    use jiff::tz::Offset;

    fn dto() -> SnapshotDto {
        SnapshotDto {
            id: "ab".repeat(32),
            time: "2026-11-09T10:15:00Z".to_owned(),
            offset: None,
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["ladderback-plan:demo-home".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
            tags_truncated: false,
        }
    }

    fn offset(seconds: i32) -> Offset {
        Offset::from_seconds(seconds).unwrap()
    }

    /// The contract of the field (§12.1): a daemon that knows the offset
    /// sends it as RFC 3339 writes one, and a listing from one older than the
    /// field, which never sends it, reads as having none.
    #[test]
    fn the_offset_travels_as_rfc_3339_writes_it_and_is_absent_from_an_older_daemon() {
        for (seconds, written) in [
            (-6 * 3600, "-06:00"),
            (0, "+00:00"),
            (13 * 3600, "+13:00"),
            (5 * 3600 + 45 * 60, "+05:45"),
            (-(9 * 3600 + 30 * 60), "-09:30"),
            (19 * 60 + 32, "+00:19:32"),
            (-(25 * 3600 + 59 * 60 + 59), "-25:59:59"),
        ] {
            let taken = dto().taken_at_offset(offset(seconds));
            assert_eq!(taken.offset, Some(ClockOffset::new(offset(seconds))));
            let json = serde_json::to_string(&taken).unwrap();
            assert!(json.contains(&format!(r#""offset":"{written}""#)), "{json}");
            assert_eq!(serde_json::from_str::<SnapshotDto>(&json).unwrap(), taken);
        }
        let older = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","hostname":"h","label":"","tags":[],"paths":[]}"#;
        assert_eq!(
            serde_json::from_str::<SnapshotDto>(older).unwrap().offset,
            None
        );
        let json = serde_json::to_string(&dto()).unwrap();
        assert!(!json.contains("offset"), "{json}");
    }

    #[test]
    fn an_offset_is_read_only_as_rfc_3339_writes_one() {
        assert_eq!("-00:00".parse(), Ok(ClockOffset::new(Offset::UTC)));
        assert_eq!("+25:59:59".parse(), Ok(ClockOffset::new(offset(93_599))));
        for refused in [
            "",
            "Z",
            "UTC",
            "-0600",
            "-06",
            "+6:00",
            "-06:0",
            "06:00",
            " -06:00",
            "-06:00 ",
            "\u{2212}06:00",
            "+26:00",
            "+06:60",
            "+06:00:60",
            "+06:00:0",
            "+06:00:00:00",
            "+0a:00",
            "+0::00",
            "+06-00",
            "-06:00[America/Chicago]",
        ] {
            assert_eq!(
                refused.parse::<ClockOffset>(),
                Err(InvalidOffset),
                "{refused:?}"
            );
        }
        let hostile = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","offset":"America/Chicago","hostname":"h","label":"","tags":[],"paths":[]}"#;
        assert!(serde_json::from_str::<SnapshotDto>(hostile).is_err());
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let dto = dto();
        let built = SnapshotDto::new(
            dto.id.clone(),
            dto.time.clone(),
            dto.hostname.clone(),
            dto.label.clone(),
            dto.tags.clone(),
            dto.paths.clone(),
        )
        .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
        let json = serde_json::to_string(&built).unwrap();
        assert_eq!(serde_json::from_str::<SnapshotDto>(&json).unwrap(), built);
        assert_eq!(built, dto);
    }

    #[test]
    fn an_id_that_is_not_64_hex_digits_is_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                "nope".to_owned(),
                dto.time,
                dto.hostname,
                dto.label,
                dto.tags,
                dto.paths,
            ),
            Err(InvalidSnapshot::Id)
        );
    }

    #[test]
    fn a_time_that_is_not_rfc_3339_is_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                dto.id,
                "not a time".to_owned(),
                dto.hostname,
                dto.label,
                dto.tags,
                dto.paths,
            ),
            Err(InvalidSnapshot::Time)
        );
    }

    #[test]
    fn a_snapshot_that_lost_paths_or_tags_says_which() {
        let truncated = dto().truncated(true, false);
        assert!(truncated.paths_truncated && !truncated.tags_truncated);
        let truncated = dto().truncated(false, true);
        assert!(!truncated.paths_truncated && truncated.tags_truncated);
        let json = serde_json::to_string(&dto().truncated(true, true)).unwrap();
        assert!(json.contains(r#""paths_truncated":true"#), "{json}");
        assert!(json.contains(r#""tags_truncated":true"#), "{json}");
    }

    #[test]
    fn a_snapshot_from_a_daemon_that_never_said_is_not_truncated() {
        let json = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","hostname":"h","label":"","tags":[],"paths":[]}"#;
        let dto: SnapshotDto = serde_json::from_str(json).unwrap();
        assert!(!dto.paths_truncated && !dto.tags_truncated);
    }

    #[test]
    fn tags_beyond_the_cap_are_refused_and_the_cap_itself_is_taken() {
        let dto = dto();
        let build = |tags: usize| {
            SnapshotDto::new(
                dto.id.clone(),
                dto.time.clone(),
                dto.hostname.clone(),
                dto.label.clone(),
                vec!["tag".to_owned(); tags],
                dto.paths.clone(),
            )
        };
        assert_eq!(build(MAX_SNAPSHOT_TAGS + 1), Err(InvalidSnapshot::Tags));
        assert!(build(MAX_SNAPSHOT_TAGS).is_ok());
    }

    #[test]
    fn paths_beyond_the_cap_are_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                dto.id,
                dto.time,
                dto.hostname,
                dto.label,
                dto.tags,
                vec!["/home/demo".to_owned(); MAX_SNAPSHOT_PATHS + 1],
            ),
            Err(InvalidSnapshot::Paths)
        );
    }
}

#[cfg(test)]
mod ui_event_tests {
    use serde_json::{Value, from_value, to_value};

    use super::*;

    fn started() -> UiEvent {
        UiEvent::OperationStarted {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            repository: Some("home.demo".into()),
            started_at: "2026-11-09T10:00:00Z".into(),
        }
    }

    fn progress() -> UiEvent {
        UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step: None,
            unit: ProgressUnit::Count,
            current: 3,
            total: Some(10),
        }
    }

    fn succeeded() -> UiEvent {
        UiEvent::OperationSucceeded {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            finished_at: "2026-11-09T10:05:00Z".into(),
        }
    }

    fn retrying() -> UiEvent {
        UiEvent::BackupRetrying {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            plan: "home".into(),
            attempt: 2,
            attempts: 3,
            at: "2026-11-09T10:20:00Z".into(),
        }
    }

    #[test]
    fn each_event_serializes_with_its_kebab_case_type() {
        for (event, kind) in [
            (started(), "operation-started"),
            (progress(), "operation-progress"),
            (
                UiEvent::OperationWarning {
                    operation: "018f0000-0000-7000-8000-000000000001".into(),
                    code: WarningCode::SourceUnreadable,
                    args: Vec::new(),
                },
                "operation-warning",
            ),
            (succeeded(), "operation-succeeded"),
            (
                UiEvent::OperationFailed {
                    operation: "018f0000-0000-7000-8000-000000000001".into(),
                    code: ErrorCode::EngineUnavailable,
                    args: Vec::new(),
                    finished_at: "2026-11-09T10:05:00Z".into(),
                },
                "operation-failed",
            ),
            (
                UiEvent::AgentOnline {
                    agent: "demo".into(),
                    at: "2026-11-09T10:00:00Z".into(),
                },
                "agent-online",
            ),
            (
                UiEvent::AgentOffline {
                    agent: "demo".into(),
                    at: "2026-11-09T10:00:00Z".into(),
                },
                "agent-offline",
            ),
            (retrying(), "backup-retrying"),
        ] {
            let wire = to_value(&event).unwrap();
            assert_eq!(
                wire.get("type").and_then(Value::as_str),
                Some(kind),
                "{event:?}"
            );
        }
    }

    #[test]
    fn the_fields_of_each_event_carry_their_own_names() {
        let wire = to_value(started()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "operation-started",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "repository": "home.demo",
                "started_at": "2026-11-09T10:00:00Z",
            })
        );
        let wire = to_value(progress()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "operation-progress",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "step": null,
                "unit": "count",
                "current": 3,
                "total": 10,
            })
        );
        let wire = to_value(&UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step: None,
            unit: ProgressUnit::Count,
            current: 1,
            total: None,
        })
        .unwrap();
        assert_eq!(wire.get("total"), Some(&Value::Null));
        let wire = to_value(retrying()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "backup-retrying",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "plan": "home",
                "attempt": 2,
                "attempts": 3,
                "at": "2026-11-09T10:20:00Z",
            })
        );
    }

    #[test]
    fn every_event_round_trips_through_its_wire_form() {
        let events = [
            started(),
            progress(),
            UiEvent::OperationProgress {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                step: None,
                unit: ProgressUnit::Count,
                current: 1,
                total: None,
            },
            UiEvent::OperationWarning {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: WarningCode::SourceUnreadable,
                args: Vec::new(),
            },
            succeeded(),
            UiEvent::OperationFailed {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                finished_at: "2026-11-09T10:05:00Z".into(),
            },
            UiEvent::AgentOnline {
                agent: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
            UiEvent::AgentOffline {
                agent: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
            retrying(),
        ];
        for event in events {
            assert_eq!(
                from_value::<UiEvent>(to_value(&event).unwrap()).unwrap(),
                event
            );
        }
    }

    /// Each step's code on the wire, pinned: a page reads these, and a page
    /// of another version reads them too.
    #[test]
    fn a_progress_event_names_its_step_by_a_code_of_its_own() {
        use OperationStep as S;
        for (step, code) in [
            (S::ReadingIndex, "reading-index"),
            (S::GettingSnapshots, "getting-snapshots"),
            (S::BackingUp, "backing-up"),
            (S::ListingPacks, "listing-packs"),
            (S::CheckingCache, "checking-cache"),
            (S::CheckingHotRepository, "checking-hot-repository"),
            (S::CheckingTrees, "checking-trees"),
            (S::ReadingPackData, "reading-pack-data"),
            (S::FindingUsedData, "finding-used-data"),
            (S::MarkingUnusedPacks, "marking-unused-packs"),
            (S::Repacking, "repacking"),
            (S::RebuildingIndex, "rebuilding-index"),
            (S::DeletingUnusedData, "deleting-unused-data"),
            (S::DeletingSnapshots, "deleting-snapshots"),
            (S::SavingSnapshots, "saving-snapshots"),
        ] {
            let event = UiEvent::OperationProgress {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                step: Some(step),
                unit: ProgressUnit::Count,
                current: 3,
                total: Some(10),
            };
            let wire = to_value(&event).unwrap();
            assert_eq!(
                wire,
                serde_json::json!({
                    "type": "operation-progress",
                    "operation": "018f0000-0000-7000-8000-000000000001",
                    "step": code,
                    "unit": "count",
                    "current": 3,
                    "total": 10,
                })
            );
            assert_eq!(from_value::<UiEvent>(wire).unwrap(), event);
        }
    }

    /// A page reads the progress of a daemon older than the step as no step,
    /// and a step newer than itself as one it doesn't know: the progress
    /// still counts.
    #[test]
    fn a_progress_event_with_no_step_or_one_the_page_doesnt_know_still_reads() {
        let progress_with = |step: Option<Value>| {
            let mut wire = serde_json::json!({
                "type": "operation-progress",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "current": 3,
                "total": 10,
            });
            if let (Some(step), Some(fields)) = (step, wire.as_object_mut()) {
                fields.insert("step".to_owned(), step);
            }
            from_value::<UiEvent>(wire).unwrap()
        };
        let read = |step| UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step,
            unit: ProgressUnit::Count,
            current: 3,
            total: Some(10),
        };
        assert_eq!(progress_with(None), read(None));
        assert_eq!(progress_with(Some(Value::Null)), read(None));
        assert_eq!(
            progress_with(Some(Value::from("defragmenting"))),
            read(Some(OperationStep::Unknown))
        );
        assert_eq!(
            progress_with(Some(Value::from("backing-up"))),
            read(Some(OperationStep::BackingUp))
        );
    }

    /// A bar of bytes says so, so a page can show its amounts as sizes. A
    /// page reads the progress of a daemon older than the unit as a count,
    /// and a unit newer than itself as one it doesn't know: the progress
    /// still counts.
    #[test]
    fn a_progress_event_says_what_it_counts_and_reads_without_a_unit_it_knows() {
        let wire = |unit: Option<&str>| {
            let mut wire = serde_json::json!({
                "type": "operation-progress",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "step": "backing-up",
                "current": 3,
                "total": 10,
            });
            if let (Some(unit), Some(fields)) = (unit, wire.as_object_mut()) {
                fields.insert("unit".to_owned(), Value::from(unit));
            }
            wire
        };
        let read = |unit| UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            step: Some(OperationStep::BackingUp),
            unit,
            current: 3,
            total: Some(10),
        };
        assert_eq!(
            to_value(read(ProgressUnit::Bytes)).unwrap(),
            wire(Some("bytes"))
        );
        assert_eq!(
            to_value(read(ProgressUnit::Count)).unwrap(),
            wire(Some("count"))
        );
        assert_eq!(
            from_value::<UiEvent>(wire(None)).unwrap(),
            read(ProgressUnit::Count)
        );
        assert_eq!(
            from_value::<UiEvent>(wire(Some("furlongs"))).unwrap(),
            read(ProgressUnit::Unknown)
        );
    }

    #[test]
    fn an_event_of_an_unknown_type_is_not_one() {
        let wire = serde_json::json!({"type": "operation-paused"});
        assert!(from_value::<UiEvent>(wire).is_err());
    }
}

/// One operation of the history page, as the daemon tells the web UI: what
/// a list view needs — ids, the kind and status, the lifecycle times, and
/// the typed outcome. Times are RFC 3339, UTC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationSummaryDto {
    /// A UUID, hyphenated.
    pub id: String,
    /// The plan the operation belongs to.
    pub plan: String,
    /// `backup`, `prune`, `check`, …
    pub kind: String,
    /// `queued`, `running`, `succeeded`, `succeeded-with-warnings`,
    /// `failed`, `cancelled`.
    pub status: String,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Every warning the operation recorded, by code, newest last.
    pub warning_codes: Vec<WarningCode>,
    /// Set when the operation failed.
    pub failure_code: Option<ErrorCode>,
    /// Why the operation failed, set when it did: the failure's code, the
    /// arguments its message takes, and the engine's own words, as the
    /// engine masked them of every secret of the job (I4). Added beside
    /// `failure-code`, which keeps its place in the published shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ErrorDto>,
    /// What the engine reported when the operation succeeded.
    pub result: Option<OperationResultDto>,
    /// The alias of the repository a maintenance operation ran on, as it
    /// stood when the run started. Absent for a backup, which `plan` names,
    /// and for an operation an earlier version logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// For a repository reached at several locations: every location as the
    /// run's choice saw it, in order, so a page says "Used location 2 of 2"
    /// and why each earlier one was skipped (decision 80). Left out for a
    /// repository of one location and an operation that opens none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocationTriedDto>,
}

impl OperationSummaryDto {
    /// The same summary, with the locations the run's choice saw.
    #[must_use]
    pub fn with_locations(self, locations: Vec<LocationTriedDto>) -> Self {
        Self { locations, ..self }
    }

    /// The location the run used, and how many the repository had then,
    /// when it had several.
    pub fn used_location(&self) -> Option<(&LocationTriedDto, usize)> {
        self.locations
            .iter()
            .find(|tried| tried.outcome == "used")
            .map(|tried| (tried, self.locations.len()))
    }
}

/// Why an operation DTO can't be: an ID that isn't a UUID, or a time that
/// isn't RFC 3339.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidOperation {
    Id,
    Time,
}

impl std::fmt::Display for InvalidOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidOperation {}

impl OperationSummaryDto {
    /// Builds the summary, refusing an ID that isn't a hyphenated UUID and
    /// a time that isn't RFC 3339, like [`SnapshotDto::new`]. The arity is
    /// the wire's: one argument per field of the published shape, except
    /// `failure-code`, which is the code of `failure`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        plan: String,
        kind: String,
        status: String,
        queued_at: String,
        started_at: Option<String>,
        finished_at: Option<String>,
        warning_codes: Vec<WarningCode>,
        failure: Option<ErrorDto>,
        result: Option<OperationResultDto>,
        repository: Option<String>,
    ) -> Result<Self, InvalidOperation> {
        if !is_uuid(&id) {
            return Err(InvalidOperation::Id);
        }
        for time in std::iter::once(queued_at.as_str())
            .chain(started_at.as_deref())
            .chain(finished_at.as_deref())
        {
            if time.parse::<jiff::Timestamp>().is_err() {
                return Err(InvalidOperation::Time);
            }
        }
        Ok(Self {
            id,
            plan,
            kind,
            status,
            queued_at,
            started_at,
            finished_at,
            warning_codes,
            failure_code: failure.as_ref().map(|failure| failure.code),
            failure,
            result,
            repository,
            locations: Vec::new(),
        })
    }
}

/// A hyphenated UUID: hex digits, with hyphens where a UUID has them.
fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// One page of the operation history: newest first, and `next` set when
/// older operations remain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationsPageDto {
    pub operations: Vec<OperationSummaryDto>,
    pub next: Option<String>,
}

/// One plan of the plans list, as the daemon tells the web UI: the ID a
/// start request names, the repository its backups go to, and when it runs
/// on its own (§5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlanSummaryDto {
    pub id: String,
    pub repository: String,
    /// Whether the plan has a schedule. A plan without one runs only when
    /// it is started.
    pub scheduled: bool,
    /// The scheduler's planned run of a scheduled plan, RFC 3339 in UTC;
    /// `None` while the scheduler has planned none (it hasn't yet, or the
    /// schedule has no run left).
    pub next_run: Option<String>,
    /// What the configuration holds that the user should know about this
    /// plan: one `glob-never-matches` per glob that can never match. Left
    /// out when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningDto>,
}

/// The plans list: what the operations page offers to start.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlansDto {
    pub plans: Vec<PlanSummaryDto>,
}

/// The repositories list: what the Repositories page shows and the snapshots
/// page offers to pick from, in the order of their aliases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoriesDto {
    pub repositories: Vec<RepositoryDto>,
}

/// One configured repository, as a list shows it: where it is, with any
/// password in the location masked, and where its password comes from.
/// Never the password itself (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[serde(rename_all = "kebab-case")]
pub struct LocationsTestDto {
    pub locations: Vec<LocationTriedDto>,
}

/// Where a configured repository's password comes from, without its value:
/// all five sources the configuration takes, the command by its program
/// only, since its arguments may hold the secret itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConnectRepositoryDto {
    pub alias: String,
    pub settings: RepositorySettingsDto,
    #[serde(default)]
    pub maintain_here: bool,
}

/// `POST /api/v1/repositories/_init`: create a repository where there is
/// none, and save it under `alias` when one is given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InitRepositoryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub settings: RepositorySettingsDto,
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
/// long the session lasts unused (`idle-seconds`), and how long it has left
/// however it is used (`ends-in-seconds`): the web UI warns before the
/// first ends it (components §4.5). `exposure` is `network` when the daemon
/// listens beyond loopback, and left out on loopback, as before it existed:
/// with sign-in off, the web UI then warns on every page. The build the
/// daemon runs (`version`, `commit`, `dirty`, `release`) is told to a page
/// that may see the pages, with sign-in off or a live session, and left out
/// for a page that has yet to sign in.
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
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDto>,
}

/// The build a program came from (§3.1, decision NN): its version, the
/// commit it was built from (`null` when the build had no git to ask), whether
/// the tree had uncommitted changes, and whether the commit is the one its
/// version's release tag names. Separate fields, so no reader parses the
/// shown form ([`BuildDto::display`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// Where a request stands with sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignInStateDto {
    /// No admin password is set: there is nothing to sign in to.
    Off,
    /// Sign-in is on and the request carries no live session.
    SignedOut,
    /// The request's session is live.
    SignedIn,
}

/// Who can reach the daemon at the address it listens on (§10.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// `POST /api/v1/session`: the password, as typed. Never logged or shown
/// (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInDto {
    pub password: String,
}

impl std::fmt::Debug for SignInDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignInDto { password: [hidden] }")
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

/// One repository's snapshots, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotsDto {
    pub snapshots: Vec<SnapshotDto>,
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
    /// none.
    pub agent: Option<String>,
    /// The `ladderback-plan:` tag's text, `null` when the snapshots carry
    /// none.
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
#[serde(rename_all = "kebab-case")]
pub struct TallyDto {
    pub groups: u64,
    pub snapshots: u64,
}

/// The history a forget leaves alone (I6): groups no plan of this agent
/// owns, which only an adoption or an explicit deletion touches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

/// One recorded warning of an operation's detail: its typed code and the
/// free-form fields the engine reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationWarningDto {
    pub code: WarningCode,
    /// The affected item, usually a path.
    pub item: Option<String>,
    /// What the engine was doing.
    pub during: String,
    /// The engine's own message, verbatim.
    pub detail: String,
}

/// One operation's detail: its summary and the warnings it recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationDetailDto {
    pub operation: OperationSummaryDto,
    pub warnings: Vec<OperationWarningDto>,
}

/// What a successful operation produced, as the history page shows it: a
/// small typed summary per kind, mapped from the app's engine result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum OperationResultDto {
    Backup {
        /// Files the backup went through: new, changed and unmodified.
        files: u64,
        /// Bytes of new data the backup added to the repository, before
        /// compression.
        bytes: u64,
        duration_ms: u64,
        /// The snapshot the backup wrote: 64 hex digits. Absent from a
        /// result an earlier version sent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        /// Bytes of the source the backup went through, changed or not.
        /// Absent when the log didn't keep them, for a backup an earlier
        /// version ran.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_processed: Option<u64>,
    },
    Prune {
        packs_deleted: u64,
        duration_ms: u64,
    },
    Check {
        errors: u64,
        warnings: u64,
        duration_ms: u64,
    },
    Forget {
        removed: u64,
        duration_ms: u64,
    },
    /// What an adoption produced: the snapshots rewritten.
    Adopt {
        rewritten: u64,
        duration_ms: u64,
    },
}

#[cfg(test)]
mod operation_dto_tests {
    use serde_json::{from_str, to_string};

    use super::{
        InvalidOperation, LocationTriedDto, OperationDetailDto, OperationResultDto,
        OperationSummaryDto, OperationWarningDto, OperationsPageDto, WarningCode,
    };

    fn summary() -> OperationSummaryDto {
        OperationSummaryDto::new(
            "018f0000-0000-7000-8000-000000000001".to_owned(),
            "home".to_owned(),
            "backup".to_owned(),
            "succeeded".to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            Some("2026-11-09T10:00:01Z".to_owned()),
            Some("2026-11-09T10:00:02Z".to_owned()),
            vec![WarningCode::SourceUnreadable],
            None,
            Some(OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
                snapshot: Some("cd".repeat(32)),
                bytes_processed: Some(65_536),
            }),
            None,
        )
        .unwrap_or_else(|e| panic!("a valid summary: {e:?}"))
    }

    /// A summary whose run saw `/srv/lan`, then `/srv/public`, with these
    /// outcomes.
    fn of_two(outcomes: [&str; 2]) -> OperationSummaryDto {
        let tried = |position: u8, address: &str, outcome: &str| LocationTriedDto {
            position,
            address: address.to_owned(),
            outcome: outcome.to_owned(),
            source: None,
        };
        summary().with_locations(vec![
            tried(1, "/srv/lan", outcomes[0]),
            tried(2, "/srv/public", outcomes[1]),
        ])
    }

    /// A run on a repository reached at several locations names the one it
    /// used, by position and address, and how many the repository had,
    /// wherever the used one is in the order; a run that used none, and one
    /// on a repository of one location, name none (decision 80).
    #[test]
    fn a_summary_names_the_location_its_run_used_of_how_many() {
        let used = |dto: &OperationSummaryDto| {
            dto.used_location()
                .map(|(used, count)| (used.position, used.address.clone(), count))
        };
        assert_eq!(
            used(&of_two(["silent", "used"])),
            Some((2, "/srv/public".to_owned(), 2))
        );
        assert_eq!(
            used(&of_two(["used", "not-tried"])),
            Some((1, "/srv/lan".to_owned(), 2))
        );
        assert_eq!(used(&of_two(["silent", "refused"])), None);
        assert_eq!(used(&summary()), None);
    }

    /// The summary carries every location its run saw, on the wire; one of
    /// a repository of one location leaves the key out.
    #[test]
    fn a_summary_carries_the_locations_its_run_saw() {
        let dto = of_two(["silent", "used"]);
        let json = to_string(&dto).unwrap();
        assert!(
            json.contains(
                r#""locations":[{"position":1,"address":"/srv/lan","outcome":"silent"},{"position":2,"address":"/srv/public","outcome":"used"}]"#
            ),
            "{json}"
        );
        assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
        assert!(!to_string(&summary()).unwrap().contains("locations"));
    }

    #[test]
    fn a_summary_round_trips_through_json() {
        let dto = summary();
        let json = to_string(&dto).unwrap();
        assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
        assert!(
            json.contains("\"queued-at\""),
            "times are kebab-case on the wire: {json}"
        );

        // A maintenance row names its repository; a row an earlier version
        // sent has no such key and reads without one.
        let mut check = summary();
        check.kind = "check".to_owned();
        check.result = None;
        check.repository = Some("demo".to_owned());
        let json = to_string(&check).unwrap();
        assert!(json.contains(r#""repository":"demo""#), "{json}");
        assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), check);
        check.repository = None;
        let earlier = to_string(&check).unwrap();
        assert!(!earlier.contains("repository"), "{earlier}");
        assert_eq!(from_str::<OperationSummaryDto>(&earlier).unwrap(), check);
    }

    #[test]
    fn a_summary_refuses_an_id_that_is_not_a_uuid() {
        let dto = summary();
        assert_eq!(
            OperationSummaryDto::new(
                "nope".to_owned(),
                dto.plan,
                dto.kind,
                dto.status,
                dto.queued_at,
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure,
                dto.result,
                dto.repository,
            ),
            Err(InvalidOperation::Id)
        );
    }

    #[test]
    fn a_summary_refuses_an_id_of_the_wrong_length() {
        let dto = summary();
        // A digit short: the right shape, not a UUID.
        let short = "018f0000-0000-7000-8000-00000000000".to_owned();
        assert_eq!(
            OperationSummaryDto::new(
                short,
                dto.plan,
                dto.kind,
                dto.status,
                dto.queued_at,
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure,
                dto.result,
                dto.repository,
            ),
            Err(InvalidOperation::Id)
        );
    }

    #[test]
    fn a_summary_refuses_a_time_that_is_not_rfc_3339() {
        let dto = summary();
        assert_eq!(
            OperationSummaryDto::new(
                dto.id,
                dto.plan,
                dto.kind,
                dto.status,
                "not a time".to_owned(),
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure,
                dto.result,
                dto.repository,
            ),
            Err(InvalidOperation::Time)
        );
    }

    #[test]
    fn a_detail_round_trips_through_json() {
        let detail = OperationDetailDto {
            operation: summary(),
            warnings: vec![OperationWarningDto {
                code: WarningCode::SourceUnreadable,
                item: Some("/home/demo/lost".to_owned()),
                during: "backup".to_owned(),
                detail: "unreadable".to_owned(),
            }],
        };
        let json = to_string(&detail).unwrap();
        assert_eq!(from_str::<OperationDetailDto>(&json).unwrap(), detail);
    }

    #[test]
    fn a_page_round_trips_through_json() {
        let page = OperationsPageDto {
            operations: vec![summary()],
            next: Some("018f0000-0000-7000-8000-000000000000".to_owned()),
        };
        let json = to_string(&page).unwrap();
        assert_eq!(from_str::<OperationsPageDto>(&json).unwrap(), page);
    }

    #[test]
    fn a_result_names_its_kind_on_the_wire() {
        let results = [
            OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
                snapshot: Some("cd".repeat(32)),
                bytes_processed: Some(65_536),
            },
            OperationResultDto::Prune {
                packs_deleted: 2,
                duration_ms: 1000,
            },
            OperationResultDto::Check {
                errors: 0,
                warnings: 1,
                duration_ms: 1000,
            },
        ];
        for result in results {
            let json = to_string(&result).unwrap();
            assert_eq!(from_str::<OperationResultDto>(&json).unwrap(), result);
        }
        // A result an earlier version sent, without the snapshot and the
        // bytes processed, reads as one, and is sent as it was.
        let earlier = r#"{"type":"backup","files":3,"bytes":4096,"duration-ms":1000}"#;
        let read = OperationResultDto::Backup {
            files: 3,
            bytes: 4096,
            duration_ms: 1000,
            snapshot: None,
            bytes_processed: None,
        };
        assert_eq!(from_str::<OperationResultDto>(earlier).unwrap(), read);
        assert_eq!(to_string(&read).unwrap(), earlier);
    }
}

#[cfg(test)]
mod catalog_dto_tests {
    use serde_json::{from_str, to_string};

    use super::{
        MsgArg, PasswordSourceDto, PlanSummaryDto, PlansDto, RepositoriesDto, RepositoryDto,
        SnapshotDto, SnapshotsDto, WarningCode, WarningDto,
    };

    #[test]
    fn the_catalog_lists_carry_their_own_field_names() {
        let plans = PlansDto {
            plans: vec![
                PlanSummaryDto {
                    id: "demo-home".to_owned(),
                    repository: "demo".to_owned(),
                    scheduled: true,
                    next_run: Some("2026-11-13T03:00:00Z".to_owned()),
                    warnings: Vec::new(),
                },
                PlanSummaryDto {
                    id: "demo-docs".to_owned(),
                    repository: "demo".to_owned(),
                    scheduled: false,
                    next_run: None,
                    warnings: vec![WarningDto::new(
                        WarningCode::GlobNeverMatches,
                        [
                            ("plan", MsgArg::Text("demo-docs".to_owned())),
                            ("glob", MsgArg::Text("!a/b".to_owned())),
                            ("absolute", MsgArg::Text("!/home/user/a/b".to_owned())),
                        ],
                    )],
                },
            ],
        };
        assert_eq!(
            to_string(&plans).unwrap(),
            r#"{"plans":[{"id":"demo-home","repository":"demo","scheduled":true,"next-run":"2026-11-13T03:00:00Z"},{"id":"demo-docs","repository":"demo","scheduled":false,"next-run":null,"warnings":[{"code":"glob-never-matches","args":{"absolute":{"type":"text","value":"!/home/user/a/b"},"glob":{"type":"text","value":"!a/b"},"plan":{"type":"text","value":"demo-docs"}}}]}]}"#
        );
        assert_eq!(
            from_str::<PlansDto>(&to_string(&plans).unwrap()).unwrap(),
            plans
        );

        let repositories = RepositoriesDto {
            repositories: vec![RepositoryDto {
                alias: "demo".to_owned(),
                repository: "/srv/demo/repo".to_owned(),
                repo_hot: None,
                password: PasswordSourceDto::Inline,
                maintained_here: false,
                create_offered: false,
                warnings: Vec::new(),
                locations: Vec::new(),
            }],
        };
        assert_eq!(
            to_string(&repositories).unwrap(),
            r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":false}]}"#
        );
        assert_eq!(
            from_str::<RepositoriesDto>(&to_string(&repositories).unwrap()).unwrap(),
            repositories
        );
    }

    #[test]
    fn a_repository_row_names_where_its_password_comes_from_and_never_its_value() {
        for (source, wire) in [
            (PasswordSourceDto::Inline, r#"{"source":"inline"}"#),
            (
                PasswordSourceDto::File {
                    path: "/run/pw".to_owned(),
                },
                r#"{"source":"file","path":"/run/pw"}"#,
            ),
            (
                PasswordSourceDto::Keyring {
                    entry: "ladderback/nas".to_owned(),
                },
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
            ),
            (
                PasswordSourceDto::Env {
                    name: "PW".to_owned(),
                },
                r#"{"source":"env","name":"PW"}"#,
            ),
            (
                PasswordSourceDto::Command {
                    program: "pass".to_owned(),
                },
                r#"{"source":"command","program":"pass"}"#,
            ),
        ] {
            assert_eq!(to_string(&source).unwrap(), wire);
            assert_eq!(from_str::<PasswordSourceDto>(wire).unwrap(), source);
        }
        let row = RepositoryDto {
            alias: "nas".to_owned(),
            repository: "rest:https://u:***@nas.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordSourceDto::Inline,
            maintained_here: false,
            create_offered: false,
            warnings: Vec::new(),
            locations: Vec::new(),
        };
        assert_eq!(
            to_string(&row).unwrap(),
            r#"{"alias":"nas","repository":"rest:https://u:***@nas.example/repo","repo-hot":"/mnt/hot","password":{"source":"inline"},"maintained-here":false}"#
        );
    }

    #[test]
    fn a_repository_snapshot_list_round_trips() {
        let snapshot = SnapshotDto::new(
            "ab".repeat(32),
            "2026-11-09T10:15:00Z".to_owned(),
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["ladderback-plan:demo-home".to_owned()],
            vec!["/home/demo".to_owned()],
        )
        .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
        let list = SnapshotsDto {
            snapshots: vec![snapshot],
        };
        let wire = to_string(&list).unwrap();
        assert_eq!(from_str::<SnapshotsDto>(&wire).unwrap(), list);
    }
}

#[cfg(test)]
mod repository_request_tests {
    use serde_json::{from_str, json, to_string};

    use super::{
        ConnectRepositoryDto, ErrorCode, InitRepositoryDto, PasswordDto, RepositorySettingsDto,
        TestRepositoryDto,
    };

    fn settings() -> RepositorySettingsDto {
        RepositorySettingsDto {
            repository: "rest:https://user:hunter2@backup.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordDto::Inline {
                value: "correct-horse".to_owned(),
            },
            options: [("api-key".to_owned(), "s3cret".to_owned())].into(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        }
    }

    #[test]
    fn settings_carry_the_configurations_own_field_names() {
        let wire = to_string(&settings()).unwrap();
        assert_eq!(
            from_str::<serde_json::Value>(&wire).unwrap(),
            json!({
                "repository": "rest:https://user:hunter2@backup.example/repo",
                "repo-hot": "/mnt/hot",
                "password": {"source": "inline", "value": "correct-horse"},
                "options": {"api-key": "s3cret"},
            })
        );
        assert_eq!(
            from_str::<RepositorySettingsDto>(&wire).unwrap(),
            settings()
        );
    }

    #[test]
    fn what_a_request_leaves_out_is_empty() {
        let read: RepositorySettingsDto =
            from_str(r#"{"repository":"/x","password":{"source":"env","name":"PW"}}"#).unwrap();
        assert_eq!(read.repo_hot, None);
        assert!(read.options.is_empty() && read.options_hot.is_empty());
        assert!(read.options_cold.is_empty());
        assert_eq!(read.password, PasswordDto::Env { name: "PW".into() });
    }

    #[test]
    fn every_password_source_the_api_takes_reads_back() {
        for (wire, expected) in [
            (
                r#"{"source":"inline","value":"p"}"#,
                PasswordDto::Inline { value: "p".into() },
            ),
            (
                r#"{"source":"file","path":"/run/pw"}"#,
                PasswordDto::File {
                    path: "/run/pw".into(),
                },
            ),
            (
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
                PasswordDto::Keyring {
                    entry: "ladderback/nas".into(),
                },
            ),
            (
                r#"{"source":"env","name":"PW"}"#,
                PasswordDto::Env { name: "PW".into() },
            ),
        ] {
            assert_eq!(from_str::<PasswordDto>(wire).unwrap(), expected, "{wire}");
            assert_eq!(
                from_str::<serde_json::Value>(&to_string(&expected).unwrap()).unwrap(),
                from_str::<serde_json::Value>(wire).unwrap()
            );
        }
    }

    #[test]
    fn a_body_the_route_does_not_take_is_not_read() {
        for wire in [
            // The command source runs a program: not over this API.
            r#"{"source":"command","command":"pass show x"}"#,
            r#"{"source":"inline"}"#,
            r#"{"source":"inline","value":"p","extra":1}"#,
            r#"{"value":"p"}"#,
            r#"{"source":"file","path":1}"#,
        ] {
            assert!(from_str::<PasswordDto>(wire).is_err(), "{wire}");
        }
        for wire in [
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"repo_hot":"/y"}"#,
            r#"{"repository":"/x"}"#,
            r#"{"password":{"source":"env","name":"PW"}}"#,
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"options":{"a":1}}"#,
        ] {
            assert!(from_str::<RepositorySettingsDto>(wire).is_err(), "{wire}");
        }
    }

    #[test]
    fn the_requests_name_their_parts() {
        let body = json!({"settings": settings()});
        assert_eq!(
            from_str::<TestRepositoryDto>(&body.to_string())
                .unwrap()
                .settings,
            settings()
        );
        assert!(from_str::<TestRepositoryDto>("{}").is_err());
        assert!(from_str::<TestRepositoryDto>(r#"{"settings":null}"#).is_err());

        let connect = json!({"alias": "demo", "settings": settings()});
        let connect: ConnectRepositoryDto = from_str(&connect.to_string()).unwrap();
        assert_eq!(connect.alias, "demo");
        assert!(!connect.maintain_here, "left out, nobody is asked");
        let maintained = json!({"alias": "demo", "settings": settings(), "maintain-here": true});
        let maintained: ConnectRepositoryDto = from_str(&maintained.to_string()).unwrap();
        assert!(maintained.maintain_here);
        assert!(
            from_str::<ConnectRepositoryDto>(&json!({"settings": settings()}).to_string()).is_err()
        );

        let init: InitRepositoryDto = from_str(&body.to_string()).unwrap();
        assert_eq!(init.alias, None);
        let init: InitRepositoryDto =
            from_str(&json!({"alias": "fresh", "settings": settings()}).to_string()).unwrap();
        assert_eq!(init.alias.as_deref(), Some("fresh"));
        // Unknown fields are refused at every level of a request.
        assert!(
            from_str::<InitRepositoryDto>(&json!({"settings": settings(), "x": 1}).to_string())
                .is_err()
        );
    }

    #[test]
    fn debug_shows_no_text_a_person_could_have_put_a_secret_in() {
        let shown = format!(
            "{:?} {:?}",
            settings(),
            ConnectRepositoryDto {
                alias: "demo".into(),
                settings: settings(),
                maintain_here: false,
            }
        );
        for secret in [
            "hunter2",
            "correct-horse",
            "s3cret",
            "backup.example",
            "/mnt/hot",
        ] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert!(shown.contains("api-key"), "an option's name helps: {shown}");
    }

    #[test]
    fn debug_names_where_a_password_comes_from_but_never_a_typed_one() {
        let inline = format!(
            "{:?}",
            PasswordDto::Inline {
                value: "hunter2".into()
            }
        );
        assert!(!inline.contains("hunter2"), "{inline}");
        assert!(inline.contains("Inline"), "the source helps: {inline}");
        let file = format!(
            "{:?}",
            PasswordDto::File {
                path: "/etc/demo/pw".into()
            }
        );
        assert!(file.contains("/etc/demo/pw"), "a path is no secret: {file}");
    }

    #[test]
    fn debug_of_a_sign_in_names_it_but_never_the_password_typed() {
        let shown = format!(
            "{:?}",
            super::SignInDto {
                password: "demo-admin-pass".into()
            }
        );
        assert!(!shown.contains("demo-admin-pass"), "{shown}");
        assert!(shown.contains("SignInDto"), "the type helps: {shown}");
    }

    #[test]
    fn a_repository_that_did_not_answer_is_a_code_with_no_arguments() {
        assert_eq!(
            to_string(&ErrorCode::RepositoryTimedOut).unwrap(),
            r#""repository-timed-out""#
        );
        assert!(ErrorCode::RepositoryTimedOut.args().is_empty());
    }

    #[test]
    fn a_name_that_is_not_an_alias_is_a_code_with_no_arguments() {
        assert_eq!(
            to_string(&ErrorCode::RepositoryAliasInvalid).unwrap(),
            r#""repository-alias-invalid""#
        );
        assert!(ErrorCode::RepositoryAliasInvalid.args().is_empty());
    }
}

#[cfg(test)]
mod maintenance_dto_tests {
    use serde_json::{from_str, json, to_string, to_value};

    use super::{
        AdoptionFromDto, ErrorCode, ForgetPlanDto, LeftAloneDto, MAX_GROUP_SNAPSHOTS,
        MAX_PREVIEW_SNAPSHOTS, MaintenancePreviewDto, MaintenanceRequestDto, OrphanedGroupDto,
        PlanGroupDto, SnapshotDto, SnapshotGroupsDto, TallyDto, UnmanagedGroupDto, WarningDto,
    };

    fn snapshot() -> SnapshotDto {
        SnapshotDto {
            id: "ab".repeat(32),
            time: "2026-11-09T10:15:00Z".to_owned(),
            offset: None,
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["ladderback-plan:demo-home".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
            tags_truncated: false,
        }
    }

    fn previews() -> [MaintenancePreviewDto; 4] {
        [
            MaintenancePreviewDto::Forget {
                plans: vec![ForgetPlanDto {
                    plan: "demo-home".to_owned(),
                    total: 3,
                    remove: 2,
                    listed: vec![snapshot()],
                }],
                left_alone: LeftAloneDto {
                    orphaned: TallyDto {
                        groups: 1,
                        snapshots: 4,
                    },
                    unmanaged: TallyDto::default(),
                },
                selection: "68497038cf038672".to_owned(),
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Adopt {
                into: "demo-home".to_owned(),
                count: 1,
                listed: vec![snapshot()],
                selection: "68497038cf038672".to_owned(),
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Prune {
                packs_marked: 2,
                packs_deleted: 1,
                packs_recovered: 0,
                bytes_deleted: 4096,
                keep_delete_hours: 144,
                warnings: Vec::new(),
            },
            MaintenancePreviewDto::Check { read_data: false },
        ]
    }

    #[test]
    fn each_preview_names_its_kind_on_the_wire_and_reads_back() {
        for (preview, kind) in previews()
            .into_iter()
            .zip(["forget", "adopt", "prune", "check"])
        {
            let wire = to_value(&preview).unwrap();
            assert_eq!(wire["kind"], kind);
            assert_eq!(
                from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
                preview
            );
        }
    }

    #[test]
    fn a_preview_carries_kebab_case_field_names() {
        let [forget, adopt, prune, check] = previews();
        let forget = to_value(&forget).unwrap();
        assert_eq!(
            forget["left-alone"],
            json!({
                "orphaned": {"groups": 1, "snapshots": 4},
                "unmanaged": {"groups": 0, "snapshots": 0},
            })
        );
        assert_eq!(forget["plans"][0]["remove"], 2);
        assert_eq!(forget["selection"], "68497038cf038672");
        let adopt = to_value(&adopt).unwrap();
        assert_eq!(adopt["into"], "demo-home");
        assert_eq!(adopt["count"], 1);
        assert_eq!(
            to_value(&prune).unwrap(),
            json!({
                "kind": "prune",
                "packs-marked": 2,
                "packs-deleted": 1,
                "packs-recovered": 0,
                "bytes-deleted": 4096,
                "keep-delete-hours": 144,
            })
        );
        assert_eq!(
            to_value(&check).unwrap(),
            json!({"kind": "check", "read-data": false})
        );
    }

    #[test]
    fn a_preview_warns_of_what_it_is_given_and_a_check_of_nothing() {
        let warned = vec![WarningDto::other_hosts(1, &["demo-desktop".to_owned()])];
        let [forget, adopt, prune, check] = previews();
        for preview in [forget, adopt, prune] {
            assert!(preview.warnings().is_empty(), "{preview:?}");
            let preview = preview.warned(warned.clone());
            assert_eq!(preview.warnings(), warned.as_slice());
            let wire = to_value(&preview).unwrap();
            assert_eq!(
                wire["warnings"],
                json!([{
                    "code": "maintenance-other-hosts",
                    "args": {
                        "count": {"type": "count", "value": 1},
                        "hosts": {"type": "text", "value": "demo-desktop"},
                    },
                }])
            );
            assert_eq!(
                from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
                preview
            );
        }
        let check = check.warned(warned);
        assert!(check.warnings().is_empty(), "{check:?}");
        assert_eq!(
            to_value(&check).unwrap(),
            json!({"kind": "check", "read-data": false})
        );
    }

    #[test]
    fn a_request_names_only_what_it_holds() {
        let bare = MaintenanceRequestDto {
            kind: "forget".to_owned(),
            into: None,
            from: None,
            selection: None,
        };
        assert_eq!(to_string(&bare).unwrap(), r#"{"kind":"forget"}"#);
        assert_eq!(
            from_str::<MaintenanceRequestDto>(r#"{"kind":"forget"}"#).unwrap(),
            bare
        );
    }

    #[test]
    fn an_adoption_request_names_its_destination_source_and_selection() {
        for (from, wire) in [
            (
                AdoptionFromDto::Orphaned {
                    agent: "agent-1".to_owned(),
                    plan: "old-home".to_owned(),
                },
                json!({"from": "orphaned", "agent": "agent-1", "plan": "old-home"}),
            ),
            (AdoptionFromDto::Unmanaged, json!({"from": "unmanaged"})),
        ] {
            let request = MaintenanceRequestDto {
                kind: "adopt".to_owned(),
                into: Some("demo-home".to_owned()),
                from: Some(from),
                selection: Some("68497038cf038672".to_owned()),
            };
            assert_eq!(
                to_value(&request).unwrap(),
                json!({
                    "kind": "adopt",
                    "into": "demo-home",
                    "from": wire,
                    "selection": "68497038cf038672",
                })
            );
            assert_eq!(
                from_str::<MaintenanceRequestDto>(&to_string(&request).unwrap()).unwrap(),
                request
            );
        }
    }

    #[test]
    fn a_source_that_is_no_kind_of_group_is_not_read() {
        assert!(from_str::<AdoptionFromDto>(r#"{"from":"somewhere"}"#).is_err());
        assert!(from_str::<AdoptionFromDto>(r#"{"agent":"a","plan":"p"}"#).is_err());
    }

    #[test]
    fn a_source_with_a_key_its_kind_does_not_know_is_not_read() {
        for body in [
            r#"{"kind":"adopt","into":"demo-home","from":{"from":"unmanaged","agent":"a"}}"#,
            r#"{"kind":"adopt","into":"demo-home","from":{"from":"orphaned","agent":"a","plan":"p","extra":1}}"#,
        ] {
            assert!(from_str::<MaintenanceRequestDto>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn the_groups_carry_their_own_field_names_and_read_back() {
        let groups = SnapshotGroupsDto {
            plans: vec![PlanGroupDto {
                plan: "demo-home".to_owned(),
                count: 1,
                snapshots: vec![snapshot()],
            }],
            orphaned: vec![OrphanedGroupDto {
                agent: Some("agent-1".to_owned()),
                plan: None,
                count: 1,
                snapshots: vec![snapshot()],
                adopt_into: vec!["demo-home".to_owned()],
            }],
            unmanaged: vec![UnmanagedGroupDto {
                hostname: "demo-host".to_owned(),
                label: String::new(),
                paths: vec!["/home/demo".to_owned()],
                count: 1,
                snapshots: vec![snapshot()],
                adopt_into: vec![],
            }],
            warnings: Vec::new(),
        };
        let wire = to_value(&groups).unwrap();
        assert_eq!(wire["orphaned"][0]["adopt-into"], json!(["demo-home"]));
        assert!(wire["orphaned"][0]["plan"].is_null());
        assert_eq!(wire["unmanaged"][0]["adopt-into"], json!([]));
        assert_eq!(wire["plans"][0]["count"], 1);
        assert_eq!(
            from_str::<SnapshotGroupsDto>(&wire.to_string()).unwrap(),
            groups
        );
    }

    #[test]
    fn the_codes_of_a_changed_selection_and_a_slow_read_take_no_arguments() {
        for (code, wire) in [
            (
                ErrorCode::MaintenanceSelectionChanged,
                r#""maintenance-selection-changed""#,
            ),
            (
                ErrorCode::RepositoryReadTimedOut,
                r#""repository-read-timed-out""#,
            ),
        ] {
            assert_eq!(to_string(&code).unwrap(), wire);
            assert_eq!(from_str::<ErrorCode>(wire).unwrap(), code);
            assert!(code.args().is_empty());
        }
    }

    /// Snapshot `n` as the wire would show it: its ID is `n`.
    fn numbered(n: &u32) -> Option<SnapshotDto> {
        Some(SnapshotDto {
            id: format!("{n:064x}"),
            ..snapshot()
        })
    }

    fn ids(list: &[SnapshotDto]) -> Vec<String> {
        list.iter().map(|snapshot| snapshot.id.clone()).collect()
    }

    fn first(count: u32) -> Vec<String> {
        (0..count).map(|n| format!("{n:064x}")).collect()
    }

    /// More snapshots than any list shows.
    fn many() -> Vec<u32> {
        (0..=u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap()).collect()
    }

    #[test]
    fn a_group_lists_its_first_snapshots_and_counts_them_all() {
        let items = many();
        let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();

        let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
        assert_eq!((plan.plan.as_str(), plan.count), ("demo-home", 501));
        assert_eq!(ids(&plan.snapshots), first(cap));

        let orphaned = OrphanedGroupDto::new(
            Some("agent-1".to_owned()),
            None,
            &items,
            vec!["demo-home".to_owned()],
            numbered,
        );
        assert_eq!(orphaned.agent.as_deref(), Some("agent-1"));
        assert_eq!(orphaned.plan, None);
        assert_eq!(orphaned.adopt_into, ["demo-home"]);
        assert_eq!(orphaned.count, 501);
        assert_eq!(ids(&orphaned.snapshots), first(cap));

        let unmanaged = UnmanagedGroupDto::new(
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["/home/demo".to_owned()],
            &items,
            Vec::new(),
            numbered,
        );
        assert_eq!(unmanaged.hostname, "demo-host");
        assert_eq!(unmanaged.label, "home");
        assert_eq!(unmanaged.paths, ["/home/demo"]);
        assert!(unmanaged.adopt_into.is_empty());
        assert_eq!(unmanaged.count, 501);
        assert_eq!(ids(&unmanaged.snapshots), first(cap));
    }

    #[test]
    fn a_group_that_fits_lists_every_snapshot() {
        let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();
        for held in [0, 3, cap] {
            let items: Vec<u32> = (0..held).collect();
            let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
            assert_eq!(plan.count, u64::from(held));
            assert_eq!(ids(&plan.snapshots), first(held));
        }
    }

    #[test]
    fn a_preview_lists_fewer_snapshots_than_a_group_does() {
        let items = many();
        let cap = u32::try_from(MAX_PREVIEW_SNAPSHOTS).unwrap();

        let forget = ForgetPlanDto::new("demo-home".to_owned(), 900, &items, numbered);
        assert_eq!(forget.plan, "demo-home");
        assert_eq!((forget.total, forget.remove), (900, 501));
        assert_eq!(ids(&forget.listed), first(cap));

        let adopt = MaintenancePreviewDto::adopt(
            "demo-home".to_owned(),
            &items,
            "68497038cf038672".to_owned(),
            numbered,
        );
        let MaintenancePreviewDto::Adopt {
            into,
            count,
            listed,
            selection,
            ..
        } = adopt
        else {
            panic!("an adoption preview");
        };
        assert_eq!(
            (into.as_str(), count, selection.as_str()),
            ("demo-home", 501, "68497038cf038672")
        );
        assert_eq!(ids(&listed), first(cap));

        let few: Vec<u32> = (0..3).collect();
        let forget = ForgetPlanDto::new("demo-home".to_owned(), 3, &few, numbered);
        assert_eq!((forget.remove, ids(&forget.listed)), (3, first(3)));
    }

    #[test]
    fn only_the_snapshots_a_list_shows_are_built() {
        let items = many();
        let mut built = 0;
        let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
            built += 1;
            numbered(n)
        });
        assert_eq!(built, MAX_GROUP_SNAPSHOTS);
        assert_eq!(group.count, 501);
    }

    #[test]
    fn a_snapshot_the_wire_cannot_carry_is_left_out_and_still_counted() {
        let items: Vec<u32> = (0..3).collect();
        let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
            (*n != 1).then(|| numbered(n)).flatten()
        });
        assert_eq!(group.count, 3);
        assert_eq!(
            ids(&group.snapshots),
            [first(1), vec![format!("{:064x}", 2)]].concat()
        );
    }

    #[test]
    fn a_request_with_a_key_it_does_not_know_is_not_read() {
        assert!(
            from_str::<MaintenanceRequestDto>(
                r#"{"kind":"forget","selectoin":"68497038cf038672"}"#
            )
            .is_err()
        );
    }
}
