//! A [`Notifier`] that records what it delivers, and fails a delivery to a
//! URL told to fail as told.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;

use crate::ports::{Notification, Notifier, NotifyError};

/// Records each notification it accepts. A URL that isn't `http` or `https`
/// is [`NotifyError::InvalidAddress`], as the real adapter has it.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    delivered: Mutex<Vec<Notification>>,
    failing: Mutex<HashMap<String, NotifyError>>,
}

impl RecordingNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every delivery to `url` from now on fails with `error`.
    pub fn fail_at(&self, url: &str, error: NotifyError) {
        self.failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(url.to_owned(), error);
    }

    /// What was delivered, in order.
    pub fn delivered(&self) -> Vec<Notification> {
        self.delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
        let _ = notification;
        todo!()
    }
}
