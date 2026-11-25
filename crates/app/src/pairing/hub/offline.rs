//! The hub's offline alerts (technical design §7.2, §11; §16, decision
//! 168): every [`MACHINES_WATCHED_EVERY`], the hub tells its notification
//! rules, once, of each approved machine offline for
//! [`OFFLINE_ALERT_AFTER`](domain::fleet::OFFLINE_ALERT_AFTER), with
//! `machine.offline`, and of each that came back after such an alert, with
//! `machine.online`.

use std::sync::Arc;

use domain::SignedDuration;

use super::HubPairing;
use crate::notifications::{Notify, Report};

/// How often the hub watches its machines for offline alerts: an alert
/// goes out at most this long after the machine has been offline 15
/// minutes, and a return at most this long after it came back.
pub const MACHINES_WATCHED_EVERY: SignedDuration = SignedDuration::from_secs(30);

/// Watches the hub's machines and tells its notification rules when one
/// has stayed offline, and when one it alerted came back.
pub struct WatchMachines {
    pairing: Arc<HubPairing>,
    notify: Arc<Notify>,
}

impl WatchMachines {
    pub fn new(pairing: Arc<HubPairing>, notify: Arc<Notify>) -> Self {
        Self { pairing, notify }
    }

    /// Tells each alert now due, and returns what telling them left to
    /// report.
    pub async fn watch(&self) -> Vec<Report> {
        let _ = (&self.pairing, &self.notify);
        todo!()
    }
}
