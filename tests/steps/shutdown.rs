//! Steps for `features/execution/shutdown.feature`: the real `ladderback
//! serve` process, told to stop with SIGTERM, the data folder it leaves and
//! what it logged.

use std::io::{Read, Write};
use std::time::Duration;

use cucumber::{given, then, when};

use super::events_steps::Events;
use super::{Agent, blocking};

/// What the daemon logs once it has stopped, before the time it took.
const STOPPED: &str = "stopped after ";

/// How long the daemon may take to ask for a request's body: a bound on a
/// failure, never a wait the step sits out.
const ASKED_WITHIN: Duration = Duration::from_secs(10);

#[given("a client has begun a request it never finishes")]
async fn request_never_finished(agent: &mut Agent) {
    let addr = agent.events.addr();
    let stream = blocking(move || {
        let mut stream = testkit::net::connect(addr).expect("a connection to the daemon");
        // The headers promise a body and ask whether to send it. The daemon
        // says "100 Continue" once a handler reads the body: from then on the
        // request is open in the daemon, not waiting in its listener's queue,
        // however slow the machine.
        let request = format!(
            "POST /api/v1/operations HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\n\
             Content-Type: application/json\r\nContent-Length: 64\r\n\
             Expect: 100-continue\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .expect("the request is begun");
        stream
            .set_read_timeout(Some(ASKED_WITHIN))
            .expect("a read timeout");
        let mut answer = Vec::new();
        let mut chunk = [0u8; 256];
        while !answer.ends_with(b"\r\n\r\n") {
            match stream.read(&mut chunk) {
                Ok(read @ 1..) => answer.extend_from_slice(chunk.get(..read).unwrap_or_default()),
                // A signal the runner handles (SIGCHLD) can interrupt the
                // wait; that says nothing of the daemon.
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                other => panic!(
                    "the daemon didn't ask for the body: {other:?} after {:?}",
                    String::from_utf8_lossy(&answer)
                ),
            }
        }
        assert!(
            answer.starts_with(b"HTTP/1.1 100 Continue\r\n"),
            "the daemon answered a request it has none of the body of: {:?}",
            String::from_utf8_lossy(&answer)
        );
        // Only the start of the body is ever sent.
        stream.write_all(b"{\"plan\":").expect("the body is begun");
        stream
    })
    .await;
    agent.events.held = Some(stream);
}

#[when("the daemon is told to stop")]
async fn told_to_stop(agent: &mut Agent) {
    Events::stop(agent).await;
}

#[when("the daemon is told to stop, and the plan's after hook is let go once it is stopping")]
async fn told_to_stop_letting_the_hook_go(agent: &mut Agent) {
    let hold = super::operations_steps::after_hook_hold(agent);
    Events::stop_letting_go(agent, Some(hold)).await;
}

#[when(
    "Ladderback is started again on the same folders, and told to stop the moment it says it listens"
)]
async fn started_and_told_to_stop_at_once(agent: &mut Agent) {
    Events::start_and_stop_at_once(agent).await;
}

#[then(expr = "the daemon has exited with status {int}")]
fn exited_with(agent: &mut Agent, code: i32) {
    let status = agent.events.stopped();
    assert_eq!(status.code(), Some(code), "the daemon ended with {status}");
}

#[then(expr = "the daemon stopped within {int} seconds")]
fn stopped_within(agent: &mut Agent, seconds: u64) {
    let took = agent.events.stop_took();
    assert!(
        took < Duration::from_secs(seconds),
        "the daemon took {took:?} to stop:\n{}",
        agent.events.stopped_log()
    );
}

#[then(
    expr = "the operation log is closed, with no {string} or {string} file of it left in the data folder"
)]
fn the_log_is_closed(agent: &mut Agent, wal: String, shm: String) {
    let data = Events::data_dir(agent);
    let database = data.join("ladderback.db");
    assert!(database.is_file(), "{} is there", database.display());
    let left: Vec<String> = [wal, shm]
        .iter()
        .map(|suffix| data.join(format!("ladderback.db{suffix}")))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect();
    assert!(
        left.is_empty(),
        "the daemon left the operation log open: {left:?} outlived it"
    );
}

#[then(expr = "the daemon's log says {string}")]
fn log_says(agent: &mut Agent, words: String) {
    let log = agent.events.stopped_log();
    assert!(
        log.contains(&words),
        "the log doesn't say {words:?}:\n{log}"
    );
}

/// The line the daemon logged once it had stopped, after the one that says
/// it began to.
fn stopped_line(log: &str) -> &str {
    let began = log
        .find("stopping (")
        .unwrap_or_else(|| panic!("no line says the daemon began to stop:\n{log}"));
    log.get(began..)
        .and_then(|rest| rest.lines().find(|line| line.contains(STOPPED)))
        .unwrap_or_else(|| panic!("no line after the start of the stop says it stopped:\n{log}"))
}

#[then("the daemon's log says how long it took to stop, and how long it waited for each step")]
fn log_says_how_long(agent: &mut Agent) {
    let log = agent.events.stopped_log();
    let line = stopped_line(log);
    let took = line
        .split_once(STOPPED)
        .and_then(|(_, rest)| rest.split_once(" s"))
        .and_then(|(seconds, _)| seconds.parse::<f64>().ok());
    assert!(took.is_some(), "the line doesn't say how long: {line}");
    for step in ["open requests", "operation log", "workers", "tasks"] {
        assert!(
            line.contains(step),
            "the line doesn't name {step:?}: {line}"
        );
    }
    assert!(!line.contains("gave up"), "a step was given up: {line}");
}

#[then("the daemon's log says it stopped without waiting longer for the open requests")]
fn log_says_gave_up_on_requests(agent: &mut Agent) {
    let log = agent.events.stopped_log();
    let line = stopped_line(log);
    assert!(
        line.split(", ")
            .any(|step| step.contains("open requests") && step.contains("gave up")),
        "the line doesn't say it gave up on the open requests: {line}"
    );
}
