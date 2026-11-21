//! Steps for `features/automation/mcp_daemon_log.feature`: `read_log`, the
//! tool that reads the daemon's recent log, called on the scenario's MCP
//! server through the steps of `mcp.rs`, and the Log page's own read of the
//! same log, `GET /api/v1/log`, to hold it to.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use cucumber::{given, then, when};
use serde_json::{Value, json};

use super::Agent;
use super::mcp_steps::{call, content, result};
use super::request_checks_steps::Request;

/// How long a line the daemon logs after an operation's end may take to
/// show: `wait_operation` answers once the operation's end is recorded, and
/// the line saying so is logged just after.
const LOGGED: Duration = Duration::from_secs(10);

/// The most bytes of text one read answers (§16, decision 134).
const READ_MAX_BYTES: usize = 32 * 1024;

/// Calls `read_log` with `arguments` and keeps the page it answers, unless
/// it refused; a refusal is remembered, so no step reads an earlier page as
/// its answer.
async fn read(agent: &mut Agent, arguments: Value) {
    call(agent, "read_log".to_owned(), arguments).await;
    agent.mcp.log_refused = result(agent)["isError"] == true;
    if agent.mcp.log_refused {
        return;
    }
    let page = content(agent).clone();
    agent.mcp.log_pages.push(page);
}

/// The page read last, which the latest read answered.
fn page(agent: &Agent) -> &Value {
    assert!(
        !agent.mcp.log_refused,
        "the latest read was refused: {}",
        result(agent)
    );
    agent.mcp.log_pages.last().expect("the log was read")
}

/// The lines of the page read last.
fn lines(agent: &Agent) -> &Vec<Value> {
    let page = page(agent);
    page["lines"]
        .as_array()
        .unwrap_or_else(|| panic!("no list of lines: {page}"))
}

/// A line's message, without the marks that isolate a value in it.
fn message(line: &Value) -> String {
    line["message"]
        .as_str()
        .unwrap_or_else(|| panic!("a line without its message: {line}"))
        .replace(['\u{2068}', '\u{2069}'], "")
}

fn number(line: &Value) -> u64 {
    line["number"]
        .as_u64()
        .unwrap_or_else(|| panic!("a line without its number: {line}"))
}

fn at(line: &Value) -> String {
    line["at"]
        .as_str()
        .unwrap_or_else(|| panic!("a line without its instant: {line}"))
        .to_owned()
}

/// What the line saying that the scenario's operation ended says.
fn ended(agent: &Agent) -> String {
    format!("operation {} ended", agent.ops.the_operation())
}

/// Reads the log of the scenario's operation until a line says it ended,
/// for [`LOGGED`] at most, and answers that line.
async fn the_end_line(agent: &mut Agent) -> Value {
    let said = ended(agent);
    let operation = agent.ops.the_operation();
    let start = Instant::now();
    loop {
        read(agent, json!({"operation": operation})).await;
        if let Some(line) = lines(agent)
            .iter()
            .find(|line| message(line).contains(&said))
        {
            return line.clone();
        }
        assert!(
            start.elapsed() < LOGGED,
            "no line says {said:?}: {}",
            page(agent)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[given("the daemon's config file holds 150 repositories, each with a password in its address")]
fn many_repositories(agent: &mut Agent) {
    let mut text = "schema = 1\n".to_owned();
    for number in 0..150 {
        write!(
            text,
            "\n[repositories.nas-{number}]\nrepository = \"rest:https://demo:demo-location-password-{number}@backup.example.com/nas-{number}\"\npassword-env = \"DEMO_PASSWORD\"\n"
        )
        .expect("a config in memory");
    }
    let folder = testkit::daemon::config_dir(&agent.scratch().join("daemon"));
    std::fs::create_dir_all(&folder).expect("the daemon's config folder");
    std::fs::write(folder.join("config.toml"), text).expect("the daemon's config file");
}

/// With sign-in off, `read_log` is held to the `[without-token]` table's
/// Daemon log (decision 127), which its defaults leave out: the config file
/// written so far allows it.
#[given("the daemon's config file lets AI agents without a token read the log")]
fn config_allows_daemon_log(agent: &mut Agent) {
    let file = testkit::daemon::config_dir(&agent.scratch().join("daemon")).join("config.toml");
    let mut text = std::fs::read_to_string(&file).expect("the daemon's config file");
    text.push_str("\n[without-token]\ndaemon-log = \"read\"\n");
    std::fs::write(&file, text).expect("the daemon's config file");
}

/// With sign-in off, `read_log` is held to the `[without-token]` table's
/// Daemon log (decision 127), which its defaults leave out.
#[given("AI agents without a token may read the daemon's log")]
async fn without_token_daemon_log(agent: &mut Agent) {
    use domain::access::permissions::{Permission, Permissions};
    let log = Permission::DaemonLog;
    let permissions = Permissions::default()
        .with(log, Some(log.level()))
        .expect("Daemon log has its level");
    agent.backup.without_token = Some(permissions);
    super::events_steps::save_daemon_config(agent, None).await;
}

#[when("the MCP client reads the log")]
async fn reads(agent: &mut Agent) {
    read(agent, json!({})).await;
}

#[when(expr = "the MCP client reads the log at the level {string}")]
async fn reads_at(agent: &mut Agent, level: String) {
    read(agent, json!({"level": level, "limit": 200})).await;
}

#[when(expr = "the MCP client reads the log at the level {string}, {int} lines to a page")]
async fn reads_at_paged(agent: &mut Agent, level: String, limit: u64) {
    read(agent, json!({"level": level, "limit": limit})).await;
}

#[when(expr = "the MCP client reads the log, {int} lines to a page")]
async fn reads_paged(agent: &mut Agent, limit: u64) {
    read(agent, json!({"limit": limit})).await;
}

#[when("the MCP client reads the next page of the log")]
async fn reads_next(agent: &mut Agent) {
    let before = page(agent)["next"].clone();
    assert!(before.is_u64(), "no page after: {}", page(agent));
    let limit = lines(agent).len();
    read(agent, json!({"before": before, "limit": limit})).await;
}

#[when("the MCP client reads the log of that operation")]
async fn reads_of_operation(agent: &mut Agent) {
    the_end_line(agent).await;
}

#[when(expr = "the MCP client reads the log of the plan {string}")]
async fn reads_of_plan(agent: &mut Agent, plan: String) {
    read(agent, json!({"plan": plan, "limit": 200})).await;
}

#[when("the MCP client reads the log since the time of the line saying that operation ended")]
async fn reads_since_the_end(agent: &mut Agent) {
    let since = at(&the_end_line(agent).await);
    agent.mcp.log_bound = Some(since.clone());
    read(agent, json!({"since": since, "limit": 200})).await;
}

#[when(expr = "the MCP client reads the log until the time of the line saying {string}")]
async fn reads_until(agent: &mut Agent, said: String) {
    read(agent, json!({"limit": 200})).await;
    let line = lines(agent)
        .iter()
        .find(|line| message(line).contains(&said))
        .unwrap_or_else(|| panic!("no line says {said:?}: {}", page(agent)))
        .clone();
    let until = at(&line);
    agent.mcp.log_bound = Some(until.clone());
    read(agent, json!({"until": until, "limit": 200})).await;
}

#[then(expr = "every line read is at the level {string} or worse")]
fn every_line_at(agent: &mut Agent, level: String) {
    let order = ["error", "warn", "info", "debug", "trace"];
    let rank = |level: &str| order.iter().position(|named| *named == level);
    let threshold = rank(&level).expect("a level");
    let lines = lines(agent);
    assert!(!lines.is_empty(), "no line read: {}", page(agent));
    for line in lines {
        let at = line["level"].as_str().and_then(rank);
        assert!(at.is_some_and(|at| at <= threshold), "{line}");
    }
}

#[then("every line read names that operation")]
fn every_line_names_operation(agent: &mut Agent) {
    let operation = agent.ops.the_operation();
    let lines = lines(agent);
    assert!(!lines.is_empty(), "no line read: {}", page(agent));
    for line in lines {
        assert!(message(line).contains(&operation), "{line}");
    }
}

#[then(expr = "every line read names the plan {string}")]
fn every_line_names_plan(agent: &mut Agent, plan: String) {
    let named = format!("plan {plan}");
    let lines = lines(agent);
    assert!(!lines.is_empty(), "no line read: {}", page(agent));
    for line in lines {
        assert!(message(line).contains(&named), "{line}");
    }
}

#[then("every line read was written at that time or later")]
fn every_line_since(agent: &mut Agent) {
    let bound = agent.mcp.log_bound.clone().expect("a time read");
    for line in lines(agent) {
        assert!(at(line) >= bound, "{line} before {bound}");
    }
}

#[then("every line read was written at that time or earlier")]
fn every_line_until(agent: &mut Agent) {
    let bound = agent.mcp.log_bound.clone().expect("a time read");
    for line in lines(agent) {
        assert!(at(line) <= bound, "{line} after {bound}");
    }
}

#[then(expr = "a line read says {string}")]
fn a_line_says(agent: &mut Agent, said: String) {
    assert!(
        lines(agent)
            .iter()
            .any(|line| message(line).contains(&said)),
        "no line says {said:?}: {}",
        page(agent)
    );
}

#[then("a line read says that operation ended")]
fn a_line_says_ended(agent: &mut Agent) {
    let said = ended(agent);
    a_line_says(agent, said);
}

#[then(expr = "a line read says the manual run of the plan {string} didn't start")]
fn a_line_says_not_started(agent: &mut Agent, plan: String) {
    a_line_says(agent, format!("the manual run of plan {plan} didn't start"));
}

#[then(expr = "no line read says {string}")]
fn no_line_says(agent: &mut Agent, said: String) {
    let page = page(agent);
    for line in lines(agent) {
        assert!(!message(line).contains(&said), "{line} in {page}");
    }
}

#[then("no line read says that operation ended")]
fn no_line_says_ended(agent: &mut Agent) {
    let said = ended(agent);
    no_line_says(agent, said);
}

#[then(expr = "the lines read are those the Log page shows under {string}")]
async fn as_the_log_page_shows(agent: &mut Agent, choice: String) {
    assert_eq!(choice, "Warnings and errors", "a level the step knows");
    let response = super::api_tokens_steps::exchange(
        agent,
        Request {
            method: "GET".to_owned(),
            path: "/api/v1/log".to_owned(),
            headers: Vec::new(),
            default_host: true,
            body: String::new(),
        },
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.body);
    let answer: Value = serde_json::from_str(&response.body).expect("the log as JSON");
    let read = lines(agent);
    let newest = read.first().map_or(0, number);
    // The Log page's "Warnings and errors" keeps the errors and warnings;
    // the lines logged after the read are left out of the comparison.
    let shown: Vec<&Value> = answer["lines"]
        .as_array()
        .expect("a list of lines")
        .iter()
        .filter(|line| matches!(line["level"].as_str(), Some("error" | "warn")))
        .filter(|line| number(line) <= newest)
        .collect();
    let read: Vec<&Value> = read.iter().collect();
    assert_eq!(read, shown);
}

#[then(expr = "{int} lines are read, newest first")]
fn lines_newest_first(agent: &mut Agent, count: usize) {
    let numbers: Vec<u64> = lines(agent).iter().map(number).collect();
    assert_eq!(numbers.len(), count, "{}", page(agent));
    assert!(
        numbers.windows(2).all(|pair| pair[0] > pair[1]),
        "not newest first: {numbers:?}"
    );
}

#[then(expr = "{int} lines are read, newest first, each older than the page before's")]
fn lines_older_than_before(agent: &mut Agent, count: usize) {
    lines_newest_first(agent, count);
    let pages = &agent.mcp.log_pages;
    let before = &pages[pages.len() - 2];
    let oldest_before = before["lines"]
        .as_array()
        .and_then(|lines| lines.last())
        .map(number)
        .expect("a page before");
    for line in lines(agent) {
        assert!(
            number(line) < oldest_before,
            "{line} not older than {oldest_before}"
        );
    }
}

#[then(expr = "the read was cut short by its {string} cap")]
fn cut_short_by(agent: &mut Agent, cap: String) {
    let page = page(agent);
    assert_eq!(page["stopped"], cap.as_str(), "{page}");
    let oldest = lines(agent).last().map(number);
    assert_eq!(page["next"].as_u64(), oldest, "{page}");
}

#[then(expr = "fewer than {int} lines are read, holding at most 32 KiB of text")]
fn fewer_within_bytes(agent: &mut Agent, most: usize) {
    let lines = lines(agent);
    assert!(
        !lines.is_empty() && lines.len() < most,
        "{} lines",
        lines.len()
    );
    let bytes: usize = lines
        .iter()
        .map(|line| {
            line["source"].as_str().map_or(0, str::len)
                + line["message"].as_str().map_or(0, str::len)
        })
        .sum();
    assert!(bytes <= READ_MAX_BYTES, "{bytes} bytes");
}
