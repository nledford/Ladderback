//! Steps of `features/execution/restart_from_settings.feature`: a daemon a
//! service manager runs (`serve --service-managed`, as the service file
//! starts it), asked to restart over the API as the Settings page asks with
//! sign-in off. How it stops, and what its log says, are read by the steps of
//! `shutdown.rs`.

use cucumber::{given, then, when};

use super::{Agent, blocking, events_steps};

/// The argument a service file passes after `serve`.
const SERVICE_MANAGED: &str = "--service-managed";

/// What the restart route answered, kept for the steps that read it.
#[derive(Default)]
pub struct Restarting {
    answer: Option<(u16, String)>,
}

#[given("Ladderback is serving on loopback under a service manager")]
async fn serving_under_a_manager(agent: &mut Agent) {
    agent.events.args = vec![SERVICE_MANAGED.to_owned()];
    events_steps::serve(agent).await;
}

#[given(expr = "Ladderback is serving on loopback under a service manager with the plan {string}")]
async fn serving_under_a_manager_with_plan(agent: &mut Agent, plan: String) {
    agent.events.args = vec![SERVICE_MANAGED.to_owned()];
    events_steps::serve_with_plan(agent, &plan).await;
}

#[given("Ladderback is serving on loopback, started by hand")]
async fn serving_by_hand(agent: &mut Agent) {
    events_steps::serve(agent).await;
}

/// Accepted, a restart now stops the daemon at once: the step waits for it
/// to exit, as a stop does.
#[when("the admin asks Ladderback to restart now")]
async fn asks_to_restart_now(agent: &mut Agent) {
    let answer = events_steps::Events::restart_route(agent, "POST", r#"{"when":"now"}"#).await;
    let accepted = answer.0 == 202;
    agent.restarting.answer = Some(answer);
    if accepted {
        events_steps::Events::await_exit(agent).await;
    }
}

#[when("the admin asks Ladderback to restart once the running operations finish")]
async fn asks_to_restart_when_finished(agent: &mut Agent) {
    let answer = events_steps::Events::restart_route(agent, "POST", r#"{"when":"finished"}"#).await;
    agent.restarting.answer = Some(answer);
}

#[when("the admin cancels the restart")]
async fn cancels_the_restart(agent: &mut Agent) {
    let (status, body) = events_steps::Events::restart_route(agent, "DELETE", "{}").await;
    assert_eq!(status, 200, "the cancel was answered {status}: {body}");
}

#[then("the daemon exits of itself with the restart status")]
async fn exits_with_the_restart_status(agent: &mut Agent) {
    events_steps::Events::await_exit(agent).await;
    let status = agent.events.stopped();
    assert_eq!(
        status.code(),
        Some(75),
        "the daemon ended with {status}:\n{}",
        agent.events.stopped_log()
    );
}

fn answer(agent: &Agent) -> &(u16, String) {
    agent
        .restarting
        .answer
        .as_ref()
        .expect("a restart was asked for")
}

#[then("the request is accepted")]
fn accepted(agent: &mut Agent) {
    let (status, body) = answer(agent);
    assert_eq!(*status, 202, "the restart was answered {status}: {body}");
}

#[then(expr = "the request is refused with the code {string}")]
fn refused_with(agent: &mut Agent, code: String) {
    let (status, body) = answer(agent);
    assert_eq!(
        code_of(*status, body),
        code,
        "the restart was answered {status}: {body}"
    );
}

/// The code of the refusal `body`, answered with `status`.
fn code_of(status: u16, body: &str) -> String {
    let json: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|error| panic!("the refusal {status} is JSON ({error}): {body}"));
    json.get("code")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("the refusal {status} has a code: {body}"))
        .to_owned()
}

/// What the restart route says now, as Settings reads it.
async fn told(agent: &Agent) -> serde_json::Value {
    let (status, body) = events_steps::Events::restart_route(agent, "GET", "").await;
    assert_eq!(status, 200, "the restart route answered {status}: {body}");
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("the answer is JSON ({error}): {body}"))
}

#[then("Ladderback says no restart is offered")]
async fn says_none_offered(agent: &mut Agent) {
    let told = told(agent).await;
    assert_eq!(
        told.get("offered"),
        Some(&serde_json::Value::Bool(false)),
        "{told}"
    );
}

#[then("Ladderback says a restart waits for the running operations")]
async fn says_pending(agent: &mut Agent) {
    let told = told(agent).await;
    assert_eq!(
        told.get("pending"),
        Some(&serde_json::Value::Bool(true)),
        "{told}"
    );
}

#[then("Ladderback says no restart waits")]
async fn says_none_pending(agent: &mut Agent) {
    let told = told(agent).await;
    assert_eq!(
        told.get("pending"),
        Some(&serde_json::Value::Bool(false)),
        "{told}"
    );
}

#[then(expr = "a prune of the repository is refused with the code {string}")]
async fn a_prune_is_refused(agent: &mut Agent, code: String) {
    let addr = agent.events.addr();
    let (status, body) = blocking(move || {
        super::operations_steps::exchange_at(
            addr,
            "POST",
            "/api/v1/repositories/demo/maintenance",
            r#"{"kind":"prune"}"#,
        )
    })
    .await;
    assert_eq!(
        code_of(status, &body),
        code,
        "the prune was answered {status}: {body}"
    );
}

/// A download of a snapshot's file, confirmed by its short ID as with
/// sign-in off: refused before the snapshot is looked for.
#[then(expr = "a download from the repository is refused with the code {string}")]
async fn a_download_is_refused(agent: &mut Agent, code: String) {
    let addr = agent.events.addr();
    let (status, body) = blocking(move || {
        super::operations_steps::exchange_at(
            addr,
            "POST",
            "/api/v1/repositories/demo/snapshots/3f2a1b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708/download",
            r#"{"path":"source/notes.txt","format":"tar","confirmation":"3f2a1b4c"}"#,
        )
    })
    .await;
    assert_eq!(
        code_of(status, &body),
        code,
        "the download was answered {status}: {body}"
    );
}
