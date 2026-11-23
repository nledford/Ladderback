//! Whether each repository a paired machine uses holds another agent's
//! tagged snapshots, as the machine tells its hub with every report
//! (technical design §7.3, I1; §16, decisions 62 and NN). The hub grants
//! `agent:<id>` only for a repository no other agent uses, and only the
//! machine can see whose snapshots a repository holds.
//!
//! A sync session doesn't open the repository to find out: every listing
//! the daemon runs anyway (the Snapshots page, a preview, a maintenance run)
//! passes through [`NoteListings`], which keeps, for the repository the
//! listing opened, the newest snapshot another agent tagged. Each report
//! judges that time against the `Clock` port's now, so a snapshot stops
//! counting 30 days after it was made without another listing. A repository
//! not listed since the daemon started, or listed under another machine ID,
//! is reported as holding another agent's snapshots: the hub then refuses
//! the claim, which only ever keeps a lease from being granted or renewed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::maintenance::{OtherAgentsSnapshots, RepositoryReport};
use domain::planning::Maintenance;
use domain::{AgentId, OperationId, RepositoryId, Timestamp};

use crate::ports::{
    Clock, ConfigStore, Engine, EngineError, Job, OperationStore, Random, RunningJob,
    SnapshotSummary,
};
use crate::snapshots::ListSnapshots;

/// What the daemon's listings showed of other agents' snapshots, by
/// repository.
#[derive(Debug, Default)]
pub struct OthersSnapshots {
    seen: Mutex<BTreeMap<RepositoryId, Seen>>,
}

/// One repository's last listing: the machine ID it was judged for, and
/// the newest snapshot another agent tagged in it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    agent: AgentId,
    others: OtherAgentsSnapshots,
}

impl OthersSnapshots {
    /// Keeps what a listing of `repository`, run for the machine `agent`
    /// and started at `at` on its clock, showed: `snapshots`, in place of
    /// any earlier listing of it.
    pub fn listed(
        &self,
        repository: RepositoryId,
        agent: AgentId,
        at: Timestamp,
        snapshots: &[SnapshotSummary],
    ) {
        let _ = (&self.seen, repository, agent, at, snapshots);
        todo!()
    }

    /// Whether the machine `agent` should list `repository` before its next
    /// sync, at `now` on its clock: it has no listing standing.
    pub fn due(&self, repository: &RepositoryId, agent: &AgentId, now: Timestamp) -> bool {
        let _ = (repository, agent, now);
        todo!()
    }

    /// What the machine `agent` reports of `repository`, which it asks
    /// `request` of, at `now` on its clock: whether it holds another
    /// agent's recent snapshots, as its last listing showed. One not
    /// listed, or listed for another machine ID, does.
    pub fn report(
        &self,
        repository: RepositoryId,
        request: Maintenance,
        agent: &AgentId,
        now: Timestamp,
    ) -> RepositoryReport {
        let _ = (repository, request, agent, now);
        todo!()
    }
}

/// An engine that notes, of each listing it runs, what [`OthersSnapshots`]
/// keeps: the repository the listing opened and the snapshots it found.
/// Every event passes on as it came.
pub struct NoteListings {
    inner: Arc<dyn Engine>,
    seen: Arc<OthersSnapshots>,
    agent: AgentId,
}

impl NoteListings {
    /// Runs every job on `inner`, keeping in `seen` what its listings show,
    /// judged for the machine `agent`, as of `clock`'s time at each
    /// listing's start.
    pub fn new(
        inner: Arc<dyn Engine>,
        seen: Arc<OthersSnapshots>,
        agent: AgentId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let _ = clock;
        Self { inner, seen, agent }
    }
}

/// The listings a paired machine's sync runs before each session: of each
/// repository it asks to maintain here that has no listing standing.
pub struct ListingsDue {
    seen: Arc<OthersSnapshots>,
}

impl ListingsDue {
    /// Lists with `listing`, for the machine `agent`, each repository
    /// `config` asks to maintain here that `seen` has no listing standing
    /// of, unless `operations` has a maintenance of it running.
    pub fn new(
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        seen: Arc<OthersSnapshots>,
        listing: ListSnapshots,
        random: Arc<dyn Random>,
        agent: AgentId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let _ = (config, operations, listing, random, agent, clock);
        Self { seen }
    }

    /// Where these listings are noted: what the sync reports of each
    /// repository.
    pub fn seen(&self) -> &Arc<OthersSnapshots> {
        &self.seen
    }

    /// Lists, once each, every repository due; a failed listing is warned
    /// of once and tried again at the next call.
    pub async fn list(&self) {
        todo!()
    }
}

#[async_trait]
impl Engine for NoteListings {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let _ = (&self.inner, &self.seen, self.agent, op, job);
        todo!()
    }
}

#[cfg(test)]
mod tests;
