//! How the database spells the log's enums, failures and warning kinds.
//!
//! The words are this store's own, equal today to `api`'s kebab-case codes but
//! not tied to them: a database outlives the API's names. Writing matches
//! every variant of the closed type, so a new one doesn't compile until it has
//! a spelling. Reading is strict, so a spelling this version never wrote is
//! reported, not guessed at, with one exception: a warning code this version
//! doesn't know is a warning of another kind ([`WarningKind::Other`]), since a
//! newer worker may have written it.
//!
//! An operation's kind and a warning's code are each written and read by one
//! spelling, the writer's (`kind_text` here, [`WarningKind::code`] in the
//! domain): reading goes through a table of the kinds (`KINDS`,
//! `WARNING_KINDS`) by that same spelling, so the two directions can't drift
//! apart. The tests walk every variant through both. The SQL behind I7 names
//! the maintenance kinds again (`sql.rs`, the `one_maintenance_per_repo`
//! index; §9).

use std::num::NonZeroU32;

use api::MsgArg;
use domain::execution::refusal::SettingRefusal;
use domain::execution::{
    FailureReason, HookEnd, HookLevel, HookPlace, OperationKind, OperationStatus, RepositoryHalf,
    Transition, Trigger, WarningKind,
};

use crate::error_args;
use crate::report::shown;

/// The word the log's `kind` column stores `kind` under, which
/// [`kind_from_text`] reads by too.
pub(crate) fn kind_text(kind: OperationKind) -> &'static str {
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

/// Every operation kind, the table the `kind` column is read through. `Ls` is
/// in it though a listing is a read the log never writes (§9): a stored `ls`
/// reads back as one, as it always has.
const KINDS: [OperationKind; 11] = [
    OperationKind::Fake,
    OperationKind::Identify,
    OperationKind::Init,
    OperationKind::Backup,
    OperationKind::Forget,
    OperationKind::Adopt,
    OperationKind::Prune,
    OperationKind::Check,
    OperationKind::Ls,
    OperationKind::Restore,
    OperationKind::Download,
];

/// The kind [`kind_text`] writes as `text`; none for a word no kind is
/// written as, which the reader reports as corrupt.
pub(crate) fn kind_from_text(text: &str) -> Option<OperationKind> {
    KINDS.into_iter().find(|&kind| kind_text(kind) == text)
}

/// The trigger column's word for an operation an API token started: the one
/// trigger that carries more than a word, in `token_id` and `token_label`.
pub(crate) const API_TOKEN: &str = "api-token";

pub(crate) fn trigger_text(trigger: &Trigger) -> &'static str {
    match trigger {
        Trigger::Schedule => "schedule",
        Trigger::Manual => "manual",
        Trigger::CatchUp => "catch-up",
        Trigger::Retry => "retry",
        Trigger::Hub => "hub",
        Trigger::Mcp => "mcp",
        Trigger::ApiToken { .. } => API_TOKEN,
    }
}

/// A trigger that is a word alone: every one but [`API_TOKEN`].
pub(crate) fn trigger_from_text(text: &str) -> Option<Trigger> {
    match text {
        "schedule" => Some(Trigger::Schedule),
        "manual" => Some(Trigger::Manual),
        "catch-up" => Some(Trigger::CatchUp),
        "retry" => Some(Trigger::Retry),
        "hub" => Some(Trigger::Hub),
        "mcp" => Some(Trigger::Mcp),
        _ => None,
    }
}

/// The status column's word for an operation that failed: the one status that
/// carries more than a word, so the others are read apart from it.
pub(crate) const FAILED: &str = "failed";

pub(crate) fn status_text(status: &OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Succeeded => "succeeded",
        OperationStatus::SucceededWithWarnings => "succeeded_with_warnings",
        OperationStatus::Failed { .. } => FAILED,
        OperationStatus::Cancelled => "cancelled",
    }
}

/// Any status but [`FAILED`], which needs its reason.
pub(crate) fn unfailed_status_from_text(text: &str) -> Option<OperationStatus> {
    match text {
        "queued" => Some(OperationStatus::Queued),
        "running" => Some(OperationStatus::Running),
        "succeeded" => Some(OperationStatus::Succeeded),
        "succeeded_with_warnings" => Some(OperationStatus::SucceededWithWarnings),
        "cancelled" => Some(OperationStatus::Cancelled),
        _ => None,
    }
}

/// Every kind of warning but [`WarningKind::Other`], the table the
/// `op_warnings.code` column is read through, each by the code the domain
/// writes for it ([`WarningKind::code`]).
const WARNING_KINDS: [WarningKind; 10] = [
    WarningKind::SourceUnreadable,
    WarningKind::Hook,
    WarningKind::MaintenancePartial,
    WarningKind::CheckWarning,
    WarningKind::CheckError,
    WarningKind::OtherHosts,
    WarningKind::LocationSkipped,
    WarningKind::SecretFileReadable,
    WarningKind::SecretFileWritable,
    WarningKind::FullDiskAccessMissing,
];

/// The kind of warning stored as `code`. A code this version doesn't know,
/// `unknown` included, is a warning of another kind, as the domain has it: a
/// newer worker may have written it.
pub(crate) fn warning_kind_from_text(code: &str) -> WarningKind {
    WARNING_KINDS
        .into_iter()
        .find(|kind| kind.code() == code)
        .unwrap_or(WarningKind::Other)
}

fn transition_text(transition: Transition) -> &'static str {
    match transition {
        Transition::Start => "start",
        Transition::ReportProgress => "report-progress",
        Transition::RecordWarning => "record-warning",
        Transition::Succeed => "succeed",
        Transition::Fail => "fail",
        Transition::Cancel => "cancel",
    }
}

fn transition_from_text(text: &str) -> Option<Transition> {
    match text {
        "start" => Some(Transition::Start),
        "report-progress" => Some(Transition::ReportProgress),
        "record-warning" => Some(Transition::RecordWarning),
        "succeed" => Some(Transition::Succeed),
        "fail" => Some(Transition::Fail),
        "cancel" => Some(Transition::Cancel),
        _ => None,
    }
}

/// The three columns a failure is stored in.
pub(crate) struct FailureColumns<'a> {
    /// `error_code`.
    pub code: &'static str,
    /// `error_args`: a JSON array of `[name, argument]` pairs, `[]` for none.
    pub args: String,
    /// `error_detail`: the engine's or platform's own text, when the reason has one.
    pub detail: Option<&'a str>,
}

pub(crate) fn encode_failure(
    reason: &FailureReason,
) -> Result<FailureColumns<'_>, serde_json::Error> {
    let text_arg = |name, value: String| vec![(name, MsgArg::Text(value))];
    let (code, args, detail): (&'static str, Vec<(&str, MsgArg)>, Option<&str>) = match reason {
        FailureReason::EngineUnavailable { detail } => {
            ("engine-unavailable", Vec::new(), Some(detail))
        }
        FailureReason::JobRejected { detail } => ("invalid-job-spec", Vec::new(), Some(detail)),
        FailureReason::EngineExited { exit_code } => (
            "engine-exited",
            exit_code.map_or_default(|code| text_arg("exit_code", code.to_string())),
            None,
        ),
        FailureReason::EngineFailed { detail } => ("operation-failed", Vec::new(), Some(detail)),
        FailureReason::ProtocolViolated { transition } => (
            "protocol-violated",
            text_arg("transition", transition_text(*transition).to_owned()),
            None,
        ),
        FailureReason::DeadlinePassed => ("deadline-passed", Vec::new(), None),
        FailureReason::RepositoryExists => ("repository-exists", Vec::new(), None),
        FailureReason::RepositoryReplaced => ("repository-replaced", Vec::new(), None),
        FailureReason::RepositoryNotFound => ("repository-not-found", Vec::new(), None),
        FailureReason::RepositoryConfigNotFound { detail } => {
            ("repository-config-not-found", Vec::new(), Some(detail))
        }
        FailureReason::CredentialsRefused { half } => (
            "credentials-refused",
            half.map_or_default(|half| text_arg("half", half.code().to_owned())),
            None,
        ),
        FailureReason::EngineStartTimedOut => ("engine-start-timed-out", Vec::new(), None),
        FailureReason::EngineExitTimedOut => ("engine-exit-timed-out", Vec::new(), None),
        FailureReason::HookFailed { hook, ended } => {
            ("hook-failed", hook_failed_args(*hook, *ended), None)
        }
        FailureReason::SecretsTooLargeToMask { field, limit } => (
            "secrets-too-large-to-mask",
            std::iter::once((
                "limit",
                MsgArg::Bytes(u64::try_from(*limit).unwrap_or(u64::MAX)),
            ))
            .chain(
                field
                    .as_deref()
                    .map(|field| ("field", MsgArg::Text(field.to_owned()))),
            )
            .collect(),
            None,
        ),
        FailureReason::DaemonStopped => ("daemon-stopped", Vec::new(), None),
        FailureReason::DaemonShutDown => ("daemon-shut-down", Vec::new(), None),
        FailureReason::Superseded => ("superseded", Vec::new(), None),
        FailureReason::PlanRemoved => ("plan-removed", Vec::new(), None),
        FailureReason::PlanMoved => ("plan-moved", Vec::new(), None),
        FailureReason::ConfigUnusable => ("config-unusable", Vec::new(), None),
        FailureReason::KeyFilesNotLeftOut => ("key-files-not-left-out", Vec::new(), None),
        FailureReason::NoLocationUsable => ("no-location-usable", Vec::new(), None),
        FailureReason::RestoreTargetUnwritable => ("restore-target-unwritable", Vec::new(), None),
        FailureReason::NoLocationRuleHolds => ("no-location-rule-holds", Vec::new(), None),
        FailureReason::PasswordNotAccepted => ("password-not-accepted", Vec::new(), None),
        FailureReason::SecretRefused { field, refusal } => (
            refusal.code(),
            std::iter::once(("field", MsgArg::Text(field.clone())))
                .chain(refusal_args(refusal))
                .collect(),
            refusal_detail(refusal),
        ),
    };
    Ok(FailureColumns {
        code,
        args: error_args::encode(&args)?,
        detail,
    })
}

/// What a refusal keeps beside its `field`: where the secret was looked
/// for, and its number, by name.
fn refusal_args(refusal: &SettingRefusal) -> Vec<(&'static str, MsgArg)> {
    use SettingRefusal as R;
    let source = |source: &str| ("source", MsgArg::Text(source.to_owned()));
    match refusal {
        R::SecretMissing { source: from }
        | R::SecretUnreadable { source: from, .. }
        | R::SecretKeyringUnavailable { source: from, .. }
        | R::SecretKeyringLocked { source: from, .. }
        | R::SecretNotText { source: from }
        | R::SecretCommandFailed { source: from, .. }
        | R::SecretCommandOutputOpen { source: from } => vec![source(from)],
        R::SecretTooLarge {
            source: from,
            limit,
        } => vec![source(from), ("limit", MsgArg::Bytes(*limit))],
        R::SecretCommandTimedOut {
            source: from,
            seconds,
        }
        | R::SecretKeyringTimedOut {
            source: from,
            seconds,
        }
        | R::SecretFileTimedOut {
            source: from,
            seconds,
        } => vec![source(from), ("seconds", MsgArg::Count(*seconds))],
        R::SecretExpansionTooLong { limit } => vec![("limit", MsgArg::Bytes(*limit))],
        R::TemplateInvalid { at } => at
            .map(|at| ("at", MsgArg::Text(at.to_string())))
            .into_iter()
            .collect(),
        R::SecretInProcessArguments | R::LocationTooManyCredentials | R::RepositoryPathRelative => {
            Vec::new()
        }
    }
}

/// What a platform or a command said of a refused secret, masked before it
/// was kept.
fn refusal_detail(refusal: &SettingRefusal) -> Option<&str> {
    use SettingRefusal as R;
    match refusal {
        R::SecretUnreadable { detail, .. }
        | R::SecretKeyringUnavailable { detail, .. }
        | R::SecretKeyringLocked { detail, .. }
        | R::SecretCommandFailed { detail, .. } => detail.as_deref(),
        _ => None,
    }
}

/// A column of a failed operation that can't be read, and why.
pub(crate) struct BadColumn {
    pub column: &'static str,
    pub why: String,
}

fn bad(column: &'static str, why: impl Into<String>) -> BadColumn {
    BadColumn {
        column,
        why: why.into(),
    }
}

/// The reason a failed operation's three columns stand for.
pub(crate) fn decode_failure(
    code: Option<&str>,
    args: Option<&str>,
    detail: Option<&str>,
) -> Result<FailureReason, BadColumn> {
    let code = code.ok_or_else(|| bad("error_code", "is NULL on a failed operation"))?;
    let args = args.ok_or_else(|| bad("error_args", "is NULL on a failed operation"))?;
    let args = error_args::decode(args).map_err(|error| bad("error_args", error.to_string()))?;
    let said = detail;
    let detail = || {
        detail
            .map(str::to_owned)
            .ok_or_else(|| bad("error_detail", format!("is NULL, but {code} has a detail")))
    };
    match code {
        "engine-unavailable" => Ok(FailureReason::EngineUnavailable { detail: detail()? }),
        "invalid-job-spec" => Ok(FailureReason::JobRejected { detail: detail()? }),
        "operation-failed" => Ok(FailureReason::EngineFailed { detail: detail()? }),
        "engine-exited" => Ok(FailureReason::EngineExited {
            exit_code: exit_code(&args)?,
        }),
        "protocol-violated" => Ok(FailureReason::ProtocolViolated {
            transition: transition(&args)?,
        }),
        "deadline-passed" => Ok(FailureReason::DeadlinePassed),
        "repository-exists" => Ok(FailureReason::RepositoryExists),
        "repository-replaced" => Ok(FailureReason::RepositoryReplaced),
        "repository-not-found" => Ok(FailureReason::RepositoryNotFound),
        "repository-config-not-found" => {
            Ok(FailureReason::RepositoryConfigNotFound { detail: detail()? })
        }
        "credentials-refused" => Ok(FailureReason::CredentialsRefused { half: half(&args)? }),
        "engine-start-timed-out" => Ok(FailureReason::EngineStartTimedOut),
        "engine-exit-timed-out" => Ok(FailureReason::EngineExitTimedOut),
        "daemon-stopped" => Ok(FailureReason::DaemonStopped),
        "daemon-shut-down" => Ok(FailureReason::DaemonShutDown),
        "superseded" => Ok(FailureReason::Superseded),
        "plan-removed" => Ok(FailureReason::PlanRemoved),
        "plan-moved" => Ok(FailureReason::PlanMoved),
        "config-unusable" => Ok(FailureReason::ConfigUnusable),
        "key-files-not-left-out" => Ok(FailureReason::KeyFilesNotLeftOut),
        "no-location-usable" => Ok(FailureReason::NoLocationUsable),
        "restore-target-unwritable" => Ok(FailureReason::RestoreTargetUnwritable),
        "no-location-rule-holds" => Ok(FailureReason::NoLocationRuleHolds),
        "password-not-accepted" => Ok(FailureReason::PasswordNotAccepted),
        "hook-failed" => Ok(FailureReason::HookFailed {
            hook: hook_place(&args)?,
            ended: hook_end(&args)?,
        }),
        "secrets-too-large-to-mask" => Ok(FailureReason::SecretsTooLargeToMask {
            field: field(&args)?,
            limit: limit(&args)?,
        }),
        other => match refused(other, &args, said)? {
            Some(refusal) => Ok(FailureReason::SecretRefused {
                field: text(&args, "field")?,
                refusal,
            }),
            None => Err(bad(
                "error_code",
                format!("{} is not an error code of this version", shown(other)),
            )),
        },
    }
}

/// The refusal `code` names, read from its arguments and detail, or `None`
/// when the code is no refusal's.
fn refused(
    code: &str,
    args: &[(String, MsgArg)],
    detail: Option<&str>,
) -> Result<Option<SettingRefusal>, BadColumn> {
    use SettingRefusal as R;
    let source = || text(args, "source");
    let said = detail.map(str::to_owned);
    Ok(Some(match code {
        "secret-missing" => R::SecretMissing { source: source()? },
        "secret-unreadable" => R::SecretUnreadable {
            source: source()?,
            detail: said,
        },
        "secret-keyring-unavailable" => R::SecretKeyringUnavailable {
            source: source()?,
            detail: said,
        },
        "secret-keyring-locked" => R::SecretKeyringLocked {
            source: source()?,
            detail: said,
        },
        "secret-not-text" => R::SecretNotText { source: source()? },
        "secret-too-large" => R::SecretTooLarge {
            source: source()?,
            limit: bytes(args, "limit")?,
        },
        "secret-command-failed" => R::SecretCommandFailed {
            source: source()?,
            detail: said,
        },
        "secret-command-timed-out" => R::SecretCommandTimedOut {
            source: source()?,
            seconds: count(args, "seconds")?,
        },
        "secret-keyring-timed-out" => R::SecretKeyringTimedOut {
            source: source()?,
            seconds: count(args, "seconds")?,
        },
        "secret-file-timed-out" => R::SecretFileTimedOut {
            source: source()?,
            seconds: count(args, "seconds")?,
        },
        "secret-command-output-open" => R::SecretCommandOutputOpen { source: source()? },
        "secret-in-process-arguments" => R::SecretInProcessArguments,
        "secret-expansion-too-long" => R::SecretExpansionTooLong {
            limit: bytes(args, "limit")?,
        },
        "location-too-many-credentials" => R::LocationTooManyCredentials,
        "repository-path-relative" => R::RepositoryPathRelative,
        "template-invalid" => {
            R::TemplateInvalid {
                at: match argument(args, "at") {
                    None => None,
                    Some(MsgArg::Text(at)) => Some(at.parse().map_err(|_| {
                        bad("error_args", format!("at {} is not a place", shown(at)))
                    })?),
                    Some(_) => return Err(bad("error_args", "at is not text")),
                },
            }
        }
        _ => return Ok(None),
    }))
}

/// The half a refusal of the credentials names, if any: `cold` or `hot`.
fn half(args: &[(String, MsgArg)]) -> Result<Option<RepositoryHalf>, BadColumn> {
    match argument(args, "half") {
        None => Ok(None),
        Some(MsgArg::Text(half)) => RepositoryHalf::parse(half)
            .map(Some)
            .ok_or_else(|| bad("error_args", format!("half {} is no half", shown(half)))),
        Some(_) => Err(bad("error_args", "half is not text")),
    }
}

/// A before-hook's failure as the store spells it: `level` and `position`
/// when the hook is known, `ended` when how it ended is, and `exit_code` for
/// an exit. [`hook_place`] and [`hook_end`] read it back.
fn hook_failed_args(
    hook: Option<HookPlace>,
    ended: Option<HookEnd>,
) -> Vec<(&'static str, MsgArg)> {
    let text = |name, value: String| (name, MsgArg::Text(value));
    let mut args = Vec::with_capacity(4);
    if let Some(hook) = hook {
        args.push(text("level", hook.level.code().to_owned()));
        args.push(text("position", hook.position.to_string()));
    }
    if let Some(ended) = ended {
        args.push(text("ended", ended.code().to_owned()));
        if let Some(code) = ended.exit_code() {
            args.push(text("exit_code", code.to_string()));
        }
    }
    args
}

/// Which hook stopped the operation: its level and its place in that
/// level's list, both or neither. A failure an earlier version kept has
/// neither.
fn hook_place(args: &[(String, MsgArg)]) -> Result<Option<HookPlace>, BadColumn> {
    let level = match argument(args, "level") {
        None => None,
        Some(MsgArg::Text(level)) => Some(HookLevel::parse(level).ok_or_else(|| {
            bad(
                "error_args",
                format!("level {} is no hook level", shown(level)),
            )
        })?),
        Some(_) => return Err(bad("error_args", "level is not text")),
    };
    let position = match argument(args, "position") {
        None => None,
        Some(MsgArg::Text(position)) => Some(
            position
                .parse::<NonZeroU32>()
                .ok()
                // Written as plain digits: a sign or a leading zero is no
                // row this version wrote.
                .filter(|place| place.to_string() == *position)
                .ok_or_else(|| {
                    bad(
                        "error_args",
                        format!("position {} is no place in a list", shown(position)),
                    )
                })?,
        ),
        Some(_) => return Err(bad("error_args", "position is not text")),
    };
    match (level, position) {
        (Some(level), Some(position)) => Ok(Some(HookPlace { level, position })),
        (None, None) => Ok(None),
        (Some(_), None) => Err(bad("error_args", "has a hook level without its place")),
        (None, Some(_)) => Err(bad("error_args", "has a hook place without its level")),
    }
}

/// How the hook ended. A failure an earlier version kept says no more than
/// its exit status, if it had one.
fn hook_end(args: &[(String, MsgArg)]) -> Result<Option<HookEnd>, BadColumn> {
    let exit_code = exit_code(args)?;
    match argument(args, "ended") {
        None => Ok(exit_code.map(|code| HookEnd::Exited { code })),
        Some(MsgArg::Text(ended)) => HookEnd::parse(ended, exit_code).map(Some).ok_or_else(|| {
            bad(
                "error_args",
                format!(
                    "ended {} is no hook end with exit_code {exit_code:?}",
                    shown(ended)
                ),
            )
        }),
        Some(_) => Err(bad("error_args", "ended is not text")),
    }
}

/// The text argument `name`, which the failure must have.
fn text(args: &[(String, MsgArg)], name: &str) -> Result<String, BadColumn> {
    match argument(args, name) {
        None => Err(bad("error_args", format!("has no {name}"))),
        Some(MsgArg::Text(text)) => Ok(text.clone()),
        Some(_) => Err(bad("error_args", format!("{name} is not text"))),
    }
}

/// The number of bytes `name`, which the failure must have.
fn bytes(args: &[(String, MsgArg)], name: &str) -> Result<u64, BadColumn> {
    match argument(args, name) {
        None => Err(bad("error_args", format!("has no {name}"))),
        Some(MsgArg::Bytes(bytes)) => Ok(*bytes),
        Some(_) => Err(bad(
            "error_args",
            format!("{name} is not a number of bytes"),
        )),
    }
}

/// The count `name`, which the failure must have.
fn count(args: &[(String, MsgArg)], name: &str) -> Result<u64, BadColumn> {
    match argument(args, name) {
        None => Err(bad("error_args", format!("has no {name}"))),
        Some(MsgArg::Count(count)) => Ok(*count),
        Some(_) => Err(bad("error_args", format!("{name} is not a count"))),
    }
}

fn argument<'a>(args: &'a [(String, MsgArg)], name: &str) -> Option<&'a MsgArg> {
    args.iter()
        .find(|(arg, _)| arg == name)
        .map(|(_, value)| value)
}

/// `field` is left out when the job's own secrets passed the cap.
fn field(args: &[(String, MsgArg)]) -> Result<Option<String>, BadColumn> {
    match argument(args, "field") {
        None => Ok(None),
        Some(MsgArg::Text(text)) => Ok(Some(text.clone())),
        Some(_) => Err(bad("error_args", "field is not text")),
    }
}

fn limit(args: &[(String, MsgArg)]) -> Result<usize, BadColumn> {
    match argument(args, "limit") {
        None => Err(bad("error_args", "has no limit")),
        Some(MsgArg::Bytes(bytes)) => usize::try_from(*bytes)
            .map_err(|_| bad("error_args", format!("limit {bytes} is out of range"))),
        Some(_) => Err(bad("error_args", "limit is not a number of bytes")),
    }
}

/// `exit_code` is left out when no exit status is known.
fn exit_code(args: &[(String, MsgArg)]) -> Result<Option<i32>, BadColumn> {
    match argument(args, "exit_code") {
        None => Ok(None),
        Some(MsgArg::Text(text)) => text.parse().map(Some).map_err(|_| {
            bad(
                "error_args",
                format!("exit_code {} is not an exit status", shown(text)),
            )
        }),
        Some(_) => Err(bad("error_args", "exit_code is not text")),
    }
}

fn transition(args: &[(String, MsgArg)]) -> Result<Transition, BadColumn> {
    match argument(args, "transition") {
        None => Err(bad("error_args", "has no transition")),
        Some(MsgArg::Text(text)) => transition_from_text(text).ok_or_else(|| {
            bad(
                "error_args",
                format!("transition {} is not one this version knows", shown(text)),
            )
        }),
        Some(_) => Err(bad("error_args", "transition is not text")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant of a field-less enum, from its derived `Deserialize`,
    /// which names them all: a list nobody keeps by hand, so a variant added
    /// later is in it, whether or not a table has a row for it. A variant
    /// marked `#[serde(skip)]` or `skip_deserializing` isn't named, so it
    /// isn't walked; one with fields, or an enum that isn't derived as
    /// externally tagged, fails here.
    fn every<T: serde::de::DeserializeOwned>() -> Vec<T> {
        use serde::de::value::{Error, StrDeserializer};
        use serde::de::{Deserializer, Error as _, Visitor};

        /// Asked for an enum, keeps the names of its variants and reads nothing.
        struct Names(&'static [&'static str]);

        impl<'de> Deserializer<'de> for &mut Names {
            type Error = Error;

            fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Error> {
                Err(Error::custom("only an enum's variants are asked for"))
            }

            fn deserialize_enum<V: Visitor<'de>>(
                self,
                _: &'static str,
                variants: &'static [&'static str],
                _: V,
            ) -> Result<V::Value, Error> {
                self.0 = variants;
                Err(Error::custom("the variants' names are all that is read"))
            }

            serde::forward_to_deserialize_any! {
                bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
                bytes byte_buf option unit unit_struct newtype_struct seq tuple
                tuple_struct map struct identifier ignored_any
            }
        }

        let mut names = Names(&[]);
        assert!(T::deserialize(&mut names).is_err(), "nothing is read");
        assert!(!names.0.is_empty(), "the enum names its variants");
        names
            .0
            .iter()
            .map(
                |name| match T::deserialize(StrDeserializer::<Error>::new(name)) {
                    Ok(variant) => variant,
                    Err(error) => panic!("{name} is a variant without fields: {error}"),
                },
            )
            .collect()
    }

    /// The word the log stores each kind under, spelled here apart from the
    /// codec, through a match with no wildcard: a kind added later doesn't
    /// compile until it has a word here.
    fn stored_kind(kind: OperationKind) -> &'static str {
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

    /// Every kind is written under its own word and read back from it as
    /// that kind, so no stored operation becomes unreadable.
    #[test]
    fn every_kind_is_written_under_its_word_and_read_back_from_it() {
        for kind in every::<OperationKind>() {
            let word = stored_kind(kind);
            assert_eq!(kind_text(kind), word, "{kind:?} is written as {word}");
            assert_eq!(kind_from_text(word), Some(kind), "{word} reads back");
        }
    }

    /// A word no kind is written as is no kind: the reader reports the row as
    /// corrupt rather than guess at it.
    #[test]
    fn a_word_no_kind_is_written_as_reads_as_no_kind() {
        for word in ["", "restores", "Backup", " backup", "backup ", "unknown"] {
            assert_eq!(kind_from_text(word), None, "{word:?}");
        }
    }

    /// A listing is a read the log never writes (§9), yet a stored `ls`
    /// reads back as one, as it always has, and not as corruption.
    #[test]
    fn a_stored_listing_reads_back_as_one() {
        assert_eq!(kind_from_text("ls"), Some(OperationKind::Ls));
    }

    /// The code the log keeps each kind of warning under, spelled here apart
    /// from the domain, through a match with no wildcard: a kind added later
    /// doesn't compile until it has a code here.
    fn stored_warning(kind: WarningKind) -> &'static str {
        match kind {
            WarningKind::SourceUnreadable => "source-unreadable",
            WarningKind::Hook => "hook-warning",
            WarningKind::MaintenancePartial => "maintenance-partial",
            WarningKind::CheckWarning => "check-warning",
            WarningKind::CheckError => "check-error",
            WarningKind::OtherHosts => "maintenance-other-hosts",
            WarningKind::LocationSkipped => "location-skipped",
            WarningKind::SecretFileReadable => "secret-file-readable",
            WarningKind::SecretFileWritable => "secret-file-writable",
            WarningKind::FullDiskAccessMissing => "full-disk-access-missing",
            WarningKind::Other => "unknown",
        }
    }

    /// Every kind of warning is kept under its own code, the one the domain
    /// writes, and read back from it as that kind.
    #[test]
    fn every_warning_kind_is_kept_under_its_code_and_read_back_from_it() {
        for kind in every::<WarningKind>() {
            let code = stored_warning(kind);
            assert_eq!(kind.code(), code, "{kind:?} is kept as {code}");
            assert_eq!(warning_kind_from_text(code), kind, "{code} reads back");
        }
    }

    /// A code this version never wrote is a warning of another kind, not a
    /// corrupt row: a newer worker may have written it.
    #[test]
    fn a_warning_code_this_version_never_wrote_reads_as_another_kind() {
        for code in ["", "restore-skipped", "Hook-warning", "hook", "other"] {
            assert_eq!(warning_kind_from_text(code), WarningKind::Other, "{code:?}");
        }
    }

    /// A persisted `repository-exists` failure reads back as the typed
    /// refusal, both directions of the column spelling.
    #[test]
    fn repository_exists_round_trips() {
        let columns = encode_failure(&FailureReason::RepositoryExists)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "repository-exists");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::RepositoryExists);
    }

    /// A job refused for more secrets than it masks reads back with its
    /// limit and, when it named one, the setting where they passed it.
    #[test]
    fn secrets_too_large_to_mask_round_trips() {
        for field in [Some("HTTPS_PROXY".to_owned()), None] {
            let reason = FailureReason::SecretsTooLargeToMask {
                field,
                limit: 262_144,
            };
            let columns = encode_failure(&reason).expect("the failure encodes");
            assert_eq!(columns.code, "secrets-too-large-to-mask");
            assert_eq!(columns.detail, None);
            let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail)
            {
                Ok(reason) => reason,
                Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
            };
            assert_eq!(back, reason);
        }
    }

    /// A persisted `repository-replaced` failure reads back as the typed
    /// reason: no arguments, no detail.
    #[test]
    fn repository_replaced_round_trips() {
        let columns = encode_failure(&FailureReason::RepositoryReplaced)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "repository-replaced");
        assert_eq!(columns.args, "[]");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::RepositoryReplaced);
    }

    /// Encodes `reason` and decodes what it wrote.
    fn round_trip(reason: &FailureReason) -> FailureReason {
        let columns = encode_failure(reason).expect("the failure encodes");
        match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        }
    }

    /// A persisted `credentials-refused` failure reads back as the typed
    /// reason: no detail, and no argument unless a half is named.
    #[test]
    fn credentials_refused_round_trips() {
        let columns = encode_failure(&FailureReason::CredentialsRefused { half: None })
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "credentials-refused");
        assert_eq!(columns.args, "[]");
        assert_eq!(columns.detail, None);
        for half in [None, Some(RepositoryHalf::Cold), Some(RepositoryHalf::Hot)] {
            let reason = FailureReason::CredentialsRefused { half };
            assert_eq!(round_trip(&reason), reason);
        }
    }

    /// A half the log names that is neither `cold` nor `hot`, or isn't
    /// text, is a column that can't be read, never a refusal of another
    /// half or of none.
    #[test]
    fn credentials_refused_with_a_half_of_no_name_does_not_decode() {
        for half in [MsgArg::Text("warm".to_owned()), MsgArg::Count(1)] {
            let args = error_args::encode(&[("half", half)]).expect("arguments encode");
            let Err(bad) = decode_failure(Some("credentials-refused"), Some(&args), None) else {
                panic!("{args} decoded");
            };
            assert_eq!(bad.column, "error_args");
        }
    }

    /// A persisted `repository-config-not-found` failure reads back with the
    /// engine's masked words, which it must have.
    #[test]
    fn repository_config_not_found_round_trips_with_its_detail() {
        let reason = FailureReason::RepositoryConfigNotFound {
            detail: "no config file at local:/media/usb/restic".to_owned(),
        };
        let columns = encode_failure(&reason).expect("the failure encodes");
        assert_eq!(columns.code, "repository-config-not-found");
        assert_eq!(columns.args, "[]");
        assert_eq!(round_trip(&reason), reason);
        let Err(bad) = decode_failure(Some("repository-config-not-found"), Some("[]"), None) else {
            panic!("a failure with no detail decoded");
        };
        assert_eq!(bad.column, "error_detail");
    }

    /// A persisted `repository-not-found` failure reads back as the typed
    /// reason, which a saved repository's row reads (decision 53).
    #[test]
    fn repository_not_found_round_trips() {
        let columns = encode_failure(&FailureReason::RepositoryNotFound)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "repository-not-found");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::RepositoryNotFound);
    }

    /// A persisted `daemon-stopped` failure reads back as the typed reason:
    /// it has no arguments and no detail, and its columns say exactly that.
    #[test]
    fn daemon_stopped_round_trips() {
        let columns = encode_failure(&FailureReason::DaemonStopped)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "daemon-stopped");
        assert_eq!(columns.args, "[]");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::DaemonStopped);
    }

    /// A backup that gave way to a newer run of its plan reads back as
    /// superseded, not as a stopped daemon (#1245).
    #[test]
    fn superseded_round_trips() {
        let columns = encode_failure(&FailureReason::Superseded)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "superseded");
        assert_eq!(columns.args, "[]");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::Superseded);
    }

    /// A queued backup that waited for its keyring and never ran reads
    /// back with its own reason, not as a stopped daemon (#1293).
    #[test]
    fn the_reasons_a_waited_backup_never_ran_for_round_trip() {
        for (reason, code) in [
            (FailureReason::PlanRemoved, "plan-removed"),
            (FailureReason::PlanMoved, "plan-moved"),
            (FailureReason::ConfigUnusable, "config-unusable"),
            (FailureReason::KeyFilesNotLeftOut, "key-files-not-left-out"),
        ] {
            let columns = encode_failure(&reason).expect("a failure with no arguments encodes");
            assert_eq!(columns.code, code);
            assert_eq!(columns.args, "[]");
            assert_eq!(columns.detail, None);
            let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail)
            {
                Ok(reason) => reason,
                Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
            };
            assert_eq!(back, reason);
        }
    }

    /// A run the schedule started that was refused for a setting, for each
    /// refusal, with its texts as awkward as a source and a platform make
    /// them.
    fn refused_runs() -> Vec<FailureReason> {
        use SettingRefusal as R;
        let source = || "/home/demo/données ☃/\"pass\"\nword".to_owned();
        let said = || Some("line one\nline \"two\" ✓ 日本語".to_owned());
        [
            R::SecretMissing { source: source() },
            R::SecretUnreadable {
                source: source(),
                detail: said(),
            },
            R::SecretUnreadable {
                source: source(),
                detail: None,
            },
            R::SecretKeyringUnavailable {
                source: "ladderback/demo".to_owned(),
                detail: said(),
            },
            R::SecretKeyringUnavailable {
                source: "ladderback/demo".to_owned(),
                detail: None,
            },
            R::SecretKeyringLocked {
                source: "ladderback/demo".to_owned(),
                detail: said(),
            },
            R::SecretKeyringLocked {
                source: "ladderback/demo".to_owned(),
                detail: None,
            },
            R::SecretNotText { source: source() },
            R::SecretTooLarge {
                source: source(),
                limit: 65_536,
            },
            R::SecretCommandFailed {
                source: "pass".to_owned(),
                detail: said(),
            },
            R::SecretCommandFailed {
                source: "pass".to_owned(),
                detail: None,
            },
            R::SecretCommandTimedOut {
                source: "pass".to_owned(),
                seconds: 30,
            },
            R::SecretKeyringTimedOut {
                source: "ladderback/demo".to_owned(),
                seconds: 60,
            },
            R::SecretFileTimedOut {
                source: "/mnt/share/demo.password".to_owned(),
                seconds: 60,
            },
            R::SecretCommandOutputOpen {
                source: "pass".to_owned(),
            },
            R::SecretInProcessArguments,
            R::SecretExpansionTooLong { limit: 65_536 },
            R::LocationTooManyCredentials,
            R::RepositoryPathRelative,
            R::TemplateInvalid { at: Some(12) },
            R::TemplateInvalid { at: None },
        ]
        .into_iter()
        .map(|refusal| FailureReason::SecretRefused {
            field: "options-hot.token".to_owned(),
            refusal,
        })
        .collect()
    }

    /// Every refusal a run the schedule started records reads back as it
    /// was written, under the refusal's own code: the setting, where the
    /// secret was looked for, its number and what was said.
    #[test]
    fn every_refused_run_round_trips_under_its_refusals_code() {
        let mut codes = std::collections::BTreeSet::new();
        for reason in refused_runs() {
            let columns = match encode_failure(&reason) {
                Ok(columns) => columns,
                Err(error) => panic!("{reason:?} encodes: {error}"),
            };
            assert_eq!(columns.code, reason.code(), "{reason:?}");
            codes.insert(columns.code);
            let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail)
            {
                Ok(reason) => reason,
                Err(bad) => panic!("{reason:?} decodes: {} {}", bad.column, bad.why),
            };
            assert_eq!(back, reason);
        }
        assert_eq!(codes.len(), 16, "one code per refusal: {codes:?}");
    }

    /// A refusal's columns that lack what its code needs, or hold it as the
    /// wrong kind of argument, are reported, not guessed at.
    #[test]
    fn a_refusal_missing_an_argument_or_holding_the_wrong_kind_is_reported() {
        let args = |args: &[(&str, MsgArg)]| match error_args::encode(args) {
            Ok(text) => text,
            Err(error) => panic!("the arguments encode: {error}"),
        };
        let text = |text: &str| MsgArg::Text(text.to_owned());
        let cases = [
            (
                "secret-missing",
                args(&[("field", text("password"))]),
                "has no source",
            ),
            (
                "secret-missing",
                args(&[("source", text("DEMO"))]),
                "has no field",
            ),
            (
                "secret-missing",
                args(&[("field", text("password")), ("source", MsgArg::Count(1))]),
                "source is not text",
            ),
            (
                "secret-too-large",
                args(&[("field", text("password")), ("source", text("DEMO"))]),
                "has no limit",
            ),
            (
                "secret-expansion-too-long",
                args(&[("field", text("repository")), ("limit", MsgArg::Count(9))]),
                "limit is not a number of bytes",
            ),
            (
                "secret-command-timed-out",
                args(&[
                    ("field", text("password")),
                    ("source", text("pass")),
                    ("seconds", MsgArg::Bytes(30)),
                ]),
                "seconds is not a count",
            ),
            (
                "template-invalid",
                args(&[("field", text("repository")), ("at", text("twelve"))]),
                "at \"twelve\" is not a place",
            ),
            (
                "template-invalid",
                args(&[("field", text("repository")), ("at", MsgArg::Count(12))]),
                "at is not text",
            ),
        ];
        for (code, args, why) in cases {
            match decode_failure(Some(code), Some(&args), None) {
                Ok(reason) => panic!("{code} {args} decodes as {reason:?}"),
                Err(bad) => {
                    assert_eq!(bad.column, "error_args", "{code} {args}");
                    assert_eq!(bad.why, why, "{code} {args}");
                }
            }
        }
    }
}
