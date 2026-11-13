//! The in-memory fakes pass the same contract suites as the real adapters.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app::contract;
use app::contract::clock::ClockHarness;
use app::contract::hooks::HooksHarness;
use app::contract::secrets::SecretsHarness;
use app::fakes::{
    FakeClock, FakeHookRunner, FakeSecrets, InMemoryEventBus, InMemoryOperationStore, Script,
    SimulatedEngine,
};
use app::fakes::{InMemoryConfigStore, InMemoryScheduleStore, SeededRandom};
use app::ports::{
    Clock, Engine, EventBus, HookOutcome, HookOutput, HookStatus, ScheduleStore, SecretResolver,
};
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
async fn in_memory_operation_store_recovers_its_unfinished_operations() {
    contract::operation_store::run_recovery(&InMemoryOperationStore::new()).await;
}

/// The in-memory schedule store: a new one for each `empty`, and the same
/// one again for `reopened`, as the real store's file outlives its handle.
#[derive(Default)]
struct InMemorySchedules(Mutex<Option<Arc<InMemoryScheduleStore>>>);

#[async_trait]
impl contract::schedule_store::ScheduleHarness for InMemorySchedules {
    async fn empty(&self) -> Arc<dyn ScheduleStore> {
        let store = Arc::new(InMemoryScheduleStore::new());
        *self.0.lock().unwrap() = Some(Arc::clone(&store));
        store
    }

    async fn reopened(&self) -> Arc<dyn ScheduleStore> {
        self.0.lock().unwrap().clone().expect("a store was opened")
    }
}

#[tokio::test]
async fn in_memory_schedule_store_meets_the_schedule_store_contract() {
    contract::schedule_store::run(&InMemorySchedules::default()).await;
}

#[tokio::test]
async fn in_memory_schedule_store_keeps_each_plans_pending_retry() {
    contract::schedule_store::run_retries(&InMemorySchedules::default()).await;
}

#[test]
fn seeded_randomness_meets_the_random_contract() {
    contract::random::run(&SeededRandom::new(0x5eed));
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

    fn link(&self, name: &str, target: &str) -> String {
        let path = format!("/fake/secrets/{name}");
        self.secrets.link(&path, target);
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

/// The fake reader gives the profile a harness method scripted, at the path
/// the method returns, and the fake writer renders in memory.
struct FakeProfilesHarness {
    reader: Arc<app::fakes::FakeProfileReader>,
    writer: Arc<app::fakes::FakeProfileWriter>,
}

impl FakeProfilesHarness {
    fn new() -> Self {
        Self {
            reader: Arc::new(app::fakes::FakeProfileReader::new(Err(
                app::ports::ProfileError::Unreadable {
                    file: "nothing scripted".to_owned(),
                },
            ))),
            writer: Arc::new(app::fakes::FakeProfileWriter::new()),
        }
    }

    /// Scripts a profile of the plan `home` into `repository`, at `location`.
    fn script_profile(&self, repository: &domain::RepositoryAlias, location: &str) -> PathBuf {
        use domain::planning::{
            GroupBy, Hooks, Maintenance, Plan, RepositoryConfig, RepositorySettings, Source,
        };
        use domain::secrets::{FileRead, SecretRef};
        let plan = Plan::new(
            domain::PlanId::parse("home").expect("a plan ID"),
            repository.clone(),
            vec![Source::parse("/home/demo").expect("a source")],
        );
        let settings = RepositorySettings {
            repository: location.to_owned(),
            repo_hot: None,
            password: SecretRef::File {
                path: "/srv/demo-password".to_owned(),
                read: FileRead::FirstLine,
            },
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        };
        self.reader.script(Ok(app::ports::ImportedProfile {
            repository: RepositoryConfig {
                locations: Default::default(),
                id: None,
                settings,
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
            plans: vec![plan],
            hooks: Hooks::default(),
            notices: Vec::new(),
        }));
        PathBuf::from("/demo/nas.toml")
    }
}

impl contract::profiles::ProfilesHarness for FakeProfilesHarness {
    fn reader(&self) -> Arc<dyn app::ports::ProfileReader> {
        self.reader.clone()
    }

    fn writer(&self) -> Arc<dyn app::ports::ProfileWriter> {
        self.writer.clone()
    }

    fn good_profile(&self, repository: &domain::RepositoryAlias) -> PathBuf {
        self.script_profile(repository, "/srv/demo-repo")
    }

    fn reference_profile(&self, repository: &domain::RepositoryAlias) -> PathBuf {
        // What the real reader hands back for the text `${file:…}`: escaped.
        self.script_profile(repository, "s3:$${file:/srv/demo-endpoint}")
    }

    fn broken_profile(&self) -> PathBuf {
        let file = PathBuf::from("/demo/broken.toml");
        self.reader.script(Err(app::ports::ProfileError::Invalid {
            file: file.display().to_string(),
            key: None,
            at: None,
            problem: app::ports::ProfileProblem::Syntax,
            detail: String::new(),
        }));
        file
    }

    fn missing_profile(&self) -> PathBuf {
        let file = PathBuf::from("/demo/missing.toml");
        self.reader
            .script(Err(app::ports::ProfileError::Unreadable {
                file: file.display().to_string(),
            }));
        file
    }
}

#[tokio::test]
async fn the_fake_profile_reader_and_writer_meet_the_profiles_contract() {
    contract::profiles::run(&FakeProfilesHarness::new()).await;
}

/// The fake power supply, told what to say.
struct FakePowerHarness;

impl FakePowerHarness {
    fn saying(
        answer: Result<domain::execution::conditions::PowerSource, app::ports::PlatformError>,
    ) -> Arc<dyn app::ports::PowerSupply> {
        let power = Arc::new(app::fakes::FakePower::new());
        power.set(answer);
        power
    }
}

impl contract::power::PowerHarness for FakePowerHarness {
    fn on_ac(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Ok(domain::execution::conditions::PowerSource::Ac))
    }

    fn on_battery(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Ok(domain::execution::conditions::PowerSource::Battery))
    }

    fn without_battery(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Ok(domain::execution::conditions::PowerSource::Ac))
    }

    fn battery_not_discharging(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Ok(domain::execution::conditions::PowerSource::Ac))
    }

    fn unrecognized(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Err(app::ports::PlatformError::Unrecognized {
            from: "fake".to_owned(),
            detail: "Solar".to_owned(),
        }))
    }

    fn unreadable(&self) -> Arc<dyn app::ports::PowerSupply> {
        Self::saying(Err(app::ports::PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "gone".to_owned(),
        }))
    }
}

/// The fake answers from its own memory.
struct FakeService(app::fakes::FakeServiceManager);

#[async_trait]
impl contract::service::ServiceHarness for FakeService {
    fn manager(&self) -> &dyn app::ports::ServiceManager {
        &self.0
    }

    fn programs(&self) -> [domain::service::ServicePath; 2] {
        [
            "/home/demo/bin/ladderback",
            "/opt/ladderback/bin/ladderback",
        ]
        .map(|path| domain::service::ServicePath::parse(std::path::Path::new(path)).unwrap())
    }

    async fn runs(&self) -> bool {
        self.0.runs()
    }

    fn file_text(&self) -> Option<String> {
        self.0.file_text()
    }
}

#[tokio::test]
async fn the_fake_power_supply_meets_the_power_contract() {
    contract::power::run(&FakePowerHarness).await;
}

#[tokio::test]
async fn the_fake_power_supply_is_on_ac_power_until_told_otherwise() {
    use app::ports::PowerSupply;
    let power = app::fakes::FakePower::new();
    assert_eq!(power.asked(), 0);
    assert_eq!(
        power.source().await,
        Ok(domain::execution::conditions::PowerSource::Ac)
    );
    assert_eq!(power.asked(), 1);
    let _ = power.source().await;
    assert_eq!(power.asked(), 2);
}

/// The fake network, told what to say.
struct FakeNetworkHarness;

impl FakeNetworkHarness {
    fn saying(answer: Result<bool, app::ports::PlatformError>) -> Arc<dyn app::ports::Network> {
        let network = Arc::new(app::fakes::FakeNetwork::new());
        network.set(answer);
        network
    }
}

impl contract::network::NetworkHarness for FakeNetworkHarness {
    fn online(&self) -> Arc<dyn app::ports::Network> {
        Self::saying(Ok(true))
    }

    fn offline(&self) -> Arc<dyn app::ports::Network> {
        Self::saying(Ok(false))
    }

    fn only_loopback(&self) -> Arc<dyn app::ports::Network> {
        Self::saying(Ok(false))
    }

    fn unrecognized(&self) -> Arc<dyn app::ports::Network> {
        Self::saying(Err(app::ports::PlatformError::Unrecognized {
            from: "fake".to_owned(),
            detail: "maybe".to_owned(),
        }))
    }

    fn unreadable(&self) -> Arc<dyn app::ports::Network> {
        Self::saying(Err(app::ports::PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "gone".to_owned(),
        }))
    }
}

#[tokio::test]
async fn the_fake_network_meets_the_network_contract() {
    contract::network::run(&FakeNetworkHarness).await;
}

/// The fake network link, told what to say.
struct FakeNetworkLinkHarness;

impl FakeNetworkLinkHarness {
    fn saying(
        answer: Result<domain::planning::Link, app::ports::PlatformError>,
    ) -> Arc<dyn app::ports::NetworkLink> {
        let link = Arc::new(app::fakes::FakeNetworkLink::new());
        link.set(answer);
        link
    }
}

impl contract::network_link::NetworkLinkHarness for FakeNetworkLinkHarness {
    fn wired(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Ok(domain::planning::Link::Wired))
    }

    fn wired_reads_as(&self) -> domain::planning::Link {
        domain::planning::Link::Wired
    }

    fn wifi(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Ok(domain::planning::Link::WiFi))
    }

    fn tunnel(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Ok(domain::planning::Link::Other))
    }

    fn no_route(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Ok(domain::planning::Link::NoRoute))
    }

    fn unrecognized(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Err(app::ports::PlatformError::Unrecognized {
            from: "fake".to_owned(),
            detail: "maybe".to_owned(),
        }))
    }

    fn unreadable(&self) -> Arc<dyn app::ports::NetworkLink> {
        Self::saying(Err(app::ports::PlatformError::Unreadable {
            from: "fake".to_owned(),
            detail: "gone".to_owned(),
        }))
    }
}

#[tokio::test]
async fn the_fake_network_link_meets_the_network_link_contract() {
    contract::network_link::run(&FakeNetworkLinkHarness).await;
}

#[tokio::test]
async fn the_fake_network_link_counts_each_reading() {
    let link = app::fakes::FakeNetworkLink::new();
    assert_eq!(
        app::ports::NetworkLink::link(&link).await,
        Ok(domain::planning::Link::Wired)
    );
    assert_eq!(link.asked(), 1);
}

#[tokio::test]
async fn the_fake_network_is_online_until_told_otherwise() {
    use app::ports::Network;
    let network = app::fakes::FakeNetwork::new();
    assert_eq!(network.asked(), 0);
    assert_eq!(network.is_online().await, Ok(true));
    assert_eq!(network.asked(), 1);
    let _ = network.is_online().await;
    assert_eq!(network.asked(), 2);
}

/// While the machine sleeps, wall-clock time moves on and monotonic time
/// stands still: a sleeper on the wall clock wakes, one on monotonic time
/// waits on for the rest of its time.
#[tokio::test]
async fn a_suspended_fake_clock_moves_only_its_wall_clock() {
    use futures::FutureExt;
    let clock = FakeClock::new(epoch());
    let monotonic = clock.monotonic();
    let mut on_the_wall = clock.sleep_until(epoch() + SignedDuration::from_mins(1));
    let mut monotonic_sleeper = clock.sleep(SignedDuration::from_mins(1));
    assert!((&mut on_the_wall).now_or_never().is_none());
    assert!((&mut monotonic_sleeper).now_or_never().is_none());

    clock.suspend(SignedDuration::from_hours(3));
    assert_eq!(clock.now(), epoch() + SignedDuration::from_hours(3));
    assert_eq!(clock.monotonic(), monotonic, "monotonic time stood still");
    assert!((&mut on_the_wall).now_or_never().is_some());
    assert!((&mut monotonic_sleeper).now_or_never().is_none());

    clock.advance(SignedDuration::from_secs(59));
    assert!((&mut monotonic_sleeper).now_or_never().is_none());
    clock.advance(SignedDuration::from_secs(1));
    assert!(monotonic_sleeper.now_or_never().is_some());
    assert_eq!(clock.monotonic(), monotonic + SignedDuration::from_mins(1));
}

/// An auto-advancing fake clock jumps straight to a monotonic sleeper's
/// time, as it does to a wall-clock deadline.
#[tokio::test]
async fn an_auto_advancing_fake_clock_jumps_to_a_monotonic_sleepers_time() {
    let clock = FakeClock::auto_advancing(epoch());
    clock.sleep(SignedDuration::from_mins(5)).await;
    assert_eq!(clock.now(), epoch() + SignedDuration::from_mins(5));
    assert_eq!(clock.monotonic(), SignedDuration::from_mins(5));
}

#[tokio::test]
async fn in_memory_admin_password_meets_the_admin_password_contract() {
    contract::admin_password::run(&app::fakes::InMemoryAdminPassword::default()).await;
}

#[tokio::test]
async fn fake_service_manager_meets_the_service_manager_contract() {
    let harness = FakeService(app::fakes::FakeServiceManager::new(PathBuf::from(
        "/home/demo/.config/systemd/user/ladderback.service",
    )));
    contract::service::run(&harness).await;
}

/// Channels for the recording fake: each URL is fresh, and one that doesn't
/// accept is told to fail as its behaviour says.
#[derive(Default)]
struct FakeNotifierHarness {
    notifier: app::fakes::RecordingNotifier,
    next: AtomicUsize,
}

#[async_trait]
impl contract::notifier::NotifierHarness for FakeNotifierHarness {
    fn notifier(&self) -> &dyn app::ports::Notifier {
        &self.notifier
    }

    fn url(
        &self,
        kind: domain::notifications::ChannelKind,
        behaviour: contract::notifier::Behaviour,
    ) -> String {
        use app::ports::NotifyError;
        use contract::notifier::Behaviour;
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        let url = format!("http://127.0.0.1/{}/{number}", kind.name());
        let failure = match behaviour {
            Behaviour::Accepts => None,
            Behaviour::Refuses => Some(NotifyError::Unreachable),
            Behaviour::Hangs => Some(NotifyError::TimedOut),
            Behaviour::Answers(status) if (200..300).contains(&status) => None,
            Behaviour::Answers(status) => Some(NotifyError::Rejected { status }),
        };
        if let Some(error) = failure {
            self.notifier.fail_at(&url, error);
        }
        url
    }

    async fn received(&self) -> Vec<contract::notifier::Received> {
        use app::ports::{Credentials, Message};
        use contract::notifier::{Received, ReceivedAuth};
        use domain::notifications::ChannelKind;
        self.notifier
            .delivered()
            .into_iter()
            .map(|notification| {
                let kind = notification.destination.kind;
                let pinged = kind == ChannelKind::Healthchecks;
                Received {
                    kind,
                    ping: pinged.then(|| notification.event.kind.ping()),
                    event: if pinged {
                        String::new()
                    } else {
                        notification.event.kind.name().to_owned()
                    },
                    message: match notification.message {
                        Message::Default => {
                            format!("default text of {}", notification.event.kind.name())
                        }
                        Message::Custom(text) => text,
                    },
                    auth: notification.destination.auth.map(|auth| match auth {
                        Credentials::Bearer(token) => {
                            ReceivedAuth::Bearer(token.expose().to_owned())
                        }
                        Credentials::Basic { username, password } => ReceivedAuth::Basic {
                            username: username.as_str().to_owned(),
                            password: password.expose().to_owned(),
                        },
                    }),
                }
            })
            .collect()
    }
}

#[tokio::test]
async fn the_recording_notifier_meets_the_notifier_contract() {
    contract::notifier::run(&FakeNotifierHarness::default()).await;
}
