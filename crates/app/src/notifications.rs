//! Telling the channels of the rules an event matches (technical design §11).
//!
//! The use case hears the news a run announces once the operation log has it
//! (§4.4), reads the operation from the log, and makes the domain event of
//! it. Each rule that sends the event reads its channel's secrets and sends
//! one notification, in the rule's template or the default text; the
//! deliveries of one event run side by side. A delivery that fails is
//! reported and not tried again, and the next channel and the next event
//! are sent as usual.

use std::sync::Arc;

use api::UiEvent;
use domain::execution::OperationStatus;
use domain::notifications::{
    Auth, ChannelName, Event, EventKind, Matched, Outcome, RuleName, Subject,
};
use domain::secrets::{SecretRef, SecretValue};
use domain::{OperationId, Timestamp, Uuid};
use futures::StreamExt;
use futures::future::join_all;
use futures::stream::BoxStream;

use crate::ports::{
    ConfigError, ConfigStore, Credentials, Destination, Message, Notification, Notifier,
    NotifyError, OperationRecord, OperationStore, SecretResolver,
};

/// Why a notification wasn't sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsent {
    /// A secret of its channel (the URL, the token or the password) couldn't
    /// be read. Nothing was sent.
    SecretUnavailable,
    /// The channel didn't take it.
    Delivery(NotifyError),
}

impl Unsent {
    /// The reason, as the `notification-undelivered` warning's `reason`
    /// argument says it.
    pub fn reason(self) -> &'static str {
        match self {
            Self::SecretUnavailable => "secret-unavailable",
            Self::Delivery(error) => error.reason(),
        }
    }
}

/// A notification that wasn't delivered: which event, by which rule, to
/// which channel, and why. It holds no secret (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undelivered {
    pub event: EventKind,
    pub rule: RuleName,
    pub channel: ChannelName,
    pub reason: Unsent,
}

/// What telling an event leaves for the daemon to say: a notification that
/// wasn't delivered, or a configuration that couldn't be read when the event
/// came, so none of its rules was told of it. Neither holds a secret (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    Undelivered(Undelivered),
    ConfigUnavailable(ConfigError),
}

/// Sends the notifications of the agent's rules.
pub struct Notify {
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    secrets: Arc<dyn SecretResolver>,
    notifier: Arc<dyn Notifier>,
}

/// How the news says an operation went: started, or ended with an outcome.
enum Turn {
    Started,
    Ended(Outcome),
}

impl Notify {
    pub fn new(
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        secrets: Arc<dyn SecretResolver>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            config,
            operations,
            secrets,
            notifier,
        }
    }

    /// Tells the channels of every rule that sends the event `news` is, and
    /// returns what there is to report: each notification that wasn't
    /// delivered, or, once, a configuration that couldn't be read. News that
    /// is no event (progress, a warning), and an operation the log doesn't
    /// have, tell nothing.
    pub async fn tell(&self, news: &UiEvent) -> Vec<Report> {
        let (operation, turn, at) = match news {
            UiEvent::OperationStarted {
                operation,
                started_at,
                ..
            } => (operation, Turn::Started, started_at),
            UiEvent::OperationSucceeded {
                operation,
                finished_at,
            } => (operation, Turn::Ended(Outcome::Succeeded), finished_at),
            UiEvent::OperationFailed {
                operation,
                finished_at,
                ..
            } => (operation, Turn::Ended(Outcome::Failed), finished_at),
            _ => return Vec::new(),
        };
        let config = match self.config.load().await {
            Ok(config) => config,
            Err(error) => return vec![Report::ConfigUnavailable(error)],
        };
        let notifications = config.notifications();
        if notifications.rules().is_empty() {
            return Vec::new();
        }
        let Ok(id) = Uuid::try_parse(operation) else {
            return Vec::new();
        };
        let Ok(Some(detail)) = self.operations.get(OperationId::from_uuid(id)).await else {
            return Vec::new();
        };
        let record = &detail.record;
        let at = at.parse::<Timestamp>().ok().or(match turn {
            Turn::Started => record.started_at,
            Turn::Ended(_) => record.finished_at,
        });
        let Some(at) = at else {
            return Vec::new();
        };
        let plan_repository = config
            .plans()
            .iter()
            .find(|plan| record.target.plan.as_ref() == Some(&plan.id))
            .map(|plan| &plan.repository);
        let subject = Subject {
            operation: record.id,
            kind: record.kind,
            target: &record.target,
            repository: record.repository_alias.as_ref().or(plan_repository),
            status: &record.status,
        };
        let event = match turn {
            Turn::Started => Event::started(subject, at),
            Turn::Ended(outcome) => Event::ended(subject, with_warnings(outcome, record), at),
        };
        let Some(event) = event else {
            return Vec::new();
        };
        let deliveries = notifications
            .matching(&event)
            .map(|matched| self.send(matched, &event));
        join_all(deliveries)
            .await
            .into_iter()
            .flatten()
            .map(Report::Undelivered)
            .collect()
    }

    /// Tells each piece of `news` in turn until the stream ends, giving
    /// `report` what there is to report of each.
    pub async fn run(&self, mut news: BoxStream<'static, UiEvent>, mut report: impl FnMut(Report)) {
        while let Some(next) = news.next().await {
            for told in self.tell(&next).await {
                report(told);
            }
        }
    }

    /// Sends `event` to the channel of `matched`, or says why it wasn't.
    async fn send(&self, matched: Matched<'_>, event: &Event) -> Option<Undelivered> {
        let unsent = |reason| Undelivered {
            event: event.kind,
            rule: matched.rule.clone(),
            channel: matched.channel_name.clone(),
            reason,
        };
        let Some(destination) = self.destination(matched).await else {
            return Some(unsent(Unsent::SecretUnavailable));
        };
        let message = match matched.template {
            Some(template) => Message::Custom(template.render(event)),
            None => Message::Default,
        };
        let notification = Notification {
            destination,
            event: event.clone(),
            message,
        };
        self.notifier
            .deliver(&notification)
            .await
            .err()
            .map(|error| unsent(Unsent::Delivery(error)))
    }

    /// The channel's URL and credentials, read for this delivery, or `None`
    /// when one of its secrets can't be read.
    async fn destination(&self, matched: Matched<'_>) -> Option<Destination> {
        let channel = matched.channel;
        let url = self.secret(channel.url()).await?;
        let auth = match channel.auth() {
            None => None,
            Some(Auth::Bearer(token)) => Some(Credentials::Bearer(self.secret(token).await?)),
            Some(Auth::Basic { username, password }) => Some(Credentials::Basic {
                username: username.clone(),
                password: self.secret(password).await?,
            }),
        };
        Some(Destination {
            kind: channel.kind(),
            url,
            auth,
        })
    }

    async fn secret(&self, secret: &SecretRef) -> Option<SecretValue> {
        self.secrets
            .resolve(secret)
            .await
            .ok()
            .map(|resolved| resolved.value)
    }
}

/// A success the log records with warnings is told as one.
fn with_warnings(outcome: Outcome, record: &OperationRecord) -> Outcome {
    let warned =
        record.status == OperationStatus::SucceededWithWarnings || record.warning_count > 0;
    match outcome {
        Outcome::Succeeded if warned => Outcome::SucceededWithWarnings,
        other => other,
    }
}
