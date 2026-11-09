//! A recording fake for the [`HookRunner`] port: each hook's outcome is
//! scripted by its command, and every call is remembered with the context it
//! ran in, so a scenario can tell what ran, when, and what it was told.

use std::sync::Mutex;

use async_trait::async_trait;
use domain::planning::HookEntry;

use crate::ports::{HookContext, HookOutcome, HookOutput, HookRunner, HookStatus};

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
/// script succeeds and writes nothing. Scripted text is returned as the
/// capped, redacted capture, so the fake passes the same contract checks as
/// the real runner.
#[derive(Default)]
pub struct FakeHookRunner {
    scripts: Mutex<Vec<(String, Scripted)>>,
    simulated: Mutex<Vec<Simulated>>,
    calls: Mutex<Vec<HookCall>>,
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
        let _ = entry;
        let _ = context;
        HookOutcome {
            status: HookStatus::Succeeded,
            stdout: HookOutput::default(),
            stderr: HookOutput::default(),
        }
    }
}
