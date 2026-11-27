//! The `@e2e` scenarios of `features/fleet/hub_commands_e2e.feature`: a
//! person signed in on a real hub's page runs a plan on a real machine's
//! daemon connected to it, which records that the hub's session asked; a
//! page on another site that carries the person's session is refused before
//! anything reaches the machine (§7.2, §10.3; §16, decision 176).

use std::time::{Duration, Instant};

use app::ports::{HistoryQuery, OperationStore as _};
use cucumber::{given, then, when};
use domain::execution::{OperationKind, Trigger};
use domain::{OperationId, Uuid};
use store::SqliteStore;

use super::hub_page::page_asks;
use super::{hub_address, hub_root, ladderback, machine_root};
use crate::events_steps::save_daemon_config_in;
use crate::operations_steps::exchange_at;
use crate::request_checks_steps::{Request, exchange};
use crate::{Agent, backup_steps, blocking};

/// How long the machine's backup of a few small files may take, with slack
/// for a loaded runner.
const BACKUP_WITHIN: Duration = Duration::from_secs(60);

#[given(expr = "the machine has the plan {string} over a real repository")]
async fn machine_has_plan(agent: &mut Agent, plan: String) {
    backup_steps::new_repository(agent).await;
    backup_steps::source_with_files(agent, &plan, 3);
    let dir = testkit::daemon::config_dir(&machine_root(agent));
    save_daemon_config_in(agent, &dir, None).await;
}

#[given(expr = "the hub's admin password is {string}")]
async fn hub_password(agent: &mut Agent, password: String) {
    let said = ladderback(
        hub_root(agent),
        &["set-password"],
        Some(format!("{password}\n")),
    )
    .await;
    assert!(said.success, "{said:?}");
}

#[given(expr = "the person signed in on the hub's page with {string}")]
async fn signed_in(agent: &mut Agent, password: String) {
    let address = hub_address(agent);
    let request = Request {
        method: "POST".to_owned(),
        path: "/api/v1/session".to_owned(),
        headers: vec![
            "Origin: http://localhost:{port}".to_owned(),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body: serde_json::json!({ "password": password }).to_string(),
    };
    let answer = blocking(move || exchange(address, &request)).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let cookie = answer
        .header("set-cookie")
        .and_then(|cookie| cookie.split(';').next())
        .unwrap_or_else(|| panic!("the sign-in set a cookie: {}", answer.body))
        .to_owned();
    let body: serde_json::Value = serde_json::from_str(&answer.body)
        .unwrap_or_else(|error| panic!("{error}: {}", answer.body));
    let csrf = body["csrf-token"]
        .as_str()
        .unwrap_or_else(|| panic!("the sign-in named a CSRF token: {body}"))
        .to_owned();
    agent.pairing_terminal.session = Some((cookie, csrf));
}

/// The agent ID of the machine the hub's dashboard lists as `hostname`.
async fn machine_id(agent: &Agent, hostname: &str) -> String {
    let (status, body) = page_asks(agent, "GET", api::HUB_DASHBOARD_PATH, String::new()).await;
    assert_eq!(status, 200, "{body}");
    let machines: api::HubMachinesDto =
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"));
    machines
        .machines
        .into_iter()
        .find(|machine| machine.hostname == hostname)
        .unwrap_or_else(|| panic!("the hub lists no {hostname}: {body}"))
        .machine_id
}

fn run_body(plan: String) -> String {
    serde_json::to_string(&api::MachineRunDto { plan }).expect("a run's body")
}

#[when(expr = "the person runs {string} on {string} from the hub's page")]
async fn runs_from_page(agent: &mut Agent, plan: String, hostname: String) {
    let id = machine_id(agent, &hostname).await;
    let answer = page_asks(agent, "POST", &api::machine_runs_path(&id), run_body(plan)).await;
    agent.pairing_terminal.page_answer = Some(answer);
}

#[when(
    expr = "a page on another site asks the hub to run {string} on {string} with the person's session"
)]
async fn another_site_runs(agent: &mut Agent, plan: String, hostname: String) {
    let id = machine_id(agent, &hostname).await;
    let (cookie, _) = agent
        .pairing_terminal
        .session
        .clone()
        .expect("the person signed in");
    let address = hub_address(agent);
    // The browser sends the session's cookie with the other site's request,
    // but that site can't read the CSRF token, so it sends none.
    let request = Request {
        method: "POST".to_owned(),
        path: api::machine_runs_path(&id),
        headers: vec![
            "Origin: http://evil.example".to_owned(),
            "Content-Type: application/json".to_owned(),
            format!("Cookie: {cookie}"),
        ],
        default_host: true,
        body: run_body(plan),
    };
    let answer = blocking(move || exchange(address, &request)).await;
    agent.pairing_terminal.page_answer = Some((answer.status, answer.body));
}

/// The machine's operation the hub's answer named.
fn started_operation(agent: &Agent) -> String {
    let (status, body) = agent
        .pairing_terminal
        .page_answer
        .as_ref()
        .expect("the page asked the hub");
    assert_eq!(*status, 202, "{body}");
    let done: api::MachineCommandDoneDto =
        serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"));
    done.operation
}

#[then("the hub answers that the machine started a backup")]
fn answers_started(agent: &mut Agent) {
    started_operation(agent);
}

#[then("the machine's backup soon succeeds")]
async fn backup_succeeds(agent: &mut Agent) {
    let id = started_operation(agent);
    let address = agent
        .pairing_terminal
        .machine_address
        .expect("the machine's daemon serves");
    let began = Instant::now();
    let mut last = String::new();
    while began.elapsed() < BACKUP_WITHIN {
        let path = format!("/api/v1/operations/{id}");
        let (status, body) = blocking(move || exchange_at(address, "GET", &path, "")).await;
        assert_eq!(status, 200, "the machine knows the operation: {body}");
        let detail: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|error| panic!("{error}: {body}"));
        match detail["operation"]["status"].as_str() {
            Some("succeeded") => return,
            Some("queued" | "running") => {}
            _ => panic!("the machine's backup didn't succeed: {body}"),
        }
        last = body;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("the machine's backup didn't end within {BACKUP_WITHIN:?}: {last}");
}

/// Stops the machine's daemon and opens its store.
async fn machine_store_once_stopped(agent: &mut Agent) -> SqliteStore {
    let machine = agent.pairing_terminal.machine.take();
    // Dropping the daemon ends it and waits for it.
    blocking(move || drop(machine)).await;
    let path = testkit::daemon::data_dir(&machine_root(agent)).join("ladderback.db");
    SqliteStore::open(&path)
        .await
        .expect("the machine's store opens once its daemon stopped")
}

#[then(
    "once the machine's daemon stops, its store says the hub started that backup for the person's session"
)]
async fn recorded_for_session(agent: &mut Agent) {
    let id = started_operation(agent);
    // The cookie is `<name>=<token>`: the tag is of the token's digest.
    let (cookie, _) = agent
        .pairing_terminal
        .session
        .as_ref()
        .expect("the person signed in on the hub");
    let token = cookie
        .split_once('=')
        .and_then(|(_, token)| domain::access::Token::parse(token))
        .unwrap_or_else(|| panic!("the session's cookie holds a token: {cookie}"));
    let tag = domain::access::SessionTag::of(&token.digest());
    let store = machine_store_once_stopped(agent).await;
    let uuid = Uuid::parse_str(&id).unwrap_or_else(|error| panic!("{error}: {id}"));
    let detail = store.get(OperationId::from_uuid(uuid)).await;
    store.close().await;
    let record = detail
        .expect("the machine's store answers")
        .unwrap_or_else(|| panic!("the machine's store holds no {id}"))
        .record;
    assert_eq!(record.kind, OperationKind::Backup, "{record:?}");
    assert_eq!(
        record.trigger,
        Trigger::Hub {
            asked_by: Some(tag)
        },
        "{record:?}"
    );
}

#[then("once the machine's daemon stops, its store holds no backup")]
async fn no_backup(agent: &mut Agent) {
    let store = machine_store_once_stopped(agent).await;
    let page = store.list(&HistoryQuery::default()).await;
    store.close().await;
    let page = page.expect("the machine's store lists its operations");
    let backups: Vec<_> = page
        .operations
        .iter()
        .filter(|operation| operation.kind == OperationKind::Backup)
        .collect();
    assert!(backups.is_empty(), "{backups:?}");
}
