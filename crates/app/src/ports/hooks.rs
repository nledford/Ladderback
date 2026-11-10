//! The hook runner port (architecture.md §5, `HookRunner`): runs one hook
//! command around an operation, with a scrubbed environment and captured,
//! capped, redacted output (security.md §10.2).

use async_trait::async_trait;
use domain::planning::{HookEntry, Hooks};
use domain::{OperationId, PlanId, RepositoryAlias, SnapshotId};
use std::time::Duration;

/// How long one hook may run before it is stopped: SIGTERM to its process
/// group, then SIGKILL after 5 s, as the password-command runner does. A
/// constant for now; making it configurable is left to a later milestone.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The most that is kept of each of a hook's output streams. The excess is
/// dropped, with `truncated` set on the stream that lost some.
pub const MAX_HOOK_OUTPUT_BYTES: usize = 64 * 1024;

/// Which list a hook runs from, and what `LADDERBACK_HOOK` says to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookPhase {
    Before,
    After,
    Failed,
    Finally,
}

impl HookPhase {
    /// The value of `LADDERBACK_HOOK` for this phase.
    pub fn as_env(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
            Self::Failed => "failed",
            Self::Finally => "finally",
        }
    }
}

/// The hooks of the three levels around one operation (§6.1, decision 10),
/// and the names its context variables carry. The order the lists run in
/// belongs to the use case, not the runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookSetup {
    pub agent: Hooks,
    pub repository: Hooks,
    pub plan: Hooks,
    pub plan_id: PlanId,
    pub repository_alias: RepositoryAlias,
}

impl HookSetup {
    /// Whether any list holds anything.
    pub fn is_empty(&self) -> bool {
        self.agent.is_empty() && self.repository.is_empty() && self.plan.is_empty()
    }
}

/// What a hook is told about the operation it runs for, through the
/// `LADDERBACK_*` context variables alone: never a secret, never the
/// repository URL (security.md §10.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookContext {
    pub phase: HookPhase,
    pub plan: PlanId,
    pub repository: RepositoryAlias,
    pub operation: OperationId,
    /// `LADDERBACK_SNAPSHOT_ID`, only after a backup succeeded.
    pub snapshot: Option<SnapshotId>,
    /// `LADDERBACK_ERROR`, the operation's stored failure code, only for the
    /// `failed` and `finally` phases after a failure.
    pub error: Option<String>,
}

/// One captured output stream: at most [`MAX_HOOK_OUTPUT_BYTES`] were kept,
/// redacted, of what the hook wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookOutput {
    pub text: String,
    /// Some of the stream was dropped: it wrote past the cap.
    pub truncated: bool,
}

/// How a hook ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookStatus {
    /// The command exited zero.
    Succeeded,
    /// The command ran and exited non-zero. `exit_code` is `None` when no
    /// exit status is known, because a signal ended it.
    Failed { exit_code: Option<i32> },
    /// The command outlived [`HOOK_TIMEOUT`] (or the limit the runner was
    /// built with) and was killed.
    TimedOut,
    /// The command could not be started at all. `detail` is the platform's
    /// own message, never a command line or a secret.
    NotStarted { detail: String },
}

/// What running one hook produced: how it ended, and what it wrote, capped
/// and redacted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookOutcome {
    pub status: HookStatus,
    pub stdout: HookOutput,
    pub stderr: HookOutput,
}

impl HookOutcome {
    /// Whether the hook ran to a zero exit.
    pub fn succeeded(&self) -> bool {
        matches!(self.status, HookStatus::Succeeded)
    }
}

/// Runs hook commands with a scrubbed environment (architecture.md §5,
/// process spawner).
///
/// Contract (checked by `contract::hooks`):
/// - The command runs as parsed, without a shell, in a process group of its
///   own, and is stopped the way the password-command runner stops one
///   (SIGTERM to the group, then SIGKILL after 5 s) when it outlives its
///   timeout. A hook that ends on the SIGTERM is not signalled again, so a
///   process it started that ignored the SIGTERM can outlive it
///   (scheduling-and-config.md §6.1).
/// - Its environment is the worker's allow-list (components.md §4.1) plus
///   the `LADDERBACK_*` variables of the context, and nothing else: no
///   secret, no repository URL with credentials.
/// - Its stdout and stderr are captured, cut at
///   [`MAX_HOOK_OUTPUT_BYTES`] each with `truncated` set, and redacted
///   against the run's secrets, before they are stored or logged.
/// - A non-zero exit is `Failed` with the exit code; a command past the
///   timeout is `TimedOut`; one that can't start is `NotStarted` with a
///   short platform detail.
#[async_trait]
pub trait HookRunner: Send + Sync {
    async fn run(&self, entry: &HookEntry, context: &HookContext) -> HookOutcome;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use domain::planning::{HookEntry, Hooks, OnFailure};
    use domain::{PlanId, RepositoryAlias};

    use super::{HOOK_TIMEOUT, HookSetup, MAX_HOOK_OUTPUT_BYTES};

    fn setup(plan: Hooks) -> HookSetup {
        HookSetup {
            agent: Hooks::default(),
            repository: Hooks::default(),
            plan,
            plan_id: PlanId::parse("demo-plan").expect("the demo plan parses"),
            repository_alias: RepositoryAlias::parse("nas").expect("the demo alias parses"),
        }
    }

    #[test]
    fn the_limits_are_the_documented_ones() {
        assert_eq!(HOOK_TIMEOUT, Duration::from_secs(300));
        assert_eq!(MAX_HOOK_OUTPUT_BYTES, 64 * 1024);
    }

    #[test]
    fn a_setup_is_empty_only_when_no_list_holds_anything() {
        assert!(setup(Hooks::default()).is_empty());
        let entry = HookEntry::new("demo-hook", ["demo"], OnFailure::Error)
            .expect("the demo hook is valid");
        let mut plan = Hooks::default();
        plan.run_finally.push(entry);
        assert!(!setup(plan).is_empty());
    }
}
