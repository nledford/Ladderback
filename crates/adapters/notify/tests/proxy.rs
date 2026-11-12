//! The notifier honours the standard proxy variables, as the workers do
//! (technical design §11, §10.2): a channel is sent through the proxy
//! `HTTP_PROXY` names, with the proxy's own credentials, unless `NO_PROXY`
//! names the channel's host.
//!
//! The variables are read when the client is built, from the process's
//! environment, which a test can't change safely. So the test changes none
//! of its own: it runs itself again with them set, and that copy sends. A
//! stand-in proxy on loopback, in this process, records what reaches it.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use app::ports::{Destination, Message, Notification, Notifier, NotifyError};
use domain::notifications::{ChannelKind, Event, EventKind};
use domain::secrets::SecretValue;
use domain::{OperationId, Timestamp, Uuid};
use l10n::Localizer;
use notify_adapter::HttpNotifier;

const NAME: &str =
    "a_channel_is_sent_through_the_proxy_the_environment_names_unless_no_proxy_names_it";
/// Set in the copy: what it expects of its delivery.
const EXPECT: &str = "LADDERBACK_NOTIFY_PROXY_TEST_EXPECT";
/// Set in the copy: the channel's URL.
const CHANNEL: &str = "LADDERBACK_NOTIFY_PROXY_TEST_CHANNEL";
/// The stand-in proxy's credentials: canaries.
const PROXY_USER: &str = "demo-proxy-user";
const PROXY_PASSWORD: &str = "canary-proxy-password";

const PROXY_VARIABLES: [&str; 9] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "REQUEST_METHOD",
];

#[test]
fn a_channel_is_sent_through_the_proxy_the_environment_names_unless_no_proxy_names_it() {
    if let Some(expect) = std::env::var_os(EXPECT) {
        deliver_in_copy(expect.to_str().unwrap_or_default());
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").expect("a port for the proxy");
    let proxy_port = proxy.local_addr().expect("an address").port();
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let recording = seen.clone();
    std::thread::spawn(move || {
        for stream in proxy.incoming().flatten() {
            if let Some(head) = read_head(&stream) {
                recording
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(head);
            }
            let mut stream = stream;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    // A channel on a loopback port nothing listens at: sent there directly,
    // it is unreachable; sent through the proxy, the proxy takes it.
    let closed = TcpListener::bind("127.0.0.1:0").expect("a port");
    let channel_port = closed.local_addr().expect("an address").port();
    drop(closed);
    let channel = format!("http://127.0.0.1:{channel_port}/demo-hook");
    let proxy_url = format!("http://{PROXY_USER}:{PROXY_PASSWORD}@127.0.0.1:{proxy_port}");

    run_copy(&channel, &[("HTTP_PROXY", &proxy_url)], "through-proxy");
    let heads = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        heads.len(),
        1,
        "the proxy is sent the notification once: {heads:?}"
    );
    let head = heads.first().cloned().unwrap_or_default();
    assert_eq!(
        head.first().map(String::as_str),
        Some(format!("POST {channel} HTTP/1.1").as_str()),
        "the proxy is asked for the channel's URL"
    );
    let signed_in = head.iter().any(|line| {
        line.to_ascii_lowercase()
            .starts_with("proxy-authorization: basic ")
    });
    assert!(
        signed_in,
        "the proxy's credentials are sent to it: {head:?}"
    );

    run_copy(
        &channel,
        &[("http_proxy", &proxy_url), ("no_proxy", "127.0.0.1")],
        "direct",
    );
    let after = seen.lock().unwrap_or_else(PoisonError::into_inner).len();
    assert_eq!(
        after, 1,
        "a host NO_PROXY names is not sent through the proxy"
    );
}

/// Runs this test again with `variables` as the only proxy variables set,
/// and checks that the copy passed.
fn run_copy(channel: &str, variables: &[(&str, &str)], expect: &str) {
    let mut copy = Command::new(std::env::current_exe().expect("the test binary"));
    copy.args(["--exact", NAME, "--nocapture", "--test-threads=1"]);
    for variable in PROXY_VARIABLES {
        copy.env_remove(variable);
    }
    copy.envs(variables.iter().copied())
        .env(EXPECT, expect)
        .env(CHANNEL, channel)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = copy.output().expect("the copy runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // "1 passed" shows that the copy ran the test, and did not just match none.
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{expect}: {stdout}{stderr}"
    );
}

/// The copy's half: one delivery, which ends as `expect` says.
fn deliver_in_copy(expect: &str) {
    let channel = std::env::var(CHANNEL).expect("the channel's URL");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let sent = runtime.block_on(async {
        let localizer = Localizer::new(&["en-US".parse().expect("a locale")]);
        let notifier = HttpNotifier::new(localizer, Duration::from_secs(5)).expect("a client");
        notifier
            .deliver(&Notification {
                destination: Destination {
                    kind: ChannelKind::Webhook,
                    url: SecretValue::new(channel),
                    auth: None,
                },
                event: Event {
                    kind: EventKind::BackupFailed,
                    operation: OperationId::from_uuid(Uuid::from_u128(1)),
                    plan: None,
                    repository: None,
                    at: Timestamp::UNIX_EPOCH,
                    code: Some("operation-failed"),
                    attempt: None,
                },
                message: Message::Default,
                locale: None,
            })
            .await
    });
    match expect {
        "through-proxy" => assert_eq!(sent, Ok(())),
        "direct" => assert_eq!(sent, Err(NotifyError::Unreachable)),
        other => panic!("no such expectation: {other}"),
    }
}

/// The request line and headers of what reached the proxy, read with its
/// body, so the answer isn't lost to a reset.
fn read_head(stream: &TcpStream) -> Option<Vec<String>> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream);
    let mut head = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end().to_owned();
        if line.is_empty() {
            break;
        }
        head.push(line);
    }
    let length: usize = head
        .iter()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length.min(64 * 1024)];
    std::io::Read::read_exact(&mut reader, &mut body).ok()?;
    Some(head)
}
