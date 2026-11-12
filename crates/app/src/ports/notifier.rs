//! The port a notification leaves the agent through (technical design §3.2,
//! §11): one delivery to one channel, which ends within
//! [`DELIVERY_TIMEOUT`] whatever the channel does.

use std::time::Duration;

use async_trait::async_trait;
use domain::notifications::{ChannelKind, Event, Username};
use domain::planning::Locale;
use domain::secrets::SecretValue;

/// The longest one delivery takes, connecting, sending and reading the
/// answer's status together. A channel that hasn't answered by then has
/// [`NotifyError::TimedOut`]; the next notification doesn't wait on it.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// How a delivery signs in, with its secret read for this delivery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Credentials {
    /// `Authorization: Bearer <token>`.
    Bearer(SecretValue),
    /// `Authorization: Basic`, of the name and the password.
    Basic {
        username: Username,
        password: SecretValue,
    },
}

/// Where a notification goes: the channel's kind, its URL as the secret it
/// is, and how it signs in, read for this delivery. `Debug` shows no secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub kind: ChannelKind,
    pub url: SecretValue,
    pub auth: Option<Credentials>,
}

/// What a notification says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// The default text for the event, which the adapter writes from the
    /// catalog in the notification's locale (§8.4).
    Default,
    /// The rule's template, rendered: the user's own words, sent as they are.
    Custom(String),
}

/// One notification of one event to one channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub destination: Destination,
    pub event: Event,
    pub message: Message,
    /// The agent's locale (`[agent] locale`), when its configuration names
    /// one: a default message is written in it, and else in the system's.
    pub locale: Option<Locale>,
}

/// Why a notification wasn't delivered. None of them holds the channel's
/// URL, its token or text the channel answered with (I4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyError {
    /// The URL is not an `http` or `https` URL with a host. Nothing was sent.
    InvalidAddress,
    /// No connection could be made, or it broke before an answer.
    Unreachable,
    /// No answer within [`DELIVERY_TIMEOUT`].
    TimedOut,
    /// The channel answered with a status other than success. A redirect is
    /// one: it is never followed, so a channel can't send a notification on
    /// to an address its owner didn't configure.
    Rejected { status: u16 },
}

impl NotifyError {
    /// The reason, as the `notification-undelivered` warning's `reason`
    /// argument says it.
    pub fn reason(self) -> &'static str {
        match self {
            Self::InvalidAddress => "invalid-address",
            Self::Unreachable => "unreachable",
            Self::TimedOut => "timed-out",
            Self::Rejected { .. } => "rejected",
        }
    }
}

/// Delivers notifications.
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Sends `notification` once. It is not tried again: a failure is the
    /// caller's to report, and the next event is sent as usual.
    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError>;
}
