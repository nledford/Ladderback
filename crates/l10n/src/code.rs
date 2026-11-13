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
    DaemonStopped => msg::ERROR_DAEMON_STOPPED,
    HostNotAllowed => msg::ERROR_HOST_NOT_ALLOWED,
    OriginNotAllowed => msg::ERROR_ORIGIN_NOT_ALLOWED,
    JsonRequired => msg::ERROR_JSON_REQUIRED,
    SignInRequired => msg::ERROR_SIGN_IN_REQUIRED,
    SignInFailed => msg::ERROR_SIGN_IN_FAILED,
    SignInWait => msg::ERROR_SIGN_IN_WAIT,
    CsrfTokenInvalid => msg::ERROR_CSRF_TOKEN_INVALID,
    SignInUnavailable => msg::ERROR_SIGN_IN_UNAVAILABLE,
    ConfigUnavailable => msg::ERROR_CONFIG_UNAVAILABLE,
    ConfigTooLarge => msg::ERROR_CONFIG_TOO_LARGE,
    ConfigInvalid => msg::ERROR_CONFIG_INVALID,
    ConfigSchemaMissing => msg::ERROR_CONFIG_SCHEMA_MISSING,
    ConfigSchemaNewer => msg::ERROR_CONFIG_SCHEMA_NEWER,
    RepositoryExists => msg::ERROR_REPOSITORY_EXISTS,
    RepositoryNotFound => msg::ERROR_REPOSITORY_NOT_FOUND,
    CreateNotOffered => msg::ERROR_CREATE_NOT_OFFERED,
    RepositoryTimedOut => msg::ERROR_REPOSITORY_TIMED_OUT,
    RepositoryAliasTaken => msg::ERROR_REPOSITORY_ALIAS_TAKEN,
    RepositoryAlreadyConfigured => msg::ERROR_REPOSITORY_ALREADY_CONFIGURED,
    RepositoryReplaced => msg::ERROR_REPOSITORY_REPLACED,
    RepositoryAliasInvalid => msg::ERROR_REPOSITORY_ALIAS_INVALID,
    SecretMissing => msg::ERROR_SECRET_MISSING,
    SecretUnreadable => msg::ERROR_SECRET_UNREADABLE,
    SecretKeyringUnavailable => msg::ERROR_SECRET_KEYRING_UNAVAILABLE,
    SecretNotText => msg::ERROR_SECRET_NOT_TEXT,
    SecretTooLarge => msg::ERROR_SECRET_TOO_LARGE,
    SecretCommandFailed => msg::ERROR_SECRET_COMMAND_FAILED,
    SecretCommandOutputOpen => msg::ERROR_SECRET_COMMAND_OUTPUT_OPEN,
    SecretCommandTimedOut => msg::ERROR_SECRET_COMMAND_TIMED_OUT,
    SecretInProcessArguments => msg::ERROR_SECRET_IN_PROCESS_ARGUMENTS,
    SecretExpansionTooLong => msg::ERROR_SECRET_EXPANSION_TOO_LONG,
    SecretsTooLargeToMask => msg::ERROR_SECRETS_TOO_LARGE_TO_MASK,
    TemplateInvalid => msg::ERROR_TEMPLATE_INVALID,
    LocationTooManyCredentials => msg::ERROR_LOCATION_TOO_MANY_CREDENTIALS,
    RepositoryPathRelative => msg::ERROR_REPOSITORY_PATH_RELATIVE,
    PlanUnknown => msg::ERROR_PLAN_UNKNOWN,
    RepositoryUnknown => msg::ERROR_REPOSITORY_UNKNOWN,
    InvalidBody => msg::ERROR_INVALID_BODY,
    BodyTooLarge => msg::ERROR_BODY_TOO_LARGE,
    OperationUnknown => msg::ERROR_OPERATION_UNKNOWN,
    OperationNotRunning => msg::ERROR_OPERATION_NOT_RUNNING,
    InvalidCursor => msg::ERROR_INVALID_CURSOR,
    MaintenanceBusy => msg::ERROR_MAINTENANCE_BUSY,
    MaintenanceNotOwner => msg::ERROR_MAINTENANCE_NOT_OWNER,
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
    ConfigFolderUnknown => msg::ERROR_CONFIG_FOLDER_UNKNOWN,
    RuntimeUnavailable => msg::ERROR_RUNTIME_UNAVAILABLE,
    MaintenanceSelectionChanged => msg::ERROR_MAINTENANCE_SELECTION_CHANGED,
    RepositoryReadTimedOut => msg::ERROR_REPOSITORY_READ_TIMED_OUT,
    NoLocationUsable => msg::ERROR_NO_LOCATION_USABLE,
    NoLocationRuleHolds => msg::ERROR_NO_LOCATION_RULE_HOLDS,
    PasswordNotAccepted => msg::ERROR_PASSWORD_NOT_ACCEPTED,
    Unknown => msg::ERROR_UNKNOWN,
});

code_messages!(WARNINGS, warning_message, WarningCode {
    SourceUnreadable => msg::WARNING_SOURCE_UNREADABLE,
    LocationSkipped => msg::WARNING_LOCATION_SKIPPED,
    LocationPlainHttpCredentials => msg::WARNING_LOCATION_PLAIN_HTTP_CREDENTIALS,
    LocationEveryRuled => msg::WARNING_LOCATION_EVERY_RULED,
    LocationLastRuledNoRetry => msg::WARNING_LOCATION_LAST_RULED_NO_RETRY,
    LocationRuleWiredUnknown => msg::WARNING_LOCATION_RULE_WIRED_UNKNOWN,
    LocationRuleNameUnreadable => msg::WARNING_LOCATION_RULE_NAME_UNREADABLE,
    ProfileLocationsNotExported => msg::WARNING_PROFILE_LOCATIONS_NOT_EXPORTED,
    HookWarning => msg::WARNING_HOOK_WARNING,
    MaintenancePartial => msg::WARNING_MAINTENANCE_PARTIAL,
    CheckWarning => msg::WARNING_CHECK_WARNING,
    CheckError => msg::WARNING_CHECK_ERROR,
    MaintenanceOtherHosts => msg::WARNING_MAINTENANCE_OTHER_HOSTS,
    ProfileKeyIgnored => msg::WARNING_PROFILE_KEY_IGNORED,
    ProfilePrune => msg::WARNING_PROFILE_PRUNE,
    ProfileEnvIgnored => msg::WARNING_PROFILE_ENV_IGNORED,
    ProfileHooksPerPlan => msg::WARNING_PROFILE_HOOKS_PER_PLAN,
    ProfileUsedMissing => msg::WARNING_PROFILE_USED_MISSING,
    InlineSecret => msg::WARNING_INLINE_SECRET,
    ConfigInlineSecret => msg::WARNING_CONFIG_INLINE_SECRET,
    LocationSecret => msg::WARNING_LOCATION_SECRET,
    ProfileTagDropped => msg::WARNING_PROFILE_TAG_DROPPED,
    ProfileOptionLeftOut => msg::WARNING_PROFILE_OPTION_LEFT_OUT,
    ProfileRetentionEmpty => msg::WARNING_PROFILE_RETENTION_EMPTY,
    ProfileSettingKept => msg::WARNING_PROFILE_SETTING_KEPT,
    ProfileKeyringCommandMissing => msg::WARNING_PROFILE_KEYRING_COMMAND_MISSING,
    NotificationUndelivered => msg::WARNING_NOTIFICATION_UNDELIVERED,
    Unknown => msg::WARNING_UNKNOWN,
});
