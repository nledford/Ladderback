//! The notifications adapter (technical design §11): the [`Notifier`] port
//! over HTTP. A webhook is posted a JSON body, an ntfy topic the message's
//! text, and a healthchecks check is pinged at its URL, or at `/start` or
//! `/fail` of it.
//!
//! The client follows no redirect, reads no answer past its status, and
//! gives up on a delivery after its timeout. It honours the standard proxy
//! variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, in either
//! case), as the workers do (§10.2); nothing it returns or reports holds the
//! channel's URL, a token, a proxy's credentials or the HTTP library's own
//! text (I4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::time::Duration;

use api::MsgArg;
use app::ports::{Credentials, Message, Notification, Notifier, NotifyError};
use async_trait::async_trait;
use domain::notifications::{ChannelKind, Event, EventKind, Ping};
use l10n::{Localizer, MessageId, msg};
use reqwest::header::{CONTENT_TYPE, HeaderValue};
use reqwest::{Client, RequestBuilder, Url, redirect};

/// The longest channel URL taken, in bytes. A URL is read from a secret,
/// which may be a file of any size; no real channel's URL comes near this.
pub const MAX_URL_BYTES: usize = 8 * 1024;

/// The most of a message sent, in bytes; a longer one is cut at a character
/// boundary. A rendered template stays far below it unless it repeats its
/// placeholders hundreds of times.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024;

/// The HTTP client couldn't be built: its TLS settings couldn't be read
/// from the system. It holds none of the library's text, which may name a
/// proxy's URL with its credentials (I4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientError;

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the notifications client couldn't be built")
    }
}

impl std::error::Error for ClientError {}

/// Delivers notifications over HTTP, writing a default message in the words
/// of `localizer`.
pub struct HttpNotifier {
    client: Client,
    localizer: Localizer,
}

impl HttpNotifier {
    /// A notifier whose every delivery ends within `timeout`.
    pub fn new(localizer: Localizer, timeout: Duration) -> Result<Self, ClientError> {
        // No `.no_proxy()`: the standard proxy variables are honoured, read
        // once here. TLS checks certificates with the platform's verifier.
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(redirect::Policy::none())
            .referer(false)
            .build()
            .map_err(|_| ClientError)?;
        Ok(Self {
            client,
            // A plan's ID or a repository's alias is sent to a machine, not
            // shown beside other text: no isolation marks around it.
            localizer: localizer.with_isolation(false),
        })
    }

    /// The text `notification` says, cut to [`MAX_MESSAGE_BYTES`].
    fn text(&self, notification: &Notification) -> String {
        let text = match &notification.message {
            Message::Custom(text) => text.clone(),
            Message::Default => default_message(&self.localizer, &notification.event),
        };
        capped(text)
    }

    /// The request that delivers `notification` to `url`.
    fn request(&self, url: Url, notification: &Notification) -> RequestBuilder {
        let event = &notification.event;
        let text = self.text(notification);
        let request = match notification.destination.kind {
            ChannelKind::Webhook => self
                .client
                .post(url)
                .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                .body(webhook_body(event, &text).to_string()),
            ChannelKind::Ntfy => self
                .client
                .post(url)
                .header(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))
                .header("Title", HeaderValue::from_static(event.kind.name()))
                .body(text),
            ChannelKind::Healthchecks => self
                .client
                .post(ping_url(url, event.kind.ping()))
                .header(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))
                .body(text),
        };
        match &notification.destination.auth {
            None => request,
            Some(Credentials::Bearer(token)) => request.bearer_auth(token.expose()),
            Some(Credentials::Basic { username, password }) => {
                request.basic_auth(username.as_str(), Some(password.expose()))
            }
        }
    }
}

#[async_trait]
impl Notifier for HttpNotifier {
    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
        let url = address(notification.destination.url.expose())?;
        let response = self
            .request(url, notification)
            .send()
            .await
            .map_err(|error| failure(&error))?;
        // The answer's body is never read: only its status counts, and the
        // response is dropped here with whatever it held.
        status(response.status().as_u16())
    }
}

/// Why a request that got no answer failed, without the library's text.
fn failure(error: &reqwest::Error) -> NotifyError {
    if error.is_timeout() {
        NotifyError::TimedOut
    } else {
        NotifyError::Unreachable
    }
}

/// A 2xx status is a delivery; anything else, a redirect included, is a
/// rejection.
fn status(code: u16) -> Result<(), NotifyError> {
    if (200..300).contains(&code) {
        Ok(())
    } else {
        Err(NotifyError::Rejected { status: code })
    }
}

/// The URL `text` is, when it is an `http` or `https` URL with a host, no
/// longer than [`MAX_URL_BYTES`], or [`NotifyError::InvalidAddress`]. Any
/// host is taken, a private address or a name on the local network as well
/// as a public one: the URL comes from the user's own configuration.
fn address(text: &str) -> Result<Url, NotifyError> {
    if text.len() > MAX_URL_BYTES {
        return Err(NotifyError::InvalidAddress);
    }
    let url = Url::parse(text).map_err(|_| NotifyError::InvalidAddress)?;
    let web = matches!(url.scheme(), "http" | "https");
    let host = url.host_str().is_some_and(|host| !host.is_empty());
    if web && host {
        Ok(url)
    } else {
        Err(NotifyError::InvalidAddress)
    }
}

/// The URL a healthchecks check is pinged at for `ping`: the check's own for
/// a success, `/start` or `/fail` of it otherwise, as healthchecks.io reads
/// them.
fn ping_url(mut url: Url, ping: Ping) -> Url {
    let segment = match ping {
        Ping::Success => return url,
        Ping::Start => "start",
        Ping::Fail => "fail",
    };
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.pop_if_empty().push(segment);
    }
    url
}

/// The JSON a webhook is posted: the event's name and facts, and the
/// message. A fact the event doesn't have is `null`.
fn webhook_body(event: &Event, message: &str) -> serde_json::Value {
    serde_json::json!({
        "event": event.kind.name(),
        "message": message,
        "operation": event.operation.as_uuid().to_string(),
        "plan": event.plan.as_ref().map(|plan| plan.as_str()),
        "repository": event.repository.as_ref().map(|repository| repository.as_str()),
        "code": event.code,
        "time": event.at.to_string(),
    })
}

/// The catalog's text for `event`, in the localizer's locale.
fn default_message(localizer: &Localizer, event: &Event) -> String {
    let operation = || event.operation.as_uuid().to_string();
    let plan = event
        .plan
        .as_ref()
        .map_or_else(operation, |plan| plan.as_str().to_owned());
    let repository = event
        .repository
        .as_ref()
        .map_or_else(operation, |repository| repository.as_str().to_owned());
    let code = event.code.unwrap_or("none").to_owned();
    let (id, args): (MessageId, Vec<(&str, MsgArg)>) = match event.kind {
        EventKind::BackupStarted => (msg::NOTIFY_BACKUP_STARTED, vec![("plan", MsgArg::Text(plan))]),
        EventKind::BackupSucceeded => {
            (msg::NOTIFY_BACKUP_SUCCEEDED, vec![("plan", MsgArg::Text(plan))])
        }
        EventKind::BackupWarning => (msg::NOTIFY_BACKUP_WARNING, vec![("plan", MsgArg::Text(plan))]),
        EventKind::BackupFailed => (
            msg::NOTIFY_BACKUP_FAILED,
            vec![("plan", MsgArg::Text(plan)), ("code", MsgArg::Text(code))],
        ),
        EventKind::ForgetFailed => (
            msg::NOTIFY_FORGET_FAILED,
            vec![("repository", MsgArg::Text(repository)), ("code", MsgArg::Text(code))],
        ),
        EventKind::PruneFailed => (
            msg::NOTIFY_PRUNE_FAILED,
            vec![("repository", MsgArg::Text(repository)), ("code", MsgArg::Text(code))],
        ),
        EventKind::CheckFailed => (
            msg::NOTIFY_CHECK_FAILED,
            vec![("repository", MsgArg::Text(repository)), ("code", MsgArg::Text(code))],
        ),
    };
    localizer.message(id, &args).text().to_owned()
}

/// `text`, cut at the last character boundary within [`MAX_MESSAGE_BYTES`].
fn capped(mut text: String) -> String {
    let end = text.floor_char_boundary(MAX_MESSAGE_BYTES);
    text.truncate(end);
    text
}

#[cfg(test)]
mod tests;
