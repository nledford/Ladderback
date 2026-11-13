//! Why a run was refused before anything ran (technical design §5, §10.2):
//! a setting of its repository couldn't be resolved. A run the schedule
//! started records it as its failure ([`FailureReason::SecretRefused`]); a
//! run asked for by hand is refused to whoever asked, with the same code.
//!
//! [`FailureReason::SecretRefused`]: super::FailureReason::SecretRefused

use serde::{Deserialize, Serialize};

/// What was wrong with the setting, one variant per code a request to start
/// the run is refused with. A source is named (a variable, a path, a keyring
/// entry, a program), never read; `source` and `detail` are masked of every
/// secret of the repository before they are kept, so none holds one (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingRefusal {
    /// Nothing is at `source`: an unset variable, a file or a keyring entry
    /// that isn't there, a command that printed nothing.
    SecretMissing { source: String },
    /// `source` is there but couldn't be read; `detail` is what the platform
    /// said, when it said anything.
    SecretUnreadable {
        source: String,
        detail: Option<String>,
    },
    /// `source` is a keyring entry, and the machine has no keyring service
    /// that can be asked for it, or the service's keyring is locked.
    SecretKeyringUnavailable {
        source: String,
        detail: Option<String>,
    },
    /// What `source` gave isn't text.
    SecretNotText { source: String },
    /// What `source` gave is more than `limit` bytes.
    SecretTooLarge { source: String, limit: u64 },
    /// The command `source` failed; `detail` is the start of what it wrote
    /// to stderr.
    SecretCommandFailed {
        source: String,
        detail: Option<String>,
    },
    /// The command `source` ran for longer than `seconds`.
    SecretCommandTimedOut { source: String, seconds: u64 },
    /// The command `source` exited while something it started still held its
    /// output open, before its secret's line ended.
    SecretCommandOutputOpen { source: String },
    /// The setting becomes an argument of a process and holds a reference,
    /// whose secret would show in `ps`.
    SecretInProcessArguments,
    /// The setting's references expand to more than `limit` bytes.
    SecretExpansionTooLong { limit: u64 },
    /// The setting is an rclone connection string with more credentials than
    /// a job masks.
    LocationTooManyCredentials,
    /// The setting is a location that, resolved, is a relative path.
    RepositoryPathRelative,
    /// The setting isn't a valid template; `at` is where it goes wrong, when
    /// the fault has a place.
    TemplateInvalid { at: Option<u64> },
}

impl SettingRefusal {
    /// The refusal's code, as a request to start the run is refused with it
    /// and as the operation log keeps it (§9).
    pub fn code(&self) -> &'static str {
        match self {
            Self::SecretMissing { .. } => "secret-missing",
            Self::SecretUnreadable { .. } => "secret-unreadable",
            Self::SecretKeyringUnavailable { .. } => "secret-keyring-unavailable",
            Self::SecretNotText { .. } => "secret-not-text",
            Self::SecretTooLarge { .. } => "secret-too-large",
            Self::SecretCommandFailed { .. } => "secret-command-failed",
            Self::SecretCommandTimedOut { .. } => "secret-command-timed-out",
            Self::SecretCommandOutputOpen { .. } => "secret-command-output-open",
            Self::SecretInProcessArguments => "secret-in-process-arguments",
            Self::SecretExpansionTooLong { .. } => "secret-expansion-too-long",
            Self::LocationTooManyCredentials => "location-too-many-credentials",
            Self::RepositoryPathRelative => "repository-path-relative",
            Self::TemplateInvalid { .. } => "template-invalid",
        }
    }
}
