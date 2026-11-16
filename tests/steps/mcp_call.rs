//! Steps for the one-shot calls of `features/automation/mcp.feature`: the
//! real `ladderback mcp call`, run as an agent runs it from a shell, with a
//! line of JSON read back for each call. A script of calls from standard
//! input is written a line at a time, each answer read before the next.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use serde_json::{Value, json};
use testkit::children::Owned;

use api::MsgArg;
use l10n::{Localizer, msg};

use super::events_steps::Events;
use super::interop_steps::{Ran, ran};
use super::mcp_steps::{
    ANSWER, OPERATION_WAIT, STOP, catalog_sentence, daemon_url, headless_root, mcp_command,
    save_token,
};
use super::{Agent, api_tokens_steps, blocking};

/// How long a one-shot command may take: a scratch session seeds its
/// folder and repository before its calls, answers each and stops after
/// them.
const RUN: Duration = ANSWER.saturating_add(STOP);

/// What the one-shot scenarios keep between steps.
#[derive(Debug, Default)]
pub struct McpCallE2e {
    /// The token the scenario's token file holds.
    token: Option<String>,
    /// A script of calls being written on the command's standard input.
    script: Option<Script>,
}

/// `ladderback mcp call --script -`, ended with the scenario.
#[derive(Debug)]
struct Script {
    child: Owned,
    input: Option<ChildStdin>,
    output: Receiver<String>,
    /// Every line read from standard output so far.
    seen: Vec<String>,
    log: PathBuf,
}

impl Script {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Writes one call and waits up to `limit` for its line.
    fn call(&mut self, call: &Value, limit: Duration) {
        let input = self.input.as_mut().expect("the script's input is open");
        writeln!(input, "{call}")
            .and_then(|()| input.flush())
            .unwrap_or_else(|error| panic!("cannot write {call}: {error}; log: {}", self.log()));
        match self.output.recv_timeout(limit) {
            Ok(line) => self.seen.push(line),
            Err(RecvTimeoutError::Timeout) => {
                panic!("no line for {call} in {limit:?}; log: {}", self.log())
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("the command ended before {call}; log: {}", self.log())
            }
        }
    }

    /// Ends the script and waits for the command to end.
    fn end(mut self) -> Ran {
        drop(self.input.take());
        let deadline = Instant::now() + STOP;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(left) {
                Ok(line) => self.seen.push(line),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    panic!(
                        "the command didn't end with its script; log: {}",
                        self.log()
                    )
                }
            }
        }
        let status = self.child.wait().expect("the command's status");
        Ran {
            success: status.success(),
            code: status.code(),
            stdout: self.seen.iter().map(|line| format!("{line}\n")).collect(),
            stderr: self.log(),
        }
    }
}

/// The token file of the scenario.
fn token_file(agent: &Agent) -> PathBuf {
    agent.scratch().join("agent-token")
}

/// `{daemon}` stands for the address of the scenario's daemon, `{scratch}`
/// for its scratch folder and `{token file}` for its token file.
fn expand(agent: &Agent, command_line: &str) -> String {
    let mut command_line =
        command_line.replace("{scratch}", &agent.scratch().display().to_string());
    if command_line.contains("{daemon}") {
        command_line = command_line.replace("{daemon}", &daemon_url(agent));
    }
    command_line.replace("{token file}", &token_file(agent).display().to_string())
}

#[given("that API token in a token file")]
fn the_token_in_a_file(agent: &mut Agent) {
    let token = agent.api_tokens.text().to_owned();
    save_token(&token_file(agent), &token, false);
    agent.mcp_call.token = Some(token);
}

#[given("a token nobody created in a token file")]
fn nobody_s_token_in_a_file(agent: &mut Agent) {
    save_token(&token_file(agent), api_tokens_steps::NOBODY_S, false);
    agent.mcp_call.token = Some(api_tokens_steps::NOBODY_S.to_owned());
}

#[given(expr = "a file of calls {string} holding:")]
fn calls_file(agent: &mut Agent, name: String, step: &Step) {
    let text = step.docstring.as_deref().expect("the step has a text");
    let text = format!("{}\n", text.trim_start_matches('\n'));
    std::fs::write(agent.scratch().join(name), text).expect("the file of calls");
}

#[given(expr = "a folder {string} where a file of calls would be")]
fn calls_folder(agent: &mut Agent, name: String) {
    std::fs::create_dir(agent.scratch().join(name)).expect("the folder");
}

/// Runs `ladderback` with `command_line`, on the folders of the scenario's
/// daemon, and waits for it to end.
#[when(expr = "an agent runs {string}")]
async fn agent_runs(agent: &mut Agent, command_line: String) {
    // Expanded whole: `{token file}` is two words. The scenario's folders
    // have no spaces in their paths.
    let mut words: Vec<String> = expand(agent, &command_line)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        words.first().map(String::as_str),
        Some("mcp"),
        "a command line of mcp"
    );
    words.remove(0);
    let scratch = agent.scratch().to_owned();
    let root = headless_root(agent);
    let ran = blocking(move || {
        let args: Vec<&str> = words.iter().map(String::as_str).collect();
        let (mut command, log) = mcp_command(&scratch, &root, &args);
        let out = scratch.join("mcp-call-output");
        let stdout = std::fs::File::create(&out).expect("a file for its output");
        command.stdin(Stdio::null()).stdout(stdout);
        let child = Owned::spawn(&mut command).expect("ladderback runs");
        let deadline = Instant::now() + RUN;
        let status = loop {
            if let Some(status) = child.try_wait().expect("the command's status") {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "the command didn't end in {RUN:?}; log: {}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        Ran {
            success: status.success(),
            code: status.code(),
            stdout: std::fs::read_to_string(&out).expect("its output"),
            stderr: std::fs::read_to_string(&log).unwrap_or_default(),
        }
    })
    .await;
    agent.interop.ran = Some(ran);
}

#[given("an agent runs a script of calls from its standard input in a scratch folder")]
async fn script_in_scratch(agent: &mut Agent) {
    let scratch = agent.scratch().to_owned();
    let root = scratch.join("mcp");
    let script = blocking(move || {
        let (mut command, log) =
            mcp_command(&scratch, &root, &["call", "--scratch", "--script", "-"]);
        command.stdin(Stdio::piped());
        let child = Owned::spawn(&mut command).expect("ladderback runs");
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
        Script {
            child,
            input,
            output,
            seen: Vec::new(),
            log,
        }
    })
    .await;
    agent.mcp_call.script = Some(script);
}

/// Writes `call` on the script, waiting up to `limit` for its line.
async fn script_calls(agent: &mut Agent, call: Value, limit: Duration) {
    let mut script = agent.mcp_call.script.take().expect("a script was started");
    let script = blocking(move || {
        script.call(&call, limit);
        script
    })
    .await;
    agent.mcp_call.script = Some(script);
}

#[when(expr = "the script calls {string} with {string}")]
async fn script_calls_with(agent: &mut Agent, tool: String, arguments: String) {
    let arguments: Value = serde_json::from_str(&arguments).expect("the arguments as JSON");
    // The first call waits for the scratch folder to be seeded.
    script_calls(agent, json!({"tool": tool, "arguments": arguments}), RUN).await;
}

#[when(expr = "the script calls {string} with the operation the run returned")]
async fn script_calls_with_the_run(agent: &mut Agent, tool: String) {
    let script = agent
        .mcp_call
        .script
        .as_ref()
        .expect("a script was started");
    let last = script.seen.last().expect("a run's line");
    let line: Value = serde_json::from_str(last).expect("a JSON line");
    let operation = line["result"]["operation"].clone();
    assert!(operation.is_string(), "no operation: {line}");
    let call = json!({"tool": tool, "arguments": {"operation": operation}});
    // `wait_operation` waits for the run to end, 60 s at most.
    script_calls(agent, call, OPERATION_WAIT + ANSWER).await;
}

/// The process ID of the one-shot command running the scenario's script.
pub fn script_pid(agent: &Agent) -> u32 {
    agent
        .mcp_call
        .script
        .as_ref()
        .expect("a script was started")
        .child
        .id()
}

#[when("the script ends")]
async fn script_ends(agent: &mut Agent) {
    let script = agent.mcp_call.script.take().expect("a script was started");
    let ran = blocking(move || script.end()).await;
    agent.interop.ran = Some(ran);
}

/// Line `number` of standard output, from 1, as JSON: a call's outcome,
/// naming its tool.
fn line(agent: &Agent, number: usize, tool: &str) -> Value {
    let stdout = &ran(agent).stdout;
    let text = stdout
        .lines()
        .nth(number.checked_sub(1).expect("lines count from 1"))
        .unwrap_or_else(|| panic!("no line {number}: {stdout:?}"));
    let line: Value =
        serde_json::from_str(text).unwrap_or_else(|error| panic!("not JSON ({error}): {text}"));
    assert_eq!(line["tool"], tool, "{line}");
    line
}

/// What the tool of line `number` gave: its result, the call having worked.
fn answered(agent: &Agent, number: usize, tool: &str) -> Value {
    let line = line(agent, number, tool);
    assert_eq!(line["ok"], true, "{line}");
    assert!(line.get("error").is_none(), "{line}");
    line["result"].clone()
}

/// The refusal of line `number`, with the code `code`.
fn refused(agent: &Agent, number: usize, tool: &str, code: &str) -> Value {
    let line = line(agent, number, tool);
    assert_eq!(line["ok"], false, "{line}");
    assert!(line.get("result").is_none(), "{line}");
    assert_eq!(line["error"]["code"], code, "{line}");
    line["error"].clone()
}

#[then(expr = "standard error says the refusal of line {int} as the catalog says it")]
fn stderr_says_the_refusal(agent: &mut Agent, number: usize) {
    let Ran { stdout, stderr, .. } = ran(agent);
    let said = refusal_said(stdout, number);
    assert!(
        stderr.contains(&said),
        "standard error {stderr:?} doesn't say {said:?}"
    );
}

#[then(expr = "standard error says the refusals of lines {int} and {int}, in that order")]
fn stderr_says_the_refusals_in_order(agent: &mut Agent, first: usize, second: usize) {
    let Ran { stdout, stderr, .. } = ran(agent);
    let at = |number| {
        let said = refusal_said(stdout, number);
        stderr
            .find(&said)
            .unwrap_or_else(|| panic!("standard error {stderr:?} doesn't say {said:?}"))
    };
    assert!(
        at(first) < at(second),
        "standard error {stderr:?} says line {second}'s refusal before line {first}'s"
    );
}

/// The stderr line that says the refusal on stdout's line `number`, as the
/// catalog says it. The command runs in an environment that asks for no
/// language.
fn refusal_said(stdout: &str, number: usize) -> String {
    let text = stdout
        .lines()
        .nth(number.checked_sub(1).expect("lines count from 1"))
        .unwrap_or_else(|| panic!("no line {number}: {stdout:?}"));
    let line: Value =
        serde_json::from_str(text).unwrap_or_else(|error| panic!("not JSON ({error}): {text}"));
    format!(
        "ladderback: {}\n",
        catalog_sentence("en-US", &line["error"])
    )
}

#[then(expr = "line {int} says the tool {string} answered, listing the plan {string}")]
fn answered_listing(agent: &mut Agent, number: usize, tool: String, plan: String) {
    let result = answered(agent, number, &tool);
    let plans = result["plans"]
        .as_array()
        .unwrap_or_else(|| panic!("no plans: {result}"));
    assert!(
        plans.iter().any(|entry| entry["id"] == plan.as_str()),
        "{result}"
    );
}

#[then(expr = "line {int} says the tool {string} answered that the operation finished as {string}")]
fn answered_finished(agent: &mut Agent, number: usize, tool: String, status: String) {
    let result = answered(agent, number, &tool);
    assert_eq!(result["finished"], true, "{result}");
    assert_eq!(result["operation"]["status"], status, "{result}");
}

#[then(expr = "line {int} says the tool {string} refused with {string}")]
fn refused_with(agent: &mut Agent, number: usize, tool: String, code: String) {
    refused(agent, number, &tool, &code);
}

#[then(
    expr = "line {int} says the tool {string} refused with {string}, naming the argument {string} as {string}"
)]
fn refused_naming_the_argument(
    agent: &mut Agent,
    number: usize,
    tool: String,
    code: String,
    argument: String,
    problem: String,
) {
    let error = refused(agent, number, &tool, &code);
    assert_eq!(
        error["args"],
        json!({
            "field": {"type": "text", "value": argument},
            "problem": {"type": "text", "value": problem},
        }),
        "{error}"
    );
}

#[then(expr = "line {int} says the tool {string} refused with {string}, naming the scope {string}")]
fn refused_naming_the_scope(
    agent: &mut Agent,
    number: usize,
    tool: String,
    code: String,
    scope: String,
) {
    let error = refused(agent, number, &tool, &code);
    assert_eq!(
        error["args"]["scope"],
        json!({"type": "text", "value": scope}),
        "{error}"
    );
}

#[then("nothing the command printed holds the token")]
fn token_never_printed(agent: &mut Agent) {
    let token = agent.mcp_call.token.as_deref().expect("a token file");
    let ran = ran(agent);
    assert!(
        !ran.stdout.contains(token),
        "standard output shows the token"
    );
    assert!(
        !ran.stderr.contains(token),
        "standard error shows the token"
    );
}

#[then("the call is refused, naming the data folder and the process that holds it")]
fn headless_call_refused(agent: &mut Agent) {
    let said = Localizer::new(&[]).message(
        msg::CLI_DATA_DIR_IN_USE,
        &[
            (
                "path",
                MsgArg::Text(Events::data_dir(agent).display().to_string()),
            ),
            ("pid", MsgArg::Text(agent.events.pid().to_string())),
        ],
    );
    let stderr = &ran(agent).stderr;
    assert!(
        stderr.contains(said.text()),
        "standard error {stderr:?} doesn't say {:?}",
        said.text()
    );
}
