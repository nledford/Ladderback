//! How the database spells the log's enums, failures and warning kinds.
//!
//! The words are this store's own, equal today to `api`'s kebab-case codes but
//! not tied to them: a database outlives the API's names. Writing matches
//! every variant of the closed type, so a new one doesn't compile until it has
//! a spelling; reading is strict, so a spelling this version never wrote is
//! reported, not guessed at.

use api::MsgArg;
use domain::execution::{
    FailureReason, OperationKind, OperationStatus, Transition, Trigger, WarningKind,
};

use crate::error_args;
use crate::report::shown;

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
    }
}

pub(crate) fn kind_from_text(text: &str) -> Option<OperationKind> {
    match text {
        "fake" => Some(OperationKind::Fake),
        "identify" => Some(OperationKind::Identify),
        "init" => Some(OperationKind::Init),
        "backup" => Some(OperationKind::Backup),
        "forget" => Some(OperationKind::Forget),
        "adopt" => Some(OperationKind::Adopt),
        "prune" => Some(OperationKind::Prune),
        "check" => Some(OperationKind::Check),
        "ls" => Some(OperationKind::Ls),
        _ => None,
    }
}

pub(crate) fn trigger_text(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Schedule => "schedule",
        Trigger::Manual => "manual",
        Trigger::CatchUp => "catch-up",
        Trigger::Retry => "retry",
        Trigger::Hub => "hub",
        Trigger::Mcp => "mcp",
    }
}

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

pub(crate) fn warning_code_text(kind: WarningKind) -> &'static str {
    match kind {
        WarningKind::SourceUnreadable => "source-unreadable",
        WarningKind::Hook => "hook-warning",
        WarningKind::MaintenancePartial => "maintenance-partial",
        WarningKind::CheckWarning => "check-warning",
        WarningKind::CheckError => "check-error",
        WarningKind::OtherHosts => "maintenance-other-hosts",
        WarningKind::Other => "unknown",
    }
}

/// A code this version doesn't know is a warning of another kind, as the
/// domain has it: a newer worker may have written it.
pub(crate) fn warning_kind_from_text(text: &str) -> WarningKind {
    match text {
        "source-unreadable" => WarningKind::SourceUnreadable,
        "hook-warning" => WarningKind::Hook,
        "maintenance-partial" => WarningKind::MaintenancePartial,
        "check-warning" => WarningKind::CheckWarning,
        "check-error" => WarningKind::CheckError,
        "maintenance-other-hosts" => WarningKind::OtherHosts,
        _ => WarningKind::Other,
    }
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
            exit_code
                .map(|code| text_arg("exit_code", code.to_string()))
                .unwrap_or_default(),
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
        FailureReason::EngineStartTimedOut => ("engine-start-timed-out", Vec::new(), None),
        FailureReason::EngineExitTimedOut => ("engine-exit-timed-out", Vec::new(), None),
        FailureReason::HookFailed { exit_code } => (
            "hook-failed",
            exit_code
                .map(|code| text_arg("exit_code", code.to_string()))
                .unwrap_or_default(),
            None,
        ),
        FailureReason::DaemonStopped => ("daemon-stopped", Vec::new(), None),
    };
    Ok(FailureColumns {
        code,
        args: error_args::encode(&args)?,
        detail,
    })
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
        "engine-start-timed-out" => Ok(FailureReason::EngineStartTimedOut),
        "engine-exit-timed-out" => Ok(FailureReason::EngineExitTimedOut),
        "daemon-stopped" => Ok(FailureReason::DaemonStopped),
        "hook-failed" => Ok(FailureReason::HookFailed {
            exit_code: exit_code(&args)?,
        }),
        other => Err(bad(
            "error_code",
            format!("{} is not an error code of this version", shown(other)),
        )),
    }
}

fn argument<'a>(args: &'a [(String, MsgArg)], name: &str) -> Option<&'a MsgArg> {
    args.iter()
        .find(|(arg, _)| arg == name)
        .map(|(_, value)| value)
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

    /// Every kind round-trips: what the log writes for an operation is what
    /// it reads back, `init` included.
    #[test]
    fn every_kind_round_trips() {
        for kind in [
            OperationKind::Fake,
            OperationKind::Identify,
            OperationKind::Init,
            OperationKind::Backup,
            OperationKind::Prune,
            OperationKind::Check,
        ] {
            let text = kind_text(kind);
            assert_eq!(kind_from_text(text), Some(kind), "{text} reads back");
        }
    }

    /// Every kind of warning round-trips, a partial maintenance run and other
    /// hosts included.
    #[test]
    fn every_warning_kind_round_trips() {
        for kind in [
            WarningKind::SourceUnreadable,
            WarningKind::Hook,
            WarningKind::MaintenancePartial,
            WarningKind::OtherHosts,
            WarningKind::Other,
        ] {
            let text = warning_code_text(kind);
            assert_eq!(warning_kind_from_text(text), kind, "{text} reads back");
        }
    }

    /// A check's findings are kept under the codes the wire names them by,
    /// so a log written by this version reads back as what the check found.
    #[test]
    fn a_checks_findings_round_trip_under_their_wire_codes() {
        for (kind, text) in [
            (WarningKind::CheckWarning, "check-warning"),
            (WarningKind::CheckError, "check-error"),
        ] {
            assert_eq!(warning_code_text(kind), text);
            assert_eq!(warning_kind_from_text(text), kind, "{text} reads back");
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
}
