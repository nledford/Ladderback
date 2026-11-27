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

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use api::UiEvent;
use domain::fleet::{
    Accepts, AgentStatus, CommandId, MAX_COMMANDS_IN_FLIGHT, OfflineAlerts, WireName,
};
use domain::maintenance::RunningBackup;
use domain::{AgentId, SignedDuration};
use futures::channel::{mpsc, oneshot};

use super::HubPairing;
use crate::hub_commands::CommandAnswer;

/// What closes a live connection the hub let go of.
pub type CloseConnection = Box<dyn FnOnce() + Send>;

/// A command the hub sends a machine on its live connection: its kind and
/// payload, which the transport signs for the connection as it sends it
/// (decision 176).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingCommand {
    pub kind: WireName,
    pub payload: Vec<u8>,
}

/// The commands the hub sends on one live connection, in the order asked,
/// at most [`MAX_COMMANDS_IN_FLIGHT`] waiting: what the transport reads
/// and sends. It ends when the connection is no longer live.
pub type CommandQueue = mpsc::Receiver<OutgoingCommand>;

/// One machine's live connection: the transport's number for it, what
/// closes it, the commands the machine takes on it, as its `Hello` said
/// (`None`: it didn't say, as a release before them), and where the hub's
/// commands to it go (decision 176).
struct Live {
    connection: u64,
    close: CloseConnection,
    accepts: Option<Accepts>,
    commands: mpsc::Sender<OutgoingCommand>,
}

/// A command the hub waits for a machine to answer: the machine, and
/// where its answer goes.
pub(super) struct Awaited {
    pub(super) agent: AgentId,
    pub(super) answer: oneshot::Sender<CommandAnswer>,
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
    /// The commands sent and not answered yet, at most
    /// [`MAX_COMMANDS_IN_FLIGHT`] for each machine.
    pub(super) awaited: Mutex<HashMap<CommandId, Awaited>>,
    /// When on the hub's monotonic clock it sent each machine its commands
    /// in the last hour, oldest first, at most
    /// [`HUB_COMMANDS_PER_HOUR`](domain::fleet::HUB_COMMANDS_PER_HOUR) for
    /// each; kept while the hub runs, across the machine's connections.
    pub(super) sent: Mutex<HashMap<AgentId, VecDeque<SignedDuration>>>,
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
    pub async fn went_live(
        &self,
        agent: AgentId,
        connection: u64,
        close: CloseConnection,
    ) -> CommandQueue {
        self.went_live_taking(agent, connection, close, None).await
    }

    /// [`Self::went_live`], the machine taking `accepts` from the hub on
    /// this connection, `None` when its `Hello` didn't say (decision 176).
    /// The hub's commands to it come out of the queue this returns.
    pub async fn went_live_taking(
        &self,
        agent: AgentId,
        connection: u64,
        close: CloseConnection,
        accepts: Option<Accepts>,
    ) -> CommandQueue {
        let (commands, queue) = mpsc::channel(MAX_COMMANDS_IN_FLIGHT);
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
                    commands,
                },
            );
        let was_online = replaced.is_some();
        // Closed outside the lock: what closes it may take its own.
        if let Some(older) = replaced {
            // The older connection answers none of the commands sent on it.
            self.forget_commands(agent);
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
        queue
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
            self.forget_commands(agent);
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

    /// The commands the machine `agent` takes from the hub, as its `Hello`
    /// said: `None` while it is offline, or when it didn't say (decision
    /// 176).
    pub fn accepts_of(&self, agent: AgentId) -> Option<Accepts> {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&agent)
            .and_then(|held| held.accepts.clone())
    }

    /// What the live machine `agent` takes and where its commands go; none
    /// while it is offline.
    pub(super) fn command_line(
        &self,
        agent: AgentId,
    ) -> Option<(Option<Accepts>, mpsc::Sender<OutgoingCommand>)> {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&agent)
            .map(|held| (held.accepts.clone(), held.commands.clone()))
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

    /// Lets go of the answers awaited of `agent`: their askers hear none.
    fn forget_commands(&self, agent: AgentId) {
        self.presence
            .awaited
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, awaited| awaited.agent != agent);
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
        self.forget_commands(agent);
        self.presence
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&agent);
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
