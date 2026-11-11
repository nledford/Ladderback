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
mod names;
mod plan;
mod repository;
mod retention;
mod schedule;

pub use agent::{AgentConfig, AgentSettings, Concurrency, Nice, Violation};
pub use duration::{MAX_DURATION, format_duration, parse_duration};
pub use filter::SourceFilter;
pub use hooks::{HookEntry, Hooks, OnFailure};
pub use names::{GlobFile, Hostname, Locale, Source};
pub use plan::{Plan, RetryPolicy};
pub use repository::{GroupBy, Maintenance, RepositoryConfig, RepositorySettings, masked_options};
pub use retention::{KeepCount, KeepWithin, RetentionPolicy};
pub use schedule::{CronExpression, Schedule};

/// What a value the model refuses was meant to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Hostname,
    Source,
    GlobFile,
    Locale,
    Concurrency,
    Nice,
    Cron,
    Duration,
    Attempts,
    KeepCount,
    KeepWithin,
    Maintenance,
    GroupBy,
    OnFailure,
    HookCommand,
}

/// A value the model refuses. `value` is the text as it was written, except
/// for [`ValueKind::HookCommand`]: a command line may hold a secret, so its
/// text is never kept here, and an error shows no more than the key it is at.
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
