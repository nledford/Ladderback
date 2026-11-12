//! Steps for `features/security/events.feature`: the real `ladderback serve`
//! process, spoken to over a real WebSocket, so the scenarios check what a
//! browser tab sees, not what the router says it would do.
//!
//! Commit 1 of the M1b event stream: the steps are complete and fail against
//! the stubbed daemon (development.md §13.1). The daemon reads its
//! configuration from `LADDERBACK_CONFIG_DIR`, which every step here points
//! at a folder of the scenario's scratch folder; its data folder is another.

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use cucumber::{given, then, when};
use futures::StreamExt;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;
use tokio_tungstenite::{WebSocketStream, connect_async};

use super::{Agent, blocking};

/// How long a page may wait for an event a run already published.
const EVENT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a daemon told to stop may take to exit: the 20 s it gives the
/// operation log to close, and a few seconds for the rest.
const STOP_TIMEOUT: Duration = Duration::from_secs(23);
const LISTENING: &str = "listening on http://";

/// What the live-updates scenarios keep between steps.
#[derive(Default)]
pub struct Events {
    daemon: Option<Daemon>,
    /// The pages that read: one per "connection from the UI's own origin".
    pages: Vec<Page>,
    /// The page that connected and then never reads again. Held for the
    /// scenario's lifetime so the connection stays open.
    #[allow(dead_code)]
    stalled: Option<Page>,
    /// The outcome of asking a daemon to serve that was expected to refuse.
    asked_to_serve: Option<Outcome>,
    /// A second daemon that started on the folders the first one holds, kept
    /// so the scenario's end stops it.
    #[allow(dead_code)]
    second: Option<Daemon>,
    /// How the daemon ended once it was told to stop.
    stopped: Option<ExitStatus>,
}

/// A running daemon, stopped when the scenario ends.
struct Daemon {
    child: Child,
    addr: SocketAddr,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One connected tab.
struct Page {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl Events {
    fn daemon(&self) -> &Daemon {
        self.daemon.as_ref().expect("Ladderback is serving")
    }

    /// Where the daemon is serving, for the operation-lifecycle steps.
    pub fn addr(&self) -> SocketAddr {
        self.daemon().addr
    }

    /// The scratch folder of the daemon these scenarios start.
    fn root(agent: &Agent) -> PathBuf {
        agent.scratch().join("events")
    }

    /// The data folder of that daemon.
    pub(crate) fn data_dir(agent: &Agent) -> PathBuf {
        testkit::daemon::data_dir(&Self::root(agent))
    }

    /// The cache folder of that daemon.
    pub(crate) fn cache_dir(agent: &Agent) -> PathBuf {
        testkit::daemon::cache_dir(&Self::root(agent))
    }

    /// The process ID of the daemon that is serving.
    pub(crate) fn pid(&self) -> u32 {
        self.daemon().child.id()
    }

    /// Kills the daemon with SIGKILL and waits for the process to be gone:
    /// a crash, as far as the folders it leaves behind can tell.
    pub(crate) fn kill(&mut self) {
        let daemon = self.daemon.take().expect("Ladderback is serving");
        drop(daemon);
    }

    /// Tells the daemon to stop, with the SIGTERM systemd and launchd send,
    /// and waits for it to exit. Runs on a blocking thread: see [`blocking`].
    pub(crate) async fn stop(agent: &mut Agent) {
        let mut daemon = agent.events.daemon.take().expect("Ladderback is serving");
        let status = blocking(move || {
            // `kill` sends it without a crate of its own for that, as
            // `tests/logging.rs` does.
            let mut kill = Command::new("kill");
            kill.arg("-TERM").arg(daemon.child.id().to_string());
            let sent = spawn_gate::hold(|| kill.spawn())
                .expect("kill runs")
                .wait()
                .expect("kill ran");
            assert!(sent.success(), "the daemon is sent SIGTERM: {sent}");
            let began = std::time::Instant::now();
            loop {
                if let Some(status) = daemon.child.try_wait().expect("the daemon's status") {
                    return status;
                }
                assert!(
                    began.elapsed() < STOP_TIMEOUT,
                    "the daemon was still running {STOP_TIMEOUT:?} after SIGTERM"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        })
        .await;
        agent.events.stopped = Some(status);
    }

    /// How the daemon ended once it was told to stop.
    pub(crate) fn stopped(&self) -> ExitStatus {
        self.stopped.expect("the daemon was told to stop")
    }

    /// The config folder of that daemon.
    pub fn config_dir(agent: &Agent) -> PathBuf {
        testkit::daemon::config_dir(&Self::root(agent))
    }

    /// Reads page `page`'s events until one of `kinds` arrives, and returns it.
    async fn learn(&mut self, page: usize, kinds: &[&str]) -> serde_json::Value {
        let page = &mut self.pages[page];
        let deadline = tokio::time::sleep(EVENT_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            let next = tokio::select! {
                _ = &mut deadline => panic!(
                    "no {kinds:?} event within {} s",
                    EVENT_TIMEOUT.as_secs()
                ),
                message = page.socket.next() => message,
            };
            let message = match next.expect("the live-updates connection is open") {
                Ok(message) => message,
                Err(error) => panic!("the live-updates connection broke: {error}"),
            };
            let text = match message {
                Message::Text(text) => text,
                Message::Close(_) => panic!("the daemon closed the live-updates connection"),
                _ => continue,
            };
            let event: serde_json::Value = serde_json::from_str(text.as_str())
                .unwrap_or_else(|error| panic!("the event is JSON: {error}"));
            let kind = event
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            if let Some(kind) = kind
                && kinds.contains(&kind.as_str())
            {
                return event;
            }
        }
    }
}

/// Starts `ladderback serve` with its folders in the scratch dir, and
/// waits until it listens or exits. The wait is on a blocking thread: see
/// [`blocking`].
async fn start(agent: &mut Agent) -> Outcome {
    let root = Events::root(agent);
    let (daemon, outcome) = blocking(move || spawn_daemon(&root)).await;
    if daemon.is_some() {
        agent.events.daemon = daemon;
    }
    outcome
}

/// Starts the daemon again on the folders a stopped one left, for the
/// scenarios that restart it, and waits until it listens.
pub(crate) async fn restart(agent: &mut Agent) {
    match start(agent).await {
        Outcome::Listening(_) => {}
        Outcome::Exited(status, log) => panic!("ladderback exited with {status}: {log}"),
    }
}

/// Starts a second daemon on the folders the first one serves from, and
/// keeps what came of it for [`refusal`].
pub(crate) async fn start_second(agent: &mut Agent) {
    let root = Events::root(agent);
    let (daemon, outcome) = blocking(move || spawn_daemon(&root)).await;
    agent.events.second = daemon;
    agent.events.asked_to_serve = Some(outcome);
}

/// How the second daemon ended: its exit status and its log. Fails the step
/// when it listened instead.
pub(crate) fn refusal(agent: &mut Agent) -> (ExitStatus, String) {
    match agent.events.asked_to_serve.take() {
        Some(Outcome::Listening(addr)) => {
            panic!("the second daemon listened on {addr} instead of refusing")
        }
        Some(Outcome::Exited(status, log)) => (status, log),
        None => panic!("a second Ladderback was started first"),
    }
}

/// Starts the daemon and reads its log until it says where it listens or
/// exits; the daemon is there when it listens.
fn spawn_daemon(root: &std::path::Path) -> (Option<Daemon>, Outcome) {
    let mut command = Command::new(super::LADDERBACK);
    testkit::daemon::isolate(&mut command, root);
    command
        .args(["serve", "--listen", "127.0.0.1:0"])
        .stderr(Stdio::piped());
    let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback serve starts");
    let stderr = child.stderr.take().expect("the daemon's stderr");
    let mut log = String::new();
    let mut lines = BufReader::new(stderr).lines();
    let listening = loop {
        let line = match lines.next() {
            Some(Ok(line)) => line,
            Some(Err(error)) => {
                log.push_str(&format!("stderr unreadable: {error}"));
                break None;
            }
            None => break None,
        };
        log.push_str(&line);
        log.push('\n');
        if let Some(url) = line.split_once(LISTENING).map(|(_, url)| url)
            && let Ok(addr) = url.trim().parse::<SocketAddr>()
        {
            break Some(addr);
        }
    };
    match listening {
        Some(addr) => {
            // Keep draining the daemon's log so a full pipe can never block it.
            std::thread::spawn(move || {
                for line in lines.by_ref() {
                    if line.is_err() {
                        return;
                    }
                }
            });
            (Some(Daemon { child, addr }), Outcome::Listening(addr))
        }
        None => {
            let status = child.wait().expect("the daemon exits");
            (None, Outcome::Exited(status, log))
        }
    }
}

/// What asking a daemon to serve led to.
enum Outcome {
    Listening(SocketAddr),
    Exited(ExitStatus, String),
}

/// Sends `request` over a new connection and reads one response: status line
/// and body, nothing parsed further.
fn post(agent: &Agent, path: &str, body: &str) -> (u16, String) {
    let addr = agent.events.daemon().addr;
    let mut stream = testkit::net::connect(addr).expect("the daemon accepts a connection");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {}\r\nOrigin: http://{addr}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        addr,
        body.len()
    );
    use std::io::{Read, Write};
    stream
        .write_all(request.as_bytes())
        .expect("the request is sent");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("a response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .split_once(' ')
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
}

/// Connects one tab's WebSocket from the UI's own origin.
async fn connect(agent: &Agent) -> Page {
    let addr = agent.events.daemon().addr;
    let url = format!("ws://{addr}/api/v1/events");
    let mut request = url.into_client_request().expect("a WebSocket URL");
    request.headers_mut().insert(
        ORIGIN,
        format!("http://{addr}")
            .parse()
            .expect("the UI's own origin"),
    );
    let (socket, _response) = connect_async(request)
        .await
        .expect("the live-updates handshake from the UI's own origin");
    Page { socket }
}

#[given(expr = "Ladderback is serving on loopback with the plan {string}")]
async fn serving_with_plan(agent: &mut Agent, plan: String) {
    // A real plan the daemon can run: a real scratch repository, a folder of
    // demo files, and a config.toml naming both.
    super::backup_steps::new_repository(agent).await;
    super::backup_steps::source_with_files(agent, &plan, 3);
    save_daemon_config(agent, None).await;
    match start(agent).await {
        Outcome::Listening(_) => {}
        Outcome::Exited(status, log) => panic!("ladderback exited with {status}: {log}"),
    }
}

/// Writes the daemon's config.toml: the scenario's scratch repository as
/// `demo` and its plan over the scenario's source folder, with the plan's
/// host, source filter and hooks and the agent's hostname the scenario set,
/// keeping the last `keep_last` snapshots when given, and everything
/// otherwise. The daemon reads its configuration on every request, so
/// calling this again changes what a running daemon does next.
pub(crate) async fn save_daemon_config(agent: &Agent, keep_last: Option<i64>) {
    let repository = agent.backup.repository.clone().expect("a repository");
    let source = agent.backup.source.clone().expect("a plan source");
    let plan = agent.backup.plan.clone().expect("a plan");
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let mut plan = domain::planning::Plan::new(
        plan,
        alias.clone(),
        vec![
            domain::planning::Source::parse(source.to_str().expect("a UTF-8 path"))
                .expect("a source"),
        ],
    );
    if let Some(keep) = keep_last {
        plan.retention.keep_last =
            Some(domain::planning::KeepCount::new(keep).expect("a keep count"));
    }
    plan.host = agent.backup.plan_host.clone();
    plan.filter = agent.backup.filter.clone();
    plan.hooks = agent.backup.plan_hooks.clone();
    let settings = domain::planning::AgentSettings {
        hostname: agent.backup.agent_hostname.clone(),
        ..domain::planning::AgentSettings::default()
    };
    let config = domain::planning::AgentConfig::new(
        settings,
        domain::planning::Hooks::default(),
        std::collections::BTreeMap::from([(
            alias,
            domain::planning::RepositoryConfig {
                // What the configuration recorded when the repository was
                // connected, once the scenario has made it known.
                id: agent.backup.id.clone(),
                settings: domain::planning::RepositorySettings {
                    repository: repository.to_str().expect("a UTF-8 path").to_owned(),
                    repo_hot: None,
                    password: domain::secrets::SecretRef::Inline(
                        domain::secrets::SecretValue::new(super::backup_steps::PASSWORD),
                    ),
                    options: Default::default(),
                    options_hot: Default::default(),
                    options_cold: Default::default(),
                },
                // This agent maintains it, so the maintenance scenarios run.
                maintenance: domain::planning::Maintenance::Local,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        )]),
        vec![plan],
    )
    .expect("a valid configuration");
    let dir = Events::config_dir(agent);
    std::fs::create_dir_all(&dir).expect("the config folder");
    app::ports::ConfigStore::save(
        &config_adapter::FileConfigStore::new(
            dir.join("config.toml"),
            std::sync::Arc::new(platform::SystemClock),
        ),
        &config,
    )
    .await
    .expect("the config saves");
}

#[given("Ladderback is serving on loopback with no repositories")]
async fn serving_with_no_repositories(agent: &mut Agent) {
    // No config file and no config folder: the daemon's first run.
    match start(agent).await {
        Outcome::Listening(_) => {}
        Outcome::Exited(status, log) => panic!("ladderback exited with {status}: {log}"),
    }
}

#[given("a live-updates connection from the UI's own origin")]
async fn a_connection(agent: &mut Agent) {
    let page = connect(agent).await;
    agent.events.pages.push(page);
}

#[given("a second live-updates connection from the UI's own origin")]
async fn a_second_connection(agent: &mut Agent) {
    let page = connect(agent).await;
    agent.events.pages.push(page);
}

#[given("a live-updates connection that stops reading")]
async fn a_stalled_connection(agent: &mut Agent) {
    let page = connect(agent).await;
    agent.events.stalled = Some(page);
}

#[given("the config folder holds a config.toml from a newer schema")]
fn newer_schema(agent: &mut Agent) {
    let dir = Events::config_dir(agent);
    std::fs::create_dir_all(&dir).expect("the config folder");
    std::fs::write(dir.join("config.toml"), "schema = 999999\n").expect("the config file");
}

#[when("Ladderback is asked to serve")]
async fn asked_to_serve(agent: &mut Agent) {
    let outcome = start(agent).await;
    agent.events.asked_to_serve = Some(outcome);
}

#[when(expr = "the daemon runs a backup of the plan {string}")]
fn runs_a_backup(agent: &mut Agent, plan: String) {
    let body = format!("{{\"plan\":{plan:?}}}");
    let (status, response) = post(agent, "/api/v1/operations", &body);
    assert!(
        (200..300).contains(&status),
        "the run was accepted: {status} {response}"
    );
}

#[then("the connected page learns the operation started")]
async fn learns_started(agent: &mut Agent) {
    let event = agent.events.learn(0, &["operation-started"]).await;
    assert!(
        event.get("operation").is_some(),
        "the event names its operation"
    );
}

#[then("the connected page learns the operation's progress")]
async fn learns_progress(agent: &mut Agent) {
    agent.events.learn(0, &["operation-progress"]).await;
}

#[then("the connected page learns the operation finished")]
async fn learns_finished(agent: &mut Agent) {
    agent
        .events
        .learn(0, &["operation-succeeded", "operation-failed"])
        .await;
}

#[then("each connected page learns the operation started")]
async fn each_learns_started(agent: &mut Agent) {
    for page in 0..agent.events.pages.len() {
        let event = agent.events.learn(page, &["operation-started"]).await;
        assert!(
            event.get("operation").is_some(),
            "the event names its operation"
        );
    }
}

#[then("each connected page learns the operation finished")]
async fn each_learns_finished(agent: &mut Agent) {
    for page in 0..agent.events.pages.len() {
        agent
            .events
            .learn(page, &["operation-succeeded", "operation-failed"])
            .await;
    }
}

#[then("the daemon is still serving")]
fn still_serving(agent: &mut Agent) {
    let addr = agent.events.daemon().addr;
    let mut stream = testkit::net::connect(addr).expect("the daemon accepts a connection");
    use std::io::{Read, Write};
    let request = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("the request is sent");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("a response");
    let text = String::from_utf8_lossy(&raw);
    // Liveness, not the UI: a build without crates/web/dist answers 503, and
    // CI's Check job runs no trunk build. Any status line — and a body the
    // daemon produced — proves the stalled page cost it nothing.
    assert!(
        text.starts_with("HTTP/1.1 "),
        "the daemon answers no more:\n{text}"
    );
}

#[then("Ladderback refuses to start, naming the newer schema")]
fn refuses(agent: &mut Agent) {
    match agent.events.asked_to_serve.take() {
        Some(Outcome::Listening(addr)) => {
            panic!("the daemon started on {addr} instead of refusing")
        }
        Some(Outcome::Exited(status, log)) => {
            assert!(!status.success(), "a refusal is a failure: {status}");
            assert!(
                log.to_lowercase().contains("schema"),
                "the refusal names the schema:\n{log}"
            );
            assert!(
                log.contains("999999"),
                "the refusal names the version it found:\n{log}"
            );
        }
        None => panic!("Ladderback was asked to serve first"),
    }
}
