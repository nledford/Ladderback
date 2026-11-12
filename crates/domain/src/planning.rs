//! The planning context (technical design §6.1): what an agent has been told
//! to back up, to which repository, how often, and how much of it to keep.
//!
//! These types are the configuration as the domain sees it. The file format,
//! its schema versions and their migrations belong to `adapters/config`; the
//! domain keeps the rules that hold whatever the format is: a plan's ID is
//! unique, its repository exists, and a value is the kind of value it is
//! named for. Nothing here reads a file, and a refusal is a typed value, never
//! a sentence (§8.4).

mod agent;
mod duration;
mod filter;
mod hooks;
mod local_paths;
mod names;
mod plan;
mod repository;
mod retention;
mod schedule;

pub use agent::{AgentConfig, AgentSettings, Concurrency, Nice, NotRecorded, Violation};
pub use duration::{MAX_DURATION, format_duration, parse_duration};
pub use filter::SourceFilter;
pub use hooks::{HookEntry, Hooks, OnFailure};
pub use local_paths::{RepositoryField, is_relative_location};
pub use names::{GlobFile, Hostname, Locale, Source};
pub use plan::{Plan, RetryPolicy};
pub use repository::{
    GroupBy, Maintenance, RepositoryConfig, RepositorySettings, check_location, check_option_value,
    check_options, masked_options,
};
pub use retention::{KeepCount, KeepWithin, RetentionPolicy};
pub use schedule::{Cadence, CronExpression, Interval, MIN_INTERVAL, NoNextRun, Schedule};

/// What a value the model refuses was meant to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Hostname,
    Source,
    GlobFile,
    /// A file or folder on this machine, which a repository's settings name
    /// by an absolute path: its location, or a file an option names.
    LocalPath,
    Locale,
    Concurrency,
    Nice,
    Cron,
    Duration,
    /// A schedule's `every`, shorter than a minute or longer than 366 days.
    Interval,
    Attempts,
    KeepCount,
    KeepWithin,
    Maintenance,
    GroupBy,
    OnFailure,
    HookCommand,
    /// A repository's `repository` or `repo-hot`, longer than
    /// [`MAX_LOCATION_BYTES`](crate::secrets::MAX_LOCATION_BYTES).
    Location,
    /// The value of an option, longer than
    /// [`MAX_OPTION_VALUE_BYTES`](crate::secrets::MAX_OPTION_VALUE_BYTES).
    OptionValue,
    /// A table of options (`options`, `options-hot`, `options-cold`) with
    /// more than [`MAX_OPTIONS`](crate::secrets::MAX_OPTIONS).
    Options,
    /// The program of a hook: a name looked up on `PATH`, or an absolute path.
    HookProgram,
}

/// A value the model refuses. `value` is the text as it was written, except
/// for [`ValueKind::HookCommand`], [`ValueKind::Location`] and
/// [`ValueKind::OptionValue`], which may hold a secret, so their text is
/// never kept here, and an error shows no more than the key it is at; and
/// [`ValueKind::Options`], a table, which has no text of its own.
/// A [`ValueKind::HookProgram`] keeps the program and none of its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidValue {
    pub kind: ValueKind,
    pub value: String,
}

impl InvalidValue {
    pub(crate) fn new(kind: ValueKind, value: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
        }
    }
}

impl std::fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for InvalidValue {}
