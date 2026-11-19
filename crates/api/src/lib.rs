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

pub mod sync;
#[cfg(test)]
mod tests;

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
    /// The daemon was told to stop while the operation was running (a
    /// service stop, a restart, the computer shutting down), and its orderly
    /// stop ended it. No notification is sent of it.
    DaemonShutDown,
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
    /// No repository's config file was found at the location, and nothing
    /// tells whether it is empty or out of reach (a folder that isn't there,
    /// a server that holds no repository), so nothing was opened or written.
    /// The detail is the engine's own words, masked, or, for an OpenDAL `fs`
    /// root folder that isn't there, Ladderback's words naming it.
    RepositoryConfigNotFound,
    /// The server the repository is on answered and refused the credentials
    /// it was sent (HTTP 401 or 403), so nothing was opened or written. The
    /// optional argument `half`, `cold` or `hot`, names the half whose server
    /// refused them, for a hot/cold repository only.
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
    /// service), or the service refused otherwise than as locked, as when its
    /// unlock prompt was dismissed. As `SecretMissing`; the platform's words,
    /// redacted, are the `detail`.
    SecretKeyringUnavailable,
    /// A repository's secret is in a keyring entry, and the keyring service
    /// answered that its keyring is locked. As `SecretMissing`; the
    /// platform's words, redacted, are the `detail`.
    SecretKeyringLocked,
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
    /// A keyring that holds a repository's secret didn't give it within the
    /// time limit: on macOS, the keychain may be waiting for its prompt to
    /// be answered. As `SecretMissing`, and `seconds`.
    SecretKeyringTimedOut,
    /// A file that holds a repository's secret wasn't read within the time
    /// limit, as when the network share it is on stopped answering. As
    /// `SecretMissing`, and `seconds`.
    SecretFileTimedOut,
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
    /// The password typed for a repository was to be kept in the system
    /// keyring, and this computer has no keyring service to keep it in: on
    /// Linux no Secret Service, no session bus to reach one by, or one that
    /// refused otherwise than as locked, as when its unlock prompt was
    /// dismissed. Nothing was saved or created. The argument `entry` names
    /// the keyring entry (`service/name`); the platform's words, masked, are
    /// the `detail`.
    KeyringUnavailable,
    /// The password typed for a repository was to be kept in the system
    /// keyring, and the keyring service answered that its keyring is locked.
    /// As `KeyringUnavailable`.
    KeyringLocked,
    /// The password typed for a repository was to be kept in the system
    /// keyring, and the keyring refused it, failed or didn't answer in time.
    /// As `KeyringUnavailable`.
    KeyringUnwritable,
    /// The password typed for a repository was to be kept in the system
    /// keyring, and its entry already holds another secret, which is never
    /// overwritten. Nothing was saved or created. The argument `entry` names
    /// the entry (`service/name`).
    KeyringEntryTaken,
    /// The configuration holds no plan by that name. The argument `plan`
    /// names it.
    PlanUnknown,
    /// A plan sent to be created or changed is not one the configuration
    /// file would load, so nothing was saved (decision 96). The argument
    /// `field` is the plan's setting at fault, as its key in the plan's
    /// table (`schedule.cron`), and `problem` what is wrong with it, a
    /// [`PlanProblem`]'s wire name. Neither holds a value of the plan.
    PlanInvalid,
    /// A plan was not deleted because a notification rule names it: the
    /// configuration file would no longer load. The argument `plan` names
    /// the plan, and `rule` the rule.
    PlanInUse,
    /// A backup was not started because its sources hold the data folder,
    /// which holds a secret key file, and the folder's path isn't UTF-8, so
    /// no glob can leave the key files out (§16, decision 120, "A copied
    /// data folder"). No arguments: the path can't be shown as written.
    KeyFilesNotLeftOut,
    /// A proposal holds no configuration change (decision 89).
    ProposalEmpty,
    /// A proposal's change is not one the configuration takes, so nothing
    /// was kept (decision 89). The argument `change` is the change's place
    /// in the proposal, from 1; `field` is the setting at fault, as its key
    /// in the plan's table; `problem` what is wrong with it: a plan
    /// problem's wire name, `unknown-plan` or `in-use`. None holds a value
    /// of the change.
    ProposalInvalid,
    /// A proposal is larger than a proposal may be. The argument `what`
    /// says what is over: `changes` or `rationale`; `limit` is how many
    /// changes, or characters of rationale, it may hold.
    ProposalTooLarge,
    /// The proposer has as many proposals pending as it may, `limit`; one
    /// must end before it proposes again.
    ProposalLimit,
    /// No proposal by that ID is kept that the asker may see. The argument
    /// `proposal` names it.
    ProposalUnknown,
    /// The proposal is not pending, so it can't be withdrawn. The argument
    /// `status` is its status's wire name.
    ProposalNotPending,
    /// The configuration file changed since the "Pending changes" page
    /// showed the proposal, or by hand during its apply, which kept the
    /// hand edit: nothing was applied, and the proposal is pending for a
    /// new review.
    ProposalChanged,
    /// The proposal holds a change that could delete snapshots, which is
    /// approved only with the forget preview (decision 89, point 8): it
    /// was not applied, and it stays pending for a person to reject.
    ProposalNeedsForgetPreview,
    /// What the proposal would forget, read again as it was approved, is
    /// not what the forget preview the person saw showed, or a repository
    /// it touches couldn't be read: nothing was applied, and the proposal
    /// stays pending for a new preview (decision 89, point 8).
    ProposalForgetPreviewChanged,
    /// The proposer made as many proposals in the last hour as it may.
    /// The argument `seconds` is how long until it may propose again.
    ProposalRate,
    /// A person rejected one of the proposer's proposals a short while ago.
    /// The argument `seconds` is how long until it may propose again.
    ProposalAfterRejection,
    /// A rejection's reasons don't fit its proposal. The argument `problem`
    /// says how: `count`, another number of reasons than `change` changes;
    /// or `too-long`, the reason for change `change`, from 1, is longer
    /// than a reason may be.
    ProposalReasons,
    /// The proposals couldn't be read or kept, or a proposal's ID couldn't
    /// be drawn; the daemon's log says why (decision 89).
    ProposalsUnavailable,
    /// Another plan dry run is running: dry runs run one at a time (§16,
    /// decision 89, point 13).
    DryRunBusy,
    /// A backup is running or waiting in the queue, so the plan dry run
    /// gives way to it.
    DryRunGivesWay,
    /// The proposer started as many plan dry runs in the last hour as it
    /// may. The argument `seconds` is how long until it may start another.
    DryRunRate,
    /// A path the plan reads isn't within the dry-run allowlist once its
    /// links and `..` are resolved, or can't be resolved: the same refusal
    /// either way, so it never tells whether the path exists. The argument
    /// `setting` names the plan's setting, `sources`, `glob-files`,
    /// `iglob-files` or `exclude-if-present` (whose entries must be file
    /// names alone), and `entry` its entry, from 1.
    DryRunNotAllowed,
    /// For one of the plan's settings, rustic's walker would read files in
    /// the folders above the plan's sources, outside the dry-run
    /// allowlist, so the dry run reads nothing. The argument `setting`
    /// names it: `custom-ignorefiles`, `git-ignore` or `no-require-git`.
    DryRunReadsAbove,
    /// The engine can't apply the plan's source filter, as a backup
    /// couldn't.
    DryRunFilter,
    /// The walk of the plan dry run failed; the daemon's log says why.
    DryRunFailed,
    /// A folder of the dry-run allowlist isn't an absolute path. The
    /// argument `entry` is its place in the list, from 1.
    DryRunFolderInvalid,
    /// The dry-run allowlist holds more folders than it may. The argument
    /// `limit` is how many it may hold.
    DryRunAllowlistTooLong,
    /// The configuration holds no repository by that name. The argument
    /// `alias` names it.
    RepositoryUnknown,
    /// The repository holds no snapshot by that ID: it was forgotten, or
    /// never there. The argument `snapshot` names it.
    SnapshotUnknown,
    /// The snapshot holds no folder at that path. The argument `path` names
    /// it, its names joined by `/`.
    FolderUnknown,
    /// The request names an operation the log has none of. The argument
    /// `operation` names it.
    OperationUnknown,
    /// The request asked to cancel an operation that is not running, so
    /// there is nothing to stop. The argument `operation` names it.
    OperationNotRunning,
    /// A history request's cursor is not an operation's ID. The argument
    /// `cursor` is what was sent.
    InvalidCursor,
    /// A call of an MCP tool gives an argument the tool doesn't take, leaves
    /// out one it needs, or gives one a value it doesn't take (§4.6), so the
    /// tool didn't run. The argument `field` names the argument, and
    /// `problem` what is wrong with it, an [`ArgumentProblem`]'s wire name.
    /// Neither holds the value given.
    InvalidArguments,
    /// A request's body is not the JSON the route takes.
    InvalidBody,
    /// A request's body is larger than the most its route class takes
    /// (§10.1), so the route never ran. The argument `limit` is that most.
    BodyTooLarge,
    /// A maintenance operation is running on this agent, so nothing was done:
    /// one on the repository, for a maintenance run (I7), or possibly one on
    /// another repository, for turning maintenance off (I1). The argument
    /// `operation` names it.
    MaintenanceBusy,
    /// This agent doesn't maintain the repository: its entry doesn't ask for
    /// `local` (I1, §7.3), so nothing in it was changed. The argument `alias`
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
    /// The request's API token (`Authorization: Bearer`) is not one the
    /// daemon keeps: unknown, revoked, of an admin password set since, or with
    /// a wrong secret, all alike (§10.1, decision 93). It counted as a failed
    /// sign-in.
    ApiTokenInvalid,
    /// The request's API token is good but doesn't open this route. The
    /// argument `scope` is what the route needs: `read`, `run`, `preview`,
    /// `propose`, or `session` for a route only a signed-in browser may use.
    ApiTokenScope,
    /// Sign-in is off, so there are no API tokens to create, list or revoke.
    SignInOff,
    /// A restart was asked for, but no service manager runs the daemon to
    /// start it again: its service file didn't pass `--service-managed`, as
    /// with `ladderback serve` started by hand (decision 94).
    RestartUnavailable,
    /// A maintenance run was asked for while a restart waits for the running
    /// operations to finish: it starts no new one meanwhile (decision 94).
    RestartPending,
    /// An API token's label is empty, blank, holds a control character, or is
    /// longer than `max` characters.
    ApiTokenLabel,
    /// An API token was asked for with no scope.
    ApiTokenNoScope,
    /// `limit` API tokens are kept already, so no other was created.
    ApiTokenLimit,
    /// No API token with the ID `token` is kept: it was revoked already.
    ApiTokenUnknown,
    /// The API tokens could not be read or kept: the store or the admin
    /// password file could not be read, or no random bytes drawn. The daemon's
    /// log says why.
    ApiTokensUnavailable,
    /// A new admin password is shorter than `min` characters, the rule
    /// `ladderback set-password` keeps too (§10.1), so it was not set.
    AdminPasswordTooShort,
    /// A new admin password is longer than `max` bytes, so it was not set.
    AdminPasswordTooLong,
    /// The current password a change of the admin password needs is not the
    /// admin password. It counted as a failed sign-in (§10.1).
    CurrentPasswordWrong,
    /// The admin password could not be checked or kept, so it was not set.
    /// The daemon's log says why.
    AdminPasswordUnavailable,
    /// The database could not keep or forget sessions: a sign-in's session,
    /// a sign-out, Sign out everywhere or the session length (§10.1). A
    /// session signed out has ended all the same while the daemon runs. The
    /// daemon's log says why.
    SessionsUnavailable,
    /// `ladderback mcp --connect` got no answer from a Ladderback API at the
    /// address it connects to: nothing answered, the answer didn't come in
    /// time, or it wasn't the API's.
    DaemonUnreachable,
    /// `ladderback mcp --connect` sent a request that changes something, and
    /// its answer was lost on the way back: whether it took effect is
    /// unknown, so it is never sent again (§4.6).
    DaemonAnswerLost,
    /// `ladderback mcp --connect` read an answer larger than the most it
    /// reads, `limit`, and left it unread: a long list, or an operation's
    /// detail when it recorded many long warnings, which then names the
    /// `operation` (§4.6).
    DaemonAnswerTooLarge,
    /// The step-up confirmation a restore asks for (§10.4) was not right:
    /// the admin password with sign-in on, the snapshot's short ID with it
    /// off. A wrong password counted as a failed sign-in. Nothing ran.
    StepUpFailed,
    /// A restore couldn't write where it was to write: the restore folder,
    /// or a path it restores in place, refused it (§10.4). What it wrote
    /// before stays.
    RestoreTargetUnwritable,
    /// A restore runs on the agent already, and an agent runs one at a time
    /// (§10.4): nothing ran. The argument `operation` names the one running.
    RestoreRunning,
    /// A restore in place was asked for from elsewhere than the agent's own
    /// web UI, the one place it is offered (§10.4): nothing ran.
    RestoreInPlaceNotHere,
    /// A restore was asked for through the hub, and the agent doesn't set
    /// `allow-hub-restore` (§10.4): nothing ran.
    RestoreHubNotAllowed,
    /// A restore into a new folder was asked for, and the agent has no
    /// folder to make it in: no home folder, and `[agent] restore-folder`
    /// unset (§10.4). Nothing ran.
    RestoreFolderUnset,
    /// System Settings' Full Disk Access pane was asked to open from a
    /// browser that isn't on the agent's own Mac: the daemon opens it only
    /// for a request from its own machine, by a loopback address and a
    /// loopback `Host` (§16, decision 115). Nothing opened; the page shows
    /// the steps instead.
    FullDiskAccessNotLocal,
    /// System Settings' Full Disk Access pane couldn't be opened: the agent
    /// isn't on macOS, or macOS refused to open it (§16, decision 115).
    FullDiskAccessSettingsUnopened,
    /// Two downloads are under way on the agent, confirmed or being handed
    /// over, and an agent runs two at a time (§10.4): nothing ran. A third
    /// is refused rather than queued.
    DownloadsBusy,
    /// No download could be confirmed because the agent's system gave no
    /// random bytes for its ticket (§10.4): nothing ran. The daemon's log
    /// says why.
    DownloadsUnavailable,
    /// The snapshot holds no file or folder at the argument `path`, the
    /// entry a download asked for (§10.4). Nothing was handed over.
    EntryUnknown,
    /// No download waits under the ticket asked for: it was fetched
    /// already, its minute ran out, or it never was (§10.4).
    DownloadUnknown,
    /// No route of the API serves the path asked for under `/api`: a typo,
    /// or a route of a newer or older Ladderback (§4.2). Nothing ran.
    RouteUnknown,
    /// The pairing token was already spent on another pairing (§16,
    /// decision 121, "Errors and their next step").
    TokenUsed,
    /// The pairing token lapsed unused.
    TokenExpired,
    /// A person revoked the pairing token on the hub.
    TokenRevoked,
    /// The text isn't a whole pairing token: its prefix, its encoding, its
    /// length or its checksum is wrong.
    TokenMalformed,
    /// The pairing token is of a later layout than this agent reads.
    TokenUnsupported,
    /// Another agent of the hub holds the hostname, named by the argument
    /// `hostname`, last heard from as the argument `last_seen` says. The
    /// token stays good.
    HostnameTaken,
    /// The hub has another key under this agent's ID.
    AgentKeyMismatch,
    /// The hub's key isn't the one the agent trusts, or the one its token
    /// names: nothing of the token was sent.
    HubKeyMismatch,
    /// The hub didn't prove it holds its key on this connection: nothing of
    /// the token was sent.
    HubProofInvalid,
    /// The hub refused the agent's proof: one code for every cause.
    ProofRefused,
    /// As many agents as the hub allows wait for approval already. The
    /// token stays good.
    PendingLimit,
    /// The hub closed the connection because it can't offer the binding
    /// the agent's token names, before it proved itself.
    BindingUnavailable,
    /// No hub answered at the argument `address`: the connection was
    /// refused, the name didn't resolve, or nothing answered in time.
    /// Nothing was sent.
    HubUnreachable,
    /// The certificate at the pinned argument `address` isn't the one the
    /// token names. Nothing was sent.
    HubPinMismatch,
    /// The certificate at the CA-checked argument `address` isn't one this
    /// machine's trust store accepts for its name. Nothing was sent.
    HubUntrusted,
    /// TLS with the hub at the argument `address` failed on its version or
    /// its key exchange. Nothing was sent.
    HubTlsRefused,
    /// The token's address is plain `http://` off this computer, which would
    /// show the token to the network. Nothing was sent.
    PlainAddress,
    /// The connection to the hub ended before the pairing was answered, or
    /// the hub sent what isn't the handshake's next message.
    HubConnectionLost,
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
    /// A secret was read from a file other users of the machine can read
    /// (#386). The arguments are `path`, the file's path, and `mode`, its
    /// permission bits in octal (`0644`). Never the secret (I4).
    SecretFileReadable,
    /// A location of a repository sends its credentials where whoever
    /// answers at its address can read them (decision 80, §10.6). The
    /// arguments are `position`, the location's, from 1; `cause`:
    /// `plain-http`, `unchecked-certificate`, `plain-endpoint`,
    /// `plain-rclone` or `plain-ftp`; `rule`: `network` when the location
    /// has a rule, which a network made to look like the person's own
    /// passes too, or `none`; and `locations`: `one` for a repository of
    /// one location, whose text names no position, or `several`. Never the
    /// address or a credential.
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
    /// The machine's IPv4 and IPv6 traffic leave by different interfaces,
    /// of the kinds `ipv4` and `ipv6` (`wired`, `wifi` or `other`), as it
    /// last read them, and a location of the repository has a rule (decision
    /// 80): a rule holds there only if it holds for both. `locations` as for
    /// `location-every-ruled`.
    LocationRuleMixedLink,
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
    /// A profile previewed from its text names a profile in `use-profiles`,
    /// which a preview never reads: it reads no file. The argument `name` is
    /// the name as the profile wrote it.
    ProfileUsedNotRead,
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
    /// An export left out a command the profile would run, as it was asked
    /// to: one for an AI agent or an API token has none (#856). The argument
    /// `key` is its path in the profile: `repository.password-command`, an
    /// option rustic runs as a command, as `repository.options.rclone-command`,
    /// or a hook's list, as `global.hooks.run-before`.
    ProfileCommandLeftOut,
    /// A notification of an event wasn't delivered to its channel (§11). It
    /// is not tried again. The arguments: `event`, the event's name
    /// (`backup.failed`); `rule` and `channel`, their names in the
    /// configuration; and `reason`, why: `secret-unavailable`,
    /// `invalid-address`, `unreachable`, `timed-out` or `rejected`. Never the
    /// channel's URL, its token or what it answered (I4).
    NotificationUndelivered,
    /// The daemon or a worker couldn't set its core-dump limit to 0 as it
    /// started (decision 98), so a crash may write the secrets it holds to
    /// disk. It goes on. Logged, never sent; the detail is the operating
    /// system's own words.
    CoreDumpsOn,
    /// On Linux, the daemon or a worker couldn't make itself non-dumpable as
    /// it started (decision 98), so another process of the same user may
    /// attach a debugger to it or read its memory. It goes on. Logged, never
    /// sent; the detail is the operating system's own words.
    DebuggerAttachAllowed,
    /// On macOS, a source item the system refused during a backup whose
    /// Full Disk Access probe found the grant missing (§16, decision 115):
    /// the grant explains why the item was left out. The detail is the
    /// engine's own message, as for `source-unreadable`.
    FullDiskAccessMissing,
    /// On Linux, the keyring service, the Secret Service, runs now, but no
    /// activation file names it, so nothing can start it on demand: a run
    /// that asks for it before the desktop starts it finds no keyring
    /// (#923). Said on the Repositories page only. No arguments.
    KeyringNotActivatable,
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// A code's schema: text, never the list of every code. Each tool's output
/// schema would repeat the hundreds of codes, which a client reads into its
/// context with every `tools/list`, and a code a newer Ladderback sends is
/// still one.
#[cfg(feature = "schemars")]
fn code_schema(description: &str) -> schemars::Schema {
    schemars::json_schema!({"type": "string", "description": description})
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ErrorCode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ErrorCode".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        code_schema("Why it failed, in kebab-case, such as `repository-unknown`.")
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for WarningCode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "WarningCode".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        code_schema("What to know, in kebab-case, such as `glob-never-matches`.")
    }
}

impl ErrorCode {
    /// The arguments this code's message takes. A producer sends exactly
    /// these, except that it may leave out an optional one, and the catalog
    /// tests check that the message reads exactly these. A new code lists its
    /// own here, so the compiler asks for them.
    pub const fn args(self) -> &'static [ArgSpec] {
        match self {
            Self::HubUnreachable
            | Self::HubPinMismatch
            | Self::HubUntrusted
            | Self::HubTlsRefused => &[ArgSpec {
                name: "address",
                kind: ArgKind::Text,
                optional: false,
            }],
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
            Self::ApiTokenScope => &[ArgSpec {
                name: "scope",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ApiTokenLabel => &[ArgSpec {
                name: "max",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::HostnameTaken => &[
                ArgSpec {
                    name: "hostname",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "last_seen",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::ApiTokenLimit => &[ArgSpec {
                name: "limit",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::ApiTokenUnknown => &[ArgSpec {
                name: "token",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::AdminPasswordTooShort => &[ArgSpec {
                name: "min",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::AdminPasswordTooLong => &[ArgSpec {
                name: "max",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::BodyTooLarge => &[ArgSpec {
                name: "limit",
                kind: ArgKind::Bytes,
                optional: false,
            }],
            Self::DaemonAnswerTooLarge => &[
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
                ArgSpec {
                    name: "operation",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
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
            Self::CredentialsRefused => &[ArgSpec {
                name: "half",
                kind: ArgKind::Text,
                optional: true,
            }],
            Self::RepositoryNotFound
            | Self::RepositoryConfigNotFound
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
            | Self::SecretKeyringLocked
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
            Self::SecretCommandTimedOut
            | Self::SecretKeyringTimedOut
            | Self::SecretFileTimedOut => &[
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
            Self::KeyringUnavailable
            | Self::KeyringLocked
            | Self::KeyringUnwritable
            | Self::KeyringEntryTaken => &[ArgSpec {
                name: "entry",
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
            Self::PlanInvalid => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "problem",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::PlanInUse => &[
                ArgSpec {
                    name: "plan",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "rule",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::KeyFilesNotLeftOut => &[],
            Self::ProposalEmpty => &[],
            Self::ProposalInvalid => &[
                ArgSpec {
                    name: "change",
                    kind: ArgKind::Count,
                    optional: false,
                },
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "problem",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::ProposalTooLarge => &[
                ArgSpec {
                    name: "what",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::ProposalLimit => &[ArgSpec {
                name: "limit",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::ProposalUnknown => &[ArgSpec {
                name: "proposal",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProposalNotPending => &[ArgSpec {
                name: "status",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::DryRunReadsAbove => &[ArgSpec {
                name: "setting",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::DryRunNotAllowed => &[
                ArgSpec {
                    name: "setting",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "entry",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::DryRunFolderInvalid => &[ArgSpec {
                name: "entry",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::DryRunAllowlistTooLong => &[ArgSpec {
                name: "limit",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::ProposalRate | Self::ProposalAfterRejection | Self::DryRunRate => &[ArgSpec {
                name: "seconds",
                kind: ArgKind::Count,
                optional: false,
            }],
            Self::ProposalReasons => &[
                ArgSpec {
                    name: "problem",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "change",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::RepositoryUnknown => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::SnapshotUnknown => &[ArgSpec {
                name: "snapshot",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::FolderUnknown | Self::EntryUnknown => &[ArgSpec {
                name: "path",
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
            Self::InvalidArguments => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "problem",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::MaintenanceBusy | Self::RestoreRunning => &[ArgSpec {
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
            | Self::DaemonShutDown
            | Self::HostNotAllowed
            | Self::OriginNotAllowed
            | Self::JsonRequired
            | Self::SignInRequired
            | Self::SignInFailed
            | Self::CsrfTokenInvalid
            | Self::SignInUnavailable
            | Self::ApiTokenInvalid
            | Self::SignInOff
            | Self::RestartUnavailable
            | Self::RestartPending
            | Self::ApiTokenNoScope
            | Self::ApiTokensUnavailable
            | Self::ProposalsUnavailable
            | Self::DryRunBusy
            | Self::DryRunGivesWay
            | Self::DryRunFilter
            | Self::DryRunFailed
            | Self::ProposalChanged
            | Self::ProposalNeedsForgetPreview
            | Self::ProposalForgetPreviewChanged
            | Self::CurrentPasswordWrong
            | Self::AdminPasswordUnavailable
            | Self::SessionsUnavailable
            | Self::DaemonUnreachable
            | Self::DaemonAnswerLost
            | Self::InlineSecret
            | Self::ConfigFolderUnknown
            | Self::RuntimeUnavailable
            | Self::StepUpFailed
            | Self::RestoreTargetUnwritable
            | Self::RestoreInPlaceNotHere
            | Self::RestoreHubNotAllowed
            | Self::RestoreFolderUnset
            | Self::FullDiskAccessNotLocal
            | Self::FullDiskAccessSettingsUnopened
            | Self::DownloadsBusy
            | Self::DownloadsUnavailable
            | Self::DownloadUnknown
            | Self::RouteUnknown
            | Self::TokenUsed
            | Self::TokenExpired
            | Self::TokenRevoked
            | Self::TokenMalformed
            | Self::TokenUnsupported
            | Self::AgentKeyMismatch
            | Self::HubKeyMismatch
            | Self::HubProofInvalid
            | Self::ProofRefused
            | Self::PendingLimit
            | Self::BindingUnavailable
            | Self::PlainAddress
            | Self::HubConnectionLost
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
            Self::SecretFileReadable => &[
                ArgSpec {
                    name: "path",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "mode",
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
            Self::LocationRuleMixedLink => &[
                ArgSpec {
                    name: "ipv4",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "ipv6",
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
            Self::ProfileUsedNotRead => &[ArgSpec {
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
            Self::ProfileCommandLeftOut => &[ArgSpec {
                name: "key",
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
            | Self::CoreDumpsOn
            | Self::DebuggerAttachAllowed
            | Self::FullDiskAccessMissing
            | Self::KeyringNotActivatable
            | Self::Unknown => &[],
        }
    }
}

/// A typed error, as a refusal carries it: over the HTTP API, and in the
/// CLI's `--json` output (§8.4, §12.1). The code; the arguments its message
/// takes, by name, for the reader to render; and, when the sender has words
/// of its own to show, the `detail`, such as the engine's message as it came.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// What a command prints in `--json` mode when it refused: the typed error,
/// under the key `error`, which no document of a command that did what it
/// was asked has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FailureDto {
    pub error: ErrorDto,
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
    /// A proposal's status changed (decision 89, point 14): facts only,
    /// never its rationale, its changes or its proposer's label. A page
    /// that shows proposals reads them again on hearing it.
    ProposalChanged {
        proposal: String,
        status: ProposalStatusDto,
    },
}

/// One operation of the history page, as the daemon tells the web UI: what
/// a list view needs — ids, the kind and status, the lifecycle times, and
/// the typed outcome. Times are RFC 3339, UTC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
    /// The codes of the operation's warnings, each code once, from the kept
    /// warnings (the first ten thousand), in the order the first of each
    /// was recorded (#508). The operation's detail lists the warnings.
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
    /// The alias of the repository a maintenance operation ran on, or a
    /// restore or a download read, as it stood when the log first wrote the
    /// run. Absent for a backup, which `plan` names, and for an operation an
    /// earlier version logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// For a repository reached at several locations: every location as the
    /// run's choice saw it, in order, so a page says "Used location 2 of 2"
    /// and why each earlier one was skipped (decision 80). Left out for a
    /// repository of one location and an operation that opens none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<LocationTriedDto>,
    /// The entry of a snapshot a restore or a download read, as the log
    /// kept it: its path from the snapshot's top, `/` first, with every
    /// secret of the repository masked and a path past 1 KiB cut to its end
    /// after `…` (#847). Left out for every other operation and for one an
    /// earlier version logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

impl OperationSummaryDto {
    /// The same summary, with the locations the run's choice saw.
    #[must_use]
    pub fn with_locations(self, locations: Vec<LocationTriedDto>) -> Self {
        Self { locations, ..self }
    }

    /// The same summary, naming the entry of a snapshot a restore or a
    /// download read, as the log kept it (#847).
    #[must_use]
    pub fn with_entry(self, entry: String) -> Self {
        Self {
            entry: Some(entry),
            ..self
        }
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
    #[expect(
        clippy::too_many_arguments,
        reason = "the arity is the wire's: one argument per field of the published shape"
    )]
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
            entry: None,
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationsPageDto {
    pub operations: Vec<OperationSummaryDto>,
    pub next: Option<String>,
}

/// One plan of the plans list, as the daemon tells the web UI: the ID a
/// start request names, the repository its backups go to, and when it runs
/// on its own (§5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// One plan as the Plans page edits it: the plan's table in the
/// configuration file's keys, as a `[[plans]]` entry writes it, each default
/// left out (decision 96). The daemon reads it as the file is read when it
/// loads, so the wire holds no second schema of a plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PlanDto(pub serde_json::Map<String, serde_json::Value>);

/// What is wrong with a plan's setting a [`ErrorCode::PlanInvalid`] names,
/// sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanProblem {
    /// A setting the plan needs isn't there.
    Missing,
    /// A setting a plan doesn't have.
    UnknownKey,
    /// A value of another type than the setting takes: a number where it
    /// wants text, say.
    WrongType,
    /// A value the setting doesn't take: a schedule that is not one, a tag
    /// Ladderback keeps for its own, an ID that is not a name.
    Invalid,
    /// The plan's ID is another plan's.
    Taken,
    /// The plan's repository is not one the configuration has.
    UnknownRepository,
    /// The plan backs up nothing: it has no source.
    NoSources,
}

impl PlanProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::UnknownKey => "unknown-key",
            Self::WrongType => "wrong-type",
            Self::Invalid => "invalid",
            Self::Taken => "taken",
            Self::UnknownRepository => "unknown-repository",
            Self::NoSources => "no-sources",
        }
    }
}

/// What is wrong with a proposal's change a [`ErrorCode::ProposalInvalid`]
/// names, sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeProblem {
    /// The change's plan isn't one the configuration file would load.
    Plan(PlanProblem),
    /// The change names a plan the configuration doesn't have.
    UnknownPlan,
    /// The change removes a plan a notification rule names.
    InUse,
    /// The change gives a plan hooks: a new plan any, a changed plan others
    /// than it has. Hooks come with approval (decision 89).
    Hooks,
}

impl ChangeProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan(problem) => problem.as_str(),
            Self::UnknownPlan => "unknown-plan",
            Self::InUse => "in-use",
            Self::Hooks => "hooks",
        }
    }
}

/// One configuration change of a proposal (decision 89), by its `kind`. A
/// plan travels as its table in the configuration file's keys, as the plan
/// routes take it ([`PlanDto`]); a removal names the plan by its ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProposalChangeDto {
    /// Adds the plan after the configuration's last one.
    AddPlan { plan: PlanDto },
    /// Puts the plan in place of the plan with its ID.
    ChangePlan { plan: PlanDto },
    /// Removes the plan with this ID; its snapshots stay.
    RemovePlan { plan: String },
}

/// `POST /api/v1/proposals`: the changes proposed, in the order they apply,
/// and why, in the proposer's own words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct NewProposalDto {
    pub changes: Vec<ProposalChangeDto>,
    #[serde(default)]
    pub rationale: String,
}

/// `POST /api/v1/dry-runs`: the draft plan to walk, its table in the
/// configuration file's keys, as [`PlanDto`] holds one (§16, decision 89,
/// point 13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DryRunPlanDto {
    pub plan: PlanDto,
}

/// What a plan dry run counted: counts and sizes only, never a path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct PlanDryRunDto {
    /// The files the plan would back up.
    pub files: u64,
    /// Their size in bytes.
    pub bytes: u64,
    /// The entries the walk couldn't read.
    pub unreadable: u64,
    /// What each exclude removes on its own, in the order of the plan's
    /// settings; those the dry run didn't reach before it stopped are left
    /// out.
    #[serde(default)]
    pub excludes: Vec<ExcludeTallyDto>,
    /// Why the dry run stopped before the end, if it did: its counts are
    /// then what it read until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<DryRunStopDto>,
}

/// What one exclude of a plan's source filter removes on its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ExcludeTallyDto {
    /// The plan's setting, as the configuration file names it
    /// (`globs`, `exclude-if-present`, `git-ignore`).
    pub setting: String,
    /// The setting's entry, from 1, for a setting that is a list; none for
    /// a switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u64>,
    /// The files it removes, and their bytes.
    pub files: u64,
    pub bytes: u64,
}

/// Why a plan dry run stopped before the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DryRunStopDto {
    /// A walk read as many entries as one may.
    Entries,
    /// Its time ran out.
    Time,
    /// A backup took a place in the queue, and it gave way.
    GaveWay,
}

/// `GET` and `PUT /api/v1/settings/dry-run-allowlist`: the folders a plan
/// dry run may read, a person's setting only (§16, decision 89, point 13).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DryRunAllowlistDto {
    /// The folders, absolute paths, as the configuration file holds them.
    pub folders: Vec<String>,
    /// The daemon's home folder, which "Add home folder" adds; sent by the
    /// daemon only, and none when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
}

/// Where a proposal is in its life (decision 89).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ProposalStatusDto {
    Pending,
    Applying,
    Applied,
    Failed,
    Rejected,
    Withdrawn,
    Expired,
    Stale,
}

/// One proposal as the proposal routes answer it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct ProposalDto {
    pub id: String,
    /// The label of the API token that proposed it, as it was then; `null`
    /// for a client with no token.
    #[serde(default)]
    pub proposed_by: Option<String>,
    pub status: ProposalStatusDto,
    pub created_at: String,
    /// 48 hours after `created-at`.
    pub expires_at: String,
    pub changes: Vec<ProposalChangeDto>,
    pub rationale: String,
    /// When a person approved or rejected it; `null` until one did.
    #[serde(default)]
    pub decided_at: Option<String>,
    /// A person's reason for rejecting each change, in order, `""` for a
    /// change given none; empty unless it was rejected.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// What the submission checks warn of, in the answer to its creation
    /// only: the proposal was kept all the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ProposalWarningDto>,
}

/// What the submission checks found that doesn't refuse a proposal
/// (decision 89, point 5), by its `kind`. `change` counts from 1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProposalWarningDto {
    /// The change leaves its plan `plan` backing up the same sources as
    /// the plan `other`.
    SameSources {
        change: u64,
        plan: String,
        other: String,
    },
    /// The change leaves its plan with the glob `glob`, which can never
    /// match, read against its sources as written.
    GlobNeverMatches { change: u64, glob: String },
}

/// `GET /api/v1/proposals`: the proposals the asker may see, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalsDto {
    pub proposals: Vec<ProposalDto>,
}

/// A plan's setting, as a summary line names what a change changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanSettingDto {
    Repository,
    Host,
    Label,
    Sources,
    Tags,
    Schedule,
    Overlap,
    Retry,
    Filter,
    Retention,
}

/// One line of the summary Ladderback writes of a proposal's changes
/// (decision 89, point 4), by its `kind`; never the AI agent's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SummaryLineDto {
    AddsPlan {
        plan: String,
    },
    /// Changes these settings of the plan.
    ChangesPlan {
        plan: String,
        settings: Vec<PlanSettingDto>,
    },
    RemovesPlan {
        plan: String,
    },
}

/// `GET /api/v1/proposals/{id}/review`, a browser session's only: the
/// proposal as the "Pending changes" page shows it (decision 89, point 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalReviewDto {
    pub proposal: ProposalDto,
    /// What its changes do, one line each, in order.
    pub summary: Vec<SummaryLineDto>,
    /// The unified diff of `config.toml` an approval applies, each secret
    /// the file holds masked; empty unless the proposal is pending.
    pub diff: String,
    /// The SHA-256, in hex, of the file the diff was rebuilt against, which
    /// an approval sends back.
    pub revision: String,
    /// The SHA-256, in hex, of `diff`, which an approval sends back.
    pub diff_hash: String,
    /// Whether the file has changed since the proposal was drafted.
    pub changed_since_drafted: bool,
    /// Whether a change of it could delete snapshots: such a proposal is
    /// approved only with the forget preview, so until then it is only
    /// rejected (decision 89, point 8).
    pub needs_forget_preview: bool,
    /// How many of its proposer's kept proposals a person rejected.
    pub rejected: u64,
    /// The ID's first characters, which confirm an approval while sign-in
    /// is off.
    pub short_id: String,
}

/// `POST /api/v1/proposals/{id}/forget-preview`, a browser session's only:
/// what the pending proposal would change about the snapshots retention
/// forgets, for each repository its changes touch, in the order they name
/// them (decision 89, point 8; I6). Computed when the person asks: it reads
/// each repository as a forget's preview does, and changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProposalForgetPreviewDto {
    pub repositories: Vec<RepositoryForgetDto>,
    /// What an approval of the proposal brings to say this preview is the
    /// one the person saw; none while a repository couldn't be read, so
    /// the page offers no approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// What a proposal would change about what retention forgets in one
/// repository, or why the repository couldn't be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum RepositoryForgetDto {
    /// What a forget of this agent's would do under the configuration as it
    /// is (`before`) and as the proposal makes it (`after`).
    Compared {
        repository: String,
        before: ForgetSideDto,
        after: ForgetSideDto,
        /// What `after` removes and `before` doesn't, per plan that removes
        /// any such: `remove` counts them and `listed` names them.
        forgets_more: Vec<ForgetPlanDto>,
        /// What `before` removes and `after` doesn't, per plan, the same.
        forgets_less: Vec<ForgetPlanDto>,
    },
    /// The repository couldn't be read: why, as the maintenance routes tell
    /// it.
    Refused { repository: String, error: ErrorDto },
}

/// What a forget of this agent's would do to a repository under one
/// configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum ForgetSideDto {
    /// This agent doesn't maintain the repository: no forget of its runs
    /// there, so its retention forgets nothing.
    NotMaintained,
    /// What it would remove: each plan with history there, as a forget's
    /// preview tells it.
    Forgets { plans: Vec<ForgetPlanDto> },
    /// It would be refused, and so remove nothing: the policy of `plan`,
    /// which has history there, keeps nothing.
    KeepsNothing { plan: String },
}

/// `POST /api/v1/proposals/{id}/approve`, a browser session's only: what
/// the page showed, from [`ProposalReviewDto`], and the step-up
/// confirmation: the admin password with sign-in on, the proposal's short
/// ID otherwise. The confirmation is never logged or shown (I4): its
/// `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ApproveProposalDto {
    pub revision: String,
    pub diff_hash: String,
    /// The digest of the forget preview the page showed, from
    /// [`ProposalForgetPreviewDto`]; a proposal that could delete
    /// snapshots is approved only with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget_digest: Option<String>,
    pub confirmation: String,
}

impl std::fmt::Debug for ApproveProposalDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApproveProposalDto")
            .field("revision", &self.revision)
            .field("diff_hash", &self.diff_hash)
            .field("forget_digest", &self.forget_digest)
            .field("confirmation", &"[hidden]")
            .finish()
    }
}

/// `POST /api/v1/proposals/{id}/reject`, a browser session's only: a
/// reason for each change, in order, `""` for none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RejectProposalDto {
    pub reasons: Vec<String>,
}

/// What is wrong with an argument an [`ErrorCode::InvalidArguments`] names,
/// sent as its `problem` argument by its wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArgumentProblem {
    /// An argument the tool needs isn't there.
    Missing,
    /// An argument the tool doesn't take.
    UnknownKey,
    /// A value the argument doesn't take: one of another type, or a number
    /// out of its range.
    Invalid,
}

impl ArgumentProblem {
    /// The name the `problem` argument carries, which the message selects
    /// on.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::UnknownKey => "unknown-key",
            Self::Invalid => "invalid",
        }
    }
}

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

/// One repository's snapshots, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotsDto {
    pub snapshots: Vec<SnapshotDto>,
}

/// The most entries one [`FolderDto`] lists: a folder is read in pages
/// (§16, decision 102), and a page is an input-derived size, capped (§13.5).
pub const MAX_FOLDER_ENTRIES: usize = 1_000;

/// One page of one folder of a snapshot, in the order the snapshot records
/// its entries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FolderDto {
    /// The folder's path in the snapshot, its names joined by `/`; empty
    /// for the snapshot's top folder.
    pub path: String,
    /// At most [`MAX_FOLDER_ENTRIES`].
    pub entries: Vec<EntryDto>,
    /// How many entries the folder holds in all.
    pub total: u64,
    /// The position the next page starts at, sent back as `from`; absent
    /// on the folder's last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

/// One entry of a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct EntryDto {
    /// The name as rustic records it: the key a request opens the entry by,
    /// never shown. A byte the file system's name had that isn't UTF-8 is
    /// written as `\xNN`, and a backslash, a quote and the controls
    /// `\a\b\f\n\r\t\v` escaped as Go's `strconv.Quote` does.
    pub name: String,
    /// The name as a person reads it: as the file system wrote it, a byte
    /// that isn't UTF-8 as U+FFFD, and a control, bidirectional or
    /// separator character made visible (`domain::browse::DisplayName`).
    pub display: String,
    pub kind: EntryKindDto,
    /// A file's size in bytes; absent for anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// When the entry was last modified, RFC 3339 in UTC, when the snapshot
    /// recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKindDto {
    Folder,
    File,
    /// A symbolic link, recorded as a link.
    Link,
    /// A device, a pipe or a socket.
    Other,
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

/// One recorded warning of an operation's detail: its typed code and the
/// free-form fields the engine reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct OperationDetailDto {
    pub operation: OperationSummaryDto,
    pub warnings: Vec<OperationWarningDto>,
}

/// What a successful operation produced, as the history page shows it: a
/// small typed summary per kind, mapped from the app's engine result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
    /// What a restore wrote: the files written, new or overwritten, and
    /// their bytes.
    Restore {
        files: u64,
        bytes: u64,
        duration_ms: u64,
    },
}
