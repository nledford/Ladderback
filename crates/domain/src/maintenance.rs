//! Which snapshots a forget run may remove, and who may run maintenance at
//! all (technical design §7.3). [`Ownership`] is invariant I1 on an agent
//! with no hub: only a repository whose entry asks for `local` is forgotten,
//! pruned, checked or adopted from here. The partition into managed,
//! orphaned and unmanaged groups is the maintenance owner's own decision,
//! and it is the whole of invariant I6: forget only ever removes snapshots a
//! plan of this agent owns, by tag. Untagged snapshots are never touched,
//! and a tagged group whose plan or agent this configuration doesn't know is
//! left alone and listed as orphaned, for the user to adopt, give a policy,
//! or delete explicitly.
//!
//! Which of a managed group's snapshots the plan's policy lets go is not
//! decided here: retention is evaluated with rustic's own rules, through the
//! `Retention` port in `app` (§3.2).

use std::collections::{BTreeMap, BTreeSet};

use crate::execution::plan_tags;
use crate::planning::{AgentSettings, Maintenance, Plan, RetentionPolicy};
use crate::{AgentId, Offset, OperationId, PlanId, SnapshotId, Timestamp};

mod held;
mod lease;
mod other_hosts;
mod others_snapshots;
#[cfg(test)]
mod properties;
mod start_window;
mod stats;

pub use held::{EpochFloor, EpochLower, Grant, HeldLease, HeldLeases, NoLease, RepoGrants};
pub use lease::{
    ClaimOutcome, ClaimRefused, LEASE_GRACE, LEASE_LIFE, Lease, LeaseEpoch, REPORT_LIFE,
    ReleaseRefused, RepositoryOwnership, RepositoryReport, RepositoryReports, report_counts,
};
pub use other_hosts::{MAX_NAMED_OTHER_HOSTS, OtherHosts};
pub use others_snapshots::{LISTING_LASTS, OtherAgentsSnapshots, listing_stands};
pub use start_window::{START_WINDOW, StartWindow};
pub use stats::{READINGS_KEPT_FOR, RepositoryStats, StatsReading};

/// One snapshot as the owner sees it: what a listing reports, minus what
/// only the engine needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: SnapshotId,
    /// When the snapshot was taken.
    pub time: Timestamp,
    /// The offset of the clock that took it: with `time`, the snapshot time
    /// (§3.1).
    pub offset: Offset,
    pub hostname: String,
    pub label: String,
    /// The snapshot's tags, as the listing reports them.
    pub tags: Vec<String>,
    pub paths: Vec<String>,
}

impl Snapshot {
    /// The plan this snapshot's `ladderback-plan:` tags name.
    pub fn plan(&self) -> OwnerTag<'_> {
        self.tagged_by(plan_tags::PLAN_TAG_PREFIX)
    }

    /// The agent this snapshot's `ladderback-agent:` tags name.
    pub fn agent(&self) -> OwnerTag<'_> {
        self.tagged_by(plan_tags::AGENT_TAG_PREFIX)
    }

    fn tagged_by(&self, prefix: &str) -> OwnerTag<'_> {
        let mut texts = self.tags.iter().filter_map(|tag| tag.strip_prefix(prefix));
        match texts.next() {
            None => OwnerTag::Absent,
            Some(first) if texts.all(|text| text == first) => OwnerTag::One(first),
            Some(_) => OwnerTag::Disagree,
        }
    }
}

/// What a snapshot's owner tags of one kind, `ladderback-agent:` or
/// `ladderback-plan:`, say about who owns it. Tags are compared exactly, so
/// two that differ only in case or spacing disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerTag<'a> {
    /// The snapshot carries no tag of the kind.
    Absent,
    /// Every tag of the kind carries this text, however many there are.
    One(&'a str),
    /// Two tags of the kind carry different texts, as a hand edit can
    /// leave them: they name no owner, so no plan owns the snapshot and it
    /// is orphaned, whichever tag comes first (I6).
    Disagree,
}

impl<'a> OwnerTag<'a> {
    /// The text the tags agree on, if they carry one.
    pub fn named(self) -> Option<&'a str> {
        match self {
            OwnerTag::One(text) => Some(text),
            OwnerTag::Absent | OwnerTag::Disagree => None,
        }
    }
}

/// A plan of this agent's configuration, with the policy its snapshots keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedPlan {
    pub plan: PlanId,
    pub policy: RetentionPolicy,
}

/// The snapshots one plan of this agent owns, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedGroup {
    pub plan: PlanId,
    pub snapshots: Vec<Snapshot>,
}

/// Snapshots a `ladderback-` tag owns that no plan of this agent does:
/// another agent's plan, a plan this configuration no longer has, or owner
/// tags that disagree on whose they are. Forget leaves them alone (I6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanedGroup {
    /// The text the agent tags agree on and the text the plan tags agree
    /// on, each absent when the group's snapshots carry no such tag or two
    /// that disagree ([`OwnerTag`]).
    pub agent: Option<String>,
    pub plan: Option<String>,
    pub snapshots: Vec<Snapshot>,
}

/// Untagged snapshots, grouped as the repository's `group-by` groups them
/// (host, label, paths by default). Forget never touches them (I6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmanagedGroup {
    pub hostname: String,
    pub label: String,
    pub paths: Vec<String>,
    pub snapshots: Vec<Snapshot>,
}

/// What a forget run found in a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Partition {
    /// One group per plan of this agent that has snapshots, the plan's own
    /// order, each newest first.
    pub managed: Vec<ManagedGroup>,
    /// Tagged groups no plan of this agent owns, ordered by agent then plan.
    pub orphaned: Vec<OrphanedGroup>,
    /// The untagged groups, ordered by host, label then paths.
    pub unmanaged: Vec<UnmanagedGroup>,
}

impl Partition {
    /// Every snapshot the partition saw, once each.
    pub fn len(&self) -> usize {
        self.managed
            .iter()
            .map(|g| g.snapshots.len())
            .sum::<usize>()
            + self
                .orphaned
                .iter()
                .map(|g| g.snapshots.len())
                .sum::<usize>()
            + self
                .unmanaged
                .iter()
                .map(|g| g.snapshots.len())
                .sum::<usize>()
    }

    /// Whether the partition saw no snapshot at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Partitions a repository's snapshots for a forget run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForgetSelection;

impl ForgetSelection {
    /// Splits `snapshots` by who owns them: the plans of this agent, another
    /// owner (`orphaned`), or nobody (`unmanaged`). A snapshot with no
    /// `ladderback-` tag is unmanaged whatever else it carries; a snapshot
    /// whose agent tags agree on this agent and whose plan tags agree on a
    /// plan in `plans` is managed; anything else tagged, a snapshot whose
    /// owner tags disagree among them, is orphaned. Each group's snapshots
    /// are newest first.
    pub fn partition(snapshots: &[Snapshot], agent: &AgentId, plans: &[ManagedPlan]) -> Partition {
        let own = agent.as_uuid().hyphenated().to_string();
        let mut partition = Partition::default();
        // Managed groups follow the plans' own order; each group's
        // snapshots are newest first.
        for plan in plans {
            let mut group: Vec<Snapshot> = snapshots
                .iter()
                .filter(|snapshot| {
                    snapshot.agent() == OwnerTag::One(own.as_str())
                        && snapshot.plan() == OwnerTag::One(plan.plan.as_str())
                })
                .cloned()
                .collect();
            if !group.is_empty() {
                sort_newest_first(&mut group);
                partition.managed.push(ManagedGroup {
                    plan: plan.plan.clone(),
                    snapshots: group,
                });
            }
        }
        let mut orphaned = Vec::new();
        let mut unmanaged = Vec::new();
        for snapshot in snapshots {
            let tagged =
                snapshot.agent() != OwnerTag::Absent || snapshot.plan() != OwnerTag::Absent;
            let managed = snapshot.agent() == OwnerTag::One(own.as_str())
                && plans
                    .iter()
                    .any(|plan| snapshot.plan() == OwnerTag::One(plan.plan.as_str()));
            if !tagged {
                unmanaged.push(snapshot.clone());
            } else if !managed {
                orphaned.push(snapshot.clone());
            }
        }
        partition.orphaned = grouped_by_tag(orphaned);
        partition.unmanaged = grouped_by_unmanaged_key(unmanaged);
        partition
    }
}

/// Newest first; snapshots taken at the same time in ID order, so a
/// repository that lists them in another order sorts them the same.
fn sort_newest_first(snapshots: &mut [Snapshot]) {
    snapshots.sort_by(|one, other| {
        other
            .time
            .cmp(&one.time)
            .then_with(|| one.id.cmp(&other.id))
    });
}

/// Tagged snapshots by the texts their agent and plan tags agree on, ordered
/// by agent then plan, each group newest first.
fn grouped_by_tag(snapshots: Vec<Snapshot>) -> Vec<OrphanedGroup> {
    let mut groups: BTreeMap<(Option<String>, Option<String>), Vec<Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        groups
            .entry((
                snapshot.agent().named().map(str::to_owned),
                snapshot.plan().named().map(str::to_owned),
            ))
            .or_default()
            .push(snapshot);
    }
    groups
        .into_iter()
        .map(|((agent, plan), mut snapshots)| {
            sort_newest_first(&mut snapshots);
            OrphanedGroup {
                agent,
                plan,
                snapshots,
            }
        })
        .collect()
}

/// Untagged snapshots by `(host, label, paths)`, ordered by that key, each
/// group newest first.
fn grouped_by_unmanaged_key(snapshots: Vec<Snapshot>) -> Vec<UnmanagedGroup> {
    let mut groups: BTreeMap<(String, String, Vec<String>), Vec<Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        groups
            .entry((
                snapshot.hostname.clone(),
                snapshot.label.clone(),
                snapshot.paths.clone(),
            ))
            .or_default()
            .push(snapshot);
    }
    groups
        .into_iter()
        .map(|((hostname, label, paths), mut snapshots)| {
            sort_newest_first(&mut snapshots);
            UnmanagedGroup {
                hostname,
                label,
                paths,
                snapshots,
            }
        })
        .collect()
}

/// Why an adoption found nothing to rewrite: the group names no plan of this
/// agent, or more than one plan matches it and the owner must say which.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdoptionRefusal {
    /// No plan of this agent matches the group's host, label and sources,
    /// or the listing holds no such group.
    NoMatch,
    /// Every plan named here matches the group, and there is more than one:
    /// which one the history belongs to is the owner's decision, not a
    /// rule's, so nothing is written (the adoption half of I6).
    Ambiguous { plans: Vec<PlanId> },
}

/// Partitions adoption's input: which group an adoption rewrites, and which
/// plan of this agent it becomes history of (§7.3, step 3). Nothing is
/// decided about tags or trees here; the group's own snapshots are the whole
/// of what may be rewritten.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdoptSelection;

impl AdoptSelection {
    /// The orphaned group tagged `(agent, plan)`, as the listing grouped
    /// it: the history of one owner's one plan, which this configuration no
    /// longer knows.
    pub fn orphaned<'a>(
        partition: &'a Partition,
        agent: &str,
        plan: &str,
    ) -> Option<&'a OrphanedGroup> {
        partition.orphaned.iter().find(|group| {
            group.agent.as_deref() == Some(agent) && group.plan.as_deref() == Some(plan)
        })
    }

    /// The unmanaged group whose host, label and paths equal `into`'s
    /// snapshot host, label and sources (the host is [`Plan::snapshot_host`],
    /// else `machine`, the name the engine records when neither the plan nor
    /// `agent` names one, and the label an empty one when the plan has
    /// none), and every other plan that would claim the same group. The
    /// paths are compared exactly, in order, so the caller hands in plans
    /// whose sources are already normalized the way the engine records a
    /// snapshot's paths (decision 26).
    pub fn unmanaged<'a>(
        partition: &'a Partition,
        agent: &AgentSettings,
        machine: &str,
        into: &'a Plan,
        plans: &'a [Plan],
    ) -> Result<&'a UnmanagedGroup, AdoptionRefusal> {
        let key = |plan: &Plan| adoption_key(plan, agent, machine);
        let mine = key(into);
        let Some(group) = partition.unmanaged.iter().find(|group| {
            (group.hostname.as_str(), &group.label, &group.paths) == (&mine.0, &mine.1, &mine.2)
        }) else {
            return Err(AdoptionRefusal::NoMatch);
        };
        let also: Vec<PlanId> = plans
            .iter()
            .filter(|plan| plan.id != into.id && key(plan) == mine)
            .map(|plan| plan.id.clone())
            .collect();
        if also.is_empty() {
            Ok(group)
        } else {
            let mut ambiguous = vec![into.id.clone()];
            ambiguous.extend(also);
            Err(AdoptionRefusal::Ambiguous { plans: ambiguous })
        }
    }

    /// The plans that would adopt each of `groups`: every plan whose
    /// snapshot host (`machine` when neither the plan nor `agent` names
    /// one), label and sources equal the group's, in `plans`' order, one list
    /// per group in the groups' order. None when no plan would, one when it
    /// is clear which does, several when which one takes the history is the
    /// person's decision (the same key `unmanaged` matches by, so a group
    /// listed with one claimant is one an adoption into that plan finds).
    pub fn claimants(
        groups: &[UnmanagedGroup],
        agent: &AgentSettings,
        machine: &str,
        plans: &[Plan],
    ) -> Vec<Vec<PlanId>> {
        let keys: Vec<_> = plans
            .iter()
            .map(|plan| (&plan.id, adoption_key(plan, agent, machine)))
            .collect();
        groups
            .iter()
            .map(|group| {
                keys.iter()
                    .filter(|(_, (host, label, paths))| {
                        *host == group.hostname && *label == group.label && *paths == group.paths
                    })
                    .map(|(id, _)| (*id).clone())
                    .collect()
            })
            .collect()
    }
}

/// The host, label and sources an adoption matches a plan by, as the engine
/// records them on a snapshot of the plan: its snapshot host
/// ([`Plan::snapshot_host`]), or `machine` when that names none, its label
/// or an empty one, and its sources in order.
fn adoption_key(
    plan: &Plan,
    agent: &AgentSettings,
    machine: &str,
) -> (String, String, Vec<String>) {
    (
        plan.snapshot_host(agent)
            .map_or(machine, |host| host.as_str())
            .to_owned(),
        plan.label.clone().unwrap_or_default(),
        plan.sources
            .iter()
            .map(|source| source.as_str().to_owned())
            .collect(),
    )
}

/// A fingerprint of the snapshots a forget or an adoption acts on. A preview
/// hands it out and a start hands it back, so a run that would act on other
/// snapshots than the preview showed — a backup landed in between — is
/// refused instead of removing what nobody reviewed (I6). It guards against
/// change, not against an adversary, so a short, fixed hash is enough: the
/// FNV-1a 64-bit hash of the snapshot IDs, sorted and joined by newlines,
/// written as 16 lower-case hex digits. An adoption's token also covers the
/// plan the history goes to: the text it hashes starts with the line
/// `into:<plan>`, which no ID can be, so no forget's token is an adoption's
/// and a preview of one destination can't start another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionToken(String);

impl SelectionToken {
    /// The token of the set `ids` names, whatever the order they come in.
    pub fn of<'a>(ids: impl IntoIterator<Item = &'a SnapshotId>) -> Self {
        Self::hashed(None, ids)
    }

    /// The token of an adoption of the set `ids` names into the plan `into`.
    pub fn of_adoption<'a>(into: &PlanId, ids: impl IntoIterator<Item = &'a SnapshotId>) -> Self {
        Self::hashed(Some(into), ids)
    }

    fn hashed<'a>(into: Option<&PlanId>, ids: impl IntoIterator<Item = &'a SnapshotId>) -> Self {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut sorted: Vec<&str> = ids.into_iter().map(SnapshotId::as_str).collect();
        sorted.sort_unstable();
        sorted.dedup();
        let destination = into.into_iter().flat_map(|plan| {
            b"into:"
                .iter()
                .copied()
                .chain(plan.as_str().bytes())
                .chain(std::iter::once(b'\n'))
        });
        let listed = sorted
            .iter()
            .enumerate()
            .flat_map(|(n, id)| (n > 0).then_some(b'\n').into_iter().chain(id.bytes()));
        let hash = destination.chain(listed).fold(OFFSET, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(PRIME)
        });
        Self(format!("{hash:016x}"))
    }

    /// The token `text` writes: exactly 16 lower-case hex digits, nothing
    /// else.
    pub fn parse(text: &str) -> Option<Self> {
        let written =
            text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        written.then(|| Self(text.to_owned()))
    }

    /// The token as it is written on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a forget would do to one plan's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanForget {
    pub plan: PlanId,
    /// How many snapshots the plan owns.
    pub total: usize,
    /// The plan's own snapshots its policy lets go, newest first.
    pub remove: Vec<Snapshot>,
}

/// How many groups and snapshots a forget leaves alone of one kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub groups: usize,
    pub snapshots: usize,
}

/// The history a forget leaves alone (I6): tagged groups no plan of this
/// agent owns, and untagged ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeftAlone {
    pub orphaned: Tally,
    pub unmanaged: Tally,
}

/// A forget, decided and not yet done: what each plan's policy lets go and
/// what is left alone. A run and its preview are made from this one value,
/// so what a person reviews is what a run removes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetPreview {
    /// One entry per plan of this agent that has snapshots, in the plans'
    /// own order.
    pub plans: Vec<PlanForget>,
    pub left_alone: LeftAlone,
}

impl ForgetPreview {
    /// The preview of a forget over `partition`, where `removals` says, per
    /// plan, which of its snapshots the policy lets go. An ID the plan
    /// doesn't own is not part of the preview, so a forget never names a
    /// snapshot outside the plan's own group (I6), whatever a retention
    /// evaluation answered.
    pub fn new(partition: &Partition, removals: Vec<(PlanId, Vec<SnapshotId>)>) -> Self {
        let plans = partition
            .managed
            .iter()
            .map(|group| {
                let named: BTreeSet<&SnapshotId> = removals
                    .iter()
                    .filter(|(plan, _)| *plan == group.plan)
                    .flat_map(|(_, ids)| ids)
                    .collect();
                PlanForget {
                    plan: group.plan.clone(),
                    total: group.snapshots.len(),
                    remove: group
                        .snapshots
                        .iter()
                        .filter(|snapshot| named.contains(&snapshot.id))
                        .cloned()
                        .collect(),
                }
            })
            .collect();
        let left_alone = LeftAlone {
            orphaned: Tally {
                groups: partition.orphaned.len(),
                snapshots: partition
                    .orphaned
                    .iter()
                    .map(|group| group.snapshots.len())
                    .sum(),
            },
            unmanaged: Tally {
                groups: partition.unmanaged.len(),
                snapshots: partition
                    .unmanaged
                    .iter()
                    .map(|group| group.snapshots.len())
                    .sum(),
            },
        };
        Self { plans, left_alone }
    }

    /// The IDs a forget removes: each plan's, in the plans' order, newest
    /// first within a plan.
    pub fn removal(&self) -> Vec<SnapshotId> {
        self.removed().cloned().collect()
    }

    /// The fingerprint of what the forget removes.
    pub fn selection(&self) -> SelectionToken {
        SelectionToken::of(self.removed())
    }

    /// The IDs a forget removes, as `removal` lists them, without copying.
    fn removed(&self) -> impl Iterator<Item = &SnapshotId> {
        self.plans
            .iter()
            .flat_map(|plan| plan.remove.iter().map(|snapshot| &snapshot.id))
    }
}

/// What this agent's forget would do to one repository before a proposal
/// and after it (§16, decision 89, point 8), each side as
/// [`ForgetSide`] tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetComparison {
    pub before: ForgetSide,
    pub after: ForgetSide,
}

/// What this agent's forget would do to one repository under one
/// configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForgetSide {
    /// This agent doesn't maintain the repository under it: no forget of
    /// this agent's runs there, so its retention forgets nothing.
    NotMaintained,
    /// What a forget would remove.
    Forgets(ForgetPreview),
    /// A forget would be refused, and so remove nothing: the policy of
    /// `plan`, which has history there, keeps nothing.
    KeepsNothing { plan: PlanId },
}

impl ForgetSide {
    /// What a forget would remove under this side; none when it wouldn't
    /// run, or would be refused.
    pub fn preview(&self) -> Option<&ForgetPreview> {
        match self {
            Self::Forgets(preview) => Some(preview),
            Self::NotMaintained | Self::KeepsNothing { .. } => None,
        }
    }
}

impl ForgetComparison {
    /// What a forget would remove after the proposal and not before: per
    /// plan of the after side that removes any, in its order, the plan's
    /// total snapshots after the proposal and those it removes only then,
    /// newest first.
    pub fn forgets_more(&self) -> Vec<PlanForget> {
        difference(self.after.preview(), self.before.preview())
    }

    /// What a forget would remove before the proposal and not after: per
    /// plan of the before side, as [`forgets_more`](Self::forgets_more)
    /// tells the other way.
    pub fn forgets_less(&self) -> Vec<PlanForget> {
        difference(self.before.preview(), self.after.preview())
    }
}

/// What `side` removes and `other` doesn't, by snapshot: per plan of
/// `side` that removes any such, in its order. A side that is `None`
/// removes nothing.
fn difference(side: Option<&ForgetPreview>, other: Option<&ForgetPreview>) -> Vec<PlanForget> {
    let Some(side) = side else {
        return Vec::new();
    };
    let removed: BTreeSet<&SnapshotId> = other
        .map(|other| other.removed().collect())
        .unwrap_or_default();
    side.plans
        .iter()
        .filter_map(|plan| {
            let remove: Vec<Snapshot> = plan
                .remove
                .iter()
                .filter(|snapshot| !removed.contains(&snapshot.id))
                .cloned()
                .collect();
            (!remove.is_empty()).then(|| PlanForget {
                plan: plan.plan.clone(),
                total: plan.total,
                remove,
            })
        })
        .collect()
}

/// Who may maintain a repository from this agent: exactly one party does
/// (I1, §7.3). Forget, prune, check and adoption, which rewrites snapshots,
/// are the owner's alone, and so are their previews.
pub struct Ownership;

impl Ownership {
    /// Whether this agent owns the maintenance of a repository whose entry
    /// asks for `request`, on an agent with no hub to grant it. Only
    /// `local` asks for this agent, and without a hub that request runs
    /// directly. `hub` waits for a hub this agent doesn't have, and `none`
    /// leaves the repository to something outside Ladderback, such as a
    /// cron job: neither runs anything here.
    pub fn owned_here(request: Maintenance) -> bool {
        matches!(request, Maintenance::Local)
    }

    /// What turning maintenance from this agent on (`maintain_here`) or
    /// off does to a repository whose entry asks for `request` (§7.3).
    /// A `hub` request is replaced when turned on and left as it is when
    /// turned off: this agent doesn't maintain it either way.
    pub fn change(request: Maintenance, maintain_here: bool) -> OwnershipChange {
        match (Self::owned_here(request), maintain_here) {
            (false, true) => OwnershipChange::TakeOver,
            (true, false) => OwnershipChange::GiveUp,
            (true, true) | (false, false) => OwnershipChange::Unchanged,
        }
    }

    /// The request of a repository this agent gives up maintaining: `none`,
    /// unless `running`, a maintenance operation of it running here, still
    /// runs. Then the entry keeps `local`, and the refusal names that
    /// operation: once it was given up, the person could start another on
    /// another machine while this one still runs, two maintainers at once
    /// (I1).
    pub fn give_up(running: Option<OperationId>) -> Result<Maintenance, OperationId> {
        match running {
            Some(running) => Err(running),
            None => Ok(Maintenance::None),
        }
    }
}

/// What turning maintenance from this agent on or off does to a
/// repository's request (§7.3, I1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnershipChange {
    /// The request already says what was asked: nothing is written.
    Unchanged,
    /// This agent takes over: the request becomes `local`. Without a hub
    /// nothing can tell whether another agent maintains the repository, so
    /// the person who asks it has been told that only one place may, as
    /// when connecting it.
    TakeOver,
    /// This agent gives up: the request becomes `none`, once
    /// [`Ownership::give_up`] allows it.
    GiveUp,
}

#[cfg(test)]
mod tests;
