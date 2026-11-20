//! Steps for the `@e2e` rules of `features/security/api_tokens.feature`: API
//! tokens the admin creates, changes and revokes in a session over HTTP,
//! presented to the real daemon as `Authorization: Bearer <token>` (§10.1,
//! decisions 93 and 127).

use cucumber::{given, then, when};

use super::request_checks_steps::{self as web, Request, Response};
use super::sign_in_steps;
use super::{Agent, blocking};

/// The token the scenario created last.
#[derive(Debug, Default)]
pub struct ApiTokensE2e {
    created: Option<Created>,
}

#[derive(Debug)]
struct Created {
    id: String,
    /// The whole text, as the answer that created it gave it, this once.
    text: String,
}

/// What precedes a token's secret: the prefix, the 12-character ID and the
/// `_` after it. The secret may hold `_` itself.
const BEFORE_SECRET: usize = "ladderback_".len() + 12 + 1;

/// A token nobody created: well formed, so that only its check refuses it.
pub(super) const NOBODY_S: &str =
    "ladderback_abcdefghijkl_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

impl ApiTokensE2e {
    fn created(&self) -> &Created {
        self.created
            .as_ref()
            .expect("the admin created an API token")
    }

    /// The created token's whole text, as a program presents it.
    pub(super) fn text(&self) -> &str {
        &self.created().text
    }

    /// The created token's secret, without the ID before it.
    fn secret(&self) -> String {
        let text = &self.created().text;
        text.get(BEFORE_SECRET..)
            .unwrap_or_else(|| panic!("a token's text holds a secret: {text:?}"))
            .to_owned()
    }
}

/// Sends `request` to the daemon from a blocking thread: see [`blocking`].
pub(crate) async fn exchange(agent: &Agent, request: Request) -> Response {
    let addr = agent.web.server().addr;
    blocking(move || web::exchange(addr, &request)).await
}

/// A request in the admin's session, with its CSRF token and a JSON body.
pub(crate) fn in_session(agent: &Agent, method: &str, path: &str, body: String) -> Request {
    Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: vec![
            sign_in_steps::session_cookie(agent),
            sign_in_steps::csrf_header(agent),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body,
    }
}

/// Every permission, as the wire names it.
const PERMISSIONS: [&str; 7] = [
    "backups",
    "backup-runs",
    "previews",
    "configuration",
    "snapshot-contents",
    "daemon-log",
    "repository-check",
];

/// Permissions as a scenario names them, `backups: read, daemon-log: read`,
/// as the wire writes them: each one named at its level, every other at
/// `none`.
pub(crate) fn permissions_body(named: &str) -> serde_json::Value {
    let mut body: serde_json::Map<String, serde_json::Value> = PERMISSIONS
        .into_iter()
        .map(|permission| (permission.to_owned(), "none".into()))
        .collect();
    for pair in named
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
    {
        let (permission, level) = pair
            .split_once(':')
            .unwrap_or_else(|| panic!("a permission as `name: level`: {pair:?}"));
        let permission = permission.trim();
        assert!(
            PERMISSIONS.contains(&permission),
            "no permission {permission:?}"
        );
        body.insert(permission.to_owned(), level.trim().into());
    }
    serde_json::Value::Object(body)
}

#[given(
    expr = "the admin has created an API token over HTTP with the permissions {string} and the label {string}"
)]
async fn has_created(agent: &mut Agent, permissions: String, label: String) {
    let body = serde_json::json!({ "label": label, "permissions": permissions_body(&permissions) })
        .to_string();
    let request = in_session(agent, "POST", "/api/v1/tokens", body);
    let response = exchange(agent, request).await;
    assert_eq!(response.status, 201, "body {:?}", response.body);
    let created: serde_json::Value =
        serde_json::from_str(&response.body).expect("a created token's answer is JSON");
    let text = created["secret"].as_str().expect("the token's text");
    let id = created["token"]["id"].as_str().expect("the token's ID");
    assert!(
        text.starts_with(&format!("ladderback_{id}_")),
        "the text {text:?} doesn't name the ID {id:?}"
    );
    agent.api_tokens.created = Some(Created {
        id: id.to_owned(),
        text: text.to_owned(),
    });
}

#[given("the admin has revoked that API token over HTTP")]
async fn has_revoked(agent: &mut Agent) {
    let path = format!("/api/v1/tokens/{}", agent.api_tokens.created().id);
    let request = in_session(agent, "DELETE", &path, String::new());
    let response = exchange(agent, request).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
}

fn with_bearer(agent: &mut Agent, method: String, path: String, token: &str) {
    web::request_for(agent, method, path);
    agent
        .web
        .request()
        .headers
        .push(format!("Authorization: Bearer {token}"));
}

#[given(expr = "a {string} request for {string} with the API token")]
fn with_the_token(agent: &mut Agent, method: String, path: String) {
    let text = agent.api_tokens.created().text.clone();
    with_bearer(agent, method, path, &text);
}

#[given(expr = "a {string} request for {string} with a token nobody created")]
fn with_nobody_s_token(agent: &mut Agent, method: String, path: String) {
    with_bearer(agent, method, path, NOBODY_S);
}

#[when(
    expr = "the admin changes the admin password over HTTP from {string} to {string} with the API token"
)]
fn changes_with_the_token(agent: &mut Agent, current: String, new: String) {
    let text = agent.api_tokens.created().text.clone();
    with_bearer(
        agent,
        "POST".to_owned(),
        sign_in_steps::ADMIN_PASSWORD_PATH.to_owned(),
        &text,
    );
    let request = agent.web.request();
    request
        .headers
        .push("Content-Type: application/json".to_owned());
    request.body = sign_in_steps::admin_password_body(Some(&current), &new);
    web::send(agent);
}

/// The body of a change of the created token's permissions, in a request
/// that has no credentials yet.
fn change_request(agent: &mut Agent, permissions: &str, password: &str) {
    let path = format!(
        "/api/v1/tokens/{}/permissions",
        agent.api_tokens.created().id
    );
    web::request_for(agent, "PUT".to_owned(), path);
    let request = agent.web.request();
    request
        .headers
        .push("Content-Type: application/json".to_owned());
    request.body = serde_json::json!({
        "permissions": permissions_body(permissions),
        "current-password": password,
    })
    .to_string();
}

#[given(
    expr = "a request in the admin's session that changes the API token's permissions to {string} with the admin password {string}"
)]
fn change_in_session(agent: &mut Agent, permissions: String, password: String) {
    change_request(agent, &permissions, &password);
    let headers = [
        sign_in_steps::session_cookie(agent),
        sign_in_steps::csrf_header(agent),
    ];
    agent.web.request().headers.extend(headers);
}

#[given(
    expr = "a request with the API token that changes its own permissions to {string} with the admin password {string}"
)]
fn change_with_the_token(agent: &mut Agent, permissions: String, password: String) {
    change_request(agent, &permissions, &password);
    let text = agent.api_tokens.created().text.clone();
    agent
        .web
        .request()
        .headers
        .push(format!("Authorization: Bearer {text}"));
}

#[then(expr = "the refusal names the permission {string}")]
fn names_the_permission(agent: &mut Agent, permission: String) {
    let response = agent.web.response();
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("JSON");
    assert_eq!(
        body["args"]["permission"],
        serde_json::json!({ "type": "text", "value": permission }),
        "body {:?}",
        response.body
    );
}

/// The answer is permissions on the wire: every one, those named at their
/// level and the others `none`.
#[then(expr = "the answer lists the permissions {string}")]
fn lists_the_permissions(agent: &mut Agent, permissions: String) {
    let response = agent.web.response();
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("JSON");
    assert_eq!(
        body,
        permissions_body(&permissions),
        "body {:?}",
        response.body
    );
}

/// The session's list of tokens, which a wrong password's wait doesn't hold
/// up as it holds up the token itself.
#[then(expr = "the admin's session lists the API token with the permissions {string}")]
async fn session_lists_the_permissions(agent: &mut Agent, permissions: String) {
    let request = in_session(agent, "GET", "/api/v1/tokens", String::new());
    let response = exchange(agent, request).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("JSON");
    let tokens = body.get("tokens").unwrap_or(&body);
    let id = &agent.api_tokens.created().id;
    let token = tokens
        .as_array()
        .and_then(|tokens| tokens.iter().find(|token| token["id"] == id.as_str()))
        .unwrap_or_else(|| panic!("the list holds the token {id}: {body}"));
    assert_eq!(
        token["permissions"],
        permissions_body(&permissions),
        "{token}"
    );
}

#[then("the answer doesn't hold the API token's secret")]
fn answer_doesnt_hold(agent: &mut Agent) {
    let secret = agent.api_tokens.secret();
    let response = agent.web.response();
    assert!(
        !response.body.contains(&secret),
        "the answer holds the secret: {:?}",
        response.body
    );
}

#[then("no file Ladderback keeps holds the API token's secret")]
async fn no_file_holds(agent: &mut Agent) {
    let secret = agent.api_tokens.secret();
    let home = sign_in_steps::home(agent);
    blocking(move || sign_in_steps::no_file_under_holds(&home, &secret)).await;
}

#[then("the daemon's log doesn't hold the API token's secret")]
fn log_doesnt_hold(agent: &mut Agent) {
    let secret = agent.api_tokens.secret();
    let log = std::fs::read_to_string(agent.web.log()).expect("the daemon's log");
    assert!(!log.is_empty(), "the daemon logged nothing");
    assert!(!log.contains(&secret), "the log holds the secret: {log}");
}
