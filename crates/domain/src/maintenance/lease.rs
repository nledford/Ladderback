//! The hub's registry of maintenance owners (technical design §7.3, I1;
//! §16, decision 147): [`RepositoryOwnership`], the aggregate that holds one
//! repository's lease and the epochs given for it. Among the agents paired
//! to a hub, each repository, known by its rustic repository ID, has at most
//! one maintenance owner. An agent claims it by reporting the repository
//! with the request `local`; the first claim is granted a lease of
//! [`LEASE_LIFE`], numbered by an epoch that only rises, and each later
//! report of its holder renews it. Rules only: every time is the hub's, read
//! through the `Clock` port, and whether another agent uses the repository
//! is passed in.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use crate::planning::Maintenance;
use crate::{AgentId, RepositoryId, SignedDuration, Timestamp};

/// How long a lease runs from its grant or its last renewal (§7.3).
pub const LEASE_LIFE: SignedDuration = SignedDuration::from_hours(7 * 24);

/// How long past a lease's expiry the hub still counts it, before it grants
/// the repository to another agent (decision 147). The holder stops at the
/// expiry itself, judged on the hub's time (decision 120, "Time"), so a
/// holder whose clock drifted by less than this since it last heard the hub
/// has stopped before anyone else may start.
pub const LEASE_GRACE: SignedDuration = SignedDuration::from_hours(1);

/// How long the hub counts an agent's report of a repository as that agent
/// using it: as long as a lease runs, so an agent gone for longer stops
/// keeping others from maintaining the repository, as a holder gone for
/// longer stops holding it.
pub const REPORT_LIFE: SignedDuration = LEASE_LIFE;

/// A lease's number among the leases given for one repository: 1 for the
/// first, and each later lease's above every one before it, so a machine
/// can tell a newer grant from an older one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeaseEpoch(NonZeroU64);

impl LeaseEpoch {
    /// The first lease's epoch.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// The epoch `number` names; none for 0.
    pub const fn new(number: u64) -> Option<Self> {
        match NonZeroU64::new(number) {
            Some(number) => Some(Self(number)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// The epoch after this one; none past the last.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// The hub's grant of a repository's maintenance to one agent, until
/// `expires_at` on the hub's clock unless renewed (§7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    pub holder: AgentId,
    pub epoch: LeaseEpoch,
    pub expires_at: Timestamp,
}

impl Lease {
    /// Whether the hub still counts the lease at `now`: until
    /// [`LEASE_GRACE`] past its expiry.
    pub fn binds(&self, now: Timestamp) -> bool {
        before_end(self.expires_at, LEASE_GRACE, now)
    }
}

/// What an agent reports of one repository it uses (§7.2, `ReposSnapshot`):
/// its request, and whether the repository holds another agent's tagged
/// snapshots, which the agent sees and the hub can't.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryReport {
    pub repository: RepositoryId,
    pub request: Maintenance,
    pub holds_other_agents_snapshots: bool,
}

impl RepositoryReport {
    /// Whether another agent uses the repository: `others_report`, another
    /// approved agent reported it within [`REPORT_LIFE`], or the repository
    /// holds another agent's tagged snapshots. Only the hub may maintain a
    /// repository several agents use, since only it sees all their backups.
    pub fn shared(&self, others_report: bool) -> bool {
        others_report || self.holds_other_agents_snapshots
    }
}

/// Whether an agent's report made at `reported_at` still counts at `now`
/// ([`REPORT_LIFE`]).
pub fn report_counts(reported_at: Timestamp, now: Timestamp) -> bool {
    before_end(reported_at, REPORT_LIFE, now)
}

/// Whether `now` comes before `start` plus `span`; always past the last
/// time there is, so what runs that long never ends.
fn before_end(start: Timestamp, span: SignedDuration, now: Timestamp) -> bool {
    start.checked_add(span).ok().is_none_or(|end| now < end)
}

/// One agent's report of every repository it uses: one entry for each
/// repository, ordered by its ID. A report that names a repository twice
/// keeps the last entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepositoryReports(Vec<RepositoryReport>);

impl RepositoryReports {
    pub fn new(entries: impl IntoIterator<Item = RepositoryReport>) -> Self {
        let by_repository: BTreeMap<RepositoryId, RepositoryReport> = entries
            .into_iter()
            .map(|entry| (entry.repository.clone(), entry))
            .collect();
        Self(by_repository.into_values().collect())
    }

    pub fn entries(&self) -> &[RepositoryReport] {
        &self.0
    }
}

/// What the hub decided of an agent's report of one repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The agent claimed it, and was granted this new lease.
    Granted(Lease),
    /// The agent holds the lease, which now runs until its new expiry.
    Renewed(Lease),
    /// The agent holds the lease, and it is not renewed: another agent uses
    /// the repository. It runs out at its expiry, and then nobody maintains
    /// the repository until a person makes the hub its owner (§7.3).
    Unrenewed(Lease),
    /// The agent claimed it and was granted nothing.
    Refused(ClaimRefused),
    /// The report asks for no lease (`hub` or `none`). A lease the agent
    /// holds isn't renewed; it ends when the agent releases it or at its
    /// expiry.
    NoClaim,
}

/// Why a claim was granted nothing (`repo.claim_rejected`, §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRefused {
    /// Another agent holds the repository's lease.
    HeldByAnother(AgentId),
    /// Another agent uses the repository ([`RepositoryReport::shared`]).
    Shared,
    /// Every epoch has been given. A hub never gives that many; only a
    /// tampered database could hold the last.
    EpochsSpent,
}

/// A release refused: the agent holds no lease of that epoch, so nothing
/// changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseRefused;

/// The hub's record of one repository's maintenance owner: the last epoch
/// given for it, and the lease given at that epoch until its holder
/// releases it (§7.3, I1). The lease is kept after it stops binding, and a
/// repository removed and reported again keeps its epochs, so they only
/// rise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryOwnership {
    pub repository: RepositoryId,
    pub last_epoch: Option<LeaseEpoch>,
    pub lease: Option<Lease>,
}

impl RepositoryOwnership {
    /// A repository no lease was ever given for.
    pub const fn new(repository: RepositoryId) -> Self {
        Self {
            repository,
            last_epoch: None,
            lease: None,
        }
    }

    /// The lease that binds at `now`, if any.
    pub fn binding(&self, now: Timestamp) -> Option<&Lease> {
        self.lease.as_ref().filter(|lease| lease.binds(now))
    }

    /// Decides `agent`'s report of this repository, which asks for
    /// `request`, at `now`; `shared` when another agent uses it
    /// ([`RepositoryReport::shared`]).
    ///
    /// A claim (`local`) while another agent's lease binds is refused; one
    /// for a repository another agent uses is granted nothing, and its
    /// holder's lease is no longer renewed. Otherwise the holder's lease is
    /// renewed for [`LEASE_LIFE`] from `now`, never to an earlier expiry
    /// than it had, so a hub clock set back can't shorten a lease its holder
    /// was told of; and with no lease binding, a new one is granted at the
    /// next epoch.
    pub fn claim(
        &mut self,
        agent: AgentId,
        request: Maintenance,
        shared: bool,
        now: Timestamp,
    ) -> ClaimOutcome {
        if request != Maintenance::Local {
            return ClaimOutcome::NoClaim;
        }
        match self.binding(now).copied() {
            Some(lease) if lease.holder != agent => {
                ClaimOutcome::Refused(ClaimRefused::HeldByAnother(lease.holder))
            }
            Some(lease) if shared => ClaimOutcome::Unrenewed(lease),
            Some(lease) => {
                let renewed = Lease {
                    expires_at: lease.expires_at.max(lease_end(now)),
                    ..lease
                };
                self.lease = Some(renewed);
                ClaimOutcome::Renewed(renewed)
            }
            None if shared => ClaimOutcome::Refused(ClaimRefused::Shared),
            None => {
                let next = match self.last_epoch {
                    None => Some(LeaseEpoch::FIRST),
                    Some(last) => last.next(),
                };
                let Some(epoch) = next else {
                    return ClaimOutcome::Refused(ClaimRefused::EpochsSpent);
                };
                let granted = Lease {
                    holder: agent,
                    epoch,
                    expires_at: lease_end(now),
                };
                self.last_epoch = Some(epoch);
                self.lease = Some(granted);
                ClaimOutcome::Granted(granted)
            }
        }
    }

    /// Ends the lease `agent` holds at `epoch`, at its holder's signed
    /// release (`ReleaseAck`, §7.2), so the repository may be granted again
    /// before the lease runs out. A release by another agent or of another
    /// epoch is refused and changes nothing.
    pub fn release(&mut self, agent: AgentId, epoch: LeaseEpoch) -> Result<(), ReleaseRefused> {
        match self.lease {
            Some(lease) if lease.holder == agent && lease.epoch == epoch => {
                self.lease = None;
                Ok(())
            }
            _ => Err(ReleaseRefused),
        }
    }
}

/// The expiry of a lease granted or renewed at `now`: [`LEASE_LIFE`] later,
/// or the last time there is.
fn lease_end(now: Timestamp) -> Timestamp {
    now.checked_add(LEASE_LIFE).unwrap_or(Timestamp::MAX)
}

#[cfg(test)]
mod tests;
