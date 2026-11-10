//! Steps for `features/security/operations.feature`: the real `ladderback
//! serve` process, spoken to in raw HTTP, so the scenarios follow what a page
//! of the UI sees — the history page's initial state, and cancel — not what
//! the router says it would do.
//!
//! Commit 1 of the E2 operation lifecycle: the steps are complete and fail
//! against the stubbed routes (development.md §13.1).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cucumber::{given, then, when};
use futures::SinkExt;
use futures::StreamExt;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::ORIGIN;
use tokio_tungstenite::{WebSocketStream, connect_async};

use super::Agent;

/// How long a page may wait for an operation to reach the expected state.
const STATE_TIMEOUT: Duration = Duration::from_secs(120);
/// How many more demo files a "many more files" source holds, so a backup
/// runs long enough to be seen running and cancelled.
const MANY_FILES: usize = 3000;

/// What the operation-lifecycle scenarios keep between steps.
#[derive(Default)]
pub struct Ops {
    /// The operations the scenario started, oldest first.
    started: Vec<String>,
    /// The typed code of the last refusal, when the last ask was refused.
    refusal: Option<String>,
    /// The page that holds a live-updates connection.
    page: Option<Page>,
}

impl std::fmt::Debug for Ops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ops")
            .field("started", &self.started)
            .field("refusal", &self.refusal)
            .finish_non_exhaustive()
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// One connected tab.
struct Page {
    socket: Socket,
}

fn hook_mark(agent: &Agent) -> PathBuf {
    agent.scratch().join("hook-ran")
}

/// The plan's source folder, as the serving step made it.
fn source(agent: &Agent) -> PathBuf {
    agent.scratch().join("source")
}

/// Writes the daemon's configuration again, with `hooks` around every
/// operation: the same repository, plan and source the serving step chose.
async fn save_config_with_hooks(agent: &Agent, run_finally: Option<String>) {
    let repository = agent
        .backup
        .repository
        .clone()
        .expect("a repository the plan backs up into");
    let source = source(agent);
    let alias = domain::RepositoryAlias::parse("demo").expect("a repository alias");
    let mut hooks = domain::planning::Hooks::default();
    if let Some(command) = run_finally {
        hooks.run_finally =
            vec![domain::planning::HookEntry::parse(&command).expect("a hook command")];
    }
    let config = domain::planning::AgentConfig::new(
        domain::planning::AgentSettings::default(),
        hooks,
        std::collections::BTreeMap::from([(
            alias.clone(),
            domain::planning::RepositoryConfig {
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
                maintenance: domain::planning::Maintenance::None,
                group_by: Default::default(),
                hooks: Default::default(),
            },
        )]),
        vec![domain::planning::Plan::new(
            domain::PlanId::parse("home").expect("a plan ID"),
            alias,
            vec![
                domain::planning::Source::parse(source.to_str().expect("a UTF-8 path"))
                    .expect("a source"),
            ],
        )],
    )
    .expect("a valid configuration");
    let dir = super::events_steps::Events::config_dir(agent);
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

/// Sends `request` over a new connection and reads one response: status and
/// body, nothing parsed further.
pub(crate) fn exchange(agent: &Agent, method: &str, path: &str, body: &str) -> (u16, String) {
    exchange_at(agent.events.addr(), method, path, body)
}

/// [`exchange`] with the daemon's address instead of an agent that knows it,
/// for a request sent from another thread.
pub(crate) fn exchange_at(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).expect("a connection to the daemon");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a read timeout");
    stream
        .write_all(request.as_bytes())
        .expect("the request is sent");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("a status line");
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status code");
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).expect("a header line");
        if line.trim_end().is_empty() {
            break;
        }
        headers.push(line.trim_end().to_owned());
    }
    let length = headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok());
    let mut raw = Vec::new();
    if let Some(length) = length {
        raw.resize(length, 0);
        reader.read_exact(&mut raw).expect("the response body");
    } else {
        reader.read_to_end(&mut raw).expect("the response body");
    }
    (status, String::from_utf8_lossy(&raw).into_owned())
}

/// One history page, as JSON.
pub(crate) async fn history_page(agent: &Agent, query: &str) -> serde_json::Value {
    let (status, body) = exchange(agent, "GET", &format!("/api/v1/operations?{query}"), "");
    assert_eq!(status, 200, "the history page answers: {body}");
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("the page is JSON: {error}: {body}"))
}

/// The ids of a history page's operations, in order.
fn page_ids(page: &serde_json::Value) -> Vec<String> {
    page.get("operations")
        .and_then(|ops| ops.as_array())
        .expect("the page lists operations")
        .iter()
        .map(|op| {
            op.get("id")
                .and_then(serde_json::Value::as_str)
                .expect("each operation has an id")
                .to_owned()
        })
        .collect()
}

/// Reads the history page until the operation `id` is there with `status`,
/// or fails when it ends any other way or never arrives. A state on the way
/// — `queued` before the engine starts — is not an ending: the page keeps
/// waiting.
pub(crate) async fn shows_status(agent: &Agent, id: &str, status: &str) {
    let started = Instant::now();
    loop {
        let page = history_page(agent, "limit=50").await;
        let found = page
            .get("operations")
            .and_then(|ops| ops.as_array())
            .map(|ops| {
                ops.iter()
                    .find(|op| op.get("id").and_then(serde_json::Value::as_str) == Some(id))
            });
        if let Some(Some(op)) = found {
            let now = op.get("status").and_then(serde_json::Value::as_str);
            if now == Some(status) {
                return;
            }
            if matches!(
                now,
                Some("succeeded" | "succeeded-with-warnings" | "failed" | "cancelled")
            ) {
                panic!("operation {id} is {now:?}, not {status:?}");
            }
        }
        assert!(
            started.elapsed() < STATE_TIMEOUT,
            "operation {id} never reached {status:?} within {} s",
            STATE_TIMEOUT.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Starts a backup of `plan`, returning the operation's ID.
pub(crate) fn start_backup(agent: &mut Agent, plan: &str) -> String {
    let body = format!("{{\"plan\":{plan:?}}}");
    let (status, response) = exchange(agent, "POST", "/api/v1/operations", &body);
    assert!(
        (200..300).contains(&status),
        "the run was accepted: {status} {response}"
    );
    serde_json::from_str::<serde_json::Value>(&response)
        .expect("the acceptance is JSON")
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .expect("the acceptance names the operation")
        .to_owned()
}

/// Reads the history page until the operation `id` has ended.
async fn wait_ended(agent: &Agent, id: &str) {
    let started = Instant::now();
    loop {
        let page = history_page(agent, "limit=50").await;
        let terminal = [
            "succeeded",
            "succeeded-with-warnings",
            "failed",
            "cancelled",
        ];
        let found = page
            .get("operations")
            .and_then(|ops| ops.as_array())
            .and_then(|ops| {
                ops.iter().find_map(|op| {
                    (op.get("id").and_then(serde_json::Value::as_str) == Some(id))
                        .then(|| op.get("status").and_then(serde_json::Value::as_str))
                        .flatten()
                })
            });

        if let Some(status) = found
            && terminal.contains(&status)
        {
            return;
        }
        assert!(
            started.elapsed() < STATE_TIMEOUT,
            "operation {id} never ended within {} s",
            STATE_TIMEOUT.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The typed code of a refusal body, when the body is one.
fn refusal_code(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

impl Ops {
    /// The operation the scenario means by "the operation": the newest it
    /// started.
    pub(crate) fn the_operation(&self) -> String {
        self.started
            .last()
            .cloned()
            .expect("the scenario started an operation")
    }

    /// Makes `id`, an operation the scenario did not start through the
    /// daemon, the one the scenario means by "the operation".
    pub(crate) fn note_started(&mut self, id: String) {
        self.started.push(id);
    }
}

#[given("the plan's source holds many more files")]
async fn many_more_files(agent: &mut Agent) {
    let source = source(agent);
    // Thousands of writes: on a blocking thread, as the runner's is shared.
    super::blocking(move || {
        for n in 1..=MANY_FILES {
            std::fs::write(
                source.join(format!("more-{n}.txt")),
                format!("demo file {n}\n"),
            )
            .expect("the demo file");
        }
    })
    .await;
}

#[given("the plan runs a finally hook that notes it ran")]
async fn a_finally_hook(agent: &mut Agent) {
    let mark = hook_mark(agent);
    let _ = std::fs::remove_file(&mark);
    let command = format!(
        "touch {}",
        mark.to_str().expect("a hook mark path without spaces")
    );
    save_config_with_hooks(agent, Some(command)).await;
}

#[when(expr = "the daemon starts a backup of the plan {string}")]
fn starts_a_backup(agent: &mut Agent, plan: String) {
    let id = start_backup(agent, &plan);
    agent.ops.started.push(id);
}

#[when(expr = "the daemon starts two backups of the plan {string}")]
fn starts_two_backups(agent: &mut Agent, plan: String) {
    for _ in 0..2 {
        let id = start_backup(agent, &plan);
        agent.ops.started.push(id);
    }
}

#[when(expr = "the daemon has finished {int} backups of the plan {string}")]
#[when(expr = "the daemon has finished {int} backup of the plan {string}")]
async fn finished_backups(agent: &mut Agent, count: usize, plan: String) {
    for _ in 0..count {
        let id = start_backup(agent, &plan);
        agent.ops.started.push(id.clone());
        wait_ended(agent, &id).await;
    }
}

#[when("the operation is cancelled")]
async fn cancel_the_operation(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    // The daemon answers a start before it has recorded the operation and
    // registered it to be cancelled, so a cancel sent at once can find
    // neither. Such a cancel is sent again: a refusal stands only when the
    // operation is unknown after a while, or has ended.
    let started = Instant::now();
    loop {
        cancel(agent, &id);
        let early = match agent.ops.refusal.as_deref() {
            Some("operation-unknown") => true,
            Some("operation-not-running") => {
                let page = history_page(agent, "limit=50").await;
                page.get("operations")
                    .and_then(|ops| ops.as_array())
                    .and_then(|ops| {
                        ops.iter().find(|op| {
                            op.get("id").and_then(serde_json::Value::as_str) == Some(&id)
                        })
                    })
                    .and_then(|op| op.get("status").and_then(serde_json::Value::as_str))
                    .is_some_and(|status| matches!(status, "queued" | "running"))
            }
            _ => false,
        };
        if !early || started.elapsed() > Duration::from_secs(10) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[when(expr = "the operation {string} is cancelled")]
fn cancel_a_named_operation(agent: &mut Agent, id: String) {
    agent.ops.started.push(id.clone());
    cancel(agent, &id);
}

/// Cancels `id`, remembering the refusal when it was refused.
fn cancel(agent: &mut Agent, id: &str) {
    let (status, body) = exchange(
        agent,
        "POST",
        &format!("/api/v1/operations/{id}/cancel"),
        "{}",
    );
    assert!(
        (200..300).contains(&status) || (400..500).contains(&status),
        "cancel answers: {status} {body}"
    );
    agent.ops.refusal = refusal_code(&body);
}

#[when(expr = "a history page is asked for with the cursor {string}")]
fn ask_with_cursor(agent: &mut Agent, cursor: String) {
    let (status, body) = exchange(
        agent,
        "GET",
        &format!("/api/v1/operations?after={cursor}"),
        "",
    );
    assert!(
        (400..500).contains(&status),
        "the cursor is refused: {status} {body}"
    );
    agent.ops.refusal = refusal_code(&body);
}

#[then(expr = "the refusal is typed {string}")]
fn the_refusal_is_typed(agent: &mut Agent, code: String) {
    assert_eq!(
        agent.ops.refusal.as_deref(),
        Some(code.as_str()),
        "the refusal's code"
    );
}

#[then("the history page shows the operation running")]
async fn shows_running(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    shows_status(agent, &id, "running").await;
}

#[then("the history page shows the operation succeeded")]
async fn shows_succeeded(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    shows_status(agent, &id, "succeeded").await;
}

#[then("the history page shows the operation cancelled")]
async fn shows_cancelled(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    shows_status(agent, &id, "cancelled").await;
}

#[then("the history page shows both operations succeeded")]
async fn shows_both_succeeded(agent: &mut Agent) {
    for id in agent.ops.started.clone() {
        shows_status(agent, &id, "succeeded").await;
    }
}

#[then("the history page lists them newest first")]
async fn newest_first(agent: &mut Agent) {
    let page = history_page(agent, "limit=10").await;
    let mut expected = agent.ops.started.clone();
    expected.reverse();
    assert_eq!(page_ids(&page), expected);
}

#[then("the history page after the newest operation's cursor omits it")]
async fn after_the_cursor(agent: &mut Agent) {
    let newest = agent.ops.the_operation();
    let page = history_page(agent, &format!("after={newest}&limit=10")).await;
    let ids = page_ids(&page);
    assert!(
        !ids.contains(&newest),
        "the newest operation is omitted: {ids:?}"
    );
    assert!(
        ids.len() >= 2,
        "the older operations are still there: {ids:?}"
    );
}

#[then(expr = "the history page filtered to the plan {string} lists all three")]
async fn filtered_to_the_plan(agent: &mut Agent, plan: String) {
    let page = history_page(agent, &format!("plan={plan}&limit=10")).await;
    let ids = page_ids(&page);
    for id in &agent.ops.started {
        assert!(
            ids.contains(id),
            "operation {id} is in the plan's page: {ids:?}"
        );
    }
}

#[then("a history page asked for 1000 operations answers, not refuses")]
async fn a_big_page_answers(agent: &mut Agent) {
    let page = history_page(agent, "limit=1000").await;
    assert!(
        page.get("operations").is_some(),
        "a limit over the cap is clamped, not refused"
    );
}

#[then("the plan's finally hook has run")]
fn the_hook_ran(agent: &mut Agent) {
    assert!(
        hook_mark(agent).exists(),
        "the finally hook's mark is there"
    );
}

#[given("a page has a live-updates connection")]
async fn a_page_connection(agent: &mut Agent) {
    let addr = agent.events.addr();
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
    agent.ops.page = Some(Page { socket });
}

#[when("the page pings the daemon")]
async fn pings_the_daemon(agent: &mut Agent) {
    let page = agent.ops.page.as_mut().expect("a connected page");
    page.socket
        .send(Message::Ping("demo".as_bytes().to_vec().into()))
        .await
        .expect("the ping is sent");
}

#[then("the page receives a pong")]
async fn receives_a_pong(agent: &mut Agent) {
    let page = agent.ops.page.as_mut().expect("a connected page");
    let deadline = tokio::time::sleep(STATE_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        let message = tokio::select! {
            _ = &mut deadline => panic!("no pong within {} s", STATE_TIMEOUT.as_secs()),
            message = page.socket.next() => message.expect("the connection is open"),
        };
        match message.expect("the page reads its socket") {
            Message::Pong(_) => return,
            Message::Text(_) | Message::Binary(_) => continue,
            other => panic!("a pong, not {other:?}"),
        }
    }
}
