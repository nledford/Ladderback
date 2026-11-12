//! Steps for the `@e2e` rules of `features/security/sign_in.feature`: the
//! real `ladderback set-password`, and the real daemon it turns sign-in on
//! for, spoken to in raw HTTP through the request-check steps' client.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cucumber::{given, then, when};

use super::request_checks_steps::{self as web, Request};
use super::{Agent, LADDERBACK, blocking};

/// What `set-password` came to, and the session the scenario signed in with.
#[derive(Debug, Default)]
pub struct SignInE2e {
    ran: Option<Ran>,
    /// The `Cookie` header's `name=value` of the session signed in last.
    cookie: Option<String>,
    /// That session's CSRF token.
    csrf: Option<String>,
}

#[derive(Debug)]
struct Ran {
    code: Option<i32>,
    stderr: String,
}

/// The folders the scenario's `set-password` and daemons share.
fn home(agent: &mut Agent) -> PathBuf {
    let scratch = agent.scratch().join("shared");
    agent.web.home.get_or_insert(scratch).clone()
}

fn hash_file(agent: &mut Agent) -> PathBuf {
    testkit::daemon::data_dir(&home(agent)).join("admin-password")
}

/// Runs `ladderback set-password` with `args`, `input` on its standard input
/// (a pipe, not a terminal), in English.
async fn set_password(agent: &mut Agent, args: &[&str], input: &str) {
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, &home(agent));
    command
        .arg("set-password")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE");
    let input = input.to_owned();
    let output = blocking(move || {
        let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback runs");
        if let Some(mut stdin) = child.stdin.take() {
            // A command that refused its arguments may have ended unread.
            let _ = stdin.write_all(input.as_bytes());
        }
        child.wait_with_output().expect("ladderback ran")
    })
    .await;
    agent.sign_in.ran = Some(Ran {
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
}

fn ran(agent: &Agent) -> &Ran {
    agent.sign_in.ran.as_ref().expect("set-password ran")
}

#[when(expr = "the admin password {string} is set from standard input")]
async fn set_from_stdin(agent: &mut Agent, password: String) {
    set_password(agent, &[], &format!("{password}\n")).await;
}

#[given(expr = "the admin password {string} is set")]
async fn is_set(agent: &mut Agent, password: String) {
    set_from_stdin(agent, password).await;
    let ran = ran(agent);
    assert_eq!(ran.code, Some(0), "set-password: {}", ran.stderr);
}

#[when(expr = "set-password is given the password {string} as an argument")]
async fn given_as_argument(agent: &mut Agent, password: String) {
    set_password(agent, &[&password], "").await;
}

#[then("the command succeeds")]
fn succeeds(agent: &mut Agent) {
    let ran = ran(agent);
    assert_eq!(ran.code, Some(0), "stderr: {}", ran.stderr);
    assert!(
        ran.stderr.contains("admin password set"),
        "stderr: {}",
        ran.stderr
    );
}

#[then("the command refuses the password")]
fn refuses_password(agent: &mut Agent) {
    let ran = ran(agent);
    // Exit code 1 is a refusal; a panic (101) or a signal isn't.
    assert_eq!(ran.code, Some(1), "stderr: {}", ran.stderr);
    assert!(
        ran.stderr
            .lines()
            .any(|line| line.starts_with("ladderback: ") && line.contains("at least 8 characters")),
        "stderr: {}",
        ran.stderr
    );
}

#[then(expr = "set-password rejects the argument {string}")]
fn rejects_argument(agent: &mut Agent, value: String) {
    let ran = ran(agent);
    // Exit code 2 is how the argument parser rejects an argument.
    assert_eq!(ran.code, Some(2), "stderr: {}", ran.stderr);
    assert!(ran.stderr.contains(&value), "stderr: {}", ran.stderr);
}

#[then("the data folder holds the admin password's hash, readable by its owner alone")]
fn holds_hash(agent: &mut Agent) {
    let path = hash_file(agent);
    let text = std::fs::read_to_string(&path).expect("the admin password file");
    assert!(
        text.starts_with("$argon2id$"),
        "not an argon2id hash: {text}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("its metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode {mode:o}");
    }
}

#[then("the data folder holds no admin password")]
fn holds_none(agent: &mut Agent) {
    let path = hash_file(agent);
    assert!(!path.exists(), "{} exists", path.display());
}

/// Every file under `folder`, at any depth.
fn files_under(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[then(expr = "no file Ladderback keeps contains {string}")]
fn no_file_contains(agent: &mut Agent, text: String) {
    let files = files_under(&home(agent));
    assert!(!files.is_empty(), "set-password kept no file");
    for path in files {
        let bytes = std::fs::read(&path).expect("a file Ladderback keeps");
        assert!(
            !bytes
                .windows(text.len())
                .any(|window| window == text.as_bytes()),
            "{} contains {text:?}",
            path.display()
        );
    }
}

#[given("the data folder holds an admin password file that is not a password hash")]
fn not_a_hash(agent: &mut Agent) {
    let path = hash_file(agent);
    std::fs::create_dir_all(path.parent().expect("the data folder")).expect("the data folder");
    std::fs::write(&path, "not a password hash\n").expect("the admin password file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("the file's mode");
    }
}

#[then("the refusal gives the platform's message")]
fn refusal_gives_platform_message(agent: &mut Agent) {
    let expected = format!("({} is not an argon2id hash)", hash_file(agent).display());
    let (_, log) = web::exited(agent);
    // Fluent wraps each placeable in Unicode isolation marks.
    let log: String = log
        .chars()
        .filter(|c| !matches!(c, '\u{2068}' | '\u{2069}'))
        .collect();
    assert!(log.contains(&expected), "no {expected:?} in: {log}");
}

#[given(expr = "Ladderback is serving on loopback with sign-in on and the admin password {string}")]
async fn serving_with_sign_in(agent: &mut Agent, password: String) {
    is_set(agent, password).await;
    web::serve_on_loopback(agent).await;
}

/// Signs in over HTTP with `password`, from the page `origin` if any, and
/// keeps the session the answer carries.
fn sign_in(agent: &mut Agent, password: &str, origin: Option<&str>) {
    let mut headers = vec!["Content-Type: application/json".to_owned()];
    if let Some(origin) = origin {
        headers.push(format!("Origin: {origin}"));
    }
    agent.web.request = Some(Request {
        method: "POST".to_owned(),
        path: "/api/v1/session".to_owned(),
        headers,
        default_host: true,
        body: serde_json::json!({ "password": password }).to_string(),
    });
    web::send(agent);
    let response = agent.web.response();
    if response.status != 200 {
        return;
    }
    let cookie = response
        .header("set-cookie")
        .and_then(|cookie| cookie.split(';').next())
        .map(str::to_owned);
    let body: serde_json::Value =
        serde_json::from_str(&response.body).expect("a sign-in's answer is JSON");
    agent.sign_in.cookie = cookie;
    agent.sign_in.csrf = body["csrf-token"].as_str().map(str::to_owned);
}

#[when(expr = "the admin signs in over HTTP with {string}")]
fn signs_in(agent: &mut Agent, password: String) {
    sign_in(agent, &password, None);
}

#[when(expr = "the admin signs in over HTTP with {string} from the page {string}")]
fn signs_in_from(agent: &mut Agent, password: String, origin: String) {
    let port = agent.web.server().addr.port().to_string();
    sign_in(agent, &password, Some(&origin.replace("{port}", &port)));
}

#[given(expr = "the admin has signed in over HTTP with {string}")]
fn has_signed_in(agent: &mut Agent, password: String) {
    sign_in(agent, &password, None);
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
    assert!(agent.sign_in.cookie.is_some(), "no session cookie");
}

fn session_cookie(agent: &Agent) -> String {
    let cookie = agent
        .sign_in
        .cookie
        .as_deref()
        .expect("the admin signed in");
    format!("Cookie: {cookie}")
}

#[given("the admin has signed out over HTTP")]
fn has_signed_out(agent: &mut Agent) {
    let cookie = session_cookie(agent);
    let csrf = agent
        .sign_in
        .csrf
        .clone()
        .expect("the session's CSRF token");
    agent.web.request = Some(Request {
        method: "DELETE".to_owned(),
        path: "/api/v1/session".to_owned(),
        headers: vec![
            cookie,
            "Content-Type: application/json".to_owned(),
            format!("X-CSRF-Token: {csrf}"),
        ],
        default_host: true,
        body: String::new(),
    });
    web::send(agent);
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
}

#[given(expr = "a {string} request for {string} with the session cookie")]
fn with_cookie(agent: &mut Agent, method: String, path: String) {
    web::request_for(agent, method, path);
    let cookie = session_cookie(agent);
    agent.web.request().headers.push(cookie);
}

#[given(expr = "a {string} request for {string} with the session cookie and its CSRF token")]
fn with_cookie_and_csrf(agent: &mut Agent, method: String, path: String) {
    with_cookie(agent, method, path);
    let csrf = agent
        .sign_in
        .csrf
        .clone()
        .expect("the session's CSRF token");
    agent
        .web
        .request()
        .headers
        .push(format!("X-CSRF-Token: {csrf}"));
}

#[then(expr = "the request is answered with status {int}")]
fn answered(agent: &mut Agent, status: u16) {
    let response = agent.web.response();
    assert_eq!(response.status, status, "body {:?}", response.body);
}

#[then(expr = "the sign-in sets a session cookie with {string}, {string} and {string}")]
fn cookie_with(agent: &mut Agent, first: String, second: String, third: String) {
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
    let cookie = response.header("set-cookie").expect("a Set-Cookie header");
    assert!(cookie.starts_with("ladderback-session="), "{cookie}");
    let attributes: Vec<&str> = cookie.split(';').map(str::trim).collect();
    for wanted in [first, second, third] {
        assert!(
            attributes.contains(&wanted.as_str()),
            "{wanted} missing: {cookie}"
        );
    }
}

#[then(expr = "the session cookie is not marked {string}")]
fn cookie_not_marked(agent: &mut Agent, attribute: String) {
    let cookie = agent
        .web
        .response()
        .header("set-cookie")
        .expect("a Set-Cookie header");
    assert!(
        !cookie.split(';').any(|part| part.trim() == attribute),
        "{cookie}"
    );
}

#[then(expr = "the sign-in is refused with status {int}, to be tried again after {int} second(s)")]
fn refused_with_wait(agent: &mut Agent, status: u16, seconds: u64) {
    let response = agent.web.response();
    assert_eq!(response.status, status, "body {:?}", response.body);
    assert_eq!(
        response.header("retry-after"),
        Some(seconds.to_string().as_str())
    );
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("JSON");
    assert_eq!(body["code"], "sign-in-wait");
    assert!(response.header("set-cookie").is_none());
}

#[then(expr = "the route answers with status {int} and the error code {string}")]
fn route_answers(agent: &mut Agent, status: u16, code: String) {
    let response = agent.web.response();
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("JSON");
    assert_eq!(
        (response.status, body["code"].as_str()),
        (status, Some(code.as_str())),
        "body {:?}",
        response.body
    );
}
