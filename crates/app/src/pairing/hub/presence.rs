//! Which approved machines are online: those whose sync connection the hub
//! holds (technical design §7.2; §16, decision 159). The transport says when
//! a machine's connection went live and when it went away; one machine has
//! one live connection, and a newer one replaces the older, which closes.
//! Presence lives in memory only: after a restart every machine is offline
//! until it connects again.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

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
        let _ = (agent, connection, close);
        todo!("a machine's connection goes live")
    }

    /// The machine `agent`'s connection `connection` ended: when it is the
    /// machine's live one, the machine is offline and the browsers hear it.
    pub async fn went_away(&self, agent: AgentId, connection: u64) {
        let _ = (agent, connection);
        todo!("a machine's connection goes away")
    }

    /// Whether the hub holds the machine `agent`'s sync connection.
    pub fn is_online(&self, agent: AgentId) -> bool {
        self.presence
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&agent)
    }
}
