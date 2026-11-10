//! Steps for `features/cli/output.feature`: the real `ladderback`, run with
//! the command line and the environment a scenario chooses, and what it
//! printed read back as bytes: as JSON, for `--json`, and for the control
//! characters a terminal acts on, for colour and redraws. A command runs as
//! the interop scenarios' do (`interop_steps::run`: the scratch folders and
//! nothing of the developer's environment), and the daemon on folders of its
//! own (`testkit::daemon::isolate`).

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use serde_json::Value;

use super::interop_steps::{Ran, ran, run, said};
use super::{Agent, LADDERBACK, blocking};

/// How long a freshly started daemon may take to listen or give up.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// What the daemon says once it is up, in English.
const LISTENING: &str = "listening on http://";

#[given("colour is forced on")]
fn colour_is_forced(agent: &mut Agent) {
    agent
        .interop
        .env
        .push(("CLICOLOR_FORCE".to_owned(), "1".to_owned()));
}

#[given(expr = "the environment sets {string} to {string}")]
fn environment_sets(agent: &mut Agent, name: String, value: String) {
    agent.interop.env.push((name, value));
}

#[when(expr = "the owner runs {string}")]
async fn runs(agent: &mut Agent, command_line: String) {
    let args: Vec<&str> = command_line.split_whitespace().collect();
    run(agent, &args).await;
}

#[when(expr = "the daemon is started with {string}")]
async fn daemon_started(agent: &mut Agent, options: String) {
    let scratch = agent.scratch().to_owned();
    let env = agent.interop.env.clone();
    let log = blocking(move || serve_until_listening(&scratch, &options, &env)).await;
    // A daemon prints nothing on standard output, and is stopped, not ended.
    agent.interop.ran = Some(Ran {
        success: false,
        code: None,
        stdout: String::new(),
        stderr: log,
    });
}

/// Starts `ladderback serve` with `options`, waits until it listens, stops
/// it, and returns what it logged. Runs on a blocking thread: see
/// [`blocking`].
fn serve_until_listening(scratch: &Path, options: &str, env: &[(String, String)]) -> String {
    let log_path = scratch.join("serve-output.log");
    let log = File::create(&log_path).expect("a log file for the daemon");
    let mut command = Command::new(LADDERBACK);
    command
        .env_clear()
        .env("HOME", scratch)
        .env("LANGUAGE", "en-US");
    // Folders of its own, not the platform's.
    testkit::daemon::isolate(&mut command, &scratch.join("daemon"));
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command
        .arg("serve")
        .args(options.split_whitespace())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .expect("ladderback starts");
    let began = Instant::now();
    loop {
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        // The log's last line may be half written: wait for its newline.
        if text
            .split(LISTENING)
            .nth(1)
            .is_some_and(|rest| rest.contains('\n'))
        {
            let _ = child.kill();
            let _ = child.wait();
            return std::fs::read_to_string(&log_path).unwrap_or(text);
        }
        if let Some(status) = child.try_wait().expect("the daemon's status") {
            panic!("ladderback exited with {status} before it listened: {text}");
        }
        if began.elapsed() >= START_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("ladderback neither listened nor exited within {START_TIMEOUT:?}: {text}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[then(expr = "the command exits with status {int}")]
fn exits_with(agent: &mut Agent, status: i32) {
    let ran = ran(agent);
    assert_eq!(
        ran.code,
        Some(status),
        "standard output {:?}, standard error {:?}",
        ran.stdout,
        ran.stderr
    );
}

/// The one JSON document standard output holds: an object on a single line,
/// ended by its newline.
fn document(agent: &Agent) -> Value {
    let stdout = &ran(agent).stdout;
    let Some(line) = stdout.strip_suffix('\n') else {
        panic!("standard output doesn't end with a newline: {stdout:?}")
    };
    assert!(
        !line.contains('\n'),
        "standard output is more than one line: {stdout:?}"
    );
    let document: Value = serde_json::from_str(line)
        .unwrap_or_else(|error| panic!("standard output is not JSON ({error}): {stdout:?}"));
    assert!(document.is_object(), "not a JSON object: {document}");
    document
}

/// The value at `path` of a document. A segment names a key; in an array it
/// is the position of an item.
fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |found, segment| match found {
        Value::Object(members) => members.get(segment),
        Value::Array(items) => items.get(segment.parse::<usize>().ok()?),
        _ => None,
    })
}

#[then("standard output is one line holding one JSON document")]
fn one_document(agent: &mut Agent) {
    document(agent);
}

#[then("the document has:")]
fn document_has(agent: &mut Agent, step: &Step) {
    let found = document(agent);
    let table = step.table.as_ref().expect("the step has a table");
    for row in table.rows.iter().skip(1) {
        let [key, value] = row.as_slice() else {
            panic!("a row of a key and a value: {row:?}")
        };
        let expected: Value = serde_json::from_str(value)
            .unwrap_or_else(|error| panic!("{value:?} is not JSON: {error}"));
        assert_eq!(
            lookup(&found, key),
            Some(&expected),
            "the value at {key:?} in {found}"
        );
    }
}

#[then(expr = "the document has no key {string}")]
fn document_lacks(agent: &mut Agent, key: String) {
    let found = document(agent);
    assert_eq!(lookup(&found, &key), None, "a key {key:?} in {found}");
}

#[then(expr = "the document has some text at {string}")]
fn document_has_text(agent: &mut Agent, key: String) {
    let found = document(agent);
    let text = lookup(&found, &key).and_then(Value::as_str);
    assert!(
        text.is_some_and(|text| !text.is_empty()),
        "no text at {key:?} in {found}"
    );
}

#[then("standard error is empty")]
fn stderr_is_empty(agent: &mut Agent) {
    let stderr = &ran(agent).stderr;
    assert!(stderr.is_empty(), "standard error holds {stderr:?}");
}

#[then(expr = "standard output has {int} line(s)")]
fn stdout_lines(agent: &mut Agent, count: usize) {
    let stdout = &ran(agent).stdout;
    assert_eq!(stdout.lines().count(), count, "standard output {stdout:?}");
}

#[then(expr = "standard error has {int} line(s)")]
fn stderr_lines(agent: &mut Agent, count: usize) {
    let stderr = &ran(agent).stderr;
    assert_eq!(stderr.lines().count(), count, "standard error {stderr:?}");
}

#[then("the output is plain text")]
fn output_is_plain(agent: &mut Agent) {
    let said = said(agent);
    assert!(!said.trim().is_empty(), "the command said nothing");
    // Colour is an escape sequence and a redraw is a carriage return or a
    // cursor sequence: all of them are control characters, and text has none
    // but its line ends.
    let controls: Vec<char> = said
        .chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect();
    assert!(
        controls.is_empty(),
        "the output holds control characters {controls:?}: {said:?}"
    );
}
