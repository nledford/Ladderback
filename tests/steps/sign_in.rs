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
    /// The `Cookie` header's `name=value` of a session another browser
    /// signed in with.
    second_cookie: Option<String>,
    /// What came of stopping `set-password` while it asked for the password
    /// at a terminal.
    stopped_at_prompt: Option<StoppedAtPrompt>,
}

#[derive(Debug)]
struct StoppedAtPrompt {
    status: std::process::ExitStatus,
    /// Whether the terminal shows what is typed once `set-password` ended.
    echo: bool,
}

#[derive(Debug)]
struct Ran {
    code: Option<i32>,
    stderr: String,
}

/// The folders the scenario's `set-password` and daemons share.
pub(crate) fn home(agent: &mut Agent) -> PathBuf {
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
        stderr: String::from_utf8_lossy_owned(output.stderr),
    });
}

fn ran(agent: &Agent) -> &Ran {
    agent.sign_in.ran.as_ref().expect("set-password ran")
}

/// The signal `how` sends: Ctrl-C at a terminal sends `SIGINT`.
fn signal_of(how: &str) -> nix::sys::signal::Signal {
    match how {
        "Ctrl-C" => nix::sys::signal::Signal::SIGINT,
        "SIGTERM" => nix::sys::signal::Signal::SIGTERM,
        other => panic!("no signal for {other:?}"),
    }
}

/// Runs `ladderback set-password` with a terminal as its standard input,
/// waits until it has turned the terminal's echo off to ask for the
/// password, then sends it the signal `how` stands for. The signal is sent
/// to the process: the terminal is not its controlling one, so a typed
/// Ctrl-C would reach nobody.
#[when(expr = "set-password is stopped by {word} while it asks for the password at a terminal")]
async fn stopped_at_prompt(agent: &mut Agent, how: String) {
    let signal = signal_of(&how);
    let pty = match nix::pty::openpty(None, None) {
        Ok(pty) => pty,
        Err(error) => testkit::skip!("no pseudo-terminal can be opened here: {error}"),
    };
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, &home(agent));
    command
        .arg("set-password")
        .stdin(Stdio::from(
            pty.slave.try_clone().expect("the terminal's other end"),
        ))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE");
    let stopped = blocking(move || {
        use std::time::{Duration, Instant};

        use nix::sys::termios::{LocalFlags, tcgetattr};
        let terminal = pty.slave;
        // The terminal stays open while the program runs.
        let _screen = pty.master;
        let echo = || {
            tcgetattr(&terminal)
                .expect("the terminal's settings")
                .local_flags
                .contains(LocalFlags::ECHO)
        };
        let bound = Duration::from_secs(5);
        let child = testkit::children::Owned::spawn(&mut command).expect("ladderback runs");
        let began = Instant::now();
        while echo() {
            assert_eq!(
                child.try_wait().expect("its status"),
                None,
                "set-password ended before it asked for the password"
            );
            assert!(
                began.elapsed() < bound,
                "set-password didn't turn echo off within {bound:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid = i32::try_from(child.id()).expect("a process ID");
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), signal)
            .expect("the signal is sent");
        let sent = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("its status") {
                break status;
            }
            assert!(
                sent.elapsed() < bound,
                "set-password didn't end within {bound:?} of {signal}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        StoppedAtPrompt {
            status,
            echo: echo(),
        }
    })
    .await;
    agent.sign_in.stopped_at_prompt = Some(stopped);
}

fn stopped_at(agent: &Agent) -> Option<&StoppedAtPrompt> {
    agent.sign_in.stopped_at_prompt.as_ref()
}

#[then(expr = "set-password ends as {word} ends any program")]
fn ends_as_the_signal_ends_any_program(agent: &mut Agent, how: String) {
    use std::os::unix::process::ExitStatusExt;
    let Some(stopped) = stopped_at(agent) else {
        testkit::skip!("no pseudo-terminal could be opened");
    };
    assert_eq!(
        stopped
            .status
            .signal()
            .map(nix::sys::signal::Signal::try_from),
        Some(Ok(signal_of(&how))),
        "{:?}",
        stopped.status
    );
}

#[then("the terminal shows what is typed again")]
fn terminal_shows_typing(agent: &mut Agent) {
    let Some(stopped) = stopped_at(agent) else {
        testkit::skip!("no pseudo-terminal could be opened");
    };
    assert!(stopped.echo, "the terminal's echo is still off");
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
    no_file_under_holds(&home(agent), &text);
}

/// Fails unless `folder` holds a file and none of its files holds `text`.
pub(crate) fn no_file_under_holds(folder: &Path, text: &str) {
    let files = files_under(folder);
    assert!(!files.is_empty(), "Ladderback kept no file");
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

/// The session cookie's token is kept neither as its text nor as its bytes:
/// the store holds its digest only (I4).
#[then("no file Ladderback keeps holds the session cookie's token")]
fn no_file_holds_the_session_token(agent: &mut Agent) {
    let cookie = agent.sign_in.cookie.clone().expect("the admin signed in");
    let (_, token) = cookie.split_once('=').expect("a cookie's name and value");
    assert_eq!(token.len(), 64, "a session token is 64 hex digits");
    let home = home(agent);
    no_file_under_holds(&home, token);
    let bytes: Vec<u8> = (0..token.len())
        .step_by(2)
        .map(|at| {
            token
                .get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .expect("a hex digit pair")
        })
        .collect();
    for path in files_under(&home) {
        let kept = std::fs::read(&path).expect("a file Ladderback keeps");
        assert!(
            !kept.windows(bytes.len()).any(|window| window == bytes),
            "{} holds the session token's bytes",
            path.display()
        );
    }
}

#[given("the data folder holds an admin password file that is not a password hash")]
#[when("the data folder holds an admin password file that is not a password hash")]
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

#[given("the data folder holds an admin password file that is a named pipe")]
#[when("the data folder holds an admin password file that is a named pipe")]
fn named_pipe(agent: &mut Agent) {
    use nix::sys::stat::Mode;
    let path = hash_file(agent);
    std::fs::create_dir_all(path.parent().expect("the data folder")).expect("the data folder");
    nix::unistd::mkfifo(&path, Mode::S_IRUSR | Mode::S_IWUSR).expect("the named pipe");
}

#[when("the admin password file is made readable by other users")]
fn made_readable_by_others(agent: &mut Agent) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(hash_file(agent), std::fs::Permissions::from_mode(0o644))
        .expect("the file's mode");
}

#[then("the refusal gives the platform's message")]
fn refusal_gives_platform_message(agent: &mut Agent) {
    refusal_says(agent, "is not an argon2id hash");
}

#[then("the refusal says the admin password file is not a regular file")]
fn refusal_says_not_a_regular_file(agent: &mut Agent) {
    refusal_says(agent, "is not a regular file");
}

/// The refusal to start gives the platform's message: the admin password
/// file's path, then `said`.
fn refusal_says(agent: &mut Agent, said: &str) {
    let expected = format!("({} {said})", hash_file(agent).display());
    let (_, log) = web::exited(agent);
    // Fluent wraps each placeable in Unicode isolation marks.
    let log: String = log
        .chars()
        .filter(|c| !matches!(c, '\u{2068}' | '\u{2069}'))
        .collect();
    assert!(log.contains(&expected), "no {expected:?} in: {log}");
}

#[given(expr = "Ladderback is serving on loopback with sign-in on and the admin password {string}")]
pub(crate) async fn serving_with_sign_in(agent: &mut Agent, password: String) {
    is_set(agent, password).await;
    web::serve_on_loopback(agent).await;
}

/// As above, the daemon logging at `debug`, so a scenario can check that
/// even its most talkative log names nothing it mustn't.
#[given(
    expr = "Ladderback is serving on loopback with sign-in on, the admin password {string} and its log at debug"
)]
async fn serving_with_sign_in_logging_debug(agent: &mut Agent, password: String) {
    is_set(agent, password).await;
    web::serve_on_loopback_logging(agent, Some("debug")).await;
}

#[given("Ladderback is serving on loopback with sign-in off")]
async fn serving_with_sign_in_off(agent: &mut Agent) {
    // The shared folders, so a later set-password writes where it serves.
    home(agent);
    web::serve_on_loopback(agent).await;
}

/// Stops the daemon and starts it again on the same folders.
#[given("Ladderback has been restarted")]
#[when("Ladderback is restarted")]
async fn restarted(agent: &mut Agent) {
    stopped(agent).await;
    web::serve_on_loopback(agent).await;
}

/// Stops the daemon, leaving its folders as they are.
#[when("Ladderback is stopped")]
async fn stopped(agent: &mut Agent) {
    let server = agent.web.server.take();
    blocking(move || drop(server)).await;
}

/// The daemon's store, opened while it is stopped, keeps no session.
#[then("Ladderback keeps no session for after a restart")]
async fn keeps_no_session(agent: &mut Agent) {
    use app::ports::SessionStore;
    let path = testkit::daemon::data_dir(&home(agent)).join("ladderback.db");
    let store = store::SqliteStore::open(&path)
        .await
        .expect("the store opens while the daemon is stopped");
    let kept = store.all().await;
    store.close().await;
    assert_eq!(kept.expect("the store reads its sessions"), Vec::new());
}

/// The session route's answer, read without a session.
fn session_answer(agent: &mut Agent) -> serde_json::Value {
    web::request_for(agent, "GET".to_owned(), "/api/v1/session".to_owned());
    web::send(agent);
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
    serde_json::from_str(&response.body).expect("the session route answers JSON")
}

#[then("Ladderback says sign-in is still off and a restart turns it on")]
fn says_restart_turns_it_on(agent: &mut Agent) {
    let read = session_answer(agent);
    assert_eq!(read["sign-in"], "off", "{read}");
    assert_eq!(read["admin-password"], "set", "{read}");
}

#[then("Ladderback says sign-in is still off and the admin password file is damaged")]
fn says_file_is_damaged(agent: &mut Agent) {
    let read = session_answer(agent);
    assert_eq!(read["sign-in"], "off", "{read}");
    assert_eq!(read["admin-password"], "unreadable", "{read}");
}

#[then("sign-in is on")]
fn sign_in_is_on(agent: &mut Agent) {
    let read = session_answer(agent);
    assert_eq!(read["sign-in"], "signed-out", "{read}");
    assert!(read.get("admin-password").is_none(), "{read}");
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

/// The route Settings sets and changes the admin password through.
pub(crate) const ADMIN_PASSWORD_PATH: &str = "/api/v1/admin-password";

/// The body Settings sends to set the admin password `new`, with the
/// `current` one when sign-in is on.
pub(crate) fn admin_password_body(current: Option<&str>, new: &str) -> String {
    match current {
        Some(current) => {
            serde_json::json!({ "current-password": current, "new-password": new }).to_string()
        }
        None => serde_json::json!({ "new-password": new }).to_string(),
    }
}

#[when(expr = "the admin password {string} is set over HTTP")]
fn set_over_http(agent: &mut Agent, password: String) {
    agent.web.request = Some(Request {
        method: "POST".to_owned(),
        path: ADMIN_PASSWORD_PATH.to_owned(),
        headers: vec!["Content-Type: application/json".to_owned()],
        default_host: true,
        body: admin_password_body(None, &password),
    });
    web::send(agent);
}

#[when(expr = "the admin changes the admin password over HTTP from {string} to {string}")]
fn changes_over_http(agent: &mut Agent, current: String, new: String) {
    agent.web.request = Some(Request {
        method: "POST".to_owned(),
        path: ADMIN_PASSWORD_PATH.to_owned(),
        headers: vec![
            session_cookie(agent),
            csrf_header(agent),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body: admin_password_body(Some(&current), &new),
    });
    web::send(agent);
}

#[then(expr = "the answer holds {string}")]
fn answer_holds(agent: &mut Agent, text: String) {
    let response = agent.web.response();
    assert!(
        response.body.contains(&text),
        "the answer doesn't hold {text:?}: {:?}",
        response.body
    );
}

#[then(expr = "the answer doesn't hold {string}")]
fn answer_doesnt_hold(agent: &mut Agent, text: String) {
    let response = agent.web.response();
    assert!(
        !response.body.contains(&text),
        "the answer holds {text:?}: {:?}",
        response.body
    );
}

#[then(expr = "the daemon's log doesn't hold {string}")]
fn log_doesnt_hold(agent: &mut Agent, text: String) {
    let log = std::fs::read_to_string(agent.web.log()).expect("the daemon's log");
    assert!(!log.is_empty(), "the daemon logged nothing");
    assert!(!log.contains(&text), "the log holds {text:?}: {log}");
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

/// Another browser signs in: its cookie is kept apart from the one the
/// scenario's later steps send.
#[given(expr = "the admin has signed in over HTTP with {string} in a second browser")]
fn has_signed_in_elsewhere(agent: &mut Agent, password: String) {
    has_signed_in(agent, password);
    agent.sign_in.second_cookie = agent.sign_in.cookie.take();
    agent.sign_in.csrf = None;
}

#[when(expr = "a {string} request for {string} is sent with the second browser's session cookie")]
fn sent_from_the_second_browser(agent: &mut Agent, method: String, path: String) {
    web::request_for(agent, method, path);
    let cookie = agent
        .sign_in
        .second_cookie
        .clone()
        .expect("a second browser signed in");
    agent
        .web
        .request()
        .headers
        .push(format!("Cookie: {cookie}"));
    web::send(agent);
}

pub(crate) fn session_cookie(agent: &Agent) -> String {
    let cookie = agent
        .sign_in
        .cookie
        .as_deref()
        .expect("the admin signed in");
    format!("Cookie: {cookie}")
}

/// The `X-CSRF-Token` header of the session signed in last.
pub(crate) fn csrf_header(agent: &Agent) -> String {
    let csrf = agent
        .sign_in
        .csrf
        .as_deref()
        .expect("the session's CSRF token");
    format!("X-CSRF-Token: {csrf}")
}

#[given("the request has the session cookie")]
fn has_the_cookie(agent: &mut Agent) {
    let cookie = session_cookie(agent);
    agent.web.request().headers.push(cookie);
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

/// The name the wire gives a choice of **Stay signed in for**, as Settings
/// shows it.
fn session_length_name(choice: &str) -> &'static str {
    match choice {
        "Until I close the browser" => "until-browser-closes",
        "7 days" => "7-days",
        "30 days" => "30-days",
        "90 days" => "90-days",
        "1 year" => "1-year",
        "Until I sign out" => "until-sign-out",
        other => panic!("no choice of Stay signed in for is {other:?}"),
    }
}

#[given(expr = "the admin has set Stay signed in for to {string} over HTTP")]
async fn has_set_session_length(agent: &mut Agent, choice: String) {
    let request = Request {
        method: "PUT".to_owned(),
        path: "/api/v1/settings/sign-in".to_owned(),
        headers: vec![
            session_cookie(agent),
            csrf_header(agent),
            "Content-Type: application/json".to_owned(),
        ],
        default_host: true,
        body: serde_json::json!({ "session-length": session_length_name(&choice) }).to_string(),
    };
    let addr = agent.web.server().addr;
    let response = blocking(move || web::exchange(addr, &request)).await;
    assert_eq!(response.status, 200, "body {:?}", response.body);
}

/// The attributes of the session cookie the last answer set, trimmed.
fn set_cookie_attributes(agent: &Agent) -> Vec<String> {
    let response = agent.web.response();
    assert_eq!(response.status, 200, "body {:?}", response.body);
    let cookie = response.header("set-cookie").expect("a Set-Cookie header");
    assert!(cookie.starts_with("ladderback-session="), "{cookie}");
    cookie
        .split(';')
        .map(|part| part.trim().to_owned())
        .collect()
}

#[then(expr = "the session cookie lasts {int} days")]
fn cookie_lasts(agent: &mut Agent, days: u64) {
    let attributes = set_cookie_attributes(agent);
    let max_age = format!("Max-Age={}", days * 24 * 3600);
    assert!(attributes.contains(&max_age), "{attributes:?}");
}

#[then(expr = "the session cookie has neither {string} nor {string}")]
fn cookie_has_neither(agent: &mut Agent, first: String, second: String) {
    let attributes = set_cookie_attributes(agent);
    for unwanted in [first, second] {
        let named = format!("{}=", unwanted.to_ascii_lowercase());
        assert!(
            !attributes
                .iter()
                .any(|part| part.to_ascii_lowercase().starts_with(&named)),
            "{unwanted}: {attributes:?}"
        );
    }
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
