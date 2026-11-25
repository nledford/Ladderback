//! The hub's offline alerts (technical design §7.2, §11; §16, decision
//! 168): every [`MACHINES_WATCHED_EVERY`], the hub tells its notification
//! rules, once, of each approved machine offline for
//! [`OFFLINE_ALERT_AFTER`](domain::fleet::OFFLINE_ALERT_AFTER), with
//! `machine.offline`, and of each that came back after such an alert, with
//! `machine.online`.

use std::sync::Arc;

use domain::fleet::AgentStatus;
use domain::notifications::{Event, EventKind};
use domain::{AgentId, SignedDuration, Timestamp};

use super::HubPairing;
use crate::notifications::{Notify, Report};
use crate::ports::StoreError;

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

    /// Tells each alert now due, the returns first, and returns what
    /// telling them left to report. An offline alert no rule sent, as
    /// while none sends `machine.offline` or the configuration can't be
    /// read, is due again at the next watch.
    pub async fn watch(&self) -> Vec<Report> {
        let due = match self.pairing.alerts_due().await {
            Ok(due) => due,
            Err(error) => {
                log::warn!("the hub couldn't read its machines to watch them: {error:?}");
                return Vec::new();
            }
        };
        let mut reports = Vec::new();
        for (agent, event) in due {
            let (sent, told) = self.notify.tell_machine(&event).await;
            if !sent && event.kind == EventKind::MachineOffline {
                self.pairing.alert_untold(agent, event.at);
            }
            reports.extend(told);
        }
        reports
    }
}

impl HubPairing {
    /// The machines' events due now, each with its machine: the returns
    /// first, in the order they came back, then the machines offline for
    /// 15 minutes, by machine ID. Each machine is named by the hostname it
    /// paired under.
    async fn alerts_due(&self) -> Result<Vec<(AgentId, Event)>, StoreError> {
        // Held so that no connection goes live or away while the watch
        // pairs each machine with whether it is online.
        let _telling = self.presence.telling.lock().await;
        let records = self.registry.agents().await?;
        let approved: Vec<(AgentId, bool)> = records
            .iter()
            .filter(|record| matches!(record.status, AgentStatus::Approved { .. }))
            .map(|record| (record.agent_id, self.is_online(record.agent_id)))
            .collect();
        let due = self.presence.alerts().due(&approved, self.clock.now());
        let hostname = |agent: AgentId| {
            records
                .iter()
                .find(|record| record.agent_id == agent)
                .map(|record| record.hostname.clone())
        };
        let online = due.online.into_iter().filter_map(|(agent, at)| {
            hostname(agent).map(|machine| (agent, Event::machine_online(machine, at)))
        });
        let offline = due.offline.into_iter().filter_map(|(agent, since)| {
            hostname(agent).map(|machine| (agent, Event::machine_offline(machine, since)))
        });
        Ok(online.chain(offline).collect())
    }

    /// No rule sent the machine `agent`'s `machine.offline` of a machine
    /// offline since `since`: it is due again at the next watch while the
    /// machine stays offline.
    fn alert_untold(&self, agent: AgentId, since: Timestamp) {
        self.presence.alerts().untold(agent, since);
    }
}
