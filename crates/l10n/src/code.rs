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
    RepositoryTimedOut => msg::ERROR_REPOSITORY_TIMED_OUT,
    RepositoryAliasTaken => msg::ERROR_REPOSITORY_ALIAS_TAKEN,
    RepositoryAliasInvalid => msg::ERROR_REPOSITORY_ALIAS_INVALID,
    SecretUnresolved => msg::ERROR_SECRET_UNRESOLVED,
    PlanUnknown => msg::ERROR_PLAN_UNKNOWN,
    RepositoryUnknown => msg::ERROR_REPOSITORY_UNKNOWN,
    InvalidBody => msg::ERROR_INVALID_BODY,
    BodyTooLarge => msg::ERROR_BODY_TOO_LARGE,
    OperationUnknown => msg::ERROR_OPERATION_UNKNOWN,
    OperationNotRunning => msg::ERROR_OPERATION_NOT_RUNNING,
    InvalidCursor => msg::ERROR_INVALID_CURSOR,
    MaintenanceBusy => msg::ERROR_MAINTENANCE_BUSY,
    RetentionKeepsNothing => msg::ERROR_RETENTION_KEEPS_NOTHING,
    MaintenanceKindUnknown => msg::ERROR_MAINTENANCE_KIND_UNKNOWN,
    AdoptionNoMatch => msg::ERROR_ADOPTION_NO_MATCH,
    AdoptionAmbiguous => msg::ERROR_ADOPTION_AMBIGUOUS,
    ProfileUnreadable => msg::ERROR_PROFILE_UNREADABLE,
    ProfileTooLarge => msg::ERROR_PROFILE_TOO_LARGE,
    ProfileInvalid => msg::ERROR_PROFILE_INVALID,
    ProfileUnsupported => msg::ERROR_PROFILE_UNSUPPORTED,
    PlanIdTaken => msg::ERROR_PLAN_ID_TAKEN,
    InlineSecret => msg::ERROR_INLINE_SECRET,
    ProfileExportReference => msg::ERROR_PROFILE_EXPORT_REFERENCE,
    ProfileExportUrlPassword => msg::ERROR_PROFILE_EXPORT_URL_PASSWORD,
    ProfileExportTagComma => msg::ERROR_PROFILE_EXPORT_TAG_COMMA,
    Unknown => msg::ERROR_UNKNOWN,
});

code_messages!(WARNINGS, warning_message, WarningCode {
    SourceUnreadable => msg::WARNING_SOURCE_UNREADABLE,
    HookWarning => msg::WARNING_HOOK_WARNING,
    MaintenancePartial => msg::WARNING_MAINTENANCE_PARTIAL,
    ProfileKeyIgnored => msg::WARNING_PROFILE_KEY_IGNORED,
    ProfilePrune => msg::WARNING_PROFILE_PRUNE,
    ProfileEnvIgnored => msg::WARNING_PROFILE_ENV_IGNORED,
    ProfileHooksPerPlan => msg::WARNING_PROFILE_HOOKS_PER_PLAN,
    ProfileUsedMissing => msg::WARNING_PROFILE_USED_MISSING,
    InlineSecret => msg::WARNING_INLINE_SECRET,
    ConfigInlineSecret => msg::WARNING_CONFIG_INLINE_SECRET,
    ProfileTagDropped => msg::WARNING_PROFILE_TAG_DROPPED,
    ProfileOptionLeftOut => msg::WARNING_PROFILE_OPTION_LEFT_OUT,
    ProfileRetentionEmpty => msg::WARNING_PROFILE_RETENTION_EMPTY,
    Unknown => msg::WARNING_UNKNOWN,
});
