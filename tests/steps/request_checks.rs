//! Steps for `features/security/request_checks.feature`: the real `ladderback
//! serve` process, spoken to in raw HTTP so each scenario controls every
//! header byte, `Host` included, which an HTTP client library would set for
//! itself.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cucumber::{given, then, when};

use super::{Agent, LADDERBACK};

/// How long a freshly started daemon may take to listen or give up.
const START_TIMEOUT: Duration = Duration::from_secs(60);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const LISTENING: &str = "listening on http://";

/// What the request-check scenarios keep between steps.
#[derive(Debug, Default)]
pub struct Web {
    server: Option<Server>,
    request: Option<Request>,
    response: Option<Response>,
    start: Option<Outcome>,
    /// Whether requests come from the UI's page, which sends an `Origin`.
    page: bool,
}

/// A running daemon, stopped when the scenario ends.
#[derive(Debug)]
struct Server {
    child: Child,
    addr: SocketAddr,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
enum Outcome {
    Listening(SocketAddr),
    Exited(ExitStatus, String),
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: Vec<String>,
    /// Whether the request names the daemon's own address as its `Host`.
    default_host: bool,
}

#[derive(Debug)]
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

static LOGS: AtomicUsize = AtomicUsize::new(0);

/// Starts `ladderback serve` with `args`, and waits until it listens or exits.
fn start(scratch: &Path, args: &[&str]) -> (Child, Outcome) {
    let started = LOGS.fetch_add(1, Ordering::Relaxed);
    let log_path = scratch.join(format!("serve-{started}.log"));
    let log = std::fs::File::create(&log_path).expect("a log file for the daemon");
    let mut command = Command::new(LADDERBACK);
    // Folders of its own, not the platform's, and not those of another daemon
    // this scenario started.
    testkit::daemon::isolate(&mut command, &scratch.join(format!("daemon-{started}")));
    let mut child = command
        .arg("serve")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        // The banner is read in English, whatever language the machine or a
        // pseudolocale asks for: `C` makes the program ignore `LANGUAGE`,
        // which is removed too.
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE")
        .spawn()
        .expect("ladderback starts");
    let began = Instant::now();
    loop {
        let text = std::fs::read_to_string(&log_path).unwrap_or_default();
        // The log's last line may be half written: wait for its newline.
        if let Some(rest) = text.split(LISTENING).nth(1)
            && rest.contains('\n')
            && let Some(addr) = rest.lines().next().and_then(|l| l.trim().parse().ok())
        {
            return (child, Outcome::Listening(addr));
        }
        if let Some(status) = child.try_wait().expect("the daemon's status") {
            return (child, Outcome::Exited(status, text));
        }
        assert!(
            began.elapsed() < START_TIMEOUT,
            "ladderback neither listened nor exited within {START_TIMEOUT:?}: {text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Request {
    fn text(&self, port: u16) -> String {
        let mut text = format!("{} {} HTTP/1.1\r\n", self.method, self.path);
        if self.default_host {
            text.push_str(&format!("Host: localhost:{port}\r\n"));
        }
        for header in &self.headers {
            text.push_str(&header.replace("{port}", &port.to_string()));
            text.push_str("\r\n");
        }
        if !self.has_header("connection") {
            text.push_str("Connection: close\r\n");
        }
        text.push_str("Content-Length: 0\r\n\r\n");
        text
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers.iter().any(|h| header_is(h, name))
    }
}

fn header_is(line: &str, name: &str) -> bool {
    line.split_once(':')
        .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(name))
}

/// Sends `request` over a new connection and reads one response.
fn exchange(addr: SocketAddr, request: &Request) -> Response {
    let mut stream = TcpStream::connect(addr).expect("a connection to the daemon");
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .expect("a read timeout");
    stream
        .write_all(request.text(addr.port()).as_bytes())
        .expect("the request is written");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("a status line");
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("a status line, got {line:?}"));
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).expect("a header line");
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').expect("a header with a colon");
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok());
    assert!(
        !headers.iter().any(|(n, _)| n == "transfer-encoding"),
        "the steps read bodies with a Content-Length only: {headers:?}"
    );
    let mut body = Vec::new();
    // A 101 Switching Protocols response has no body: the connection is the
    // WebSocket now, so reading to its end would wait for the timeout.
    if request.method != "HEAD" && status != 101 {
        match length {
            Some(length) => {
                body.resize(length, 0);
                reader.read_exact(&mut body).expect("the response body");
            }
            None => {
                reader.read_to_end(&mut body).expect("the response body");
            }
        }
    }
    Response {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Web {
    fn server(&self) -> &Server {
        self.server.as_ref().expect("Ladderback is serving")
    }

    fn request(&mut self) -> &mut Request {
        self.request.as_mut().expect("a request is being built")
    }

    fn response(&self) -> &Response {
        self.response.as_ref().expect("the request was sent")
    }
}

#[given(expr = "Ladderback is serving on loopback, configured for the host name {string}")]
fn serving(agent: &mut Agent, name: String) {
    let (child, outcome) = start(
        agent.scratch(),
        &["--listen", "127.0.0.1:0", "--allowed-host", &name],
    );
    match outcome {
        Outcome::Listening(addr) => agent.web.server = Some(Server { child, addr }),
        Outcome::Exited(status, log) => panic!("ladderback exited with {status}: {log}"),
    }
}

#[given("the request comes from the UI's page")]
fn from_the_page(agent: &mut Agent) {
    agent.web.page = true;
}

#[given(expr = "a {string} request for {string}")]
fn request_for(agent: &mut Agent, method: String, path: String) {
    // A page's request carries its origin, which is the daemon's own address.
    let headers = if agent.web.page {
        vec!["Origin: http://localhost:{port}".to_owned()]
    } else {
        Vec::new()
    };
    agent.web.request = Some(Request {
        method,
        path,
        headers,
        default_host: true,
    });
}

#[given(expr = "the request has the header {string}")]
fn add_header(agent: &mut Agent, header: String) {
    let request = agent.web.request();
    if header_is(&header, "host") {
        request.default_host = false;
    }
    request.headers.push(header);
}

#[given("the request has no Host header")]
fn no_host(agent: &mut Agent) {
    agent.web.request().default_host = false;
}

#[given("the request asks to upgrade to a WebSocket")]
fn websocket(agent: &mut Agent) {
    agent.web.request().headers.extend(
        [
            "Connection: Upgrade",
            "Upgrade: websocket",
            "Sec-WebSocket-Version: 13",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
        ]
        .map(str::to_owned),
    );
}

#[when("the request is sent")]
fn send(agent: &mut Agent) {
    let addr = agent.web.server().addr;
    let request = agent.web.request.as_ref().expect("a request is built");
    agent.web.response = Some(exchange(addr, request));
}

/// A refusal is JSON with the error code and nothing else.
fn refusal_code(response: &Response) -> Option<&str> {
    response
        .body
        .strip_prefix(r#"{"code":""#)
        .and_then(|rest| rest.strip_suffix(r#""}"#))
}

#[then("the request isn't refused by a request check")]
fn not_refused(agent: &mut Agent) {
    let response = agent.web.response();
    assert_eq!(
        refusal_code(response),
        None,
        "refused with status {}",
        response.status
    );
    // What the UI's own routes answer: the page, no such file, no such method,
    // or (when this build has no UI) none. Anything else, a 400 from a request
    // the server couldn't read, say, is not a request that was served.
    assert!(
        matches!(response.status, 200 | 404 | 405 | 503),
        "status {} with body {:?}",
        response.status,
        response.body
    );
}

#[then(expr = "the request is refused with status {int} and the error code {string}")]
fn refused(agent: &mut Agent, status: u16, code: String) {
    let response = agent.web.response();
    assert_eq!(
        (response.status, refusal_code(response)),
        (status, Some(code.as_str())),
        "body {:?}",
        response.body
    );
    assert_eq!(response.header("content-type"), Some("application/json"));
}

#[then("the WebSocket handshake is accepted")]
fn handshake_accepted(agent: &mut Agent) {
    let response = agent.web.response();
    assert_eq!(response.status, 101, "body {:?}", response.body);
    assert_eq!(response.header("upgrade"), Some("websocket"));
    // The key the upgrade step sends is the RFC 6455 sample key, whose
    // accept is the sample accept.
    assert_eq!(
        response.header("sec-websocket-accept"),
        Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
    );
}

#[then(expr = "the response has no {string} header")]
fn no_header(agent: &mut Agent, name: String) {
    assert_eq!(agent.web.response().header(&name), None);
}

#[when(expr = "Ladderback is started with the arguments {string}")]
fn started_with(agent: &mut Agent, arguments: String) {
    let args: Vec<&str> = arguments.split_whitespace().collect();
    let (child, outcome) = start(agent.scratch(), &args);
    if let Outcome::Listening(addr) = &outcome {
        // Stopped when the scenario ends, as the daemon from the Background.
        agent.web.server = Some(Server { child, addr: *addr });
    }
    agent.web.start = Some(outcome);
}

/// The daemon's exit status and log, when it didn't start listening.
fn exited(agent: &Agent) -> (ExitStatus, &str) {
    match agent.web.start.as_ref().expect("Ladderback was started") {
        Outcome::Listening(addr) => panic!("Ladderback is listening on {addr}"),
        Outcome::Exited(status, log) => (*status, log),
    }
}

#[then("Ladderback refuses to start")]
fn refuses(agent: &mut Agent) {
    let (status, log) = exited(agent);
    // Exit code 1 is a refusal; a panic (101) or a signal isn't.
    assert_eq!(status.code(), Some(1), "log: {log}");
}

#[then(expr = "Ladderback rejects the argument {string}")]
fn rejects(agent: &mut Agent, value: String) {
    let (status, log) = exited(agent);
    // Exit code 2 is how the argument parser rejects a value.
    assert_eq!(status.code(), Some(2), "log: {log}");
    assert!(log.contains(&value), "log: {log}");
}
