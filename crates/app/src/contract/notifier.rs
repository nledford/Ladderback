//! The [`Notifier`](crate::ports::Notifier) contract: the same checks run
//! against the recording fake and the real HTTP adapter, whose harness
//! serves the channels on loopback. A check fails on the first violation,
//! as the other suites do.

use domain::notifications::{Attempt, ChannelKind, Event, EventKind, Ping, Username};
use domain::secrets::SecretValue;
use domain::{OperationId, PlanId, RepositoryAlias, Timestamp, Uuid};

use crate::ports::{Credentials, Destination, Message, Notification, Notifier, NotifyError};

/// How a channel the harness serves answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    /// It takes the notification: a success status.
    Accepts,
    /// Nothing listens at its address.
    Refuses,
    /// It answers with `status`.
    Answers(u16),
    /// It takes the connection and never answers.
    Hangs,
}

/// How a request signed in, as its receiver read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceivedAuth {
    Bearer(String),
    Basic { username: String, password: String },
}

/// What a channel that accepts was sent, as its owner reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub kind: ChannelKind,
    /// The ping, for a healthchecks channel.
    pub ping: Option<Ping>,
    /// The event's name, as the channel was told it: empty for a ping,
    /// which tells it only by the ping it is.
    pub event: String,
    /// The message's text.
    pub message: String,
    /// How it signed in.
    pub auth: Option<ReceivedAuth>,
}

/// Serves channels for the notifier under test.
#[async_trait::async_trait]
pub trait NotifierHarness: Send + Sync {
    /// The notifier under test, with a timeout short enough for the hang
    /// check; the adapter's own tests check that it keeps to it.
    fn notifier(&self) -> &dyn Notifier;
    /// The URL of a fresh channel of `kind` that answers as `behaviour`.
    fn url(&self, kind: ChannelKind, behaviour: Behaviour) -> String;
    /// What the channels that accept have been sent, in the order they took
    /// it.
    async fn received(&self) -> Vec<Received>;
}

/// The token and the password the suite's channels are sent with: canaries,
/// never real ones.
const TOKEN: &str = "canary-notify-token";
const PASSWORD: &str = "canary-notify-password";
const USERNAME: &str = "demo-user";

fn bearer() -> Option<Credentials> {
    Some(Credentials::Bearer(SecretValue::new(TOKEN)))
}

/// Basic credentials, or none if the name were refused, which the check
/// that sends them then fails on.
fn basic() -> Option<Credentials> {
    Username::parse(USERNAME)
        .ok()
        .map(|username| Credentials::Basic {
            username,
            password: SecretValue::new(PASSWORD),
        })
}

/// Runs the whole suite against `harness`.
pub async fn run(harness: &dyn NotifierHarness) {
    a_webhook_is_sent_the_event_and_the_message(harness).await;
    ntfy_is_sent_the_message_with_its_token(harness).await;
    ntfy_is_sent_the_message_with_a_name_and_password(harness).await;
    a_healthchecks_channel_is_pinged_for_each_event(harness).await;
    a_default_message_says_something_for_every_event(harness).await;
    a_channel_nobody_listens_at_is_unreachable(harness).await;
    a_status_other_than_success_is_a_rejection(harness).await;
    a_redirect_is_not_followed(harness).await;
    a_channel_that_never_answers_times_out(harness).await;
    an_address_that_is_not_http_is_never_sent_to(harness).await;
}

fn event(kind: EventKind) -> Event {
    Event {
        kind,
        operation: OperationId::from_uuid(Uuid::from_u128(
            0x0193_0000_0000_7000_8000_0000_0000_0001,
        )),
        plan: PlanId::parse("demo-plan").ok(),
        repository: RepositoryAlias::parse("demo-repo").ok(),
        at: Timestamp::from_second(1_800_000_000).unwrap_or(Timestamp::UNIX_EPOCH),
        code: (kind == EventKind::BackupFailed).then_some("operation-failed"),
        attempt: (kind == EventKind::BackupRetrying).then_some(Attempt { number: 1, of: 3 }),
    }
}

fn notification(
    kind: ChannelKind,
    url: String,
    auth: Option<Credentials>,
    event: Event,
    message: Message,
) -> Notification {
    Notification {
        destination: Destination {
            kind,
            url: SecretValue::new(url),
            auth,
        },
        event,
        message,
        locale: None,
    }
}

async fn received_since(harness: &dyn NotifierHarness, before: usize) -> Vec<Received> {
    harness.received().await.into_iter().skip(before).collect()
}

async fn a_webhook_is_sent_the_event_and_the_message(harness: &dyn NotifierHarness) {
    let before = harness.received().await.len();
    let url = harness.url(ChannelKind::Webhook, Behaviour::Accepts);
    let sent = harness
        .notifier()
        .deliver(&notification(
            ChannelKind::Webhook,
            url,
            bearer(),
            event(EventKind::BackupFailed),
            Message::Custom("The backup of demo-plan failed.".to_owned()),
        ))
        .await;
    assert_eq!(sent, Ok(()), "a webhook that accepts is delivered to");
    assert_eq!(
        received_since(harness, before).await,
        [Received {
            kind: ChannelKind::Webhook,
            ping: None,
            event: "backup.failed".to_owned(),
            message: "The backup of demo-plan failed.".to_owned(),
            auth: Some(ReceivedAuth::Bearer(TOKEN.to_owned())),
        }]
    );
}

async fn ntfy_is_sent_the_message_with_its_token(harness: &dyn NotifierHarness) {
    let before = harness.received().await.len();
    let url = harness.url(ChannelKind::Ntfy, Behaviour::Accepts);
    let sent = harness
        .notifier()
        .deliver(&notification(
            ChannelKind::Ntfy,
            url,
            bearer(),
            event(EventKind::BackupStarted),
            Message::Custom("demo-plan started".to_owned()),
        ))
        .await;
    assert_eq!(sent, Ok(()));
    assert_eq!(
        received_since(harness, before).await,
        [Received {
            kind: ChannelKind::Ntfy,
            ping: None,
            event: "backup.started".to_owned(),
            message: "demo-plan started".to_owned(),
            auth: Some(ReceivedAuth::Bearer(TOKEN.to_owned())),
        }]
    );
}

async fn ntfy_is_sent_the_message_with_a_name_and_password(harness: &dyn NotifierHarness) {
    let before = harness.received().await.len();
    let url = harness.url(ChannelKind::Ntfy, Behaviour::Accepts);
    let sent = harness
        .notifier()
        .deliver(&notification(
            ChannelKind::Ntfy,
            url,
            basic(),
            event(EventKind::BackupFailed),
            Message::Custom("demo-plan failed".to_owned()),
        ))
        .await;
    assert_eq!(sent, Ok(()));
    assert_eq!(
        received_since(harness, before).await,
        [Received {
            kind: ChannelKind::Ntfy,
            ping: None,
            event: "backup.failed".to_owned(),
            message: "demo-plan failed".to_owned(),
            auth: Some(ReceivedAuth::Basic {
                username: USERNAME.to_owned(),
                password: PASSWORD.to_owned(),
            }),
        }]
    );
}

async fn a_healthchecks_channel_is_pinged_for_each_event(harness: &dyn NotifierHarness) {
    let before = harness.received().await.len();
    let url = harness.url(ChannelKind::Healthchecks, Behaviour::Accepts);
    let kinds = [
        EventKind::BackupStarted,
        EventKind::BackupSucceeded,
        EventKind::BackupWarning,
        EventKind::BackupFailed,
        EventKind::BackupRetrying,
        EventKind::PruneFailed,
    ];
    for kind in kinds {
        let sent = harness
            .notifier()
            .deliver(&notification(
                ChannelKind::Healthchecks,
                url.clone(),
                None,
                event(kind),
                Message::Custom(kind.name().to_owned()),
            ))
            .await;
        assert_eq!(sent, Ok(()), "{kind:?} pings its check");
    }
    let pings: Vec<(Option<Ping>, String)> = received_since(harness, before)
        .await
        .into_iter()
        .map(|received| (received.ping, received.message))
        .collect();
    let expected: Vec<(Option<Ping>, String)> = kinds
        .iter()
        .map(|kind| (Some(kind.ping()), kind.name().to_owned()))
        .collect();
    assert_eq!(pings, expected, "start, success and fail pings, in order");
}

async fn a_default_message_says_something_for_every_event(harness: &dyn NotifierHarness) {
    for kind in EventKind::ALL {
        let before = harness.received().await.len();
        let url = harness.url(ChannelKind::Webhook, Behaviour::Accepts);
        let sent = harness
            .notifier()
            .deliver(&notification(
                ChannelKind::Webhook,
                url,
                None,
                event(kind),
                Message::Default,
            ))
            .await;
        assert_eq!(sent, Ok(()));
        let received = received_since(harness, before).await;
        assert_eq!(received.len(), 1, "{kind:?} is sent once");
        let Some(received) = received.first() else {
            return;
        };
        assert_eq!(received.event, kind.name());
        assert!(
            !received.message.trim().is_empty(),
            "{kind:?} has a default message"
        );
        assert_eq!(received.auth, None, "no credentials, no sign-in");
    }
}

async fn a_channel_nobody_listens_at_is_unreachable(harness: &dyn NotifierHarness) {
    for kind in ChannelKind::ALL {
        let url = harness.url(kind, Behaviour::Refuses);
        let sent = harness
            .notifier()
            .deliver(&notification(
                kind,
                url,
                None,
                event(EventKind::BackupFailed),
                Message::Default,
            ))
            .await;
        assert_eq!(sent, Err(NotifyError::Unreachable), "{kind:?}");
    }
}

async fn a_status_other_than_success_is_a_rejection(harness: &dyn NotifierHarness) {
    for (kind, status) in [
        (ChannelKind::Webhook, 500),
        (ChannelKind::Ntfy, 403),
        (ChannelKind::Healthchecks, 404),
    ] {
        let before = harness.received().await.len();
        let url = harness.url(kind, Behaviour::Answers(status));
        let sent = harness
            .notifier()
            .deliver(&notification(
                kind,
                url,
                None,
                event(EventKind::BackupFailed),
                Message::Default,
            ))
            .await;
        assert_eq!(sent, Err(NotifyError::Rejected { status }), "{kind:?}");
        assert_eq!(received_since(harness, before).await, []);
    }
}

async fn a_redirect_is_not_followed(harness: &dyn NotifierHarness) {
    let url = harness.url(ChannelKind::Webhook, Behaviour::Answers(307));
    let sent = harness
        .notifier()
        .deliver(&notification(
            ChannelKind::Webhook,
            url,
            None,
            event(EventKind::BackupFailed),
            Message::Default,
        ))
        .await;
    assert_eq!(sent, Err(NotifyError::Rejected { status: 307 }));
}

async fn a_channel_that_never_answers_times_out(harness: &dyn NotifierHarness) {
    let url = harness.url(ChannelKind::Webhook, Behaviour::Hangs);
    let sent = harness
        .notifier()
        .deliver(&notification(
            ChannelKind::Webhook,
            url,
            None,
            event(EventKind::BackupFailed),
            Message::Default,
        ))
        .await;
    assert_eq!(sent, Err(NotifyError::TimedOut));
}

async fn an_address_that_is_not_http_is_never_sent_to(harness: &dyn NotifierHarness) {
    let before = harness.received().await.len();
    for url in [
        "ftp://127.0.0.1/demo",
        "file:///etc/hosts",
        "not a url",
        "",
        "http://",
    ] {
        let sent = harness
            .notifier()
            .deliver(&notification(
                ChannelKind::Webhook,
                url.to_owned(),
                None,
                event(EventKind::BackupFailed),
                Message::Default,
            ))
            .await;
        assert_eq!(sent, Err(NotifyError::InvalidAddress), "{url:?}");
    }
    assert_eq!(received_since(harness, before).await, []);
}
