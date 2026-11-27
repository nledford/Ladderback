//! Which approved machines are online: those whose sync connection the hub
//! holds (technical design §7.2; §16, decision 159). The transport says when
//! a machine's connection went live, at its first message after the
//! `Welcome`, and when it went away; one machine has one live connection,
//! and a newer one replaces the older, which closes. Presence lives in
//! memory only: after a restart every machine is offline until it connects
//! again. When a live connection ends, the registry keeps the time as when
//! the hub last heard from the machine. The offline alerts (decision 168)
//! learn here when a machine went offline or came back, and live in memory
//! too.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use api::UiEvent;
use domain::AgentId;
use domain::fleet::{Accepts, AgentStatus, OfflineAlerts};
use domain::maintenance::RunningBackup;

use super::HubPairing;

/// What closes a live connection the hub let go of.
pub type CloseConnection = Box<dyn FnOnce() + Send>;

/// One machine's live connection: the transport's number for it, what
/// closes it, and the commands the machine takes on it, as its `Hello`
/// said (decision 176).
struct Live {
    connection: u64,
    close: CloseConnection,
    accepts: Accepts,
}

/// The live connection of each online machine; and what each machine's
/// latest heartbeat said it runs, which the prune guard reads (decision
/// 166).
#[derive(Default)]
pub(super) struct Presence {
    live: Mutex<HashMap<AgentId, Live>>,
    /// The backups each machine's latest heartbeat named; a machine whose
    /// heartbeat didn't say, or that sent none yet, has none here.
    pub(super) running: Mutex<HashMap<AgentId, Vec<RunningBackup>>>,
    /// Held from a change of the table until the browsers heard it, so
    /// they hear a machine come online and go offline in the order the
    /// table changed.
    pub(super) telling: futures::lock::Mutex<()>,
    /// Since when each approved machine is offline, and which the hub
    /// alerted (decision 168).
    alerts: Mutex<OfflineAlerts>,
}

impl Presence {
    /// The offline alerts, which no one holds across an `await`.
    pub(super) fn alerts(&self) -> MutexGuard<'_, OfflineAlerts> {
        self.alerts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl HubPairing {
    /// The machine `agent`'s connection `connection` went live: it
    /// replaces the machine's older connection, which `close` of that one
    /// closes, and the browsers hear the machine came online when it wasn't.
    /// A machine no longer approved, as one a person removed while this
    /// connection made its handshake, is let go of at once instead.
    pub async fn went_live(&self, agent: AgentId, connection: u64, close: CloseConnection) {
        self.went_live_taking(agent, connection, close, Accepts::none())
            .await;
    }

    /// [`Self::went_live`], the machine taking `accepts` from the hub on
    /// this connection (decision 176).
    pub async fn went_live_taking(
        &self,
        agent: AgentId,
        connection: u64,
        close: CloseConnection,
        accepts: Accepts,
    ) {
        let _telling = self.presence.telling.lock().await;
        let replaced = self
            .presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                agent,
                Live {
                    connection,
                    close,
                    accepts,
                },
            );
        let was_online = replaced.is_some();
        // Closed outside the lock: what closes it may take its own.
        if let Some(older) = replaced {
            (older.close)();
        }
        // Read after the connection is in the table: a removal that let go
        // of nothing, having come first, is seen here (decision 154).
        let approved = match self.registry.agent(agent).await {
            Ok(found) => {
                found.is_some_and(|record| matches!(record.status, AgentStatus::Approved { .. }))
            }
            Err(error) => {
                // The handshake just found it approved; a store that can't
                // say otherwise now leaves it so.
                log::warn!("the hub couldn't read a machine that connected: {error:?}");
                true
            }
        };
        if !approved {
            self.let_go(agent);
        } else if !was_online {
            self.presence.alerts().came_online(agent, self.clock.now());
            self.tell(self.heard(agent, true)).await;
        }
    }

    /// The machine `agent`'s connection `connection` ended: when it is the
    /// machine's live one, the machine is offline, the registry keeps now
    /// as when the hub last heard from it, and the browsers hear it.
    pub async fn went_away(&self, agent: AgentId, connection: u64) {
        let _telling = self.presence.telling.lock().await;
        let ended = {
            let mut live = self
                .presence
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let this_one = live
                .get(&agent)
                .is_some_and(|held| held.connection == connection);
            this_one.then(|| live.remove(&agent)).flatten()
        };
        if ended.is_some() {
            // What it ran ends with it: a machine offline holds no prune.
            self.forget_running(agent);
            let now = self.clock.now();
            self.presence.alerts().went_offline(agent, now);
            if let Err(error) = self.registry.record_seen(agent, now).await {
                log::warn!("the hub couldn't keep when it last heard from a machine: {error:?}");
            }
            self.tell(self.heard(agent, false)).await;
        }
    }

    /// Whether the hub holds the machine `agent`'s sync connection.
    pub fn is_online(&self, agent: AgentId) -> bool {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&agent)
    }

    /// The commands the machine `agent` takes from the hub: none while it
    /// is offline (decision 176).
    pub fn accepts_of(&self, agent: AgentId) -> Accepts {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&agent)
            .map_or_else(Accepts::none, |held| held.accepts.clone())
    }

    /// The machines the hub holds a connection of, but `agent`.
    pub(super) fn online_except(&self, agent: AgentId) -> Vec<AgentId> {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .filter(|online| **online != agent)
            .copied()
            .collect()
    }

    /// Forgets what `agent`'s heartbeats said it runs.
    fn forget_running(&self, agent: AgentId) {
        self.presence
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&agent);
    }

    /// Closes the machine `agent`'s live connection, if the hub holds one,
    /// and cancels its offline alert: a person removed the machine
    /// (decision 154), which then syncs no more on a connection it proved
    /// before. The browsers heard it was removed.
    pub(super) fn let_go(&self, agent: AgentId) {
        self.presence.alerts().removed(agent);
        let held = self
            .presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&agent);
        self.forget_running(agent);
        // Closed outside the lock: what closes it may take its own.
        if let Some(held) = held {
            (held.close)();
        }
    }

    /// What the browsers hear of `agent` coming online or going offline,
    /// now.
    fn heard(&self, agent: AgentId, online: bool) -> UiEvent {
        let agent = agent.as_uuid().to_string();
        let at = self.clock.now().to_string();
        if online {
            UiEvent::MachineOnline { machine: agent, at }
        } else {
            UiEvent::MachineOffline { machine: agent, at }
        }
    }
}
