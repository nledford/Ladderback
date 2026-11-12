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
use domain::notifications::{ChannelName, EventKind, RuleName};
use futures::stream::BoxStream;

use crate::ports::{ConfigStore, Notifier, NotifyError, OperationStore, SecretResolver};

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
        todo!()
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

/// Sends the notifications of the agent's rules.
pub struct Notify {
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    secrets: Arc<dyn SecretResolver>,
    notifier: Arc<dyn Notifier>,
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
    /// returns the notifications that weren't delivered. News that is no
    /// event (progress, a warning), and an operation the log doesn't have,
    /// tell nothing.
    pub async fn tell(&self, news: &UiEvent) -> Vec<Undelivered> {
        let _ = (news, &self.config, &self.operations, &self.secrets, &self.notifier);
        todo!()
    }

    /// Tells each piece of `news` in turn until the stream ends, giving
    /// `report` each notification that wasn't delivered.
    pub async fn run(&self, news: BoxStream<'static, UiEvent>, report: impl FnMut(Undelivered)) {
        let _ = (news, report);
        todo!()
    }
}
