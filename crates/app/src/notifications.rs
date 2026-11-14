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
    Attempt, Auth, ChannelName, Event, EventKind, Matched, Outcome, RuleName, Subject,
};
use domain::planning::Locale;
use domain::secrets::{Redactor, SecretRef, SecretValue, SecretWarning};
use domain::{OperationId, Timestamp, Uuid};
use futures::StreamExt;
use futures::channel::mpsc;
use futures::future::{self, join_all};
use futures::stream::BoxStream;

use crate::ports::{
    ConfigError, ConfigStore, Credentials, Destination, Message, Notification, Notifier,
    NotifyError, OperationRecord, OperationStore, SecretResolver,
};
use crate::secrets::SecretWarnings;

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
    warned: Option<SecretWarnings>,
}

/// How the news says an operation went: started, ended with an outcome, or
/// failed and is retried.
enum Turn {
    Started,
    Ended(Outcome),
    Retrying(Attempt),
}

/// The start or the end of an operation, or the retry planned after its
/// failure, as the news says it: the operation's ID, how it went and when
/// (for a retry, when the retry runs), as the news writes them.
struct News {
    operation: String,
    turn: Turn,
    at: String,
}

impl News {
    /// What `news` says of an operation's start or end, or of a retry
    /// planned after it failed; `None` for news that is no event (progress,
    /// a warning, an agent coming or going).
    fn of(news: UiEvent) -> Option<Self> {
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
            UiEvent::BackupRetrying {
                operation,
                attempt,
                attempts,
                at,
                ..
            } => (
                operation,
                Turn::Retrying(Attempt {
                    number: attempt,
                    of: attempts,
                }),
                at,
            ),
            _ => return None,
        };
        Some(Self {
            operation,
            turn,
            at,
        })
    }
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
            warned: None,
        }
    }

    /// Tells `told` each warning reading a channel's secrets gives (§10.2).
    #[must_use]
    pub fn telling(self, told: SecretWarnings) -> Self {
        Self {
            warned: Some(told),
            ..self
        }
    }

    /// Tells the channels of every rule that sends the event `news` is, and
    /// returns what there is to report: each notification that wasn't
    /// delivered, or, once, a configuration that couldn't be read. News that
    /// is no event (progress, a warning), and an operation the log doesn't
    /// have, tell nothing.
    pub async fn tell(&self, news: UiEvent) -> Vec<Report> {
        match News::of(news) {
            Some(news) => self.told(news).await,
            None => Vec::new(),
        }
    }

    /// Tells the event `news` is, as [`Self::tell`] does.
    async fn told(&self, news: News) -> Vec<Report> {
        let News {
            operation,
            turn,
            at,
        } = news;
        let config = match self.config.load().await {
            Ok(config) => config,
            Err(error) => return vec![Report::ConfigUnavailable(error)],
        };
        let notifications = config.notifications();
        if notifications.rules().is_empty() {
            return Vec::new();
        }
        let Ok(id) = Uuid::try_parse(&operation) else {
            return Vec::new();
        };
        let Ok(Some(detail)) = self.operations.get(OperationId::from_uuid(id)).await else {
            return Vec::new();
        };
        let record = &detail.record;
        let at = at.parse::<Timestamp>().ok().or(match turn {
            Turn::Started => record.started_at,
            Turn::Ended(_) => record.finished_at,
            Turn::Retrying(_) => None,
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
            Turn::Retrying(attempt) => Event::retrying(subject, attempt, at),
        };
        let Some(event) = event else {
            return Vec::new();
        };
        let locale = config.agent().locale.as_ref();
        let deliveries = notifications
            .matching(&event)
            .map(|matched| self.send(matched, &event, locale));
        join_all(deliveries)
            .await
            .into_iter()
            .flatten()
            .map(Report::Undelivered)
            .collect()
    }

    /// Tells each piece of `news` in turn until the stream ends, giving
    /// `report` what there is to report of each.
    ///
    /// A subscriber's queue on the bus is short and loses its oldest events
    /// once it is full (§4.4), and a delivery can take its whole timeout. So
    /// the starts, ends and planned retries are read off the bus as they
    /// come, while a slow channel is still being told, and wait their turn
    /// here: one per start and end of a run and per retry planned, never per
    /// warning or progress, so a run's warnings can't push another run's end
    /// out of the queue.
    pub async fn run(&self, news: BoxStream<'static, UiEvent>, mut report: impl FnMut(Report)) {
        let (waiting, mut turns) = mpsc::unbounded();
        let read = news
            .filter_map(|next| future::ready(News::of(next).map(Ok)))
            .forward(waiting);
        let tell = async {
            while let Some(next) = turns.next().await {
                for told in self.told(next).await {
                    report(told);
                }
            }
        };
        // The reading can't fail: the turns are read until it ends.
        let (_, ()) = futures::join!(read, tell);
    }

    /// Sends `event` to the channel of `matched`, a default message in the
    /// agent's `locale`, or says why it wasn't.
    async fn send(
        &self,
        matched: Matched<'_>,
        event: &Event,
        locale: Option<&Locale>,
    ) -> Option<Undelivered> {
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
            locale: locale.cloned(),
        };
        self.notifier
            .deliver(&notification)
            .await
            .err()
            .map(|error| unsent(Unsent::Delivery(error)))
    }

    /// The channel's URL and credentials, read for this delivery, or `None`
    /// when one of its secrets can't be read. What reading them warned of is
    /// told either way, masked of every secret read (I4).
    async fn destination(&self, matched: Matched<'_>) -> Option<Destination> {
        let mut read = ChannelRead::default();
        let destination = self.read_destination(matched, &mut read).await;
        if let Some(told) = &self.warned {
            read.tell(told);
        }
        destination
    }

    async fn read_destination(
        &self,
        matched: Matched<'_>,
        read: &mut ChannelRead,
    ) -> Option<Destination> {
        let secrets = self.secrets.as_ref();
        let channel = matched.channel;
        let url = read.secret(secrets, channel.url()).await?;
        let auth = match channel.auth() {
            None => None,
            Some(Auth::Bearer(token)) => {
                Some(Credentials::Bearer(read.secret(secrets, token).await?))
            }
            Some(Auth::Basic { username, password }) => Some(Credentials::Basic {
                username: username.clone(),
                password: read.secret(secrets, password).await?,
            }),
        };
        Some(Destination {
            kind: channel.kind(),
            url,
            auth,
        })
    }
}

/// The secrets one delivery read, and what reading them warned of, each
/// once.
#[derive(Default)]
struct ChannelRead {
    values: Vec<SecretValue>,
    warnings: Vec<SecretWarning>,
}

impl ChannelRead {
    async fn secret(
        &mut self,
        secrets: &dyn SecretResolver,
        secret: &SecretRef,
    ) -> Option<SecretValue> {
        let resolved = secrets.resolve(secret).await.ok()?;
        for warning in resolved.warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
        self.values.push(resolved.value.clone());
        Some(resolved.value)
    }

    /// Tells `told` each warning, its path masked of every secret read.
    fn tell(self, told: &SecretWarnings) {
        let redactor = Redactor::new(&self.values);
        for warning in &self.warnings {
            told(&warning.masked(&redactor));
        }
    }
}

/// A success the log records with warnings is told as one. The log's status
/// says so whenever the run recorded a warning (`Operation::succeed`), or
/// was given one after it ended (`Operation::attach_warning`).
fn with_warnings(outcome: Outcome, record: &OperationRecord) -> Outcome {
    match outcome {
        Outcome::Succeeded if record.status == OperationStatus::SucceededWithWarnings => {
            Outcome::SucceededWithWarnings
        }
        other => other,
    }
}
