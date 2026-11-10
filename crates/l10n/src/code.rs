//! The API's error and warning codes, and the catalog message each one is
//! shown as (technical design §8.4). A code is an enum variant on the wire; the
//! message is chosen here, so a new variant is not complete until it has a
//! message.
//!
//! To add a code: add the variant to the enum in `api` with its
//! `args()` arm, add one line to the list below, and write the message
//! `error-<code>` or `warning-<code>` in `locales/en-US/common.ftl`.

use api::{ErrorCode, WarningCode};

use crate::{MessageId, msg};

/// Declares the table of an enum's codes and their messages, and the function
/// that looks a code up. The lookup is a `match` without a wildcard, so a
/// variant added to the enum without a line here doesn't compile, and the
/// table comes from the same list. Each line names its message constant as
/// `msg::…`, which is what the unused-ID lint looks for.
macro_rules! code_messages {
    ($table:ident, $lookup:ident, $code:ident { $($variant:ident => $message:path),+ $(,)? }) => {
        /// Every code of the enum, with its message.
        pub const $table: &[($code, MessageId)] = &[$(($code::$variant, $message)),+];

        /// The message the code is shown as.
        pub fn $lookup(code: $code) -> MessageId {
            match code {
                $($code::$variant => $message),+
            }
        }
    };
}

code_messages!(ERRORS, error_message, ErrorCode {
    InvalidJobSpec => msg::ERROR_INVALID_JOB_SPEC,
    OperationFailed => msg::ERROR_OPERATION_FAILED,
    DeadlinePassed => msg::ERROR_DEADLINE_PASSED,
    EngineUnavailable => msg::ERROR_ENGINE_UNAVAILABLE,
    EngineExited => msg::ERROR_ENGINE_EXITED,
    ProtocolViolated => msg::ERROR_PROTOCOL_VIOLATED,
    EngineStartTimedOut => msg::ERROR_ENGINE_START_TIMED_OUT,
    EngineExitTimedOut => msg::ERROR_ENGINE_EXIT_TIMED_OUT,
    HostNotAllowed => msg::ERROR_HOST_NOT_ALLOWED,
    OriginNotAllowed => msg::ERROR_ORIGIN_NOT_ALLOWED,
    JsonRequired => msg::ERROR_JSON_REQUIRED,
    ConfigUnavailable => msg::ERROR_CONFIG_UNAVAILABLE,
    ConfigTooLarge => msg::ERROR_CONFIG_TOO_LARGE,
    ConfigInvalid => msg::ERROR_CONFIG_INVALID,
    ConfigSchemaMissing => msg::ERROR_CONFIG_SCHEMA_MISSING,
    ConfigSchemaNewer => msg::ERROR_CONFIG_SCHEMA_NEWER,
    RepositoryExists => msg::ERROR_REPOSITORY_EXISTS,
    RepositoryAliasTaken => msg::ERROR_REPOSITORY_ALIAS_TAKEN,
    SecretUnresolved => msg::ERROR_SECRET_UNRESOLVED,
    PlanUnknown => msg::ERROR_PLAN_UNKNOWN,
    RepositoryUnknown => msg::ERROR_REPOSITORY_UNKNOWN,
    InvalidBody => msg::ERROR_INVALID_BODY,
    OperationUnknown => msg::ERROR_OPERATION_UNKNOWN,
    OperationNotRunning => msg::ERROR_OPERATION_NOT_RUNNING,
    InvalidCursor => msg::ERROR_INVALID_CURSOR,
    MaintenanceBusy => msg::ERROR_MAINTENANCE_BUSY,
    RetentionKeepsNothing => msg::ERROR_RETENTION_KEEPS_NOTHING,
    MaintenanceKindUnknown => msg::ERROR_MAINTENANCE_KIND_UNKNOWN,
    AdoptionNoMatch => msg::ERROR_ADOPTION_NO_MATCH,
    AdoptionAmbiguous => msg::ERROR_ADOPTION_AMBIGUOUS,
    Unknown => msg::ERROR_UNKNOWN,
});

code_messages!(WARNINGS, warning_message, WarningCode {
    SourceUnreadable => msg::WARNING_SOURCE_UNREADABLE,
    HookWarning => msg::WARNING_HOOK_WARNING,
    Unknown => msg::WARNING_UNKNOWN,
});
