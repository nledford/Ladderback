//! A recording fake for the [`HookRunner`] port: each hook's outcome is
//! scripted by its command, and every call is remembered with the context it
//! ran in, so a scenario can tell what ran, when, and what it was told.

use std::sync::Mutex;

use async_trait::async_trait;
use domain::planning::HookEntry;
use domain::secrets::{Redactor, SecretValue};

use crate::ports::{
    HookContext, HookOutcome, HookOutput, HookRunner, HookStatus, MAX_HOOK_OUTPUT_BYTES,
};

/// What the fake does with one hook: the status it reports, and the text it
/// claims the hook wrote to each stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scripted {
    pub status: HookStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Scripted {
    /// A hook that succeeds, writing nothing.
    pub fn succeeds() -> Self {
        Self {
            status: HookStatus::Succeeded,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// A hook that exits with `code`.
    pub fn fails_with(code: i32) -> Self {
        Self {
            status: HookStatus::Failed {
                exit_code: Some(code),
            },
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// A hook that outlives the timeout.
    pub fn times_out() -> Self {
        Self {
            status: HookStatus::TimedOut,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// One recorded call: the entry as it was given and the context it ran in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookCall {
    pub entry: HookEntry,
    pub context: HookContext,
}

/// The fake runner. Outcomes are scripted by the hook's program name, so
/// different hooks in one scenario can be told apart; a program with no
/// script never started. Scripted text is returned as the capped, redacted
/// capture, so the fake passes the same contract checks as the real runner.
pub struct FakeHookRunner {
    scripts: Mutex<Vec<(String, Scripted)>>,
    simulated: Mutex<Vec<Simulated>>,
    calls: Mutex<Vec<HookCall>>,
    redactor: Redactor,
}

impl Default for FakeHookRunner {
    fn default() -> Self {
        Self {
            scripts: Mutex::new(Vec::new()),
            simulated: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
            redactor: Redactor::new([]),
        }
    }
}

/// A hook whose outcome is computed from the call itself, by program name.
type Simulated = (
    String,
    Box<dyn Fn(&HookEntry, &HookContext) -> HookOutcome + Send + Sync>,
);

impl FakeHookRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a secret the scripted text is redacted against, as a real run's
    /// secrets would be.
    pub fn with_secret(mut self, secret: &str) -> Self {
        let value = SecretValue::new(secret);
        self.redactor = Redactor::new([&value]);
        self
    }

    /// Scripts every hook whose program is `program`.
    pub fn script(&self, program: &str, outcome: Scripted) {
        if let Ok(mut scripts) = self.scripts.lock() {
            scripts.push((program.to_owned(), outcome));
        }
    }

    /// Scripts every hook whose program is `program` with a closure, so a
    /// scenario or contract harness can model behavior a plain outcome
    /// can't: a hook that writes lines, or one that reads its context.
    pub fn simulate(
        &self,
        program: &str,
        behaviour: impl Fn(&HookEntry, &HookContext) -> HookOutcome + Send + Sync + 'static,
    ) {
        if let Ok(mut simulated) = self.simulated.lock() {
            simulated.push((program.to_owned(), Box::new(behaviour)));
        }
    }

    pub fn calls(&self) -> Vec<HookCall> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl HookRunner for FakeHookRunner {
    async fn run(&self, entry: &HookEntry, context: &HookContext) -> HookOutcome {
        let program = entry.command.program();
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(HookCall {
                entry: entry.clone(),
                context: context.clone(),
            });
        }
        // The latest behaviour registered for the program wins, so a
        // scenario can change its mind.
        if let Ok(simulated) = self.simulated.lock()
            && let Some((_, behaviour)) = simulated.iter().rev().find(|(name, _)| name == program)
        {
            return behaviour(entry, context);
        }
        if let Ok(scripts) = self.scripts.lock()
            && let Some((_, scripted)) = scripts.iter().rev().find(|(name, _)| name == program)
        {
            return HookOutcome {
                status: scripted.status.clone(),
                stdout: self.capture(&scripted.stdout),
                stderr: self.capture(&scripted.stderr),
            };
        }
        HookOutcome {
            status: HookStatus::NotStarted {
                detail: format!("{program} is not scripted"),
            },
            stdout: HookOutput::default(),
            stderr: HookOutput::default(),
        }
    }
}

impl FakeHookRunner {
    /// The scripted text of one stream, redacted and capped as a real
    /// capture is.
    fn capture(&self, text: &str) -> HookOutput {
        let redacted = self.redactor.redact(text);
        if redacted.len() <= MAX_HOOK_OUTPUT_BYTES {
            return HookOutput {
                text: redacted,
                truncated: false,
            };
        }
        // The cut can land inside a multi-byte character; step back the
        // three bytes that take at most, bounded so a walk that does not
        // shrink the cut stops instead of spinning, and a cut that is not
        // on a boundary yields nothing rather than panicking.
        let mut cut = MAX_HOOK_OUTPUT_BYTES;
        for _ in 0..3 {
            if redacted.is_char_boundary(cut) {
                break;
            }
            cut -= 1;
        }
        HookOutput {
            text: redacted.get(..cut).unwrap_or_default().to_owned(),
            truncated: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::planning::{HookEntry, OnFailure};
    use domain::{OperationId, PlanId, RepositoryAlias};

    use super::{FakeHookRunner, Scripted};
    use crate::ports::{HookContext, HookPhase, HookRunner, HookStatus, MAX_HOOK_OUTPUT_BYTES};

    fn entry() -> HookEntry {
        HookEntry::new("demo-hook", ["demo"], OnFailure::Error).expect("the demo hook is valid")
    }

    fn context() -> HookContext {
        HookContext {
            phase: HookPhase::Before,
            plan: PlanId::parse("demo-plan").expect("the demo plan parses"),
            repository: RepositoryAlias::parse("nas").expect("the demo alias parses"),
            operation: OperationId::from_uuid(uuid::Uuid::from_u128(7)),
            snapshot: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn a_capture_cut_mid_character_steps_back_to_a_boundary() {
        // The cut lands inside a two-byte character, so the walk back to a
        // boundary is what keeps the text a valid string.
        let runner = FakeHookRunner::new();
        runner.script(
            "demo-hook",
            Scripted {
                status: HookStatus::Succeeded,
                stdout: format!("aaa{}", "é".repeat(MAX_HOOK_OUTPUT_BYTES / 2 + 9)),
                stderr: String::new(),
            },
        );
        let outcome = runner.run(&entry(), &context()).await;
        assert!(outcome.stdout.truncated);
        assert_eq!(outcome.stdout.text.len(), MAX_HOOK_OUTPUT_BYTES - 1);
        assert!(outcome.stdout.text.len() <= MAX_HOOK_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn a_short_capture_is_kept_whole() {
        let runner = FakeHookRunner::new();
        runner.script(
            "demo-hook",
            Scripted {
                status: HookStatus::Succeeded,
                stdout: "demo output".to_owned(),
                stderr: String::new(),
            },
        );
        let outcome = runner.run(&entry(), &context()).await;
        assert_eq!(outcome.stdout.text, "demo output");
        assert!(!outcome.stdout.truncated);
    }
}
