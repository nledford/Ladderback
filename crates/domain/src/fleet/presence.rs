//! A paired machine's lasting connection to its hub (technical design §7.2;
//! §16, decision 159): an approved machine holds one connection open while
//! it runs and sends a heartbeat on it every [`HEARTBEAT_EVERY`], which the
//! hub answers, so each side learns within a bound that the other is gone.
//! The hub closes a connection that sent nothing for [`SILENCE_LIMIT`], and
//! a machine is online while the hub holds its connection. One offline for
//! [`OFFLINE_ALERT_AFTER`] is alerted once, and its return after that alert
//! told once ([`OfflineAlerts`]; decision 168).

use std::collections::BTreeMap;

use super::WireName;
use crate::{AgentId, SignedDuration, Timestamp};

/// How often an approved machine sends its hub a heartbeat while it holds
/// its connection.
pub const HEARTBEAT_EVERY: SignedDuration = SignedDuration::from_secs(30);

/// How long the hub keeps a machine's connection that sent nothing: three
/// heartbeats missed in a row.
pub const SILENCE_LIMIT: SignedDuration = SignedDuration::from_secs(90);

/// The kind of the machine's heartbeat.
pub fn heartbeat_kind() -> WireName {
    WireName::known("Heartbeat")
}

/// The kind of the hub's answer to a heartbeat.
pub fn heartbeat_ack_kind() -> WireName {
    WireName::known("HeartbeatAck")
}

/// How long a machine stays offline before its hub tells its notification
/// rules, once, with `machine.offline` (§11; §16, decision 168).
pub const OFFLINE_ALERT_AFTER: SignedDuration = SignedDuration::from_mins(15);

/// The hub's offline alerts (§11; §16, decision 168): since when each
/// approved machine is offline, whether the hub alerted it, and each
/// machine that came back after an alert, whose return is told once. The
/// hub keeps it in memory: after a restart, a machine still offline is
/// offline since the hub's first watch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfflineAlerts {
    offline: BTreeMap<AgentId, Offline>,
    back: Vec<(AgentId, Timestamp)>,
}

/// One offline machine: since when, and whether the hub alerted it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Offline {
    since: Timestamp,
    alerted: bool,
}

/// What a watch of the machines finds to tell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DueAlerts {
    /// Each machine offline for [`OFFLINE_ALERT_AFTER`] and not alerted
    /// yet, with since when it is offline, oldest first by machine ID.
    pub offline: Vec<(AgentId, Timestamp)>,
    /// Each machine that came back after an alert, with when, in the order
    /// they came back.
    pub online: Vec<(AgentId, Timestamp)>,
}

impl OfflineAlerts {
    /// `machine` went offline at `at`. One already offline stays offline
    /// since when it went.
    pub fn went_offline(&mut self, machine: AgentId, at: Timestamp) {
        self.offline.entry(machine).or_insert(Offline {
            since: at,
            alerted: false,
        });
    }

    /// `machine` came online at `at`: its return is told at the next watch
    /// when the hub alerted it offline, and else never.
    pub fn came_online(&mut self, machine: AgentId, at: Timestamp) {
        if self
            .offline
            .remove(&machine)
            .is_some_and(|offline| offline.alerted)
        {
            self.back.push((machine, at));
        }
    }

    /// A person removed `machine`: nothing of it is told any more.
    pub fn removed(&mut self, machine: AgentId) {
        self.offline.remove(&machine);
        self.back.retain(|(back, _)| *back != machine);
    }

    /// A watch at `now` of the hub's approved machines, `approved`, each
    /// with whether it is online. An approved machine offline that the
    /// alerts don't know of yet, as each is after the hub starts, is
    /// offline since `now`; a machine no longer approved is forgotten. Each
    /// machine offline for [`OFFLINE_ALERT_AFTER`] is due once, and noted
    /// as alerted; each that came back after an alert is due once.
    pub fn due(&mut self, approved: &[(AgentId, bool)], now: Timestamp) -> DueAlerts {
        let online: BTreeMap<AgentId, bool> = approved.iter().copied().collect();
        self.offline
            .retain(|machine, _| online.get(machine) == Some(&false));
        self.back
            .retain(|(machine, _)| online.contains_key(machine));
        for (machine, is_online) in approved {
            if !is_online {
                self.went_offline(*machine, now);
            }
        }
        let mut offline = Vec::new();
        for (machine, watched) in &mut self.offline {
            if !watched.alerted && now.duration_since(watched.since) >= OFFLINE_ALERT_AFTER {
                watched.alerted = true;
                offline.push((*machine, watched.since));
            }
        }
        DueAlerts {
            offline,
            online: std::mem::take(&mut self.back),
        }
    }

    /// No rule took the alert of `machine` offline since `since`: it is
    /// due again at the next watch while the machine stays offline, and a
    /// return the alert would have announced is told to no one.
    pub fn untold(&mut self, machine: AgentId, since: Timestamp) {
        if let Some(watched) = self.offline.get_mut(&machine)
            && watched.since == since
        {
            watched.alerted = false;
        }
        self.back.retain(|(back, _)| *back != machine);
    }
}

#[cfg(test)]
mod tests;
