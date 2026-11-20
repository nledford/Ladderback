//! The daemon's wiring (technical design §3.2, §4.2): the one place beside
//! `main` that knows every adapter. For `serve`, [`startup::daemon`] opens
//! the store and the config, builds the engine, secrets and the event bus,
//! and hands the result to the HTTP router. Each submodule wires one area
//! (the catalog, the repositories, the plan runner, maintenance and the
//! rest); this root keeps what more than one of them shares. Adapters stay
//! constructor-injected; nothing else sees them.
//!
//! A startup that cannot happen is a typed exit: [`Refused`] carries the
//! catalog message and its arguments, and `serve` prints it and fails — never
//! a silent empty configuration or a panic.

use std::collections::HashMap;
use std::sync::Arc;

use api::{ErrorCode, MsgArg, UiEvent, WarningCode};
use app::announce::failure;
use domain::execution::locations::LocationsRecord;

use app::ports::SecretResolver;
use app::ports::{
    AdminPasswordError, Clock, ConfigError, Engine, EngineError, EngineEvent, EngineResult,
    EventBus, Job, NormalizeSources, OperationRecord, RunningJob,
};
use app::progress::LiveProgress;
use app::secrets::{Cause, ResolveError};
use async_trait::async_trait;
use domain::OperationId;
use domain::execution::FailureReason;
use domain::execution::{OperationKind, OperationStatus, WarningKind};
use domain::planning::{Plan, RepositoryConfig, RepositorySettings};
use domain::secrets::{
    MaskedSecrets, Part, Redactor, RepositorySecrets, SecretRef, SecretValue, SecretWarning,
    Template,
};
use futures::stream::StreamExt;
use http_adapter::{AppState, RepositoriesError, SecretRefusal};
use l10n::{MessageId, msg};
use platform::SystemClock;
use platform::admin_password::FileAdminPassword;
use platform::data_dir_lock::DataDirLock;
use platform::dirs;
use store::SqliteStore;

mod catalog;
mod downloads;
mod dry_runs;
mod key_files;
mod maintenance;
pub(crate) mod mcp_server;
mod plan_runner;
mod plans;
pub(crate) mod profile_refusals;
mod profiles;
mod proposals;
mod repositories;
mod restores;
mod runs;
pub(crate) mod startup;

use startup::Background;

/// Why the daemon cannot serve, as a message for the person who started it.
pub(crate) struct Refused {
    pub(crate) message: MessageId,
    pub(crate) args: Vec<(&'static str, MsgArg)>,
}

/// A refusal with no arguments.
fn plain(message: MessageId) -> Refused {
    Refused {
        message,
        args: Vec::new(),
    }
}

/// A text argument, the kind identifiers, paths and versions are.
fn text(value: impl std::fmt::Display) -> MsgArg {
    MsgArg::Text(value.to_string())
}

/// A `glob-never-matches` warning, with its arguments, for each glob of
/// `plan` that can never match, read against its sources as `sources` walks
/// them (#510).
async fn never_matching_glob_warnings(
    plan: &Plan,
    sources: &dyn NormalizeSources,
) -> Vec<(WarningCode, Vec<(&'static str, MsgArg)>)> {
    app::globs::never_matching_globs(plan, sources)
        .await
        .into_iter()
        .map(|glob| glob_never_matches(plan.id.as_str(), &glob.glob, glob.absolute.as_deref()))
        .collect()
}

/// The `glob-never-matches` warning for `glob` of the plan `plan`, with
/// `absolute`, the whole path to write instead, left out when there is none.
fn glob_never_matches(
    plan: &str,
    glob: &str,
    absolute: Option<&str>,
) -> (WarningCode, Vec<(&'static str, MsgArg)>) {
    let mut args = vec![("plan", text(plan)), ("glob", text(glob))];
    args.extend(absolute.map(|absolute| ("absolute", text(absolute))));
    (WarningCode::GlobNeverMatches, args)
}

/// A warning's arguments, by name.
type WarningArgs = Vec<(&'static str, MsgArg)>;

/// The warning for each secret `repository` holds as text, with the
/// position, from 1, of the location that holds it.
fn inline_secret_warnings(repository: &RepositoryConfig) -> Vec<(u8, WarningCode, WarningArgs)> {
    let settings = repository.location_settings();
    repository
        .inline_secrets()
        .into_iter()
        .filter_map(|(position, secret)| {
            let at = settings.get(usize::from(position).checked_sub(1)?)?;
            let (code, args) = inline_secret_warning(at, &secret, position);
            Some((position, code, args))
        })
        .collect()
}

/// The arguments of the `location-plain-http-credentials` warning about
/// `warning`, a location of `repository`: by position, cause and whether it
/// has a rule, and whether the repository has one location or several, for
/// the text of a repository of one names no location (#473).
fn exposed_args(
    repository: &RepositoryConfig,
    warning: domain::planning::LocationWarning,
) -> Vec<(&'static str, MsgArg)> {
    vec![
        ("position", text(warning.position)),
        ("cause", text(warning.cause.code())),
        ("rule", text(warning.rule_code())),
        ("locations", text(repository.locations_code())),
    ]
}

/// The warning for a secret the configuration holds as text, with its
/// arguments: one in a location is moved differently from a setting of its
/// own, one in an rclone location differently again, and one after a flag
/// of an `rclone-command` is named by its flag too (§10.2). `settings` open
/// the location at `position`, from 1; a further one is named by it.
fn inline_secret_warning(
    settings: &RepositorySettings,
    secret: &domain::secrets::InlineSecret,
    position: u8,
) -> (WarningCode, Vec<(&'static str, MsgArg)>) {
    let field = ("field", MsgArg::Text(secret.field()));
    let (code, mut args) = match (settings.location_of(secret), secret.flag()) {
        (Some(location), _) => (
            WarningCode::LocationSecret,
            vec![
                field,
                (
                    "kind",
                    location_kind(domain::secrets::rclone_location(location)),
                ),
            ],
        ),
        (None, Some(flag)) => (
            WarningCode::RcloneCommandSecret,
            vec![field, ("flag", MsgArg::Text(flag))],
        ),
        (None, None) => (WarningCode::ConfigInlineSecret, vec![field]),
    };
    if position > 1 {
        args.push(("position", text(position)));
    }
    (code, args)
}

/// The `kind` argument of a `location-secret` warning.
fn location_kind(rclone: bool) -> MsgArg {
    text(if rclone { "rclone" } else { "url" })
}

/// What `serve` runs: the router's state, the operation log it writes, the
/// hold on the data folder that keeps every other process off it, the work
/// it does with no request (the queue left from before it started and the
/// scheduler), and its workers. The caller stops the workers once it has
/// stopped serving, then closes the log, and keeps the lock until the
/// daemon is gone (§4.2, §9).
pub(crate) struct Daemon {
    pub(crate) state: AppState,
    pub(crate) log: OperationLog,
    pub(crate) lock: DataDirLock,
    pub(crate) background: Background,
    pub(crate) workers: engine::Workers,
}

/// How long the daemon waits for the operation log to close once it stops
/// (§9): as long as a run waits for one write of its own
/// ([`app::log_write::SAVE_BOUND`]), which is past the store's own bound on
/// a call (about 15 s: 10 s for a connection, 5 s for the write lock). A
/// close waits for the calls in flight, so a busy store still finishes
/// them; one that hasn't by then is stalled (a disk that stopped
/// answering), and the daemon stops without it rather than hold up the
/// service manager, which would kill it anyway.
pub(crate) const LOG_CLOSE_BOUND: std::time::Duration =
    std::time::Duration::from_secs(app::log_write::SAVE_BOUND.as_secs().unsigned_abs());

/// The operation log the daemon writes, which it closes before it lets go
/// of the data folder.
pub(crate) struct OperationLog {
    pub(crate) store: Arc<SqliteStore>,
    database: std::path::PathBuf,
}

impl OperationLog {
    /// Closes the log once the calls in flight have ended, so no connection
    /// to it, and none of its `-wal` and `-shm` files, outlives the daemon.
    /// It waits [`LOG_CLOSE_BOUND`] at most, and says in the log when it
    /// gave up; what a call cut off then was still writing is ended at the
    /// next start, as after a crash.
    pub(crate) async fn close(&self) {
        close_within(self.store.close(), &self.database, LOG_CLOSE_BOUND).await;
    }

    /// Ends each operation still running as failed with `daemon-shut-down`
    /// at this moment, once the calls in flight have ended, and closes the
    /// log, however long that takes: the daemon's orderly stop, which calls
    /// it once its workers have stopped and bounds it as one of its steps
    /// (`shutdown`, decision 92). Nothing is announced. Ends it couldn't
    /// write, said in the log, are the next start's, as after a crash.
    pub(crate) async fn close_at_orderly_stop(&self) {
        let ended = self
            .store
            .close_ending_running(&FailureReason::DaemonShutDown, SystemClock.now())
            .await;
        match ended {
            Ok(0) => {}
            Ok(ended) => {
                log::info!("stopping: {ended} running operation(s) ended as daemon-shut-down")
            }
            Err(error) => log::warn!(
                "stopping: the operations still running couldn't be ended as daemon-shut-down, so the next start ends them as daemon-stopped: {error}"
            ),
        }
    }
}

/// Whether sign-in is on (§10.1, decision 70), decided once as the daemon
/// starts, before its store opens: on when the data folder keeps an admin
/// password's hash, off when it keeps none. Either way with where the hash
/// is kept: off, the session route looks there for one set since.
pub(crate) enum SignInDecided {
    Off(Arc<FileAdminPassword>),
    On(Arc<FileAdminPassword>),
}

impl SignInDecided {
    pub(crate) fn is_on(&self) -> bool {
        matches!(self, Self::On(_))
    }
}

/// Decides [`SignInDecided`]. A hash file that can't be read, or isn't a
/// hash this build checks, is a refusal to start, never sign-in off.
pub(crate) fn sign_in() -> Result<SignInDecided, Refused> {
    let data_dir = dirs::data_dir().ok_or_else(|| plain(msg::CLI_NO_DATA_DIR))?;
    let kept = Arc::new(FileAdminPassword::in_data_dir(&data_dir));
    match kept.is_set() {
        Ok(false) => Ok(SignInDecided::Off(kept)),
        Ok(true) => Ok(SignInDecided::On(kept)),
        Err(error) => {
            // `is_set` never answers `NotSet`: no file is sign-in off.
            let detail = match error {
                AdminPasswordError::Unreadable(said) | AdminPasswordError::Unwritable(said) => said,
                AdminPasswordError::NotSet => String::new(),
            };
            Err(Refused {
                message: msg::CLI_ADMIN_PASSWORD_UNREADABLE,
                args: vec![
                    (
                        "path",
                        text(data_dir.join(platform::admin_password::FILE).display()),
                    ),
                    ("detail", text(detail)),
                ],
            })
        }
    }
}

/// Waits for `close`, the closing of the operation log at `database`, for
/// at most `bound`, and says in the log when it gave up.
async fn close_within(
    close: impl Future<Output = ()>,
    database: &std::path::Path,
    bound: std::time::Duration,
) {
    if tokio::time::timeout(bound, close).await.is_err() {
        log::warn!(
            "the operation log {} did not close within {} s, and the daemon stops without it: an operation it was still recording is ended at the next start",
            database.display(),
            bound.as_secs_f64()
        );
    }
}

/// The engine as the daemon runs it (§4.2): a job's progress and warnings go
/// to the event bus as [`UiEvent`]s on their way through, in the order the
/// engine reported them. The daemon never coalesces: that is the worker's,
/// once (§4.1). How an operation started and ended is not announced here:
/// the run that records it announces each once the log has it (§4.4,
/// #305). A job that is no operation tells the pages nothing (#308). Each
/// progress report goes with the instant `clock` reads as the daemon hears
/// it, so a page can say when a run without a total last reported (§4.5).
struct EventEngine {
    inner: Arc<dyn Engine>,
    bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
}

#[async_trait]
impl Engine for EventEngine {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        if !is_operation(&job) {
            return self.inner.run(op, job).await;
        }
        let operation = op.as_uuid().hyphenated().to_string();
        let running = self.inner.run(op, job).await?;
        let mut publish = Publish {
            operation,
            progress: LiveProgress::new(),
            clock: Arc::clone(&self.clock),
        };
        let bus = Arc::clone(&self.bus);
        Ok(RunningJob {
            // `then` keeps the engine's order: an event is published before
            // the runner sees it.
            events: running
                .events
                .then(move |event| {
                    let bus = Arc::clone(&bus);
                    let ui = publish.ui(&event);
                    async move {
                        if let Some(ui) = ui {
                            bus.publish(ui).await;
                        }
                        event
                    }
                })
                .boxed(),
            control: running.control,
        })
    }
}

/// Whether the job is an operation, which the history has a row for, so a
/// page can show its progress and warnings there (§4.4). The reads (the look
/// that names a repository, a listing, a stats read, a prune's or a
/// restore's plan) and the initialising of a repository answer their request
/// and are written to no log (§9): a page told of their news would show it
/// on no row, or on the row of the run that read, which keeps none of it
/// (#308). A download has a row, but its use case writes it, not a run, and
/// its news is the pieces its browser reads, never told to a page.
fn is_operation(job: &Job) -> bool {
    match job {
        Job::Identify(_)
        | Job::Probe(_)
        | Job::Init(_)
        | Job::Ls(_)
        | Job::Browse(_)
        | Job::Stats(_)
        | Job::PrunePlan(_)
        | Job::RestorePlan(_)
        | Job::Dump(_) => false,
        Job::Fake(_)
        | Job::Backup(_)
        | Job::Forget(_)
        | Job::Rewrite(_)
        | Job::Prune(_)
        | Job::Check(_)
        | Job::Restore(_) => true,
    }
}

/// Turns one run's [`EngineEvent`]s into [`UiEvent`]s, but for how the run
/// started and ended (§4.4). Of its progress bars, a page is told of the one
/// [`LiveProgress`] shows: the bar begun last that is still open, with the
/// instant `clock` reads as it is heard.
struct Publish {
    operation: String,
    progress: LiveProgress,
    clock: Arc<dyn Clock>,
}

impl Publish {
    fn ui(&mut self, event: &EngineEvent) -> Option<UiEvent> {
        use EngineEvent as E;
        match event {
            E::Progress(progress) => self
                .progress
                .apply(progress)
                .map(|shown| shown.ui(self.operation.clone(), self.clock.now())),
            E::Warning(warning) => Some(UiEvent::OperationWarning {
                operation: self.operation.clone(),
                code: warning_code(warning.kind),
                args: warning_args(warning),
            }),
            // The engine's start comes before the run's write that says the
            // operation is running, and its end before the run's ending hooks
            // and final write, so a page told of either could read a history
            // that doesn't have it yet. The run announces both.
            // The choice of location is the run's record, which the
            // operation's row shows once the run writes it.
            // A dump's header and pieces go to its download alone.
            E::Started { .. }
            | E::Succeeded(_)
            | E::Failed(_)
            | E::Located(_)
            | E::Dumping(_)
            | E::Piece(_) => None,
        }
    }
}

/// One log record as the history page shows it, or `None` when it holds
/// something no page could show. The page carries the status and each
/// warning's code once, from the kept warnings (#508); the warnings
/// themselves come with the detail.
fn summary(record: &OperationRecord) -> Option<api::OperationSummaryDto> {
    let failure = match &record.status {
        OperationStatus::Failed { reason } => Some(failure(reason)),
        _ => None,
    };
    api::OperationSummaryDto::new(
        record.id.as_uuid().hyphenated().to_string(),
        record
            .target
            .plan
            .as_ref()
            .map_or_default(|plan| plan.as_str().to_owned()),
        kind_text(record.kind).to_owned(),
        status_text(&record.status).to_owned(),
        record.queued_at.to_string(),
        record.started_at.map(|at| at.to_string()),
        record.finished_at.map(|at| at.to_string()),
        record
            .warning_kinds
            .iter()
            .map(|kind| warning_code(*kind))
            .collect(),
        failure,
        record
            .result
            .as_ref()
            .and_then(|result| result_dto(result, record)),
        record
            .repository_alias
            .as_ref()
            .map(|alias| alias.as_str().to_owned()),
    )
    .map(|summary| {
        let summary =
            summary.with_locations(record.locations.as_ref().map_or_default(locations_tried));
        match &record.entry {
            Some(entry) => summary.with_entry(entry.as_str().to_owned()),
            None => summary,
        }
    })
    .map_err(|invalid| log::warn!("an operation the page cannot show: {invalid:?}"))
    .ok()
}

/// What a successful operation produced, as the page shows it: the small
/// typed summary of its kind, with the run's duration.
fn result_dto(result: &EngineResult, record: &OperationRecord) -> Option<api::OperationResultDto> {
    let duration_ms = record
        .started_at
        .zip(record.finished_at)
        .and_then(|(from, to)| u64::try_from(to.as_millisecond() - from.as_millisecond()).ok())
        .unwrap_or(0);
    match result {
        EngineResult::Backup(backup) => Some(api::OperationResultDto::Backup {
            files: backup
                .files_new
                .saturating_add(backup.files_changed)
                .saturating_add(backup.files_unmodified),
            bytes: backup.data_added,
            duration_ms,
            snapshot: Some(backup.snapshot.as_str().to_owned()),
            bytes_processed: backup.bytes_processed,
        }),
        EngineResult::Prune(prune) => Some(api::OperationResultDto::Prune {
            packs_deleted: prune.packs_deleted,
            duration_ms,
        }),
        EngineResult::Check(check) => Some(api::OperationResultDto::Check {
            errors: check.errors,
            warnings: check.warnings,
            duration_ms,
        }),
        EngineResult::Forget(forget) => Some(api::OperationResultDto::Forget {
            removed: u64::try_from(forget.removed.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Rewrite(rewrite) => Some(api::OperationResultDto::Adopt {
            rewritten: u64::try_from(rewrite.created.len()).unwrap_or(0),
            duration_ms,
        }),
        EngineResult::Restore(restore) => Some(api::OperationResultDto::Restore {
            files: restore.files_restored,
            bytes: restore.bytes_restored,
            duration_ms,
        }),
        // Reads: a result no operation row holds, so the page shows none.
        EngineResult::Fake { .. }
        | EngineResult::Repository { .. }
        | EngineResult::PrunePlan(_)
        | EngineResult::RestorePlan(_)
        | EngineResult::Probed(_)
        | EngineResult::Snapshots(_)
        | EngineResult::Folder(_)
        | EngineResult::Stats(_)
        | EngineResult::Dump(_) => None,
    }
}

/// An operation's kind, as the wire names it.
fn kind_text(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Fake => "fake",
        OperationKind::Identify => "identify",
        OperationKind::Init => "init",
        OperationKind::Backup => "backup",
        OperationKind::Forget => "forget",
        OperationKind::Adopt => "adopt",
        OperationKind::Prune => "prune",
        OperationKind::Check => "check",
        OperationKind::Ls => "ls",
        OperationKind::Restore => "restore",
        OperationKind::Download => "download",
    }
}

/// An operation's status, as the wire names it.
fn status_text(status: &OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Succeeded => "succeeded",
        OperationStatus::SucceededWithWarnings => "succeeded-with-warnings",
        OperationStatus::Failed { .. } => "failed",
        OperationStatus::Cancelled => "cancelled",
    }
}

/// A warning's kind, as the wire names it.
fn warning_code(kind: WarningKind) -> WarningCode {
    match kind {
        WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
        WarningKind::Hook => WarningCode::HookWarning,
        WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
        WarningKind::CheckWarning => WarningCode::CheckWarning,
        WarningKind::CheckError => WarningCode::CheckError,
        WarningKind::OtherHosts => WarningCode::MaintenanceOtherHosts,
        WarningKind::LocationSkipped => WarningCode::LocationSkipped,
        WarningKind::SecretFileReadable => WarningCode::SecretFileReadable,
        WarningKind::SecretFileWritable => WarningCode::SecretFileWritable,
        WarningKind::FullDiskAccessMissing => WarningCode::FullDiskAccessMissing,
        WarningKind::Other => WarningCode::Unknown,
    }
}

/// The arguments a warning's message takes, as the page gets them: the
/// path and mode of a secret file others can read or change (#386, #1137),
/// otherwise the item the warning names, if any.
fn warning_args(warning: &domain::execution::Warning) -> Vec<MsgArg> {
    match warning.kind {
        WarningKind::SecretFileReadable | WarningKind::SecretFileWritable => warning
            .item
            .iter()
            .cloned()
            .chain([warning.detail.clone()])
            .map(MsgArg::Text)
            .collect(),
        _ => warning
            .item
            .as_ref()
            .map_or_default(|item| vec![MsgArg::Text(item.clone())]),
    }
}

/// The running operations' cancel switches (§4.2): what the API cancels
/// through. A handle is registered when an operation starts running and
/// dropped when it ends, so only a running operation can be cancelled, once.
struct CancelRegistry {
    running: std::sync::Mutex<HashMap<OperationId, futures::channel::oneshot::Sender<()>>>,
}

/// Every location of a choice or a Test, as the wire carries it: by
/// position, the address masked as recorded, and the outcome's code.
fn locations_tried(record: &LocationsRecord) -> Vec<api::LocationTriedDto> {
    record
        .locations
        .iter()
        .map(|tried| api::LocationTriedDto {
            position: tried.position,
            address: tried.address.clone(),
            outcome: tried.outcome.code().to_owned(),
            source: tried.source.clone(),
        })
        .collect()
}

/// Secrets that come to more than a job masks, as the secret refusal the
/// routes answer with: the cap, and the setting where they passed it, when
/// one is known (§10.2). Neither is a secret or tells one (I4).
pub(crate) fn too_much_to_mask(field: Option<String>, limit: usize) -> SecretRefusal {
    let mut args = vec![(
        "limit",
        MsgArg::Bytes(u64::try_from(limit).unwrap_or(u64::MAX)),
    )];
    args.extend(field.map(|field| ("field", MsgArg::Text(field))));
    SecretRefusal {
        code: ErrorCode::SecretsTooLargeToMask,
        args,
        detail: None,
    }
}

fn unavailable(
    code: ErrorCode,
    args: Vec<(&'static str, MsgArg)>,
    detail: Option<String>,
) -> RepositoriesError {
    RepositoriesError::Unavailable { code, args, detail }
}

/// A failure a job ran into, in the words every route that tells one shares
/// (#742). Each route makes it its own refusal: `OperationFailed`,
/// `RepositoryNotFound` and `RepositoryConfigNotFound` become the route's
/// own variants, and `RepositoryExists` is the one told differently by
/// route, an existing repository to the Repositories API and a plain failed
/// operation to a read of the repository.
#[derive(Debug, Clone, PartialEq)]
enum FailureRefusal {
    /// The engine's own words for a failure it reports. The engine redacts
    /// what it says.
    OperationFailed { detail: String },
    /// A repository where a job was to create one.
    RepositoryExists,
    /// No repository at the location.
    RepositoryNotFound,
    /// The engine's words for a location with no repository's config in it.
    RepositoryConfigNotFound { detail: String },
    /// Secrets the job couldn't use or mask.
    SecretRefused(SecretRefusal),
    /// A typed code, its arguments, and the engine's words when it said any.
    Unavailable {
        code: ErrorCode,
        args: Vec<(&'static str, MsgArg)>,
        detail: Option<String>,
    },
}

/// What `reason` is told as, by every route that tells a failed job: a new
/// `FailureReason` fails to build here until it is given its words.
fn failure_refusal(reason: FailureReason) -> FailureRefusal {
    use FailureReason as F;
    let unavailable = |code, args, detail| FailureRefusal::Unavailable { code, args, detail };
    match reason {
        F::EngineFailed { detail } => FailureRefusal::OperationFailed { detail },
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        F::EngineExited { exit_code } => unavailable(
            ErrorCode::EngineExited,
            exit_code
                .map(|code| ("exit_code", text(code)))
                .into_iter()
                .collect(),
            None,
        ),
        F::ProtocolViolated { transition } => unavailable(
            ErrorCode::ProtocolViolated,
            vec![("transition", text(format!("{transition:?}")))],
            None,
        ),
        F::DeadlinePassed => unavailable(ErrorCode::DeadlinePassed, Vec::new(), None),
        F::RestoreTargetUnwritable => {
            unavailable(ErrorCode::RestoreTargetUnwritable, Vec::new(), None)
        }
        F::EngineStartTimedOut => unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None),
        F::EngineExitTimedOut => unavailable(ErrorCode::EngineExitTimedOut, Vec::new(), None),
        // Only a backup's before-hook stops a job this way: a repository job
        // and a read of the repository run no hooks. It is still a failure,
        // in the catalog's plainest words.
        F::HookFailed { .. } => unavailable(ErrorCode::OperationFailed, Vec::new(), None),
        F::RepositoryExists => FailureRefusal::RepositoryExists,
        F::RepositoryReplaced => unavailable(ErrorCode::RepositoryReplaced, Vec::new(), None),
        F::RepositoryNotFound => FailureRefusal::RepositoryNotFound,
        F::RepositoryConfigNotFound { detail } => {
            FailureRefusal::RepositoryConfigNotFound { detail }
        }
        F::CredentialsRefused { half } => unavailable(
            ErrorCode::CredentialsRefused,
            half.map(|half| ("half", text(half.code())))
                .into_iter()
                .collect(),
            None,
        ),
        F::SecretsTooLargeToMask { field, limit } => {
            FailureRefusal::SecretRefused(too_much_to_mask(field, limit))
        }
        // Only an operation of the log can be ended by a stopped daemon, and
        // neither a repository job nor a read of the repository is one; the
        // failure has its code all the same, as it has everywhere it is told.
        F::DaemonStopped => unavailable(ErrorCode::DaemonStopped, Vec::new(), None),
        F::DaemonShutDown => unavailable(ErrorCode::DaemonShutDown, Vec::new(), None),
        F::NoLocationUsable => unavailable(ErrorCode::NoLocationUsable, Vec::new(), None),
        F::NoLocationRuleHolds => unavailable(ErrorCode::NoLocationRuleHolds, Vec::new(), None),
        F::PasswordNotAccepted => unavailable(ErrorCode::PasswordNotAccepted, Vec::new(), None),
        // Only a run the schedule started records a refusal (§5): a
        // repository job or a read is refused before it runs, as
        // `secret_refusal` says.
        F::SecretRefused { field, refusal } => {
            let (code, args, detail) = app::announce::refused(&field, &refusal);
            FailureRefusal::SecretRefused(SecretRefusal { code, args, detail })
        }
    }
}

/// Says in the daemon's log what reading a set of secrets warned of: a file
/// other users can read or change, by its path, masked of the secrets read
/// with it, and its mode, never the secret (#386, #1137, I4). Each job,
/// snapshot listing and notification that reads secrets tells it; a re-read
/// that only masks a refusal doesn't (§10.2).
fn log_secret_warning(warning: &SecretWarning) {
    match warning {
        SecretWarning::FileReadableByOthers { path, mode } => log::warn!(
            "{:?}: other users of this machine can read the secret file {path} \
             (permissions {mode:04o}); run `chmod 600` on it so that only its \
             owner can read it",
            WarningCode::SecretFileReadable
        ),
        SecretWarning::FileWritableByOthers { path, mode } => log::warn!(
            "{:?}: other users of this machine can change the secret file {path} \
             (permissions {mode:04o}), so they can choose the secret read from \
             it; run `chmod 600` on it so that only its owner can read or change it",
            WarningCode::SecretFileWritable
        ),
    }
}

/// A secret that couldn't be resolved, as the typed refusal of its cause
/// ([`ResolveError::refusal`]). What a platform or a command said is masked
/// of every secret of the repository first (I4), read again through
/// `resolver`. A refusal that shows nothing a source said reads nothing: a
/// location with too many credentials, or a reference that would reach a
/// process's arguments, is refused before any secret is read (§10.2).
async fn secret_refusal(
    error: &ResolveError,
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> SecretRefusal {
    refusal_with(error, &refusal_redactor(error, settings, resolver).await)
}

/// [`secret_refusal`], with what `redactor` masks.
fn refusal_with(error: &ResolveError, redactor: &Redactor) -> SecretRefusal {
    match error.refusal(redactor) {
        Ok(refusal) => {
            let (code, args, detail) = app::announce::refused(&error.field.name(), &refusal);
            SecretRefusal { code, args, detail }
        }
        Err(limit) => too_much_to_mask(Some(error.field.name()), limit),
    }
}

/// What masks the texts of `error`'s refusal: only a secret that couldn't be
/// read has any, where it was looked for and what was said, so only then are
/// the repository's secrets read again ([`failure_redactor`]).
async fn refusal_redactor(
    error: &ResolveError,
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> Redactor {
    match error.cause {
        Cause::Unresolved(_) => failure_redactor(settings, resolver).await,
        Cause::Invalid(_)
        | Cause::ExpansionTooLong { .. }
        | Cause::ReferenceInProcessArguments
        | Cause::TooManyCredentials { .. }
        | Cause::TooMuchToMask { .. }
        | Cause::RelativePath => Redactor::new([]),
    }
}

/// How long a secret is read again for a redactor: a source that does not
/// answer in this time adds nothing, and a failure doesn't wait for it.
const REREAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A secret read again, for a redactor: `None` when it can't be read, since
/// what can't be read can't show in what a failure says.
async fn read_again(resolver: &dyn SecretResolver, secret: &SecretRef) -> Option<SecretValue> {
    tokio::time::timeout(REREAD_LIMIT, resolver.resolve(secret))
        .await
        .ok()?
        .ok()
        .map(|found| found.value)
}

/// A redactor for what a failure of `settings` says: its inline password and
/// credential options, and what the references of its settings, and a
/// password read from the environment or a file, resolve to through
/// `resolver`. A failed resolution stops at the first fault, so the other
/// secrets of the repository were not read yet: they are read again here, so
/// that a command that prints one of them in its error doesn't show it. A
/// command is not run a second time. Once what was read again passes what a
/// redactor masks, nothing more is read and the redactor masks every text
/// whole, since a secret not read could be in it (I4).
async fn failure_redactor(
    settings: &RepositorySettings,
    resolver: &dyn SecretResolver,
) -> Redactor {
    let mut resolved = Vec::new();
    let mut masked = MaskedSecrets::default();
    if matches!(
        settings.password,
        SecretRef::Env(_) | SecretRef::File { .. }
    ) && let Some(value) = read_again(resolver, &settings.password).await
    {
        if masked.secret(&value).is_err() {
            return Redactor::masking_everything();
        }
        resolved.push(value);
    }
    let texts = std::iter::once(&settings.repository)
        .chain(&settings.repo_hot)
        .chain(settings.options.values())
        .chain(settings.options_hot.values())
        .chain(settings.options_cold.values());
    // Each distinct reference once, as the resolution reads it.
    let mut references = std::collections::BTreeSet::new();
    for text in texts {
        if let Ok(template) = Template::parse(text) {
            for part in template.parts() {
                if let Part::Reference(reference) = part {
                    references.insert(reference.clone());
                }
            }
        }
    }
    for reference in references {
        if let Some(value) = read_again(resolver, &reference.secret_ref()).await {
            if masked.secret(&value).is_err() {
                return Redactor::masking_everything();
            }
            resolved.push(value);
        }
    }
    let none = SecretValue::new("");
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: settings.password.inline().unwrap_or(&none),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &resolved,
    })
}

/// A configuration that couldn't be read or written, as the code and the
/// arguments its message declares, which `unavailable` makes a route's
/// refusal of: every route that reads the file says the same of it (#347).
/// The store never puts a value in what it reports (`ConfigError`), so its
/// words can be shown.
fn config_unusable<R>(
    error: ConfigError,
    unavailable: impl FnOnce(ErrorCode, Vec<(&'static str, MsgArg)>, Option<String>) -> R,
) -> R {
    match error {
        ConfigError::Unavailable { file, detail } => unavailable(
            ErrorCode::ConfigUnavailable,
            vec![("file", text(file))],
            Some(detail),
        ),
        ConfigError::TooLarge { file, limit } => unavailable(
            ErrorCode::ConfigTooLarge,
            vec![("file", text(file)), ("limit", MsgArg::Bytes(limit))],
            None,
        ),
        ConfigError::SchemaMissing { file } => unavailable(
            ErrorCode::ConfigSchemaMissing,
            vec![("file", text(file))],
            None,
        ),
        ConfigError::SchemaNewer {
            file,
            found,
            supported,
        } => unavailable(
            ErrorCode::ConfigSchemaNewer,
            vec![
                ("file", text(file)),
                ("found", text(found)),
                ("supported", text(supported)),
            ],
            None,
        ),
        ConfigError::Invalid {
            file,
            key,
            at,
            detail,
            ..
        } => {
            let mut args = vec![("file", text(file))];
            if let Some(at) = at {
                args.push(("line", text(at.line)));
            }
            let detail = match (key, detail.is_empty()) {
                (Some(key), false) => Some(format!("{key}: {detail}")),
                (Some(key), true) => Some(key),
                (None, false) => Some(detail),
                (None, true) => None,
            };
            unavailable(ErrorCode::ConfigInvalid, args, detail)
        }
    }
}

/// A location of `settings`, as it may be shown: with the password a URL
/// holds masked. The typed password is not masked: it isn't in a location,
/// and a location that happens to spell it is still a location.
fn redacted_location(settings: &RepositorySettings, text: &str) -> String {
    Redactor::for_repository(RepositorySecrets {
        repository: &settings.repository,
        repo_hot: settings.repo_hot.as_deref(),
        password: &SecretValue::new(""),
        options: &settings.options,
        options_hot: &settings.options_hot,
        options_cold: &settings.options_cold,
        resolved: &[],
    })
    .redact(text)
}

impl CancelRegistry {
    fn new() -> Self {
        Self {
            running: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The map, recovered even from a poisoned lock: the alternative —
    /// handing out a receiver whose sender is gone — would cancel every
    /// operation that starts next.
    fn running(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<OperationId, futures::channel::oneshot::Sender<()>>>
    {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers `id` as running, returning the future its run ends on when
    /// the API cancels it.
    fn register(&self, id: OperationId) -> futures::channel::oneshot::Receiver<()> {
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.running().insert(id, sender);
        receiver
    }

    /// Cancels `id`: `true` when it was running and is now cancelled, `false`
    /// when nothing by that ID is running, so there is nothing to cancel.
    /// A second cancel of the same operation is `false`: the one handle is
    /// spent.
    fn cancel(&self, id: &OperationId) -> bool {
        self.running()
            .remove(id)
            .is_some_and(|sender| sender.send(()).is_ok())
    }

    /// The operation ended, cancelled or not: its handle, fired or spent, is
    /// dropped, so a later cancel is a typed refusal.
    fn finish(&self, id: &OperationId) {
        let _ = self.running().remove(id);
    }
}

/// What the daemon logs, for the tests that look at the log: the warnings it
/// gives (`lines`) and what it says at info level (`infos`). `log` takes one
/// logger for a process, so the tests of every module that reads the log
/// share this one, and each reads only the lines about what it started.
#[cfg(test)]
mod captured_log {
    use std::sync::{Mutex, PoisonError};

    struct Captured;

    /// Each record's level, target and text.
    static LOGGED: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

    impl log::Log for Captured {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            LOGGED.lock().unwrap_or_else(PoisonError::into_inner).push((
                record.level(),
                record.target().to_owned(),
                record.args().to_string(),
            ));
        }
        fn flush(&self) {}
    }

    /// Starts capturing, before the code that logs runs, at info level or
    /// more, as another test of the process may have asked.
    pub(super) fn capture() {
        capture_at(log::LevelFilter::Info);
    }

    /// Starts capturing at `level` or more: a test that asks for less never
    /// lowers what another test of the process asked for.
    pub(super) fn capture_at(level: log::LevelFilter) {
        let _ = log::set_logger(&Captured);
        log::set_max_level(log::max_level().max(level));
    }

    /// Every line logged so far at warning level or above, with its level.
    pub(super) fn lines() -> Vec<(log::Level, String)> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, _, _)| *level <= log::Level::Warn)
            .map(|(level, _, line)| (*level, line.clone()))
            .collect()
    }

    /// Every line logged so far at info level.
    pub(super) fn infos() -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, _, _)| *level == log::Level::Info)
            .map(|(_, _, line)| line.clone())
            .collect()
    }

    /// The targets of every record logged so far at `level`.
    pub(super) fn targets_at(level: log::Level) -> Vec<String> {
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(at, _, _)| *at == level)
            .map(|(_, target, _)| target.clone())
            .collect()
    }

    /// Every line logged so far that the daemon's log, under `RUST_LOG` set
    /// to `rust_log`, would write.
    pub(super) fn written_under(rust_log: &str) -> Vec<String> {
        use log::Log as _;
        let daemon = crate::logging::logger(
            Some(rust_log),
            env_logger::Target::Pipe(Box::new(std::io::sink())),
            env_logger::WriteStyle::Never,
        );
        LOGGED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(level, target, _)| {
                daemon.enabled(
                    &log::Metadata::builder()
                        .level(*level)
                        .target(target)
                        .build(),
                )
            })
            .map(|(_, _, line)| line.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
