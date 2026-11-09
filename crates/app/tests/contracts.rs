//! The in-memory fakes pass the same contract suites as the real adapters.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app::contract;
use app::contract::clock::ClockHarness;
use app::contract::hooks::HooksHarness;
use app::contract::secrets::SecretsHarness;
use app::fakes::InMemoryConfigStore;
use app::fakes::{
    FakeClock, FakeHookRunner, FakeSecrets, InMemoryEventBus, InMemoryOperationStore, Script,
    SimulatedEngine,
};
use app::ports::{Clock, Engine, EventBus, HookOutcome, HookOutput, HookStatus, SecretResolver};
use async_trait::async_trait;
use domain::secrets::{CommandLine, KeyringEntry};
use domain::{SignedDuration, Timestamp};

fn epoch() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

struct ManualClock(Arc<FakeClock>);

#[async_trait]
impl ClockHarness for ManualClock {
    fn clock(&self) -> Arc<dyn Clock> {
        self.0.clone()
    }

    async fn let_time_pass(&self, by: SignedDuration) {
        self.0.advance(by);
    }
}

#[tokio::test]
async fn fake_clock_meets_the_clock_contract() {
    let harness = ManualClock(Arc::new(FakeClock::new(epoch())));
    contract::clock::run(&harness, SignedDuration::from_secs(60)).await;
}

#[tokio::test]
async fn simulated_engine_meets_the_engine_contract() {
    let clock = Arc::new(FakeClock::auto_advancing(epoch()));
    let engine = SimulatedEngine::new(clock);
    contract::engine::run(&engine, Duration::from_secs(5)).await;
}

#[tokio::test]
async fn in_memory_operation_store_meets_the_operation_store_contract() {
    contract::operation_store::run(&InMemoryOperationStore::new()).await;
}

#[tokio::test]
async fn simulated_engine_steps_take_time_on_its_clock() {
    let clock = Arc::new(FakeClock::auto_advancing(epoch()));
    let engine = SimulatedEngine::new(clock.clone());
    let job = app::ports::FakeJob {
        steps: 3,
        step_delay: Duration::from_secs(60),
        warnings: Vec::new(),
        end: app::ports::FakeEnd::Succeed,
        deadline_budget: None,
    };
    let op = domain::OperationId::from_uuid(domain::Uuid::from_u128(1));
    let events = engine
        .run(op, app::ports::Job::Fake(job))
        .await
        .unwrap()
        .events;
    futures::StreamExt::collect::<Vec<_>>(events).await;
    assert_eq!(clock.now(), epoch() + SignedDuration::from_mins(3));
}

/// The in-memory secret store, set up through the contract's harness.
#[derive(Default)]
struct FakeSecretsHarness {
    secrets: Arc<FakeSecrets>,
    commands: AtomicUsize,
}

impl FakeSecretsHarness {
    /// A command of its own for each script, since a fake tells commands
    /// apart by their program and arguments.
    fn script(&self, program: &str, script: Script) -> CommandLine {
        let number = self.commands.fetch_add(1, Ordering::Relaxed);
        let command = CommandLine::new(program, [number.to_string()]).unwrap();
        self.secrets.script(&command, script);
        command
    }
}

impl SecretsHarness for FakeSecretsHarness {
    fn resolver(&self) -> Arc<dyn SecretResolver> {
        self.secrets.clone()
    }

    fn set_env(&self, name: &str, value: &str) {
        self.secrets.set_env(name, value);
    }

    fn write_file(&self, name: &str, contents: &[u8], mode: u32) -> String {
        let path = format!("/fake/secrets/{name}");
        self.secrets.write_file(&path, contents, mode);
        path
    }

    fn missing_file(&self) -> String {
        "/fake/secrets/nothing-here".to_owned()
    }

    fn directory(&self) -> String {
        let path = "/fake/secrets/a-directory".to_owned();
        self.secrets.make_directory(&path);
        path
    }

    fn set_keyring_entry(&self, name: &str, secret: &str) -> KeyringEntry {
        let entry = KeyringEntry::parse(&format!("ladderback-demo/{name}")).unwrap();
        self.secrets.set_keyring_entry(&entry, secret);
        entry
    }

    fn empty_keyring_entry(&self) -> KeyringEntry {
        KeyringEntry::parse("ladderback-demo/empty").unwrap()
    }

    fn printing(&self, stdout: &[u8]) -> CommandLine {
        self.script(
            "print",
            Script::Exits {
                status: 0,
                stdout: stdout.to_vec(),
                stderr: Vec::new(),
            },
        )
    }

    fn failing(&self, status: i32, stdout: &str, stderr: &str) -> CommandLine {
        self.script(
            "fail",
            Script::Exits {
                status,
                stdout: stdout.as_bytes().to_vec(),
                stderr: format!("{stderr}\n").into_bytes(),
            },
        )
    }

    fn hanging(&self) -> CommandLine {
        self.script("hang", Script::Hangs)
    }

    fn unstartable(&self) -> CommandLine {
        CommandLine::new("no-such-program", Vec::<String>::new()).unwrap()
    }
}

#[tokio::test]
async fn fake_secrets_meet_the_secrets_contract() {
    contract::secrets::run(&FakeSecretsHarness::default()).await;
}

#[tokio::test]
async fn in_memory_config_store_meets_the_config_store_contract() {
    contract::config_store::run(&InMemoryConfigStore::new()).await;
}

/// The fake hook runner, set up through the contract's harness. The
/// simulated environment follows the worker's rule (components.md §4.1):
/// the allow-list plus the `LADDERBACK_*` context variables, and nothing
/// else of the daemon's.
struct FakeHooksHarness {
    runner: Arc<FakeHookRunner>,
    written: Arc<Mutex<Vec<String>>>,
    counter: AtomicUsize,
}

impl Default for FakeHooksHarness {
    fn default() -> Self {
        Self {
            runner: Arc::new(FakeHookRunner::new().with_secret(Self::secret())),
            written: Arc::new(Mutex::new(Vec::new())),
            counter: AtomicUsize::new(0),
        }
    }
}

impl FakeHooksHarness {
    /// A fresh program name, so every scripted hook is its own.
    fn program(&self) -> String {
        format!("demo-hook-{}", self.counter.fetch_add(1, Ordering::Relaxed))
    }

    fn secret() -> &'static str {
        "hunter2-demo"
    }
}

#[async_trait]
impl HooksHarness for FakeHooksHarness {
    fn runner(&self) -> &dyn app::ports::HookRunner {
        self.runner.as_ref()
    }

    fn hook_logging(&self, label: &str) -> domain::planning::HookEntry {
        let program = self.program();
        let written = self.written.clone();
        let label = label.to_owned();
        self.runner.simulate(&program, move |_, _| {
            if let Ok(mut lines) = written.lock() {
                lines.push(format!("log: {label}"));
            }
            outcome(HookStatus::Succeeded)
        });
        no_op_entry(&program)
    }

    fn hook_env(&self, arg: &str) -> domain::planning::HookEntry {
        let program = self.program();
        let written = self.written.clone();
        self.runner.simulate(&program, move |entry, context| {
            if let Ok(mut lines) = written.lock() {
                for argument in entry.command.args() {
                    lines.push(format!("argv: {argument}"));
                }
                lines.push("env: PATH=/usr/bin".to_owned());
                lines.push("env: HOME=/home/demo".to_owned());
                lines.push(format!("env: LADDERBACK_HOOK={}", context.phase.as_env()));
                lines.push(format!("env: LADDERBACK_PLAN={}", context.plan.as_str()));
                lines.push(format!(
                    "env: LADDERBACK_REPOSITORY={}",
                    context.repository.as_str()
                ));
                lines.push(format!(
                    "env: LADDERBACK_OPERATION_ID={}",
                    context.operation.as_uuid()
                ));
            }
            outcome(HookStatus::Succeeded)
        });
        domain::planning::HookEntry::new(&program, [arg], domain::planning::OnFailure::Error)
            .expect("the demo hook is valid")
    }

    fn hook_exits(&self, code: i32, text: &str) -> domain::planning::HookEntry {
        let program = self.program();
        self.runner.script(
            &program,
            app::fakes::Scripted {
                status: HookStatus::Failed {
                    exit_code: Some(code),
                },
                stdout: text.to_owned(),
                stderr: text.to_owned(),
            },
        );
        no_op_entry(&program)
    }

    fn hook_hangs(&self) -> domain::planning::HookEntry {
        let program = self.program();
        self.runner
            .script(&program, app::fakes::Scripted::times_out());
        no_op_entry(&program)
    }

    fn hook_missing(&self) -> domain::planning::HookEntry {
        no_op_entry("no-such-demo-program")
    }

    fn hook_prints(&self, text: &str) -> domain::planning::HookEntry {
        let program = self.program();
        self.runner.script(
            &program,
            app::fakes::Scripted {
                status: HookStatus::Succeeded,
                stdout: text.to_owned(),
                stderr: String::new(),
            },
        );
        no_op_entry(&program)
    }

    fn secret(&self) -> String {
        Self::secret().to_owned()
    }

    fn leaked_env_var(&self) -> String {
        "LADDERBACK_DEMO_SECRET".to_owned()
    }

    async fn written(&self) -> Vec<String> {
        self.written
            .lock()
            .map(|lines| lines.clone())
            .unwrap_or_default()
    }
}

/// A quiet success outcome.
fn outcome(status: HookStatus) -> HookOutcome {
    HookOutcome {
        status,
        stdout: HookOutput::default(),
        stderr: HookOutput::default(),
    }
}

/// An entry whose program is `program`: the fake behaves as scripted.
fn no_op_entry(program: &str) -> domain::planning::HookEntry {
    domain::planning::HookEntry::new(program, ["demo"], domain::planning::OnFailure::Error).unwrap()
}

#[tokio::test]
async fn fake_hooks_meet_the_hooks_contract() {
    contract::hooks::run(&FakeHooksHarness::default()).await;
}

struct InMemoryEventBusHarness;

#[async_trait]
impl contract::event_bus::EventBusHarness for InMemoryEventBusHarness {
    fn bus(&self) -> Arc<dyn EventBus> {
        Arc::new(InMemoryEventBus::new())
    }
}

#[tokio::test]
async fn the_in_memory_event_bus_meets_the_event_bus_contract() {
    contract::event_bus::run(&InMemoryEventBusHarness).await;
}
