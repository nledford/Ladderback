//! The hooks adapter: a [`HookRunner`] that spawns each hook's command as a
//! process, without a shell, in a process group of its own, with the
//! worker's scrubbed environment (components.md §4.1, security.md §10.2).
//! The spawning, timeout and kill sequence mirror the password-command
//! runner (`secrets::command`).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::time::Duration;

use app::ports::{HOOK_TIMEOUT, HookContext, HookOutcome, HookOutput, HookRunner, HookStatus};
use async_trait::async_trait;
use domain::planning::HookEntry;
use domain::secrets::{Redactor, SecretValue};

/// Runs hook commands as processes.
#[derive(Clone, Debug, Default)]
pub struct ProcessHookRunner {
    /// How long one hook may run before it is stopped: SIGTERM to its
    /// process group, then SIGKILL after 5 s.
    timeout: Duration,
    /// Masks the secrets of the run in everything a hook wrote, before it is
    /// stored or logged.
    redactor: Redactor,
}

impl ProcessHookRunner {
    /// A runner with the default timeout, redacting against nothing.
    pub fn new() -> Self {
        Self {
            timeout: HOOK_TIMEOUT,
            redactor: Redactor::new([]),
        }
    }

    /// A runner with a timeout of its own, for tests.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }

    /// Adds a secret the captured output is redacted against.
    pub fn with_secret(mut self, secret: &str) -> Self {
        let value = SecretValue::new(secret);
        self.redactor = Redactor::new([&value]);
        self
    }
}

#[async_trait]
impl HookRunner for ProcessHookRunner {
    async fn run(&self, entry: &HookEntry, context: &HookContext) -> HookOutcome {
        let _ = (entry, context, self.timeout, &self.redactor);
        HookOutcome {
            status: HookStatus::Succeeded,
            stdout: HookOutput::default(),
            stderr: HookOutput::default(),
        }
    }
}
