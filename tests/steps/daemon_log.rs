//! Steps for the `@e2e` rules of `features/execution/daemon_log.feature`:
//! the recent log the real daemon keeps, read over HTTP through the
//! request-check steps' client. The requests, the session, the API tokens
//! and the answer's status are the steps of `sign_in.rs`, `api_tokens.rs`
//! and `request_checks.rs`.

use cucumber::{given, then};
use serde_json::Value;

use super::Agent;
use super::request_checks_steps as web;

/// Starts `ladderback serve` on loopback in the folders `the daemon's config
/// file holds:` wrote its config file into.
#[given("Ladderback is serving on loopback with that config file")]
async fn serving_with_that_config_file(agent: &mut Agent) {
    agent.web.home = Some(agent.scratch().join("daemon"));
    web::serve_on_loopback(agent).await;
}

/// As above, with the admin password `password` set in those folders first,
/// so sign-in is on.
#[given(
    expr = "Ladderback is serving on loopback with that config file, sign-in on and the admin password {string}"
)]
async fn serving_with_that_config_file_and_sign_in(agent: &mut Agent, password: String) {
    agent.web.home = Some(agent.scratch().join("daemon"));
    super::sign_in_steps::serving_with_sign_in(agent, password).await;
}

/// The answer's lines, each an object, newest first.
fn lines(agent: &Agent) -> Vec<Value> {
    let response = agent.web.response();
    let body: Value = serde_json::from_str(&response.body)
        .unwrap_or_else(|error| panic!("not JSON ({error}): {:?}", response.body));
    body["lines"]
        .as_array()
        .unwrap_or_else(|| panic!("no list of lines: {:?}", response.body))
        .clone()
}

/// A line's instant: RFC 3339 in UTC, to the millisecond
/// (`2026-11-15T09:00:00.000Z`), so that two compare as their text does.
fn at(line: &Value) -> &str {
    let at = line["at"]
        .as_str()
        .unwrap_or_else(|| panic!("a line without its instant: {line}"));
    assert!(
        at.len() == "2026-11-15T09:00:00.000Z".len()
            && at.ends_with('Z')
            && at.as_bytes().get(10) == Some(&b'T')
            && at.as_bytes().get(19) == Some(&b'.'),
        "not an instant in UTC to the millisecond: {at:?}"
    );
    at
}

/// A line's message, without the marks that isolate a value in it.
fn message(line: &Value) -> String {
    line["message"]
        .as_str()
        .unwrap_or_else(|| panic!("a line without its message: {line}"))
        .replace(['\u{2068}', '\u{2069}'], "")
}

#[then(
    "the answer lists the recent log newest first, its oldest line saying that Ladderback is starting"
)]
fn lists_newest_first(agent: &mut Agent) {
    let lines = lines(agent);
    let oldest = lines.last().expect("the recent log holds a line");
    assert!(
        message(oldest).ends_with(" starting") && oldest["level"] == "info",
        "the oldest line doesn't say that Ladderback is starting: {oldest}"
    );
    // The start is logged before the daemon starts its runtime, so every line
    // is timed at or after it. Two threads logging at once may be timed in
    // the other order than they are kept, so no other pair is compared.
    let instants: Vec<&str> = lines.iter().map(at).collect();
    let started = at(oldest);
    assert!(
        instants.iter().all(|instant| *instant >= started),
        "a line timed before the start: {instants:?}"
    );
}

#[then(expr = "the answer holds a warning saying {string}")]
fn holds_a_warning(agent: &mut Agent, words: String) {
    let lines = lines(agent);
    assert!(
        lines
            .iter()
            .any(|line| line["level"] == "warn" && message(line).contains(&words)),
        "no warning saying {words:?} in: {lines:?}"
    );
}
