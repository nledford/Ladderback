//! Which approved machines are online: those whose sync connection the hub
//! holds (technical design §7.2; §16, decision 159). The transport says when
//! a machine's connection went live and when it went away; one machine has
//! one live connection, and a newer one replaces the older, which closes.
//! Presence lives in memory only: after a restart every machine is offline
//! until it connects again.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use api::UiEvent;
use domain::AgentId;

use super::HubPairing;

/// What closes a live connection the hub let go of.
pub type CloseConnection = Box<dyn FnOnce() + Send>;

/// The live connection of each online machine: the transport's number for
/// it, and what closes it.
#[derive(Default)]
pub(super) struct Presence {
    live: Mutex<HashMap<AgentId, (u64, CloseConnection)>>,
}

impl HubPairing {
    /// The machine `agent`'s connection `connection` began syncing: it
    /// replaces the machine's older connection, which `close` of that one
    /// closes, and the browsers hear the machine came online when it wasn't.
    pub async fn went_live(&self, agent: AgentId, connection: u64, close: CloseConnection) {
        let replaced = self
            .presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(agent, (connection, close));
        match replaced {
            // Closed outside the lock: what closes it may take its own.
            Some((_, close_older)) => close_older(),
            None => self.tell(self.heard(agent, true)).await,
        }
    }

    /// The machine `agent`'s connection `connection` ended: when it is the
    /// machine's live one, the machine is offline and the browsers hear it.
    pub async fn went_away(&self, agent: AgentId, connection: u64) {
        let ended = {
            let mut live = self
                .presence
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let this_one = live
                .get(&agent)
                .is_some_and(|(held, _)| *held == connection);
            this_one.then(|| live.remove(&agent)).flatten()
        };
        if ended.is_some() {
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

    /// Closes the machine `agent`'s live connection, if the hub holds one:
    /// a person removed the machine (decision 154), which then syncs no more
    /// on a connection it proved before. The browsers heard it was removed.
    pub(super) fn let_go(&self, agent: AgentId) {
        let held = self
            .presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&agent);
        // Closed outside the lock: what closes it may take its own.
        if let Some((_, close)) = held {
            close();
        }
    }

    /// What the browsers hear of `agent` coming online or going offline,
    /// now.
    fn heard(&self, agent: AgentId, online: bool) -> UiEvent {
        let agent = agent.as_uuid().to_string();
        let at = self.clock.now().to_string();
        if online {
            UiEvent::AgentOnline { agent, at }
        } else {
            UiEvent::AgentOffline { agent, at }
        }
    }
}
