//! The notifications adapter (technical design §11): the [`Notifier`] port
//! over HTTP. A webhook is posted a JSON body, an ntfy topic the message's
//! text, and a healthchecks check is pinged at its URL, or at `/start` or
//! `/fail` of it.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::time::Duration;

use app::ports::{Notification, Notifier, NotifyError};
use async_trait::async_trait;
use l10n::Localizer;

/// The HTTP client couldn't be built: its TLS settings couldn't be read
/// from the system.
#[derive(Debug)]
pub struct ClientError(pub String);

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the notifications client couldn't be built: {}", self.0)
    }
}

impl std::error::Error for ClientError {}

/// Delivers notifications over HTTP, writing a default message in the words
/// of `localizer`.
pub struct HttpNotifier {
    localizer: Localizer,
    timeout: Duration,
}

impl HttpNotifier {
    /// A notifier whose every delivery ends within `timeout`.
    pub fn new(localizer: Localizer, timeout: Duration) -> Result<Self, ClientError> {
        let _ = (&localizer, timeout);
        todo!()
    }
}

#[async_trait]
impl Notifier for HttpNotifier {
    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
        let _ = (notification, &self.localizer, self.timeout);
        todo!()
    }
}

/// The URL `text` is, when it is an `http` or `https` URL with a host, or
/// [`NotifyError::InvalidAddress`]. Any host is taken, a private address or
/// a name on the local network as well as a public one: the URL comes from
/// the user's own configuration.
fn address(text: &str) -> Result<reqwest::Url, NotifyError> {
    let _ = text;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_on_the_local_network_over_plain_http_is_an_address() {
        for url in [
            "http://10.0.0.2:8080/demo-topic",
            "http://192.168.1.20/demo-topic",
            "http://ntfy.home.arpa/demo-topic",
            "http://127.0.0.1:2586/demo-topic",
            "https://ntfy.example.com/demo-topic",
        ] {
            assert_eq!(address(url).map(|url| url.to_string()), Ok(url.to_owned()));
        }
    }

    #[test]
    fn a_url_that_is_not_http_with_a_host_is_no_address() {
        for url in ["ftp://127.0.0.1/demo", "file:///etc/hosts", "not a url", "", "http://"] {
            assert_eq!(address(url), Err(NotifyError::InvalidAddress), "{url:?}");
        }
    }
}
