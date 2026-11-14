//! Steps for `features/security/operations.feature`: the real `ladderback
//! serve` process, spoken to in raw HTTP, so the scenarios follow what a page
//! of the UI sees — the history page's initial state, and cancel — not what
//! the router says it would do.
//!
//! Commit 1 of the E2 operation lifecycle: the steps are complete and fail
//! against the stubbed routes (development.md §13.1).

use std::io::{BufRead, BufReader, Read, Write};
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
/// runs long enough for another to start beside it.
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
    /// The file in the plan's source the backup waits on, until it is let go.
    waited_on: Option<WaitedOn>,
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
    let mut stream = testkit::net::connect(addr).expect("a connection to the daemon");
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
    (status, String::from_utf8_lossy_owned(raw))
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
pub(crate) async fn wait_ended(agent: &Agent, id: &str) {
    let started = Instant::now();
    while !has_ended(agent, id).await {
        assert!(
            started.elapsed() < STATE_TIMEOUT,
            "operation {id} never ended within {} s",
            STATE_TIMEOUT.as_secs()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Whether the history page shows the operation `id` ended.
pub(crate) async fn has_ended(agent: &Agent, id: &str) -> bool {
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
    found.is_some_and(|status| terminal.contains(&status))
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
    let source = agent.backup.source.clone().expect("a plan source");
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
    // The agent's hooks run around every operation, the plan's among them.
    agent.backup.agent_hooks.run_finally =
        vec![domain::planning::HookEntry::parse(&command).expect("a hook command")];
    super::events_steps::save_daemon_config(agent, None).await;
}

#[given(
    expr = "the plan's repository reads its password from a command that prints {string} and then fails saying {string}"
)]
async fn a_failing_password_command(agent: &mut Agent, printed: String, said: String) {
    // `/bin/sh` is started directly, with no shell around it: the script is
    // one argument. What it prints is on stdout, where a password would be.
    let script = format!("printf '%s\\n' '{printed}'; printf '%s\\n' '{said}' >&2; exit 3");
    let command = domain::secrets::CommandLine::new("/bin/sh", ["-c".to_owned(), script])
        .expect("a command line");
    agent.backup.password = Some(domain::secrets::SecretRef::Command(command));
    super::events_steps::save_daemon_config(agent, None).await;
}

#[given(
    expr = "the plan's repository password in the configuration is {string}, which isn't the repository's"
)]
async fn a_wrong_password(agent: &mut Agent, password: String) {
    agent.backup.password = Some(domain::secrets::SecretRef::Inline(
        domain::secrets::SecretValue::new(&password),
    ));
    super::events_steps::save_daemon_config(agent, None).await;
}

#[given("the plan's repository folder is gone")]
fn the_repository_folder_is_gone(agent: &mut Agent) {
    let repository = agent.backup.repository.clone().expect("a repository");
    std::fs::remove_dir_all(&repository).expect("the repository folder removed");
}

#[when(expr = "the client starts a backup of the plan {string}")]
async fn the_client_starts_a_backup(agent: &mut Agent, plan: String) {
    let body = serde_json::json!({ "plan": plan }).to_string();
    super::repositories_steps::api_post(agent, "/api/v1/operations", &body).await;
}

#[then(expr = "the refusal names the setting {string} and the command {string}")]
fn the_refusal_names_the_command(agent: &mut Agent, setting: String, command: String) {
    let (_, json) = super::repositories_steps::api_answer(agent);
    let text = |value: String| serde_json::json!({"type": "text", "value": value});
    assert_eq!(json["args"]["field"], text(setting), "the refusal: {json}");
    assert_eq!(json["args"]["source"], text(command), "the refusal: {json}");
}

#[then(expr = "the refusal says the command said {string}")]
fn the_refusal_says_what_the_command_said(agent: &mut Agent, said: String) {
    let (_, json) = super::repositories_steps::api_answer(agent);
    assert_eq!(
        json["detail"],
        serde_json::json!(said),
        "the refusal: {json}"
    );
}

#[then("the history page lists no operation")]
async fn lists_no_operation(agent: &mut Agent) {
    let page = history_page(agent, "limit=50").await;
    assert_eq!(
        page.get("operations"),
        Some(&serde_json::json!([])),
        "the history page: {page}"
    );
}

/// The file a held after hook waits on, in the scenario's scratch folder.
fn after_hook_hold(agent: &Agent) -> PathBuf {
    agent.scratch().join("after-hook-holds")
}

/// The script of an after hook that waits while the file `$1` is there and
/// the daemon that started it is alive. A hook keeps the daemon's `PATH`.
const WAIT_WHILE_HELD: &str =
    "while [ -e \"$1\" ] && kill -0 \"$PPID\" 2>/dev/null; do sleep 0.05; done";

/// A plan whose after hook runs until the scenario lets it go. A backup stays
/// running in the log until its ending hooks have run, since a failing after
/// hook still changes how it ended (`features/planning/hooks.feature`), so a
/// backup of the plan is certain to be seen running, however soon the engine
/// finishes, and it ends as it would have once the hook is let go. The hook
/// also ends once its daemon is gone, or the scenario's folder with the file
/// it waits on: a scenario that ends, or fails, leaves no hook behind.
#[given("the plan's after hook waits until it is let go")]
async fn an_after_hook_that_waits(agent: &mut Agent) {
    let hold = after_hook_hold(agent);
    std::fs::write(&hold, b"").expect("the file the after hook waits on");
    let hold = hold.to_str().expect("a UTF-8 scratch path").to_owned();
    let wait = domain::planning::HookEntry::new(
        "/bin/sh",
        ["-c", WAIT_WHILE_HELD, "after-hook", hold.as_str()],
        domain::planning::OnFailure::Error,
    )
    .expect("a hook that runs /bin/sh");
    agent.backup.plan_hooks.run_after = vec![wait];
    super::events_steps::save_daemon_config(agent, None).await;
}

#[when("the plan's after hook is let go")]
fn the_after_hook_is_let_go(agent: &mut Agent) {
    std::fs::remove_file(after_hook_hold(agent)).expect("the after hook was held");
}

/// The name of the file in the plan's source that the backup waits on: one
/// of the plan's custom ignore files.
const WAITED_ON: &str = ".waited-on";

/// The variable that makes the e2e runner's binary the holder of a pipe
/// a backup waits on (see [`hold_pipe`]), naming the pipe's place in the
/// plan's source.
pub(crate) const HOLD_PIPE_ENV: &str = "LADDERBACK_E2E_HOLD_PIPE";

/// How long the holder may take to let every reader of the pipe go: a
/// reader that keeps it open longer than that never reads it to its end.
const LET_GO_LIMIT: Duration = Duration::from_secs(10);

/// A named pipe in the plan's source that a backup of the plan waits on.
/// Its name is one of the plan's custom ignore files, which rustic reads in
/// each folder it walks into before it lists anything there, so the walk
/// waits on it at the source's own folder: after the worker has started and
/// locked its cache slot, before the backup has saved anything. The pipe's
/// one writer is a process of its own, the holder ([`hold_pipe`]), which
/// writes nothing, so a read of it waits until the holder lets it go. A
/// backup of the plan is certain to be running until then, however fast the
/// machine; the engine has no part of it left to finish.
///
/// The holder lets go when its stdin ends: when the scenario lets it go,
/// ends or fails, and when the runner exits without ending the scenario (its
/// time limit, a signal), as the kernel closes the runner's end then. It
/// leads a process group of its own, so a terminal's signals don't end it
/// before it has let go, and its stdout and stderr are pipes, never the
/// terminal, which would stop it for writing from the background.
struct WaitedOn {
    /// The holder, its stdin still open; `None` once it has been let go.
    holder: Option<std::process::Child>,
}

impl WaitedOn {
    /// Lets the backup's walk go on, and waits until the holder has: the
    /// folder then leaves nothing out, and no read of the pipe waits.
    async fn let_go(mut self) {
        let mut holder = self.holder.take().expect("a holder not yet let go");
        let stdin = holder.stdin.take();
        // Closed through the spawn gate: a start in flight may have copied
        // it into a child that has not run `exec` yet, and that copy would
        // keep the holder waiting (decision 58).
        spawn_gate::hold_blocking(move || drop(stdin))
            .await
            .expect("the spawn gate");
        let ended = super::blocking(move || holder.wait_with_output())
            .await
            .expect("the holder exits");
        assert!(
            ended.status.success(),
            "the holder let the pipe go: {}\n{}",
            ended.status,
            String::from_utf8_lossy(&ended.stderr)
        );
    }
}

impl Drop for WaitedOn {
    /// A scenario that ends, or fails, without letting go lets go here. The
    /// holder is reaped on a thread of its own: the runner's is shared.
    fn drop(&mut self) {
        if let Some(mut holder) = self.holder.take() {
            drop(holder.stdin.take());
            std::thread::spawn(move || {
                let _ = holder.wait_with_output();
            });
        }
    }
}

/// What the e2e runner's binary does as a pipe's holder, `place` the pipe's
/// place in the plan's source: it makes the pipe in a folder of its own,
/// links it at `place`, opens it for reading and writing (which doesn't
/// wait for a reader: Linux documents it, macOS does it too) and says
/// `held`. Then it waits for its stdin to end, and lets go:
///
/// 1. An empty file takes the pipe's place, so a walk that comes to the
///    folder from then on reads that. A source already gone needs none.
/// 2. It closes its writer, so the pipe has none.
/// 3. Until no reader is left, it opens the pipe for writing again and
///    closes it. On macOS a read that finds the pipe without writers ends
///    only the first waiting read; the next one waits again until another
///    writer has come and gone. Linux ends them all at once. An open that
///    finds no reader is refused (`ENXIO`), and then none waits.
///
/// Its own folder goes with it, so it leaves nothing behind.
pub(crate) fn hold_pipe(place: &std::path::Path) {
    use std::os::unix::fs::OpenOptionsExt;
    let own = tempfile::tempdir().expect("the holder's own folder");
    let pipe = own.path().join("pipe");
    nix::unistd::mkfifo(&pipe, nix::sys::stat::Mode::S_IRWXU).expect("the pipe is made");
    std::fs::hard_link(&pipe, place).expect("the pipe is linked into the source");
    let writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe)
        .expect("the pipe opens");
    let mut out = std::io::stdout();
    writeln!(out, "held")
        .and_then(|()| out.flush())
        .expect("the holder says it holds the pipe");
    // Ends when the runner closes its end, or exits.
    let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
    let spare = own.path().join("spare");
    // A failure here is reported only once every reader has been let go:
    // a panic before that would leave a read waiting for good on macOS.
    let placed = std::fs::write(&spare, b"")
        .and_then(|()| std::fs::rename(&spare, place))
        .or_else(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(error),
        });
    drop(writer);
    let deadline = Instant::now() + LET_GO_LIMIT;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(nix::fcntl::OFlag::O_NONBLOCK.bits())
            .open(&pipe)
        {
            Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ENXIO as i32) => break,
            Err(error) => panic!("the pipe opens for writing: {error}"),
            Ok(writer) => drop(writer),
        }
        assert!(
            Instant::now() < deadline,
            "a reader still holds the pipe after {LET_GO_LIMIT:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    placed.expect("the empty file takes the pipe's place");
}

#[given("the plan's source holds a file the backup waits on until it is let go")]
async fn a_file_the_backup_waits_on(agent: &mut Agent) {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let pipe = agent
        .backup
        .source
        .clone()
        .expect("a plan source")
        .join(WAITED_ON);
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("the runner's binary"));
    command
        .env(HOLD_PIPE_ENV, &pipe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut holder = spawn_gate::hold_blocking(move || command.spawn())
        .await
        .expect("the spawn gate")
        .expect("the pipe's holder starts");
    let stdout = holder.stdout.take().expect("the holder's stdout");
    // Kept from here, so that a step failing below still lets it go.
    let mut waited_on = WaitedOn {
        holder: Some(holder),
    };
    let said = super::blocking(move || {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).map(|_| line)
    })
    .await
    .expect("the holder's stdout reads");
    if said != "held\n" {
        let holder = waited_on.holder.take().expect("the holder");
        let ended = super::blocking(move || holder.wait_with_output())
            .await
            .expect("the holder exits");
        panic!(
            "the holder made the pipe and opened it, not {said:?}: {}\n{}",
            ended.status,
            String::from_utf8_lossy(&ended.stderr)
        );
    }
    agent.ops.waited_on = Some(waited_on);
    agent
        .backup
        .filter
        .custom_ignorefiles
        .push(WAITED_ON.to_owned());
    super::events_steps::save_daemon_config(agent, None).await;
}

#[when("the file the backup waits on is let go")]
async fn the_waited_on_file_is_let_go(agent: &mut Agent) {
    agent
        .ops
        .waited_on
        .take()
        .expect("the plan's source holds a file the backup waits on")
        .let_go()
        .await;
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
pub(crate) async fn finished_backups(agent: &mut Agent, count: usize, plan: String) {
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

#[then("the history page shows the operation failed")]
async fn shows_failed(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    shows_status(agent, &id, "failed").await;
}

/// The operation `id` as the history page lists it.
async fn listed(agent: &Agent, id: &str) -> serde_json::Value {
    let page = history_page(agent, "limit=50").await;
    page.get("operations")
        .and_then(|ops| ops.as_array())
        .and_then(|ops| {
            ops.iter()
                .find(|op| op.get("id").and_then(serde_json::Value::as_str) == Some(id))
        })
        .cloned()
        .unwrap_or_else(|| panic!("the history page lists {id}: {page}"))
}

#[then(
    expr = "the history page gives the operation's failure as {string}, in the engine's own words"
)]
async fn gives_the_failure(agent: &mut Agent, code: String) {
    let id = agent.ops.the_operation();
    let operation = listed(agent, &id).await;
    let failure = operation.get("failure");
    assert_eq!(
        failure
            .and_then(|failure| failure.get("code"))
            .and_then(serde_json::Value::as_str),
        Some(code.as_str()),
        "the operation's failure: {operation}"
    );
    assert!(
        failure
            .and_then(|failure| failure.get("detail"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|said| !said.trim().is_empty()),
        "the engine's own words: {operation}"
    );
    assert_eq!(
        operation
            .get("failure-code")
            .and_then(serde_json::Value::as_str),
        Some(code.as_str()),
        "the published failure code stays: {operation}"
    );
}

#[then(expr = "neither the history page nor the operation's detail shows {string}")]
async fn neither_shows(agent: &mut Agent, secret: String) {
    let id = agent.ops.the_operation();
    let (status, page) = exchange(agent, "GET", "/api/v1/operations?limit=50", "");
    assert_eq!(status, 200, "the history page answers: {page}");
    assert!(!page.contains(&secret), "the history page shows the secret");
    let (status, detail) = exchange(agent, "GET", &format!("/api/v1/operations/{id}"), "");
    assert_eq!(status, 200, "the operation's detail answers: {detail}");
    assert!(
        !detail.contains(&secret),
        "the operation's detail shows the secret"
    );
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
