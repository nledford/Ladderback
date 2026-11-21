//! Error and warning codes, their typed arguments, and the documents that carry
//! them across every boundary (technical design §8.4, §13.7.1).

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
    /// service), or the service refused otherwise than as locked, as when it
    /// has no collection. As `SecretMissing`; the platform's words,
    /// redacted, are the `detail`.
    SecretKeyringUnavailable,
    /// A repository's secret is in a keyring entry, and the keyring service
    /// answered that its keyring is locked, or its unlock prompt was
    /// dismissed, which leaves it locked. As `SecretMissing`; the
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
    /// refused otherwise than as locked, as when it has no collection.
    /// Nothing was saved or created. The argument `entry` names the keyring
    /// entry (`service/name`); the platform's words, masked, are the
    /// `detail`.
    KeyringUnavailable,
    /// The password typed for a repository was to be kept in the system
    /// keyring, and the keyring service answered that its keyring is locked,
    /// or its unlock prompt was dismissed, which leaves it locked. As
    /// `KeyringUnavailable`.
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
    /// A backup of the plan started by hand was refused: a run of the plan
    /// is waiting for its turn already, and backs up the sources as they
    /// are when it starts, so nothing was queued (decision 75, point 4).
    /// The argument `plan` names the plan.
    PlanRunWaiting,
    /// A plan sent to be created or changed is not one the configuration
    /// file would load, so nothing was saved (decision 96). The argument
    /// `field` is the plan's setting at fault, as its key in the plan's
    /// table (`schedule.cron`), and `problem` what is wrong with it, a
    /// [`PlanProblem`](crate::PlanProblem)'s wire name. Neither holds a value of the plan.
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
    /// tool didn't run; or a query parameter of a route such a tool calls
    /// through (a folder's page `limit`, a search's) is, alike. The argument
    /// `field` names the argument or parameter, and `problem` what is wrong
    /// with it, an [`ArgumentProblem`]'s wire name. Neither holds the value
    /// given.
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
    /// The profile runs commands nobody agreed the import may make
    /// runnable, so nothing was imported: `import-profile` was not told
    /// `--yes`, and had nobody to ask, or the profile's commands changed
    /// after they were listed (§6.2).
    ProfileCommandsUnconfirmed,
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
    /// The request's API token is good but doesn't open this route (§10.1,
    /// decision 127). The argument `permission` is what the route needs:
    /// `backups`, `backup-runs`, `previews`, `configuration`,
    /// `snapshot-contents`, `daemon-log`, `repository-check`, or `session`
    /// for a route only a signed-in browser may use.
    ApiTokenPermission,
    /// An AI agent without an API token asked for something the
    /// configuration's `[without-token]` table doesn't allow (§10.5), or for
    /// something only a signed-in browser may do. The argument `permission`
    /// names what it needs, as for `api-token-permission`.
    WithoutTokenPermission,
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
    /// An API token was asked for with no permission.
    ApiTokenNoPermission,
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
    /// The connection to the hub ended after the agent paired, or the hub
    /// sent what isn't the next message, or one it didn't sign on this
    /// connection: nothing in it was acted on.
    HubLinkLost,
    /// The hub's `Welcome` carries a lower generation than the agent's pin
    /// holds: the agent ignored it (decision 120, "Generation and
    /// addresses").
    GenerationLower,
    /// The hub and the agent share no protocol: one of them is on an older
    /// release than the other can use.
    ProtocolOutOfDate,
    /// A pairing token, or another high-impact action on the hub, was asked
    /// for from beyond loopback while sign-in is off, so there is no
    /// password to confirm it with (§16, decision 120, "Who may make a
    /// token, approve or re-key"). Nothing was made.
    PairingNeedsSignIn,
    /// The address asked for a pairing token isn't `http://` or
    /// `https://` with a host and a port, or is longer than a token holds.
    HubAddressInvalid,
    /// A pairing token's life outside 5 minutes to 24 hours was asked for.
    PairingTokenLife,
    /// As many pairing tokens as the hub keeps are unused already: one must
    /// be used, lapse or be canceled first.
    PairingTokenLimit,
    /// No unused pairing token has the ID asked for: it was used, lapsed or
    /// canceled already.
    PairingTokenUnknown,
    /// The hub couldn't read or keep its key or its tokens.
    HubUnavailable,
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
    /// A secret was read from a file other users of the machine can change,
    /// so they can choose the secret (#1137). The arguments are `path` and
    /// `mode`, as for [`Self::SecretFileReadable`]. Never the secret (I4).
    SecretFileWritable,
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
    /// A repository's `rclone-command` gives one of rclone's flags a
    /// credential written as text, as in `--pass=…`: the configuration file
    /// keeps it, and rclone is started with it in its arguments, where other
    /// users of the computer can read it. Said when the configuration loads
    /// and on the repository's row of the list. The argument `field` names
    /// the setting (`options.rclone-command`, `options-hot.rclone-command`,
    /// `options-cold.rclone-command`), `flag` the flag as `--NAME`; the
    /// optional `position`, from 2, is the further location that holds it,
    /// absent for the first. Never the credential itself (I4).
    RcloneCommandSecret,
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
    /// An import made a command runnable (§6.2), one notice for each. The
    /// argument `key` is where the configuration keeps it: `hooks.run-before`,
    /// `repositories.<alias>.password-command`, an option rustic runs as a
    /// command, as `repositories.<alias>.options.rclone-command`, or a
    /// hook's list, as `plans.<id>.hooks.run-after`; `program` is the program
    /// it starts.
    ProfileCommandImported,
    /// An import left the profile's `[global.hooks]` out, because the
    /// configuration already had plans or repositories whose jobs they would
    /// have run around (§6.2).
    ProfileGlobalHooksLeftOut,
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
            Self::ApiTokenPermission | Self::WithoutTokenPermission => &[ArgSpec {
                name: "permission",
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
            Self::PlanRunWaiting => &[ArgSpec {
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
            Self::MaintenanceSelectionChanged
            | Self::RepositoryReadTimedOut
            | Self::ProfileCommandsUnconfirmed => &[],
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
            | Self::ApiTokenNoPermission
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
            | Self::HubLinkLost
            | Self::GenerationLower
            | Self::ProtocolOutOfDate
            | Self::PairingNeedsSignIn
            | Self::HubAddressInvalid
            | Self::PairingTokenLife
            | Self::PairingTokenLimit
            | Self::PairingTokenUnknown
            | Self::HubUnavailable
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
            Self::SecretFileReadable | Self::SecretFileWritable => &[
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
            Self::RcloneCommandSecret => &[
                ArgSpec {
                    name: "field",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "flag",
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
            Self::ProfileCommandImported => &[
                ArgSpec {
                    name: "key",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "program",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
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
            | Self::ProfileGlobalHooksLeftOut
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
    /// ([`UiEvent::OperationFailed`](crate::UiEvent::OperationFailed)): its
    /// arguments by position, in the order [`ErrorCode::args`] declares them,
    /// up to the first it lacks, then its detail as one more text, when it
    /// has one.
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

/// What a command prints in `--json` mode when it refused: the typed error,
/// under the key `error`, which no document of a command that did what it
/// was asked has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FailureDto {
    pub error: ErrorDto,
}

#[cfg(test)]
mod schema_tests;

#[cfg(test)]
mod tests;
