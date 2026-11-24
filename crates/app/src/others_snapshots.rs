//! Whether each repository a paired machine uses holds another agent's
//! tagged snapshots, as the machine tells its hub with every report
//! (technical design §7.3, I1; §16, decisions 62 and 156). The hub grants
//! `agent:<id>` only for a repository no other agent uses, and only the
//! machine can see whose snapshots a repository holds.
//!
//! Every listing the daemon runs (the Snapshots page, a preview, a prune's
//! look for other hosts) passes through [`NoteListings`], which keeps, for
//! the repository the listing opened, when it was listed and the newest
//! snapshot another agent tagged. Before each sync session,
//! [`ListingsDue`] lists each repository the machine asks to maintain that
//! has no listing standing: none since the daemon started, one for another
//! machine ID, or one older than
//! [`LISTING_LASTS`](domain::maintenance::LISTING_LASTS). Each report
//! judges the newest snapshot's time against the `Clock` port's now, so a
//! snapshot stops counting 30 days after it was made without another
//! listing. A repository with no listing standing is reported as holding
//! another agent's snapshots: the hub then refuses the claim, which only
//! ever keeps a lease from being granted or renewed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domain::maintenance::{OtherAgentsSnapshots, RepositoryReport, listing_stands};
use domain::planning::Maintenance;
use domain::{AgentId, OperationId, RepositoryId, Timestamp, Uuid};
use futures::StreamExt;

use crate::ports::{
    Clock, ConfigStore, Engine, EngineError, EngineEvent, EngineResult, Job, OperationStore,
    Random, RunningJob, SnapshotSummary,
};
use crate::snapshots::ListSnapshots;

/// What the daemon's listings showed of other agents' snapshots, by
/// repository.
#[derive(Debug, Default)]
pub struct OthersSnapshots {
    seen: Mutex<BTreeMap<RepositoryId, Seen>>,
}

/// One repository's last listing: the machine ID it was judged for, when
/// it started on the machine's clock, and the newest snapshot another agent
/// tagged in it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    agent: AgentId,
    at: Timestamp,
    others: OtherAgentsSnapshots,
}

impl OthersSnapshots {
    /// Keeps what a listing of `repository`, run for the machine `agent`
    /// and started `at` on its clock, showed: `snapshots`, in place of any
    /// earlier listing of it.
    pub fn listed(
        &self,
        repository: RepositoryId,
        agent: AgentId,
        at: Timestamp,
        snapshots: &[SnapshotSummary],
    ) {
        let others = OtherAgentsSnapshots::of(
            &agent,
            snapshots
                .iter()
                .map(|snapshot| (snapshot.time, snapshot.tags.as_slice())),
        );
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(repository, Seen { agent, at, others });
    }

    /// The listing of `repository` that stands for the machine `agent` at
    /// `now`: its last, when it was judged for `agent` and is no older than
    /// [`LISTING_LASTS`](domain::maintenance::LISTING_LASTS).
    fn standing(
        &self,
        repository: &RepositoryId,
        agent: &AgentId,
        now: Timestamp,
    ) -> Option<OtherAgentsSnapshots> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(repository)
            .filter(|seen| seen.agent == *agent && listing_stands(seen.at, now))
            .map(|seen| seen.others)
    }

    /// Whether the machine `agent` has to list `repository` at `now` to
    /// know whose snapshots it holds: no listing of it stands.
    pub fn due(&self, repository: &RepositoryId, agent: &AgentId, now: Timestamp) -> bool {
        self.standing(repository, agent, now).is_none()
    }

    /// What the machine `agent` reports of `repository`, which it asks
    /// `request` of, at `now` on its clock: whether it holds another
    /// agent's recent snapshots, as its last listing showed. One with no
    /// listing standing does.
    pub fn report(
        &self,
        repository: RepositoryId,
        request: Maintenance,
        agent: &AgentId,
        now: Timestamp,
    ) -> RepositoryReport {
        let holds_other_agents_snapshots = self
            .standing(&repository, agent, now)
            .is_none_or(|others| others.recent_at(now));
        RepositoryReport {
            repository,
            request,
            holds_other_agents_snapshots,
            floor: None,
        }
    }
}

/// An engine that notes, of each listing it runs, what [`OthersSnapshots`]
/// keeps: the repository the listing opened, when it opened it, and the
/// snapshots it found. Every event passes on as it came.
pub struct NoteListings {
    inner: Arc<dyn Engine>,
    seen: Arc<OthersSnapshots>,
    agent: AgentId,
    clock: Arc<dyn Clock>,
}

impl NoteListings {
    /// Runs every job on `inner`, keeping in `seen` what its listings show,
    /// judged for the machine `agent`, as of their start on `clock`.
    pub fn new(
        inner: Arc<dyn Engine>,
        seen: Arc<OthersSnapshots>,
        agent: AgentId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            inner,
            seen,
            agent,
            clock,
        }
    }
}

#[async_trait]
impl Engine for NoteListings {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        if !matches!(job, Job::Ls(_)) {
            return self.inner.run(op, job).await;
        }
        let running = self.inner.run(op, job).await?;
        let seen = Arc::clone(&self.seen);
        let agent = self.agent;
        let clock = Arc::clone(&self.clock);
        // The repository the listing opened, as its start reports it: the
        // one its snapshots are of, and when it opened it, so a snapshot
        // made while the listing ran is never counted as seen.
        let mut opened = None;
        let events = running
            .events
            .inspect(move |event| match event {
                EngineEvent::Started { repository, .. } => {
                    opened = repository.clone().map(|id| (id, clock.now()));
                }
                EngineEvent::Succeeded(EngineResult::Snapshots(snapshots)) => {
                    if let Some((repository, at)) = opened.take() {
                        seen.listed(repository, agent, at, snapshots);
                    }
                }
                _ => {}
            })
            .boxed();
        Ok(RunningJob {
            events,
            control: running.control,
        })
    }
}

/// The listings a paired machine runs before each sync session (decision
/// 156), so that its report of a repository it asks to maintain says what
/// the repository holds rather than the "holds another agent's snapshots"
/// of one it hasn't listed.
pub struct ListingsDue {
    config: Arc<dyn ConfigStore>,
    operations: Arc<dyn OperationStore>,
    seen: Arc<OthersSnapshots>,
    listing: ListSnapshots,
    random: Arc<dyn Random>,
    agent: AgentId,
    clock: Arc<dyn Clock>,
    /// The repositories whose last listing here failed: each failure is
    /// warned of once, until a listing of it succeeds.
    failing: Mutex<BTreeSet<RepositoryId>>,
}

impl ListingsDue {
    /// Listings of the repositories `config` asks to maintain here, through
    /// `listing`, whose engine notes them in `seen`, each under an
    /// operation ID drawn from `random`, unless `operations` has a
    /// maintenance of the repository running; judged for the machine
    /// `agent` on `clock`.
    pub fn new(
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        seen: Arc<OthersSnapshots>,
        listing: ListSnapshots,
        random: Arc<dyn Random>,
        agent: AgentId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            operations,
            seen,
            listing,
            random,
            agent,
            clock,
            failing: Mutex::default(),
        }
    }

    /// Where these listings are noted: what the sync reports of each
    /// repository.
    pub fn seen(&self) -> &Arc<OthersSnapshots> {
        &self.seen
    }

    /// Lists, once each, every repository the configuration asks to
    /// maintain here (`local`) whose ID it records and that has no listing
    /// standing on the clock now: none of it, one for another machine ID,
    /// or one older than
    /// [`LISTING_LASTS`](domain::maintenance::LISTING_LASTS). None is
    /// listed while a maintenance of it runs here. A listing that fails is
    /// tried again at the next call; nothing here fails the sync, whose
    /// report then says the repository holds another agent's snapshots.
    pub async fn list(&self) {
        let config = match self.config.load().await {
            Ok(config) => config,
            // The sync's own read of the configuration fails the session
            // and says why.
            Err(_) => return,
        };
        for (alias, entry) in config.repositories() {
            let Some(id) = &entry.id else { continue };
            if entry.maintenance != Maintenance::Local
                || !self.seen.due(id, &self.agent, self.clock.now())
            {
                continue;
            }
            // A repository a maintenance holds is listed at a later sync,
            // once the maintenance has ended (decision 156).
            match self.operations.running_maintenance(Some(id)).await {
                Ok(None) => {}
                Ok(Some(_)) => continue,
                Err(error) => {
                    self.failed(id, &format!("{error:?}"));
                    continue;
                }
            }
            let op = match self.random.token() {
                Ok(token) => {
                    let mut bytes = [0; 16];
                    for (byte, drawn) in bytes.iter_mut().zip(token) {
                        *byte = drawn;
                    }
                    OperationId::from_uuid(Uuid::from_bytes(bytes))
                }
                Err(error) => {
                    self.failed(id, &format!("{error:?}"));
                    continue;
                }
            };
            match self.listing.list(op, alias, entry).await {
                Ok(_) => {
                    self.failing
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(id);
                }
                Err(error) => self.failed(id, &format!("{error:?}")),
            }
        }
    }

    /// Warns that the listing of `repository` failed for `why`, unless its
    /// last listing failed too.
    fn failed(&self, repository: &RepositoryId, why: &str) {
        let first = self
            .failing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(repository.clone());
        if first {
            log::warn!(
                "repository {} couldn't be listed before the sync with the hub, which is told \
                 that it may hold another machine's snapshots until a listing succeeds: {why}",
                repository.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests;
