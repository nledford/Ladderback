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
use crate::planning::{Maintenance, Plan, RetentionPolicy};
use crate::{AgentId, PlanId, SnapshotId, Timestamp};

/// One snapshot as the owner sees it: what a listing reports, minus what
/// only the engine needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: SnapshotId,
    /// When the snapshot was taken.
    pub time: Timestamp,
    pub hostname: String,
    pub label: String,
    /// The snapshot's tags, as the listing reports them.
    pub tags: Vec<String>,
    pub paths: Vec<String>,
}

impl Snapshot {
    /// The plan this snapshot's `ladderback-plan:` tag names, if it has one.
    pub fn plan(&self) -> Option<&str> {
        self.tagged_by(plan_tags::PLAN_TAG_PREFIX)
    }

    /// The agent this snapshot's `ladderback-agent:` tag names, if it has one.
    pub fn agent(&self) -> Option<&str> {
        self.tagged_by(plan_tags::AGENT_TAG_PREFIX)
    }

    fn tagged_by(&self, prefix: &str) -> Option<&str> {
        self.tags.iter().find_map(|tag| tag.strip_prefix(prefix))
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
/// another agent's plan, or a plan this configuration no longer has. Forget
/// leaves them alone (I6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanedGroup {
    /// The agent tag's text and the plan tag's text, either possibly absent
    /// from the group's snapshots, never both.
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
    /// tagged by this agent and a plan in `plans` is managed; anything else
    /// tagged is orphaned. Each group's snapshots are newest first.
    pub fn partition(snapshots: &[Snapshot], agent: &AgentId, plans: &[ManagedPlan]) -> Partition {
        let own = agent.as_uuid().hyphenated().to_string();
        let mut partition = Partition::default();
        // Managed groups follow the plans' own order; each group's
        // snapshots are newest first.
        for plan in plans {
            let mut group: Vec<Snapshot> = snapshots
                .iter()
                .filter(|snapshot| {
                    snapshot.agent() == Some(own.as_str())
                        && snapshot.plan() == Some(plan.plan.as_str())
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
            let tagged = snapshot.agent().is_some() || snapshot.plan().is_some();
            let managed = snapshot.agent() == Some(own.as_str())
                && plans
                    .iter()
                    .any(|plan| snapshot.plan() == Some(plan.plan.as_str()));
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

/// Newest first.
fn sort_newest_first(snapshots: &mut [Snapshot]) {
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
}

/// Tagged snapshots by their `(agent, plan)` tag pair, ordered by agent
/// then plan, each group newest first.
fn grouped_by_tag(snapshots: Vec<Snapshot>) -> Vec<OrphanedGroup> {
    let mut groups: BTreeMap<(Option<String>, Option<String>), Vec<Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        groups
            .entry((
                snapshot.agent().map(str::to_owned),
                snapshot.plan().map(str::to_owned),
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

    /// The unmanaged group whose host, label and paths equal `into`'s host,
    /// label and sources (`into`'s host is `host`'s when the plan doesn't
    /// name one, and its label an empty one), and every other plan that
    /// would claim the same group. The paths are compared exactly, in order,
    /// so the caller hands in plans whose sources are already normalized the
    /// way the engine records a snapshot's paths (decision 26).
    pub fn unmanaged<'a>(
        partition: &'a Partition,
        hostname: &str,
        into: &'a Plan,
        plans: &'a [Plan],
    ) -> Result<&'a UnmanagedGroup, AdoptionRefusal> {
        let key = |plan: &Plan| adoption_key(plan, hostname);
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

    /// The plans that would adopt each of `groups`: every plan whose host,
    /// label and sources equal the group's, in `plans`' order, one list per
    /// group in the groups' order. None when no plan would, one when it is
    /// clear which does, several when which one takes the history is the
    /// person's decision (the same key `unmanaged` matches by, so a group
    /// listed with one claimant is one an adoption into that plan finds).
    pub fn claimants(
        groups: &[UnmanagedGroup],
        hostname: &str,
        plans: &[Plan],
    ) -> Vec<Vec<PlanId>> {
        let keys: Vec<_> = plans
            .iter()
            .map(|plan| (&plan.id, adoption_key(plan, hostname)))
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
/// records them on a snapshot of the plan: the plan's own host, or
/// `hostname` when it names none, its label or an empty one, and its sources
/// in order.
fn adoption_key(plan: &Plan, hostname: &str) -> (String, String, Vec<String>) {
    (
        plan.host
            .as_ref()
            .map(|host| host.as_str().to_owned())
            .unwrap_or_else(|| hostname.to_owned()),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentId;
    use crate::Uuid;
    use crate::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
    use crate::planning::KeepCount;

    fn agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(1))
    }

    fn other_agent() -> AgentId {
        AgentId::from_uuid(Uuid::from_u128(2))
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + second).expect("a time in range")
    }

    fn snapshot(number: i64, tags: &[String]) -> Snapshot {
        Snapshot {
            id: SnapshotId::parse(&format!("{number:064x}")).expect("a snapshot ID"),
            time: at(second_back(number)),
            hostname: "demo-host".to_owned(),
            label: String::new(),
            tags: tags.to_vec(),
            paths: vec!["/home/demo".to_owned()],
        }
    }

    /// Times count back, so `number` rising means older.
    fn second_back(number: i64) -> i64 {
        10_000 - number
    }

    fn tagged(plan: &str, agent: &AgentId, number: i64) -> Snapshot {
        let tags = vec![
            format!("{PLAN_TAG_PREFIX}{plan}"),
            format!("{AGENT_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
        ];
        snapshot(number, &tags)
    }

    fn untagged(number: i64) -> Snapshot {
        snapshot(number, &[])
    }

    fn untagged_of(number: i64, folder: &str) -> Snapshot {
        Snapshot {
            paths: vec![folder.to_owned()],
            ..untagged(number)
        }
    }

    fn home_policy() -> RetentionPolicy {
        RetentionPolicy {
            keep_last: Some(KeepCount::new(2).expect("a count")),
            ..RetentionPolicy::default()
        }
    }

    fn plans(list: &[(&str, RetentionPolicy)]) -> Vec<ManagedPlan> {
        list.iter()
            .map(|(plan, policy)| ManagedPlan {
                plan: PlanId::parse(plan).expect("a plan ID"),
                policy: policy.clone(),
            })
            .collect()
    }

    fn ids(group: &ManagedGroup) -> Vec<&str> {
        group.snapshots.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn the_plan_of_this_agent_owns_its_tagged_snapshots_newest_first() {
        let snapshots = vec![
            tagged("home", &agent(), 3),
            tagged("home", &agent(), 1),
            tagged("home", &agent(), 2),
        ];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        assert_eq!(partition.managed.len(), 1, "one plan has snapshots");
        assert_eq!(
            partition.managed[0].plan,
            PlanId::parse("home").expect("a plan")
        );
        assert_eq!(
            ids(&partition.managed[0]),
            [
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000003"
            ],
            "newest first: 1 is the newest, for times count back"
        );
        assert!(partition.orphaned.is_empty());
        assert!(partition.unmanaged.is_empty());
    }

    #[test]
    fn another_agents_plan_is_orphaned_not_managed() {
        let snapshots = vec![tagged("home", &other_agent(), 1)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        assert!(partition.managed.is_empty());
        let [group] = partition.orphaned.try_into().expect("one orphaned group");
        assert_eq!(group.plan.as_deref(), Some("home"));
        assert_eq!(
            group.agent.as_deref(),
            Some(other_agent().as_uuid().hyphenated().to_string().as_str())
        );
        assert_eq!(group.snapshots.len(), 1);
    }

    #[test]
    fn a_plan_this_config_no_longer_has_is_orphaned() {
        let snapshots = vec![tagged("retired", &agent(), 1)];
        let partition = ForgetSelection::partition(&snapshots, &agent(), &plans(&[]));
        let [group] = partition.orphaned.try_into().expect("one orphaned group");
        assert_eq!(group.plan.as_deref(), Some("retired"));
    }

    #[test]
    fn untagged_snapshots_are_unmanaged_whatever_else_they_carry() {
        let snapshots = vec![untagged(1), untagged(2)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        assert!(partition.managed.is_empty());
        assert!(partition.orphaned.is_empty());
        let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
        assert_eq!(group.hostname, "demo-host");
        assert_eq!(
            group.snapshots.len(),
            2,
            "same host, label and paths: one group"
        );
    }

    #[test]
    fn a_plan_tag_without_an_agent_tag_is_orphaned() {
        let snapshots = vec![snapshot(1, &[format!("{PLAN_TAG_PREFIX}home")])];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        assert!(
            partition.managed.is_empty(),
            "an owner is proven by both tags"
        );
        let [group] = partition.orphaned.try_into().expect("one orphaned group");
        assert_eq!(group.agent, None);
        assert_eq!(group.plan.as_deref(), Some("home"));
    }

    #[test]
    fn each_plan_of_this_agent_keeps_its_own_group() {
        let snapshots = vec![
            tagged("home", &agent(), 1),
            tagged("photos", &agent(), 1),
            tagged("photos", &agent(), 2),
        ];
        let partition = ForgetSelection::partition(
            &snapshots,
            &agent(),
            &plans(&[("home", home_policy()), ("photos", home_policy())]),
        );
        assert_eq!(partition.managed.len(), 2);
        assert_eq!(
            partition.managed[0].plan,
            PlanId::parse("home").expect("a plan")
        );
        assert_eq!(
            partition.managed[1].plan,
            PlanId::parse("photos").expect("a plan")
        );
        assert_eq!(partition.managed[1].snapshots.len(), 2);
    }

    #[test]
    fn a_plan_with_no_snapshots_has_no_group() {
        let snapshots = vec![tagged("home", &agent(), 1)];
        let partition = ForgetSelection::partition(
            &snapshots,
            &agent(),
            &plans(&[("home", home_policy()), ("photos", home_policy())]),
        );
        assert_eq!(partition.managed.len(), 1);
    }

    #[test]
    fn the_partition_counts_every_snapshot_once() {
        let snapshots = vec![
            tagged("home", &agent(), 1),
            tagged("home", &agent(), 2),
            tagged("home", &other_agent(), 3),
            untagged(4),
        ];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        assert_eq!(partition.len(), 4, "every snapshot appears once");
        assert!(!partition.is_empty());
    }

    #[test]
    fn an_empty_partition_is_empty() {
        let partition =
            ForgetSelection::partition(&[], &agent(), &plans(&[("home", home_policy())]));
        assert_eq!(partition.len(), 0);
        assert!(partition.is_empty());
    }

    /// The plans that claim `group`, as `claimants` tells it of one group.
    fn claimants_of(group: &UnmanagedGroup, hostname: &str, plans: &[Plan]) -> Vec<PlanId> {
        AdoptSelection::claimants(std::slice::from_ref(group), hostname, plans).remove(0)
    }

    /// A plan whose host, label and sources match the untagged snapshots'
    /// hostname, label and paths.
    fn full_plan(id: &str, source: &str) -> Plan {
        let mut plan = Plan::new(
            PlanId::parse(id).expect("a plan ID"),
            crate::RepositoryAlias::parse("demo").expect("an alias"),
            vec![crate::planning::Source::parse(source).expect("a source")],
        );
        plan.host = Some(crate::planning::Hostname::parse("demo-host").expect("a hostname"));
        plan
    }

    #[test]
    fn the_orphaned_group_is_found_by_its_two_tags() {
        let snapshots = vec![tagged("home", &other_agent(), 1)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let found = AdoptSelection::orphaned(
            &partition,
            &other_agent().as_uuid().hyphenated().to_string(),
            "home",
        )
        .expect("the group is there");
        assert_eq!(found.snapshots.len(), 1);
        assert!(
            AdoptSelection::orphaned(
                &partition,
                &agent().as_uuid().hyphenated().to_string(),
                "home"
            )
            .is_none()
        );
    }

    #[test]
    fn an_unmanaged_group_matches_the_plan_with_the_same_host_label_and_sources() {
        let snapshots = vec![untagged(1), untagged(2)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let home = full_plan("home", "/home/demo");
        let plans = vec![home.clone()];
        let group = AdoptSelection::unmanaged(&partition, "demo-host", &home, &plans)
            .expect("the plan matches its own history");
        assert_eq!(group.snapshots.len(), 2);
    }

    #[test]
    fn a_plan_whose_sources_differ_matches_nothing() {
        let snapshots = vec![untagged(1)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let photos = full_plan("photos", "/srv/photos");
        let plans = vec![photos.clone()];
        assert_eq!(
            AdoptSelection::unmanaged(&partition, "demo-host", &photos, &plans),
            Err(AdoptionRefusal::NoMatch)
        );
    }

    #[test]
    fn two_plans_with_the_same_host_label_and_sources_are_refused_as_ambiguous() {
        let snapshots = vec![untagged(1)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let home = full_plan("home", "/home/demo");
        let mirror = full_plan("mirror", "/home/demo");
        let plans = vec![home.clone(), mirror.clone()];
        assert_eq!(
            AdoptSelection::unmanaged(&partition, "demo-host", &home, &plans),
            Err(AdoptionRefusal::Ambiguous {
                plans: vec![
                    PlanId::parse("home").expect("a plan"),
                    PlanId::parse("mirror").expect("a plan"),
                ]
            })
        );
    }

    #[test]
    fn a_plan_without_a_host_of_its_own_uses_the_agents() {
        let snapshots = vec![untagged(1)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let mut home = full_plan("home", "/home/demo");
        home.host = None;
        let plans = vec![home.clone()];
        assert!(
            AdoptSelection::unmanaged(&partition, "demo-host", &home, &plans).is_ok(),
            "the agent's hostname stands in"
        );
    }

    fn id(number: i64) -> SnapshotId {
        SnapshotId::parse(&format!("{number:064x}")).expect("a snapshot ID")
    }

    fn plan(name: &str) -> PlanId {
        PlanId::parse(name).expect("a plan ID")
    }

    fn token_text(ids: &[i64]) -> String {
        let ids: Vec<SnapshotId> = ids.iter().copied().map(id).collect();
        SelectionToken::of(&ids).as_str().to_owned()
    }

    #[test]
    fn a_selection_token_is_sixteen_lower_case_hex_digits() {
        for ids in [&[][..], &[1], &[1, 2, 3]] {
            let text = token_text(ids);
            assert_eq!(text.len(), 16, "{text}");
            assert!(
                text.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{text}"
            );
        }
    }

    #[test]
    fn a_selection_token_does_not_depend_on_the_order_of_the_snapshots() {
        assert_eq!(token_text(&[1, 2, 3]), token_text(&[3, 1, 2]));
        assert_eq!(token_text(&[1, 2]), token_text(&[2, 1]));
    }

    #[test]
    fn a_selection_token_differs_for_a_different_set() {
        assert_ne!(token_text(&[1, 2]), token_text(&[1, 3]));
        assert_ne!(token_text(&[1]), token_text(&[1, 2]));
        assert_ne!(token_text(&[]), token_text(&[1]));
        assert_ne!(token_text(&[1, 2]), token_text(&[3]));
    }

    #[test]
    fn a_selection_token_has_fixed_values() {
        // FNV-1a 64 of the sorted IDs joined by newlines: the empty text
        // hashes to the algorithm's offset basis.
        assert_eq!(token_text(&[]), "cbf29ce484222325");
        assert_eq!(token_text(&[1]), "68497038cf038672");
        assert_eq!(token_text(&[2, 1]), "f1ab8ae83457c94e");
        assert_eq!(token_text(&[1, 2, 3]), "312cd2115e3903a5");
    }

    #[test]
    fn a_snapshot_named_twice_counts_once() {
        assert_eq!(token_text(&[1, 1]), token_text(&[1]));
        assert_eq!(token_text(&[2, 1, 2, 1]), token_text(&[1, 2]));
    }

    fn adoption_text(into: &str, ids: &[i64]) -> String {
        let ids: Vec<SnapshotId> = ids.iter().copied().map(id).collect();
        SelectionToken::of_adoption(&plan(into), &ids)
            .as_str()
            .to_owned()
    }

    #[test]
    fn an_adoptions_token_covers_its_destination_and_its_snapshots() {
        // FNV-1a 64 of `into:<plan>`, a newline, then the sorted IDs joined
        // by newlines.
        assert_eq!(adoption_text("home", &[1]), "6d271b21522bf869");
        assert_eq!(adoption_text("home", &[2, 1]), "9ed6d13a3e9a15d3");
        assert_eq!(adoption_text("photos", &[1]), "0d3078a0fe08835b");
        assert_eq!(
            adoption_text("home", &[1, 1]),
            adoption_text("home", &[1]),
            "a snapshot named twice counts once"
        );
        assert_ne!(
            adoption_text("home", &[1]),
            token_text(&[1]),
            "no forget's selection is an adoption's"
        );
    }

    #[test]
    fn a_selection_token_reads_back_from_its_text() {
        let ids = [id(1), id(2)];
        let token = SelectionToken::of(&ids);
        assert_eq!(SelectionToken::parse(token.as_str()), Some(token));
    }

    #[test]
    fn only_sixteen_lower_case_hex_digits_read_as_a_token() {
        assert!(SelectionToken::parse("0123456789abcdef").is_some());
        for text in [
            "",
            "0123456789abcde",
            "0123456789abcdef0",
            "0123456789ABCDEF",
            "0123456789abcdeg",
            " 123456789abcdef",
            "0123456789abcdef\n",
            "+123456789abcdef",
        ] {
            assert_eq!(SelectionToken::parse(text), None, "{text:?}");
        }
    }

    /// `home` and `photos`, both keeping two: `home` holds snapshots 1 to 4
    /// and `photos` snapshot 5, all of this agent's; another agent's plan
    /// and a retired plan hold 6 to 8; 9 and 10 are untagged and 11 is
    /// untagged of another folder.
    fn busy_repository() -> Partition {
        let mut snapshots: Vec<Snapshot> = (1..=4).map(|n| tagged("home", &agent(), n)).collect();
        snapshots.push(tagged("photos", &agent(), 5));
        snapshots.push(tagged("home", &other_agent(), 6));
        snapshots.push(tagged("home", &other_agent(), 7));
        snapshots.push(tagged("retired", &agent(), 8));
        snapshots.push(untagged(9));
        snapshots.push(untagged(10));
        let mut elsewhere = untagged(11);
        elsewhere.paths = vec!["/srv/other".to_owned()];
        snapshots.push(elsewhere);
        ForgetSelection::partition(
            &snapshots,
            &agent(),
            &plans(&[("home", home_policy()), ("photos", home_policy())]),
        )
    }

    fn removals(list: &[(&str, &[i64])]) -> Vec<(PlanId, Vec<SnapshotId>)> {
        list.iter()
            .map(|(name, numbers)| (plan(name), numbers.iter().copied().map(id).collect()))
            .collect()
    }

    fn removed_ids(planned: &PlanForget) -> Vec<SnapshotId> {
        planned.remove.iter().map(|s| s.id.clone()).collect()
    }

    #[test]
    fn a_forget_preview_counts_each_plans_snapshots_and_names_what_goes() {
        let preview = ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[3, 4]), ("photos", &[])]),
        );
        let [home, photos] = preview.plans.as_slice() else {
            panic!("one entry per plan with snapshots: {:?}", preview.plans);
        };
        assert_eq!(home.plan, plan("home"));
        assert_eq!(home.total, 4, "the plan owns four snapshots");
        assert_eq!(removed_ids(home), [id(3), id(4)]);
        assert_eq!(photos.plan, plan("photos"));
        assert_eq!(photos.total, 1);
        assert_eq!(removed_ids(photos), Vec::<SnapshotId>::new());
    }

    #[test]
    fn what_a_forget_lists_for_removal_is_newest_first_within_a_plan() {
        // Asked for in any order, the plan's own order (newest first) wins.
        let preview = ForgetPreview::new(&busy_repository(), removals(&[("home", &[4, 2])]));
        let [home, _photos] = preview.plans.as_slice() else {
            panic!("one entry per plan with snapshots: {:?}", preview.plans);
        };
        assert_eq!(removed_ids(home), [id(2), id(4)]);
    }

    #[test]
    fn a_forget_preview_counts_what_it_leaves_alone() {
        let preview = ForgetPreview::new(&busy_repository(), removals(&[("home", &[3, 4])]));
        assert_eq!(
            preview.left_alone.orphaned,
            Tally {
                groups: 2,
                snapshots: 3
            },
            "another agent's `home` (two) and this agent's retired plan (one): two groups"
        );
        assert_eq!(
            preview.left_alone.unmanaged,
            Tally {
                groups: 2,
                snapshots: 3
            },
            "two untagged folders: three snapshots"
        );
    }

    #[test]
    fn a_repository_with_nothing_unowned_leaves_nothing_alone() {
        let snapshots = vec![tagged("home", &agent(), 1), tagged("home", &agent(), 2)];
        let partition =
            ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
        let preview = ForgetPreview::new(&partition, removals(&[("home", &[2])]));
        assert_eq!(preview.left_alone, LeftAlone::default());
    }

    #[test]
    fn the_removal_lists_the_ids_across_plans_in_the_plans_order() {
        let preview = ForgetPreview::new(
            &busy_repository(),
            removals(&[("photos", &[5]), ("home", &[3, 4])]),
        );
        assert_eq!(
            preview.removal(),
            [id(3), id(4), id(5)],
            "home comes before photos in the configuration, whatever order they were asked in"
        );
    }

    #[test]
    fn the_selection_is_the_token_of_everything_the_forget_removes() {
        let preview = ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[3, 4]), ("photos", &[5])]),
        );
        assert_eq!(
            preview.selection(),
            SelectionToken::of(&[id(3), id(4), id(5)])
        );
        let narrower = ForgetPreview::new(&busy_repository(), removals(&[("home", &[3, 4])]));
        assert_ne!(
            preview.selection(),
            narrower.selection(),
            "a plan's removals are part of the selection"
        );
    }

    #[test]
    fn a_preview_removing_nothing_removes_nothing() {
        let nothing = ForgetPreview::new(&busy_repository(), removals(&[("home", &[])]));
        assert!(nothing.removal().is_empty());
        let something = ForgetPreview::new(&busy_repository(), removals(&[("photos", &[5])]));
        assert_eq!(something.removal(), [id(5)]);
    }

    #[test]
    fn a_forget_never_names_a_snapshot_the_plan_does_not_own() {
        // 6 is another agent's, 9 is untagged, 5 is the other plan's and 99
        // is nowhere: only 4 is `home`'s own.
        let preview = ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[6, 9, 5, 99, 4]), ("ghost", &[1])]),
        );
        assert_eq!(preview.removal(), [id(4)]);
        assert_eq!(
            preview.plans.len(),
            2,
            "a plan with no snapshots is not listed"
        );
    }

    #[test]
    fn one_plan_with_the_same_host_label_and_sources_claims_an_unmanaged_group() {
        let snapshots = vec![untagged(1), untagged(2)];
        let partition = ForgetSelection::partition(&snapshots, &agent(), &plans(&[]));
        let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
        let candidates = vec![
            full_plan("home", "/home/demo"),
            full_plan("photos", "/srv/photos"),
        ];
        assert_eq!(
            claimants_of(&group, "demo-host", &candidates),
            [plan("home")]
        );
    }

    #[test]
    fn two_plans_with_the_same_host_label_and_sources_both_claim_it_in_the_plans_order() {
        let partition = ForgetSelection::partition(&[untagged(1)], &agent(), &plans(&[]));
        let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
        let candidates = vec![
            full_plan("mirror", "/home/demo"),
            full_plan("photos", "/srv/photos"),
            full_plan("home", "/home/demo"),
        ];
        assert_eq!(
            claimants_of(&group, "demo-host", &candidates),
            [plan("mirror"), plan("home")]
        );
    }

    #[test]
    fn no_plan_claims_a_group_of_another_folder_host_or_label() {
        let partition = ForgetSelection::partition(&[untagged(1)], &agent(), &plans(&[]));
        let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
        let mut other_host = full_plan("elsewhere", "/home/demo");
        other_host.host = Some(crate::planning::Hostname::parse("other-host").expect("a hostname"));
        let mut labelled = full_plan("labelled", "/home/demo");
        labelled.label = Some("weekly".to_owned());
        let candidates = vec![full_plan("photos", "/srv/photos"), other_host, labelled];
        assert_eq!(
            claimants_of(&group, "demo-host", &candidates),
            Vec::<PlanId>::new()
        );
    }

    #[test]
    fn a_plan_without_a_host_of_its_own_claims_by_the_agents_hostname() {
        let partition = ForgetSelection::partition(&[untagged(1)], &agent(), &plans(&[]));
        let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
        let mut home = full_plan("home", "/home/demo");
        home.host = None;
        let candidates = vec![home];
        assert_eq!(
            claimants_of(&group, "demo-host", &candidates),
            [plan("home")]
        );
        assert_eq!(
            claimants_of(&group, "another-host", &candidates),
            Vec::<PlanId>::new(),
            "the agent's hostname is the one it is asked with"
        );
    }

    #[test]
    fn each_group_is_claimed_by_the_plans_that_match_it_alone() {
        let partition = ForgetSelection::partition(
            &[untagged(1), untagged_of(2, "/srv/photos")],
            &agent(),
            &plans(&[]),
        );
        let candidates = vec![
            full_plan("photos", "/srv/photos"),
            full_plan("home", "/home/demo"),
            full_plan("mirror", "/home/demo"),
        ];
        let claimed: Vec<(String, Vec<PlanId>)> = partition
            .unmanaged
            .iter()
            .zip(AdoptSelection::claimants(
                &partition.unmanaged,
                "demo-host",
                &candidates,
            ))
            .map(|(group, plans)| (group.paths.join(","), plans))
            .collect();
        assert_eq!(
            claimed,
            [
                ("/home/demo".to_owned(), vec![plan("home"), plan("mirror")]),
                ("/srv/photos".to_owned(), vec![plan("photos")]),
            ]
        );
        assert!(AdoptSelection::claimants(&[], "demo-host", &candidates).is_empty());
    }

    #[test]
    fn only_a_repository_that_asks_for_this_agent_is_maintained_here() {
        assert!(Ownership::owned_here(Maintenance::Local));
        assert!(!Ownership::owned_here(Maintenance::None));
        assert!(!Ownership::owned_here(Maintenance::Hub));
    }
}
