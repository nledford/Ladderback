//! The hub's start window (technical design §7.3, "A rewound hub"; §16,
//! decisions 137, 157 and 178): after any start of the hub, a restore from
//! a backup or an ordinary restart, no repository changes owner until every
//! agent that used it at the start, and every agent approved at the start,
//! has reported since, or [`START_WINDOW`] has passed on the hub's
//! monotonic clock. A hub restored from a backup, or one whose last commits
//! a power cut took back, can't see a lease it granted until the holder
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
/// haven't reported since. A repository no agent's last report named is
/// held until every agent whose last report named any has reported: a hub
/// restored from a backup can't tell which of them began using it after
/// the backup. Every repository is held while an agent approved at the
/// start hasn't reported since: the grant it holds, and the report that won
/// it, may be what the hub lost (decision 178). The window holds nothing
/// once [`START_WINDOW`] has passed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartWindow {
    awaited: BTreeMap<RepositoryId, BTreeSet<AgentId>>,
    /// Every repository an agent's last report before the start named.
    listed: BTreeSet<RepositoryId>,
    /// Every agent whose last report before the start named a repository,
    /// that hasn't reported since.
    unheard: BTreeSet<AgentId>,
    /// Every agent approved at the start that hasn't reported since.
    approved: BTreeSet<AgentId>,
}

impl StartWindow {
    /// The window of a hub that has just started, from every repository
    /// each agent's last report named, and every agent `approved` at the
    /// start.
    pub fn new(
        uses: impl IntoIterator<Item = (AgentId, RepositoryId)>,
        approved: impl IntoIterator<Item = AgentId>,
    ) -> Self {
        let mut window = Self {
            approved: approved.into_iter().collect(),
            ..Self::default()
        };
        for (agent, repository) in uses {
            window.listed.insert(repository.clone());
            window.unheard.insert(agent);
            window.awaited.entry(repository).or_default().insert(agent);
        }
        window
    }

    /// `agent` reported since the start: the window waits for it no more.
    pub fn heard(&mut self, agent: AgentId) {
        self.unheard.remove(&agent);
        self.approved.remove(&agent);
        self.awaited.retain(|_, agents| {
            agents.remove(&agent);
            !agents.is_empty()
        });
    }

    /// `since_start` of monotonic time has passed since the start: at
    /// [`START_WINDOW`] the window holds nothing more.
    pub fn pass(&mut self, since_start: SignedDuration) {
        if since_start >= START_WINDOW {
            self.awaited.clear();
            self.unheard.clear();
            self.approved.clear();
        }
    }

    /// Whether the window still holds `repository`: an agent approved at
    /// the start, or one that used it then, hasn't reported since, or no
    /// agent's last report named it and one that named any hasn't reported
    /// since.
    pub fn holds(&self, repository: &RepositoryId) -> bool {
        !self.approved.is_empty()
            || self.awaited.contains_key(repository)
            || (!self.unheard.is_empty() && !self.listed.contains(repository))
    }
}

#[cfg(test)]
mod tests;
