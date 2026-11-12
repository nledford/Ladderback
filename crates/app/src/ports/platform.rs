//! What the operating system reports of the machine, for the conditions
//! before a scheduled run (technical design §5, §16 decision 63): two small
//! ports, one per question, so a use case takes only what it asks.

use async_trait::async_trait;
use domain::execution::conditions::PowerSource;

/// What powers the machine.
#[async_trait]
pub trait PowerSupply: Send + Sync {
    /// AC power or battery. An error is a source the platform couldn't
    /// report, never a guess.
    async fn source(&self) -> Result<PowerSource, PlatformError>;
}

/// Whether the machine is online.
#[async_trait]
pub trait Network: Send + Sync {
    /// Whether the machine has a working connection to a network other than
    /// itself. An error is a state the platform couldn't report.
    async fn is_online(&self) -> Result<bool, PlatformError>;
}

/// Why the platform couldn't report what was asked. `from` names where the
/// answer is read (a file, a program), and `detail` is the platform's own
/// words, or what was read; neither ever holds a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformError {
    /// The source can't be read: missing, refused, a program that couldn't
    /// start or failed.
    Unreadable { from: String, detail: String },
    /// The source was read and says something this adapter doesn't know.
    Unrecognized { from: String, detail: String },
    /// The source didn't answer in time.
    TimedOut { from: String },
}
