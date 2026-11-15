//! Steps for `features/automation/mcp.feature`: the real `ladderback mcp`,
//! spoken to over its standard input and output as an MCP client does, one
//! JSON-RPC message per line.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use cucumber::{given, then, when};
use serde_json::{Value, json};
use testkit::children::Owned;

use api::MsgArg;
use app::ports::{OperationRecord, OperationStore as _};
use domain::execution::{FailureReason, OperationStatus, Trigger};
use domain::{OperationId, Uuid};
use l10n::{Localizer, msg};
use store::SqliteStore;

use super::build_steps::expected;
use super::events_steps::{self, Events};
use super::{
    Agent, LADDERBACK, api_tokens_steps, backup_steps, blocking, maintenance_steps,
    operations_steps,
};

/// How long the server may take to answer one request: a status reads the
/// daemon over loopback.
const ANSWER: Duration = Duration::from_secs(10);

/// How long `wait_operation` is asked to wait for an operation to finish:
/// a backup of a few small files takes seconds.
const OPERATION_WAIT: Duration = Duration::from_secs(60);

/// How long a session may take to end once its input closes: a session
/// with its own folders stops its workers and closes its operation log,
/// which takes well under a second even with a backup running.
const STOP: Duration = Duration::from_secs(10);

/// The prefix of a scratch folder's name, in the temporary folder.
const SCRATCH_PREFIX: &str = "ladderback-scratch-";

/// How many MCP servers the run has started, so each logs to a file of its
/// own.
static STARTED: AtomicU64 = AtomicU64::new(0);

/// What the MCP scenarios keep between steps.
#[derive(Debug, Default)]
pub struct McpE2e {
    session: Option<Session>,
    /// The answer to the request sent last: the whole response.
    answer: Option<Value>,
    /// How a server that was expected to refuse ended, and what it said.
    refused: Option<(ExitStatus, String)>,
    /// The tool called last and its arguments, which the next page asks
    /// again with a cursor.
    last_call: Option<(String, Value)>,
    /// The results of the pages listed so far, in order.
    pages: Vec<Value>,
}

/// One `ladderback mcp`, ended with the scenario.
#[derive(Debug)]
struct Session {
    child: Owned,
    input: Option<ChildStdin>,
    /// The lines it writes on standard output, as they come.
    output: Receiver<String>,
    /// Every line read from standard output so far.
    seen: Vec<String>,
    log: PathBuf,
    next: u64,
}

/// `ladderback mcp` with `args`, on the folders under `root`, with the
/// scenario's own temporary folder (where a scratch folder goes), saying
/// what it says on standard error into a file of its own, whose path is
/// returned.
fn mcp_command(scratch: &Path, root: &Path, args: &[&str]) -> (Command, PathBuf) {
    let log = scratch.join(format!(
        "mcp-{}.log",
        STARTED.fetch_add(1, Ordering::Relaxed)
    ));
    let stderr = std::fs::File::create(&log).expect("a log file for the MCP server");
    let tmp = temporary_folder(scratch);
    std::fs::create_dir_all(&tmp).expect("the scenario's temporary folder");
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, root);
    command
        .arg("mcp")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(stderr)
        .env("TMPDIR", tmp)
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE");
    (command, log)
}

/// The temporary folder the scenario's MCP servers are given.
fn temporary_folder(scratch: &Path) -> PathBuf {
    scratch.join("tmp")
}

impl Session {
    fn start(scratch: &Path, root: &Path, args: &[&str]) -> Self {
        let (mut command, log) = mcp_command(scratch, root, args);
        command.stdin(Stdio::piped());
        let child = Owned::spawn(&mut command).expect("ladderback mcp starts");
        let input = child.take_stdin();
        let stdout = child.take_stdout().expect("its standard output");
        let (lines, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            seen: Vec::new(),
            log,
            next: 0,
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Sends `method` with `params` and waits up to `limit` for the answer
    /// to it.
    fn ask(&mut self, method: &str, params: Value, limit: Duration) -> Value {
        self.next += 1;
        let id = self.next;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let input = self.input.as_mut().expect("the server's input is open");
        writeln!(input, "{request}")
            .and_then(|()| input.flush())
            .unwrap_or_else(|error| panic!("cannot send {method}: {error}; log: {}", self.log()));
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.output.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no answer to {method} in {limit:?}; log: {}", self.log())
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!(
                        "the server ended before answering {method}; log: {}",
                        self.log()
                    )
                }
            };
            self.seen.push(line.clone());
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("not JSON ({error}): {line}"));
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Closes the server's input and reads what it writes until it ends.
    fn close(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + STOP;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(left) {
                Ok(line) => self.seen.push(line),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    panic!(
                        "the server didn't end when its input closed; log: {}",
                        self.log()
                    )
                }
            }
        }
        let status = self.child.wait().expect("the server's status");
        assert!(
            status.success(),
            "the server ended with {status}; log: {}",
            self.log()
        );
    }
}

/// The `_meta` every request of the current revision carries: its revision
/// and the client's capabilities.
fn current(mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    params
}

/// Starts `ladderback mcp` with `args` on the folders under `root`.
async fn start_on(agent: &mut Agent, root: PathBuf, args: &'static [&'static str]) {
    let scratch = agent.scratch().to_owned();
    let session = blocking(move || Session::start(&scratch, &root, args)).await;
    agent.mcp.session = Some(session);
}

/// Starts `ladderback mcp --connect url`, sending the token `token_file`
/// holds if it names one.
async fn start(agent: &mut Agent, url: String, token_file: Option<PathBuf>) {
    let scratch = agent.scratch().to_owned();
    let root = scratch.join("mcp");
    let session = blocking(move || {
        let mut args = vec!["--connect", url.as_str()];
        if let Some(file) = token_file.as_deref() {
            args.extend(["--token-file", file.to_str().expect("a UTF-8 path")]);
        }
        Session::start(&scratch, &root, &args)
    })
    .await;
    agent.mcp.session = Some(session);
}

/// Writes `token` into the file at `path`, as a person saves one: readable
/// by its owner alone, unless `shared`, when other users can read it too.
fn save_token(path: &Path, token: &str, shared: bool) {
    use std::os::unix::fs::PermissionsExt;
    let folder = path.parent().expect("a token file's folder");
    std::fs::create_dir_all(folder).expect("the token file's folder");
    std::fs::write(path, format!("{token}\n")).expect("the token file");
    let mode = if shared { 0o644 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .expect("the token file's mode");
}

/// Starts `ladderback mcp` with `args` on the folders under `root` and waits
/// until it answers, so whatever it does before it serves is done.
async fn start_serving(agent: &mut Agent, root: PathBuf, args: &'static [&'static str]) {
    start_on(agent, root, args).await;
    ask(agent, "server/discover", current(json!({}))).await;
    result(agent);
}

/// The folders a headless server of the scenario runs on: those of the
/// daemon the scenario starts, so the two share a data folder.
fn headless_root(agent: &Agent) -> PathBuf {
    Events::root(agent)
}

/// The scratch folders in the scenario's temporary folder.
fn scratch_folders(agent: &Agent) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(temporary_folder(agent.scratch())) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.expect("an entry of the temporary folder").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(SCRATCH_PREFIX))
        })
        .collect()
}

/// Sends a request on the scenario's server and keeps the answer.
async fn ask(agent: &mut Agent, method: &'static str, params: Value) {
    ask_within(agent, method, params, ANSWER).await;
}

/// Sends a request on the scenario's server, waits up to `limit` for the
/// answer and keeps it.
async fn ask_within(agent: &mut Agent, method: &'static str, params: Value, limit: Duration) {
    let mut session = agent.mcp.session.take().expect("an MCP server was started");
    let (session, answer) = blocking(move || {
        let answer = session.ask(method, params, limit);
        (session, answer)
    })
    .await;
    agent.mcp.session = Some(session);
    agent.mcp.answer = Some(answer);
}

fn answer(agent: &Agent) -> &Value {
    agent.mcp.answer.as_ref().expect("a request was answered")
}

/// The answer's `result`, which an error answer has none of.
fn result(agent: &Agent) -> &Value {
    let answer = answer(agent);
    assert!(answer["result"].is_object(), "no result: {answer}");
    &answer["result"]
}

/// A tool's result that isn't an error: its `structuredContent`.
fn content(agent: &Agent) -> &Value {
    let result = result(agent);
    assert_eq!(result["isError"], false, "{result}");
    &result["structuredContent"]
}

/// A tool's refusal: a result marked as an error, with no
/// `structuredContent` (its tool's output schema doesn't take one), whose
/// text content holds the `{code, args}` as JSON.
fn refusal_of(result: &Value) -> Value {
    assert_eq!(result["isError"], true, "{result}");
    assert!(result.get("structuredContent").is_none(), "{result}");
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text: {result}"));
    serde_json::from_str(text).unwrap_or_else(|error| panic!("{error}: {text}"))
}

/// The address of the daemon the scenario started: the one `Ladderback is
/// serving on loopback` started, or the one started with a plan, which the
/// operation steps drive.
fn daemon_url(agent: &Agent) -> String {
    let addr = match agent.web.server.as_ref() {
        Some(server) => server.addr,
        None => agent.events.addr(),
    };
    format!("http://{addr}")
}

#[given("an MCP server connected to it")]
async fn connected(agent: &mut Agent) {
    let url = daemon_url(agent);
    start(agent, url, None).await;
}

/// Starts an MCP server connected to the scenario's daemon, with `token`
/// in a token file it names.
async fn connected_with(agent: &mut Agent, token: String) {
    let file = agent.scratch().join("agent-token");
    save_token(&file, &token, false);
    let url = daemon_url(agent);
    start(agent, url, Some(file)).await;
}

#[given("an MCP server connected to it with that API token in a file")]
async fn connected_with_the_token(agent: &mut Agent) {
    let token = agent.api_tokens.text().to_owned();
    connected_with(agent, token).await;
}

#[given("an MCP server connected to it with a token nobody created in a file")]
async fn connected_with_nobody_s_token(agent: &mut Agent) {
    connected_with(agent, api_tokens_steps::NOBODY_S.to_owned()).await;
}

/// The token file `ladderback mcp --connect` reads when it names none, for
/// `the owner runs`: `mcp-token` in its configuration folder.
fn default_token_file(agent: &Agent) -> PathBuf {
    agent.scratch().join("config").join("mcp-token")
}

#[given("an API token file in the configuration folder")]
fn token_file_in_config(agent: &mut Agent) {
    save_token(
        &default_token_file(agent),
        api_tokens_steps::NOBODY_S,
        false,
    );
}

#[given("an API token file in the configuration folder that other users can read")]
fn shared_token_file_in_config(agent: &mut Agent) {
    save_token(&default_token_file(agent), api_tokens_steps::NOBODY_S, true);
}

#[given("an MCP server connected to an address where nothing answers")]
async fn connected_to_nothing(agent: &mut Agent) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let addr = listener.local_addr().expect("its address");
    drop(listener);
    start(agent, format!("http://{addr}"), None).await;
}

#[given(expr = "an MCP server running headless with the plan {string}")]
async fn headless_with_plan(agent: &mut Agent, plan: String) {
    // A real plan on a real scratch repository, in the configuration of the
    // folders a daemon of the scenario would serve from.
    backup_steps::new_repository(agent).await;
    backup_steps::source_with_files(agent, &plan, 3);
    events_steps::save_daemon_config(agent, None).await;
    let root = headless_root(agent);
    start_serving(agent, root, &["--headless"]).await;
}

#[when("an MCP server is started headless on the same folders")]
async fn headless_on_the_same_folders(agent: &mut Agent) {
    let scratch = agent.scratch().to_owned();
    let root = headless_root(agent);
    let refused = blocking(move || {
        let (mut command, log) = mcp_command(&scratch, &root, &["--headless"]);
        // Closed input: a server that isn't refused serves nothing and ends.
        command.stdin(Stdio::null());
        let child = Owned::spawn(&mut command).expect("ladderback mcp starts");
        let deadline = Instant::now() + ANSWER;
        let status = loop {
            if let Some(status) = child.try_wait().expect("the server's status") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "the server neither served nor ended in {ANSWER:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        (status, std::fs::read_to_string(&log).unwrap_or_default())
    })
    .await;
    agent.mcp.refused = Some(refused);
}

#[then("the MCP server is refused, naming the data folder and the process that holds it")]
fn headless_refused(agent: &mut Agent) {
    // The holder is the headless server the scenario runs, or else its daemon.
    let holder = match &agent.mcp.session {
        Some(session) => session.child.id(),
        None => agent.events.pid(),
    };
    let data = Events::data_dir(agent);
    let (status, log) = agent
        .mcp
        .refused
        .take()
        .expect("an MCP server was started on the same folders");
    assert!(
        !status.success(),
        "a refusal is a failure: {status}; log: {log}"
    );
    let said = Localizer::new(&[]).message(
        msg::CLI_DATA_DIR_IN_USE,
        &[
            ("path", MsgArg::Text(data.display().to_string())),
            ("pid", MsgArg::Text(holder.to_string())),
        ],
    );
    assert!(
        log.contains(said.text()),
        "the refusal says {:?}:\n{log}",
        said.text()
    );
}

#[given("an MCP server running in a scratch folder")]
#[when("an MCP server is started in a scratch folder")]
async fn in_a_scratch_folder(agent: &mut Agent) {
    let root = agent.scratch().join("mcp");
    start_serving(agent, root, &["--scratch"]).await;
}

#[given("a scratch folder a crashed session left behind")]
fn a_crashed_scratch_folder(agent: &mut Agent) {
    let left = temporary_folder(agent.scratch()).join(format!("{SCRATCH_PREFIX}crashed"));
    std::fs::create_dir_all(left.join("config")).expect("the crashed session's folder");
    std::fs::write(left.join("config").join("config.toml"), "schema = 1\n")
        .expect("the crashed session's configuration");
}

#[then("the folder the crashed session left is gone")]
fn the_crashed_folder_is_gone(agent: &mut Agent) {
    let left = temporary_folder(agent.scratch()).join(format!("{SCRATCH_PREFIX}crashed"));
    assert!(!left.exists(), "{} is still there", left.display());
}

#[then("the scratch folder can be read by its owner only")]
fn scratch_folder_is_private(agent: &mut Agent) {
    use std::os::unix::fs::PermissionsExt;
    let folders = scratch_folders(agent);
    let [folder] = folders.as_slice() else {
        panic!("one scratch folder, not {folders:?}");
    };
    let mode = std::fs::metadata(folder)
        .expect("the scratch folder's metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "the mode is {mode:o}");
}

#[when("the MCP client closes the session")]
async fn closes_the_session(agent: &mut Agent) {
    let mut session = agent.mcp.session.take().expect("an MCP server was started");
    blocking(move || session.close()).await;
}

#[then("no scratch folder is left")]
fn no_scratch_folder_left(agent: &mut Agent) {
    let folders = scratch_folders(agent);
    assert!(folders.is_empty(), "left behind: {folders:?}");
}

#[then(expr = "the result lists the plan {string}")]
fn lists_the_plan(agent: &mut Agent, plan: String) {
    let content = content(agent);
    let listed = content["plans"]
        .as_array()
        .unwrap_or_else(|| panic!("no plans: {content}"));
    assert!(
        listed.iter().any(|entry| entry["id"] == plan.as_str()),
        "{content}"
    );
}

#[then("the result names no daemon")]
fn names_no_daemon(agent: &mut Agent) {
    let content = content(agent);
    assert!(content.get("daemon").is_none(), "{content}");
}

#[then("the result says the tools reach Ladderback in this process")]
fn reaches_in_process(agent: &mut Agent) {
    let content = content(agent);
    assert_eq!(content["access"], "in-process", "{content}");
}

#[then(
    "the result says the tools reach the daemon with an API token, and nothing in it says signed out"
)]
fn reaches_with_a_token(agent: &mut Agent) {
    let content = content(agent);
    assert_eq!(content["access"], "api-token", "{content}");
    assert!(!content.to_string().contains("signed-out"), "{content}");
}

#[when("the MCP client asks the server what it offers")]
async fn discovers(agent: &mut Agent) {
    ask(agent, "server/discover", current(json!({}))).await;
}

#[when(expr = "the MCP client starts the handshake with the revision {string}")]
async fn initializes(agent: &mut Agent, revision: String) {
    let params = json!({
        "protocolVersion": revision,
        "capabilities": {},
        "clientInfo": {"name": "e2e", "version": "1"},
    });
    ask(agent, "initialize", params).await;
}

#[when("the MCP client lists the tools")]
async fn lists_tools(agent: &mut Agent) {
    ask(agent, "tools/list", current(json!({}))).await;
}

#[when(expr = "the MCP client calls the tool {string}")]
async fn calls(agent: &mut Agent, tool: String) {
    call(agent, tool, json!({})).await;
}

/// Calls `tool` with `arguments` on the scenario's server.
async fn call(agent: &mut Agent, tool: String, arguments: Value) {
    let params = current(json!({"name": tool, "arguments": arguments}));
    agent.mcp.last_call = Some((tool, arguments));
    ask(agent, "tools/call", params).await;
}

#[then(expr = "the server offers the revisions {string}")]
fn offers_revisions(agent: &mut Agent, revisions: String) {
    let expected: Vec<&str> = revisions.split(", ").collect();
    let result = result(agent);
    assert_eq!(result["supportedVersions"], json!(expected), "{result}");
}

#[then("the server offers tools")]
fn offers_tools(agent: &mut Agent) {
    let result = result(agent);
    assert!(result["capabilities"]["tools"].is_object(), "{result}");
}

#[then(expr = "the server agrees to the revision {string}")]
fn agrees(agent: &mut Agent, revision: String) {
    let result = result(agent);
    assert_eq!(result["protocolVersion"], revision.as_str(), "{result}");
}

#[then(expr = "the tools listed are {string}")]
fn tools_listed(agent: &mut Agent, names: String) {
    let expected: Vec<&str> = names.split(", ").collect();
    let result = result(agent);
    let listed: Vec<&str> = result["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools: {result}"))
        .iter()
        .map(|tool| tool["name"].as_str().expect("a tool's name"))
        .collect();
    assert_eq!(listed, expected, "{result}");
}

/// Checks that `found` names `build`'s parts, each as a field of its own.
fn names_the_build(found: &Value, build: &api::BuildDto) {
    assert_eq!(found["version"], build.version.as_str(), "{found}");
    assert_eq!(
        found["commit"],
        build.commit.as_deref().unwrap_or_default(),
        "{found}"
    );
    assert_eq!(found["dirty"], build.dirty, "{found}");
    assert_eq!(found["release"], build.release, "{found}");
}

#[then(
    "the result names this checkout's build as the program's: its version, its commit and whether it had changes"
)]
async fn names_the_programs_build(agent: &mut Agent) {
    let Some(build) = expected().await else {
        testkit::skip!("git can't say which build this checkout is");
    };
    names_the_build(&content(agent)["build"], &build);
}

#[then(
    "the result names this checkout's build as the daemon's: its version, its commit and whether it had changes"
)]
async fn names_the_daemons_build(agent: &mut Agent) {
    let Some(build) = expected().await else {
        testkit::skip!("git can't say which build this checkout is");
    };
    names_the_build(&content(agent)["daemon"]["build"], &build);
}

#[then("the result says sign-in is off and the daemon answers on loopback")]
fn sign_in_off_on_loopback(agent: &mut Agent) {
    let content = content(agent);
    assert_eq!(content["access"], "sign-in-off", "{content}");
    assert_eq!(content["daemon"]["exposure"], "loopback", "{content}");
}

#[then(expr = "the result is the error {string}")]
fn result_is_the_error(agent: &mut Agent, code: String) {
    let refusal = refusal_of(result(agent));
    assert_eq!(refusal["code"], code.as_str(), "{refusal}");
}

#[then(expr = "the error names the scope {string}")]
fn error_names_the_scope(agent: &mut Agent, scope: String) {
    let refusal = refusal_of(result(agent));
    assert_eq!(
        refusal["args"]["scope"],
        json!({"type": "text", "value": scope}),
        "{refusal}"
    );
}

#[then("standard output held only protocol messages")]
async fn only_protocol_messages(agent: &mut Agent) {
    let mut session = agent.mcp.session.take().expect("an MCP server was started");
    let seen = blocking(move || {
        session.close();
        session.seen
    })
    .await;
    assert!(!seen.is_empty(), "the server wrote nothing");
    for line in seen {
        let message: Value = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("not a protocol message ({error}): {line:?}"));
        assert_eq!(
            message["jsonrpc"], "2.0",
            "not a protocol message: {line:?}"
        );
    }
}

#[given("an MCP server running headless on that configuration")]
async fn headless_on_that_configuration(agent: &mut Agent) {
    // The folders `the daemon's config file holds:` writes the file into.
    let root = agent.scratch().join("daemon");
    start_serving(agent, root, &["--headless"]).await;
}

#[when(expr = "the MCP client lists the snapshots of the repository {string}, {int} to a page")]
async fn lists_snapshots(agent: &mut Agent, repository: String, limit: u64) {
    let arguments = json!({"repository": repository, "limit": limit});
    call(agent, "list_snapshots".to_owned(), arguments).await;
    let page = content(agent).clone();
    agent.mcp.pages.push(page);
}

#[when(expr = "the MCP client lists the snapshots of the repository {string}")]
async fn lists_snapshots_of(agent: &mut Agent, repository: String) {
    let arguments = json!({"repository": repository});
    call(agent, "list_snapshots".to_owned(), arguments).await;
}

#[when(expr = "the MCP client lists the snapshots of the repository {string} of the plan {string}")]
async fn lists_snapshots_of_plan(agent: &mut Agent, repository: String, plan: String) {
    let arguments = json!({"repository": repository, "plan": plan});
    call(agent, "list_snapshots".to_owned(), arguments).await;
}

#[when(expr = "the MCP client lists the snapshots of the repository {string} of the host {string}")]
async fn lists_snapshots_of_host(agent: &mut Agent, repository: String, host: String) {
    let arguments = json!({"repository": repository, "host": host});
    call(agent, "list_snapshots".to_owned(), arguments).await;
}

#[then(expr = "the result lists {int} snapshots")]
fn lists_snapshots_count(agent: &mut Agent, count: usize) {
    let content = content(agent);
    let snapshots = content["snapshots"]
        .as_array()
        .unwrap_or_else(|| panic!("no snapshots: {content}"));
    assert_eq!(snapshots.len(), count, "{content}");
}

#[when("the MCP client asks for the next page")]
async fn asks_for_the_next_page(agent: &mut Agent) {
    let page = agent.mcp.pages.last().expect("a page was listed");
    let next = page["next"]
        .as_str()
        .unwrap_or_else(|| panic!("no page follows: {page}"))
        .to_owned();
    let (tool, mut arguments) = agent.mcp.last_call.clone().expect("a tool was called");
    arguments["cursor"] = json!(next);
    call(agent, tool, arguments).await;
    let page = content(agent).clone();
    agent.mcp.pages.push(page);
}

#[then("the two pages list the repository's two snapshots, newest first")]
fn two_pages_of_snapshots(agent: &mut Agent) {
    let pages = &agent.mcp.pages;
    assert_eq!(pages.len(), 2, "{pages:?}");
    let listed: Vec<&Value> = pages
        .iter()
        .map(|page| {
            let snapshots = page["snapshots"]
                .as_array()
                .unwrap_or_else(|| panic!("no snapshots: {page}"));
            assert_eq!(snapshots.len(), 1, "one to a page: {page}");
            &snapshots[0]
        })
        .collect();
    assert_ne!(listed[0]["id"], listed[1]["id"], "{pages:?}");
    // RFC 3339 in UTC, as the API writes a snapshot's time: compared as text.
    let time = |snapshot: &Value| snapshot["time"].as_str().expect("a time").to_owned();
    assert!(
        time(listed[0]) >= time(listed[1]),
        "newest first: {pages:?}"
    );
}

#[then("no page follows the second")]
fn no_page_follows(agent: &mut Agent) {
    let page = agent.mcp.pages.last().expect("a page was listed");
    assert_eq!(page["next"], Value::Null, "{page}");
}

#[when(expr = "the MCP client lists the operations with the limit {int}")]
async fn lists_operations(agent: &mut Agent, limit: u64) {
    call(agent, "list_operations".to_owned(), json!({"limit": limit})).await;
}

#[then(expr = "the result lists {int} operations and no page follows")]
fn lists_operations_and_no_more(agent: &mut Agent, count: usize) {
    let content = content(agent);
    let operations = content["operations"]
        .as_array()
        .unwrap_or_else(|| panic!("no operations: {content}"));
    assert_eq!(operations.len(), count, "{content}");
    assert_eq!(content["next"], Value::Null, "{content}");
}

/// The codes of the warnings of the entry of `list` whose `key` is `name`.
fn warning_codes<'a>(content: &'a Value, list: &str, key: &str, name: &str) -> Vec<&'a str> {
    let entry = content[list]
        .as_array()
        .unwrap_or_else(|| panic!("no {list}: {content}"))
        .iter()
        .find(|entry| entry[key] == name)
        .unwrap_or_else(|| panic!("no {name} in the {list}: {content}"));
    entry["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(|warning| warning["code"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

#[then(expr = "the plan {string} carries the warning {string}")]
fn plan_carries_the_warning(agent: &mut Agent, plan: String, code: String) {
    let content = content(agent);
    let codes = warning_codes(content, "plans", "id", &plan);
    assert!(codes.contains(&code.as_str()), "{content}");
}

#[then(expr = "the repository {string} carries the warning {string}")]
fn repository_carries_the_warning(agent: &mut Agent, alias: String, code: String) {
    let content = content(agent);
    let codes = warning_codes(content, "repositories", "alias", &alias);
    assert!(codes.contains(&code.as_str()), "{content}");
}

#[then(expr = "nothing the MCP server wrote shows {string} or {string}")]
fn nothing_shows_either(agent: &mut Agent, one: String, other: String) {
    shows_none(agent, &[&one, &other]);
}

#[then(expr = "nothing the MCP server wrote shows {string}")]
fn nothing_shows(agent: &mut Agent, secret: String) {
    shows_none(agent, &[&secret]);
}

/// Checks that no line the server wrote on standard output holds any of
/// `secrets`.
fn shows_none(agent: &Agent, secrets: &[&str]) {
    let session = agent
        .mcp
        .session
        .as_ref()
        .expect("an MCP server was started");
    assert!(!session.seen.is_empty(), "the server wrote nothing");
    for line in &session.seen {
        for secret in secrets {
            assert!(!line.contains(secret), "{secret:?} shown: {line}");
        }
    }
}

#[then(expr = "nothing the MCP server or Ladderback wrote or logged shows {string} or {string}")]
fn nothing_written_or_logged_shows(agent: &mut Agent, one: String, other: String) {
    shows_none(agent, &[&one, &other]);
    let session = agent
        .mcp
        .session
        .as_ref()
        .expect("an MCP server was started");
    let logs = [
        ("the MCP server's log", Some(session.log())),
        ("the daemon's log", agent.events.whole_log_if_serving()),
    ];
    for (name, log) in logs {
        let Some(log) = log else { continue };
        for secret in [&one, &other] {
            assert!(
                !log.contains(secret.as_str()),
                "{secret:?} in {name}: {log}"
            );
        }
    }
}

#[when("the MCP client asks for that operation once it has ended")]
async fn asks_for_the_operation(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    operations_steps::wait_ended(agent, &id).await;
    call(agent, "get_operation".to_owned(), json!({"id": id})).await;
}

#[then(expr = "the result gives the operation's failure as {string}, in the engine's own words")]
fn gives_the_failure(agent: &mut Agent, code: String) {
    let content = content(agent);
    let operation = &content["operation"];
    assert_eq!(operation["status"], "failed", "{content}");
    assert_eq!(operation["failure"]["code"], code.as_str(), "{content}");
    let detail = operation["failure"]["detail"].as_str().unwrap_or_default();
    assert!(!detail.is_empty(), "no words of the engine's: {content}");
}

#[given("an MCP server running headless on that plan")]
async fn headless_on_that_plan(agent: &mut Agent) {
    events_steps::save_daemon_config(agent, None).await;
    let root = headless_root(agent);
    start_serving(agent, root, &["--headless"]).await;
}

#[when(expr = "the MCP client runs the plan {string}")]
async fn runs_the_plan(agent: &mut Agent, plan: String) {
    call(agent, "run_plan".to_owned(), json!({"plan": plan})).await;
    let content = content(agent);
    let id = content["operation"]
        .as_str()
        .unwrap_or_else(|| panic!("no operation: {content}"))
        .to_owned();
    agent.ops.note_started(id);
}

#[when("the MCP client waits for that operation")]
async fn waits(agent: &mut Agent) {
    wait_for(agent, OPERATION_WAIT).await;
}

#[given(expr = "the MCP client has run the plan {string} to the end")]
async fn has_run_the_plan(agent: &mut Agent, plan: String) {
    runs_the_plan(agent, plan).await;
    waits(agent).await;
    finished_as(agent, "succeeded".to_owned());
}

#[when(expr = "the MCP client waits {int} second(s) for that operation")]
async fn waits_seconds(agent: &mut Agent, seconds: u64) {
    wait_for(agent, Duration::from_secs(seconds)).await;
}

/// Calls `wait_operation` for the scenario's operation, to wait `length`.
async fn wait_for(agent: &mut Agent, length: Duration) {
    let arguments = json!({"id": agent.ops.the_operation(), "timeout": length.as_secs()});
    let params = current(json!({"name": "wait_operation", "arguments": arguments}));
    ask_within(agent, "tools/call", params, length + ANSWER).await;
}

#[then(expr = "the wait says the operation finished as {string}")]
fn finished_as(agent: &mut Agent, status: String) {
    let content = content(agent);
    assert_eq!(content["finished"], true, "{content}");
    assert_eq!(content["operation"]["status"], status.as_str(), "{content}");
}

#[then("the wait says the operation hasn't finished")]
fn not_finished(agent: &mut Agent) {
    let content = content(agent);
    assert_eq!(content["finished"], false, "{content}");
    let status = content["operation"]["status"].as_str();
    assert!(matches!(status, Some("queued" | "running")), "{content}");
}

#[when("the MCP client sees that operation running")]
async fn sees_running(agent: &mut Agent) {
    until_running(agent).await;
}

/// Asks for the scenario's operation through `get_operation` until it is
/// running. The daemon answers a start before it has recorded the
/// operation, so one it doesn't hold yet is asked for again.
async fn until_running(agent: &mut Agent) {
    let id = agent.ops.the_operation();
    let deadline = Instant::now() + ANSWER;
    loop {
        call(agent, "get_operation".to_owned(), json!({"id": id})).await;
        let result = result(agent);
        let status = if result["isError"] == true {
            let refusal = refusal_of(result);
            assert_eq!(refusal["code"], "operation-unknown", "{refusal}");
            "unknown".to_owned()
        } else {
            result["structuredContent"]["operation"]["status"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        if status == "running" {
            return;
        }
        assert!(
            matches!(status.as_str(), "unknown" | "queued") && Instant::now() < deadline,
            "the operation isn't running: {result}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[when("the MCP client cancels that operation once it is running")]
async fn cancels_once_running(agent: &mut Agent) {
    until_running(agent).await;
    let id = agent.ops.the_operation();
    call(agent, "cancel_operation".to_owned(), json!({"id": id})).await;
    let content = content(agent);
    assert_eq!(content["operation"], id.as_str(), "{content}");
}

#[when("the MCP client cancels that check")]
async fn cancels_the_check(agent: &mut Agent) {
    let id = maintenance_steps::started(agent);
    call(agent, "cancel_operation".to_owned(), json!({"id": id})).await;
}

/// The record of the operation `id` in the operation log of the data
/// folder `data`.
async fn recorded(data: &Path, id: &str) -> OperationRecord {
    let store = SqliteStore::open(&data.join("ladderback.db"))
        .await
        .expect("the operation log opens");
    let uuid = Uuid::parse_str(id).expect("an operation ID");
    let detail = store
        .get(OperationId::from_uuid(uuid))
        .await
        .expect("the operation log answers")
        .unwrap_or_else(|| panic!("the operation log holds no {id}"));
    store.close().await;
    detail.record
}

/// What the operation log calls `trigger`, as §9 names it.
fn trigger_name(trigger: &Trigger) -> &'static str {
    match trigger {
        Trigger::Manual => "manual",
        Trigger::Mcp => "mcp",
        Trigger::ApiToken { .. } => "api-token",
        other => panic!("not a trigger these scenarios start with: {other:?}"),
    }
}

#[then(expr = "the daemon's operation log records that operation as started by {string}")]
async fn daemon_records_trigger(agent: &mut Agent, trigger: String) {
    let record = recorded(&Events::data_dir(agent), &agent.ops.the_operation()).await;
    assert_eq!(trigger_name(&record.trigger), trigger, "{record:?}");
}

#[then(expr = "the scratch session's operation log records that operation as started by {string}")]
async fn scratch_records_trigger(agent: &mut Agent, trigger: String) {
    let folders = scratch_folders(agent);
    let [folder] = folders.as_slice() else {
        panic!("one scratch folder, not {folders:?}");
    };
    let record = recorded(&folder.join("data"), &agent.ops.the_operation()).await;
    assert_eq!(trigger_name(&record.trigger), trigger, "{record:?}");
}

#[then("the daemon's operation log records that operation as ended by the stop")]
async fn ended_by_the_stop(agent: &mut Agent) {
    let record = recorded(&Events::data_dir(agent), &agent.ops.the_operation()).await;
    assert_eq!(
        record.status,
        OperationStatus::Failed {
            reason: FailureReason::DaemonShutDown
        },
        "{record:?}"
    );
}

#[when(expr = "the MCP client tests the repository {string}")]
async fn tests_the_repository(agent: &mut Agent, repository: String) {
    let arguments = json!({"repository": repository});
    call(agent, "test_repository".to_owned(), arguments).await;
}

#[then("the test finds the repository at its first location")]
fn found_at_the_first_location(agent: &mut Agent) {
    let content = content(agent);
    let first = &content["locations"][0];
    assert_eq!(first["position"], 1, "{content}");
    assert_eq!(first["outcome"], "used", "{content}");
}

#[then(expr = "the test tells the first location's outcome as {string}")]
fn first_location_outcome(agent: &mut Agent, outcome: String) {
    let content = content(agent);
    let first = &content["locations"][0];
    assert_eq!(first["position"], 1, "{content}");
    assert_eq!(first["outcome"], outcome.as_str(), "{content}");
}

#[when(expr = "the MCP client previews a forget of the repository {string}")]
async fn previews_a_forget(agent: &mut Agent, repository: String) {
    let arguments = json!({"repository": repository});
    call(agent, "preview_forget".to_owned(), arguments).await;
}

#[then(
    expr = "the preview tells what a forget would let go of the plan {string}, without a selection to start one"
)]
fn previews_the_plan(agent: &mut Agent, plan: String) {
    let content = content(agent);
    let plans: Vec<&str> = content["plans"]
        .as_array()
        .unwrap_or_else(|| panic!("no plans: {content}"))
        .iter()
        .map(|entry| entry["plan"].as_str().expect("a plan's ID"))
        .collect();
    assert_eq!(plans, [plan.as_str()], "{content}");
    assert!(content["left-alone"].is_object(), "{content}");
    assert!(content.get("selection").is_none(), "{content}");
}

#[when(expr = "the MCP client previews importing as {string} the profile:")]
async fn previews_an_import(agent: &mut Agent, repository: String, step: &cucumber::gherkin::Step) {
    let text = step.docstring.as_deref().expect("the step has a profile");
    let content = format!("{}\n", text.trim_start_matches('\n'));
    let arguments = json!({"content": content, "repository": repository});
    call(agent, "preview_import".to_owned(), arguments).await;
}

#[then(expr = "the preview adds the repository {string} and the plan {string}")]
fn preview_adds(agent: &mut Agent, repository: String, plan: String) {
    let content = content(agent);
    assert_eq!(content["repository"], repository.as_str(), "{content}");
    assert_eq!(content["plans"], json!([plan]), "{content}");
}

#[then(expr = "the preview carries the notice {string}")]
fn preview_carries(agent: &mut Agent, code: String) {
    let content = content(agent);
    let codes: Vec<&str> = content["notices"]
        .as_array()
        .unwrap_or_else(|| panic!("no notices: {content}"))
        .iter()
        .map(|notice| notice["code"].as_str().expect("a notice's code"))
        .collect();
    assert!(codes.contains(&code.as_str()), "{content}");
}

#[when(expr = "the MCP client exports the plan {string} as a profile")]
async fn exports_the_plan(agent: &mut Agent, plan: String) {
    call(agent, "export_profile".to_owned(), json!({"plan": plan})).await;
}

#[then("the exported profile reads the repository's password from a file")]
fn exported_reads_a_password_file(agent: &mut Agent) {
    let content = content(agent);
    let text = content["text"].as_str().expect("the profile's text");
    assert!(
        text.lines().any(|line| line.starts_with("password-file")),
        "{content}"
    );
}
