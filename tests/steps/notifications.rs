//! Steps for the `@e2e` scenarios of `features/notifications/rules.feature`
//! and `features/execution/restart.feature` that a channel is told of: a
//! webhook or an ntfy topic listens on loopback, its URL is in a file in the
//! scenario's scratch folder, and the daemon's config.toml has a rule that
//! sends to it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cucumber::{given, then};
use domain::notifications::{
    Channel, ChannelKind, ChannelName, EventKind, Notifications, Rule, RuleName, Template,
};
use domain::planning::{HookEntry, Locale, OnFailure};
use domain::secrets::{FileRead, SecretRef};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::Agent;

/// The longest the daemon may take to tell the channel, after the run was
/// accepted or the daemon started again: the backup of three small files and
/// one delivery on loopback.
const TOLD_WITHIN: Duration = Duration::from_secs(30);

/// The most of one request the channel reads.
const MAX_REQUEST: usize = 64 * 1024;

/// The path of the ntfy topic's URL: a canary, since the URL is a secret.
const TOPIC: &str = "canary-topic-url";

/// One request a channel was sent: its headers, by lower-case name, and its
/// body.
#[derive(Clone, Debug)]
struct Received {
    headers: BTreeMap<String, String>,
    body: String,
}

impl Received {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map_or("", String::as_str)
    }
}

/// The notifications the daemon's config.toml holds, the agent's locale it
/// names, and the requests the scenario's channel was sent.
#[derive(Default)]
pub struct NotificationsE2e {
    pub notifications: Option<Notifications>,
    pub locale: Option<Locale>,
    received: Arc<Mutex<Vec<Received>>>,
}

/// One POST, once its headers and as much of its body as its
/// `Content-Length` says have arrived.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Received> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 || bytes.len() > MAX_REQUEST {
            return None;
        }
        bytes.extend_from_slice(chunk.get(..read)?);
        let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(bytes.get(..end)?).into_owned();
        let headers: BTreeMap<String, String> = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        let length: usize = headers
            .get("content-length")
            .and_then(|length| length.parse().ok())
            .unwrap_or(0);
        if bytes.len() >= end + 4 + length {
            let body = String::from_utf8(bytes.get(end + 4..end + 4 + length)?.to_vec()).ok()?;
            return Some(Received { headers, body });
        }
    }
}

/// Serves a channel on loopback that takes every request, keeps it and
/// answers 204, at the URL the returned `SecretRef` reads from a file: the
/// config holds no URL, only where it is (I4).
async fn listen(agent: &mut Agent, path: &str) -> SecretRef {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let port = listener.local_addr().expect("an address").port();
    let received = agent.notifications.received.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let received = received.clone();
            tokio::spawn(async move {
                if let Some(request) = read_request(&mut stream).await {
                    received
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(request);
                }
                let _ = stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    let file = agent.scratch().join("channel-url");
    std::fs::write(&file, format!("http://127.0.0.1:{port}/{path}\n")).expect("the URL file");
    SecretRef::File {
        path: file.to_str().expect("a UTF-8 path").to_owned(),
        read: FileRead::FirstLine,
    }
}

/// Has the rule `name` send `event` to one channel of `kind` at `url`, in
/// the words of `template` when there is one.
fn send_to(
    agent: &mut Agent,
    name: &str,
    event: &str,
    kind: ChannelKind,
    url: SecretRef,
    template: Option<&str>,
) {
    let channel = ChannelName::parse("hook").expect("a channel name");
    let rule = Rule::new(
        BTreeSet::from([EventKind::parse(event).expect("an event")]),
        channel.clone(),
        BTreeSet::new(),
        template.map(|text| Template::parse(text).expect("the scenario's template reads")),
    )
    .expect("a rule");
    agent.notifications.notifications = Some(
        Notifications::new(
            BTreeMap::from([(channel, Channel::new(kind, url, None).expect("a channel"))]),
            BTreeMap::from([(RuleName::parse(name).expect("a rule name"), rule)]),
        )
        .expect("the rule sends to the channel"),
    );
}

#[given(expr = "a webhook listening on loopback that the rule {string} sends {string} to")]
async fn a_webhook_on_loopback(agent: &mut Agent, name: String, event: String) {
    let url = listen(agent, "demo-hook").await;
    send_to(agent, &name, &event, ChannelKind::Webhook, url, None);
}

#[given(expr = "an ntfy topic listening on loopback that the rule {string} sends {string} to")]
async fn an_ntfy_topic_on_loopback(agent: &mut Agent, name: String, event: String) {
    let url = listen(agent, TOPIC).await;
    send_to(agent, &name, &event, ChannelKind::Ntfy, url, None);
}

#[given(
    expr = "an ntfy topic listening on loopback that the rule {string} sends {string} to with the template {string}"
)]
async fn an_ntfy_topic_with_a_template(
    agent: &mut Agent,
    name: String,
    event: String,
    template: String,
) {
    let url = listen(agent, TOPIC).await;
    send_to(
        agent,
        &name,
        &event,
        ChannelKind::Ntfy,
        url,
        Some(&template),
    );
}

#[given(expr = "the agent's locale is {string}")]
fn the_agents_locale(agent: &mut Agent, locale: String) {
    agent.notifications.locale = Some(Locale::parse(&locale).expect("the scenario's locale"));
}

#[given(expr = "the plan's before hook says {string} and exits 3, with on-failure {word}")]
async fn a_before_hook_that_fails(agent: &mut Agent, said: String, on_failure: String) {
    // What it says goes to both of its outputs, so that a notification
    // that showed either would show the canary.
    let script = format!("echo '{said}'; echo '{said}' >&2; exit 3");
    let hook = HookEntry::new(
        "/bin/sh",
        ["-c", script.as_str()],
        OnFailure::parse(&on_failure).expect("the scenario's on-failure"),
    )
    .expect("a hook that runs /bin/sh");
    agent.backup.plan_hooks.run_before = vec![hook];
    super::events_steps::save_daemon_config(agent, None).await;
}

/// The one request the channel was sent, once it has come.
async fn the_one_request(agent: &Agent) -> Received {
    let deadline = tokio::time::Instant::now() + TOLD_WITHIN;
    loop {
        let received = agent
            .notifications
            .received
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(first) = received.first() {
            assert_eq!(received.len(), 1, "one notification: {received:?}");
            return first.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the channel was told nothing within {TOLD_WITHIN:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[then(expr = "the webhook is told {string} for the plan {string}")]
async fn the_webhook_is_told(agent: &mut Agent, event: String, plan: String) {
    told_once(agent, &event, &plan).await;
}

#[then(expr = "the webhook is told {string} for the plan {string} with the code {string}")]
async fn the_webhook_is_told_the_code(
    agent: &mut Agent,
    event: String,
    plan: String,
    code: String,
) {
    let body = told_once(agent, &event, &plan).await;
    assert_eq!(body["code"], code.as_str(), "{body}");
}

/// The one notification the webhook is told, once it has come, checked for
/// its `event`, its `plan` and a message.
async fn told_once(agent: &Agent, event: &str, plan: &str) -> serde_json::Value {
    let received = the_one_request(agent).await;
    let body: serde_json::Value =
        serde_json::from_str(&received.body).expect("the webhook is posted JSON");
    assert_eq!(body["event"], event, "{body}");
    assert_eq!(body["plan"], plan, "{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "a message: {body}"
    );
    body
}

/// The text of a header's `value`: an RFC 2047 encoded word in UTF-8 read as
/// ntfy reads it, any other value as it is.
fn decoded(value: &str) -> String {
    let Some(word) = value
        .strip_prefix("=?UTF-8?Q?")
        .and_then(|word| word.strip_suffix("?="))
    else {
        return value.to_owned();
    };
    let mut bytes = Vec::new();
    let mut rest = word.bytes();
    while let Some(byte) = rest.next() {
        match byte {
            b'_' => bytes.push(b' '),
            b'=' => {
                let hex: Vec<u8> = rest.by_ref().take(2).collect();
                let hex = std::str::from_utf8(&hex).expect("two hex digits");
                bytes.push(u8::from_str_radix(hex, 16).expect("a byte in hex"));
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).expect("the word is UTF-8")
}

/// That the ntfy topic was told `message` (any, when `None`) under `title`,
/// tagged `tag`.
async fn the_topic_is_told(agent: &Agent, message: Option<&str>, title: &str, tag: &str) {
    let received = the_one_request(agent).await;
    assert_eq!(decoded(received.header("title")), title, "{received:?}");
    assert_eq!(received.header("tags"), tag, "{received:?}");
    match message {
        Some(message) => assert_eq!(received.body, message),
        None => assert!(!received.body.trim().is_empty(), "a message: {received:?}"),
    }
}

#[then(expr = "the ntfy topic is told {string} under the title {string}, tagged {string}")]
async fn the_topic_is_told_a_message(
    agent: &mut Agent,
    message: String,
    title: String,
    tag: String,
) {
    the_topic_is_told(agent, Some(&message), &title, &tag).await;
}

#[then(
    expr = "the ntfy topic is told the default message under the title {string}, tagged {string}"
)]
async fn the_topic_is_told_the_default(agent: &mut Agent, title: String, tag: String) {
    the_topic_is_told(agent, None, &title, &tag).await;
}

#[then(expr = "nothing the ntfy topic is told shows its URL or {string}")]
async fn the_topic_is_told_no_secret(agent: &mut Agent, canary: String) {
    let received = the_one_request(agent).await;
    // What a person reads: the title, the tags and the message.
    let told = [
        decoded(received.header("title")),
        received.header("tags").to_owned(),
        received.body.clone(),
    ];
    for text in told {
        assert!(!text.contains(TOPIC), "{text:?}");
        assert!(!text.contains(&canary), "{text:?}");
    }
}
