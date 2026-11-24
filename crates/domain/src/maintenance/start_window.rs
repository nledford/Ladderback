//! The hub's start window (technical design §7.3, "A rewound hub"; §16,
//! decisions 137 and 157): after any start of the hub, a restore from a
//! backup or an ordinary restart, no repository changes owner until every
//! agent that used it at the start has reported since, or [`START_WINDOW`]
//! has passed on the hub's monotonic clock. A hub restored from a backup
//! can't see that a lease was granted after the backup until the holder
//! reports, and a lease renewed just before the start can run that long.
//! Rules only: what the hub took and when are passed in.

use std::collections::{BTreeMap, BTreeSet};

use super::{LEASE_GRACE, LEASE_LIFE};
use crate::{AgentId, RepositoryId, SignedDuration};

/// How long after the start the window holds a repository whose agents
/// haven't all reported: a lease's life and the grace past it, so a lease
/// renewed just before the start, which the hub may not know of, has
/// stopped binding first (decision 157).
pub const START_WINDOW: SignedDuration = LEASE_LIFE.saturating_add(LEASE_GRACE);

/// The agents the hub still waits for since its start, for each repository
/// it holds: those whose last report before the start named it and that
/// haven't reported since. A repository no agent used at the start isn't
/// held, and the window holds nothing once [`START_WINDOW`] has passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartWindow {
    awaited: BTreeMap<RepositoryId, BTreeSet<AgentId>>,
}

impl StartWindow {
    /// The window of a hub that has just started, from every repository
    /// each agent's last report named.
    pub fn new(uses: impl IntoIterator<Item = (AgentId, RepositoryId)>) -> Self {
        let _ = uses;
        todo!()
    }

    /// `agent` reported since the start: the window waits for it no more.
    pub fn heard(&mut self, agent: AgentId) {
        let _ = agent;
        todo!()
    }

    /// `since_start` of monotonic time has passed since the start: at
    /// [`START_WINDOW`] the window holds nothing more.
    pub fn pass(&mut self, since_start: SignedDuration) {
        let _ = since_start;
        todo!()
    }

    /// Whether the window still holds `repository`: an agent that used it
    /// at the start hasn't reported since.
    pub fn holds(&self, repository: &RepositoryId) -> bool {
        let _ = (repository, &self.awaited);
        todo!()
    }
}

#[cfg(test)]
mod tests;
