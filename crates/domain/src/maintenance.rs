//! Which snapshots a forget run may remove (technical design §7.3). The
//! partition into managed, orphaned and unmanaged groups is the maintenance
//! owner's own decision, and it is the whole of invariant I6: forget only
//! ever removes snapshots a plan of this agent owns, by tag. Untagged
//! snapshots are never touched, and a tagged group whose plan or agent this
//! configuration doesn't know is left alone and listed as orphaned, for the
//! user to adopt, give a policy, or delete explicitly.
//!
//! Which of a managed group's snapshots the plan's policy lets go is not
//! decided here: retention is evaluated with rustic's own rules, through the
//! `Retention` port in `app` (§3.2).

use std::collections::BTreeMap;

use crate::execution::plan_tags;
use crate::planning::RetentionPolicy;
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
}
