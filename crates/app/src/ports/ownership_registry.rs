use async_trait::async_trait;
use domain::maintenance::{
    ClaimOutcome, HubRecovery, LeaseEpoch, ProvenFloor, ReleaseRefused, RepositoryOwnership,
    RepositoryReports, StartWindow,
};
use domain::{AgentId, RepositoryId, Timestamp};

use super::StoreError;

/// What the hub decided of one repository in an agent's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimDecision {
    pub repository: RepositoryId,
    pub outcome: ClaimOutcome,
}

/// The hub's registry of maintenance owners (technical design §7.3, I1):
/// each repository's [`RepositoryOwnership`], and the last report of each
/// approved agent, kept in its database. Times are the hub's, kept to the
/// millisecond.
#[async_trait]
pub trait OwnershipRegistry: Send + Sync {
    /// Takes `reports` as every repository `agent` uses, in place of its
    /// last report, and decides each with
    /// [`RepositoryOwnership::claim_after_start`] in `window` at `now`,
    /// shared when another agent's report of it still counts
    /// (`domain::maintenance::report_counts`) or the report says so,
    /// keeping what it decided. Each repository is decided and kept in one
    /// step with the reports it was decided on, so of two claims that
    /// race, one is granted and the other sees it (I1).
    ///
    /// First it hears `proven`, the floors of the report whose proof the
    /// hub checked ([`HubRecovery::hear`]), in the same step: once any
    /// lies above what the hub counts, the hub is in recovery mode, and
    /// every repository is decided with [`HubRecovery::decide`] instead,
    /// changing no ownership, until [`end_recovery`](Self::end_recovery).
    ///
    /// Of an agent removed since the hub read it, it keeps nothing and
    /// decides nothing: an empty list, no ownership changed.
    async fn report(
        &self,
        agent: AgentId,
        reports: &RepositoryReports,
        proven: &[ProvenFloor],
        now: Timestamp,
        window: &StartWindow,
    ) -> Result<Vec<ClaimDecision>, StoreError>;

    /// The floors that hold the hub in recovery mode: none outside it.
    async fn recovery(&self) -> Result<HubRecovery, StoreError>;

    /// Ends recovery mode, at a person's confirmation that the old hub is
    /// stopped or gone: each kept floor is taken with
    /// [`RepositoryOwnership::recover`] and the floors are dropped, in one
    /// step. Whether the hub was in recovery mode.
    async fn end_recovery(&self) -> Result<bool, StoreError>;

    /// Every repository each agent's last report named, however long ago
    /// the hub took it: what the hub's start window waits on (§7.3).
    async fn reported_uses(&self) -> Result<Vec<(AgentId, RepositoryId)>, StoreError>;

    /// Ends the lease `agent` holds on `repository` at `epoch`
    /// ([`RepositoryOwnership::release`]), in one step.
    async fn release(
        &self,
        agent: AgentId,
        repository: &RepositoryId,
        epoch: LeaseEpoch,
    ) -> Result<Result<(), ReleaseRefused>, StoreError>;

    /// The repository's ownership: a new one when no agent ever reported it.
    async fn ownership(&self, repository: &RepositoryId)
    -> Result<RepositoryOwnership, StoreError>;
}
