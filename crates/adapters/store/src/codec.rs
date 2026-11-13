//! How the database spells the log's enums, failures and warning kinds.
//!
//! The words are this store's own, equal today to `api`'s kebab-case codes but
//! not tied to them: a database outlives the API's names. Writing matches
//! every variant of the closed type, so a new one doesn't compile until it has
//! a spelling; reading is strict, so a spelling this version never wrote is
//! reported, not guessed at.

use api::MsgArg;
use domain::execution::refusal::SettingRefusal;
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
        WarningKind::LocationSkipped => "location-skipped",
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
        "location-skipped" => WarningKind::LocationSkipped,
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
        FailureReason::CredentialsRefused => ("credentials-refused", Vec::new(), None),
        FailureReason::EngineStartTimedOut => ("engine-start-timed-out", Vec::new(), None),
        FailureReason::EngineExitTimedOut => ("engine-exit-timed-out", Vec::new(), None),
        FailureReason::HookFailed { exit_code } => (
            "hook-failed",
            exit_code.map_or_default(|code| text_arg("exit_code", code.to_string())),
            None,
        ),
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
        FailureReason::NoLocationUsable => ("no-location-usable", Vec::new(), None),
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
        "credentials-refused" => Ok(FailureReason::CredentialsRefused),
        "engine-start-timed-out" => Ok(FailureReason::EngineStartTimedOut),
        "engine-exit-timed-out" => Ok(FailureReason::EngineExitTimedOut),
        "daemon-stopped" => Ok(FailureReason::DaemonStopped),
        "no-location-usable" => Ok(FailureReason::NoLocationUsable),
        "no-location-rule-holds" => Ok(FailureReason::NoLocationRuleHolds),
        "password-not-accepted" => Ok(FailureReason::PasswordNotAccepted),
        "hook-failed" => Ok(FailureReason::HookFailed {
            exit_code: exit_code(&args)?,
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
            WarningKind::LocationSkipped,
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

    /// A persisted `credentials-refused` failure reads back as the typed
    /// reason: no arguments, no detail.
    #[test]
    fn credentials_refused_round_trips() {
        let columns = encode_failure(&FailureReason::CredentialsRefused)
            .expect("a failure with no arguments encodes");
        assert_eq!(columns.code, "credentials-refused");
        assert_eq!(columns.args, "[]");
        assert_eq!(columns.detail, None);
        let back = match decode_failure(Some(columns.code), Some(&columns.args), columns.detail) {
            Ok(reason) => reason,
            Err(bad) => panic!("the written columns decode: {} {}", bad.column, bad.why),
        };
        assert_eq!(back, FailureReason::CredentialsRefused);
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
        assert_eq!(codes.len(), 13, "one code per refusal: {codes:?}");
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
