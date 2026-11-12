//! Properties of [`ForgetSelection`] and the forget it decides (invariant
//! I6): every snapshot of a listing is in exactly one group, the one its tags
//! put it in, and a forget names only snapshots a plan of this agent owns,
//! each under that plan, whatever a retention evaluation answers.

use proptest::prelude::*;
use proptest::sample::{select, subsequence};

use super::{ForgetPreview, ForgetSelection, ManagedPlan, Partition, Snapshot};
use crate::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use crate::planning::RetentionPolicy;
use crate::{AgentId, PlanId, SnapshotId, Timestamp, Uuid};

fn agent(number: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(number))
}

/// This agent.
fn own() -> AgentId {
    agent(1)
}

fn own_text() -> String {
    own().as_uuid().hyphenated().to_string()
}

/// The plans a configuration may have; snapshots name these and one more,
/// which no configuration has.
const PLANS: [&str; 3] = ["home", "photos", "mail"];
const RETIRED: &str = "retired";

fn plan_id(plan: &str) -> PlanId {
    PlanId::parse(plan).expect("a plan ID")
}

/// A tag a snapshot may carry: this agent's, another's, or one that names
/// no agent; a plan of the configuration's or not, or none; or a tag that
/// isn't an owner's at all. A snapshot may carry any number of them, both
/// tags of an owner, one, neither, or two that disagree.
fn tag() -> impl Strategy<Value = String> {
    let agents = vec![
        own_text(),
        agent(2).as_uuid().hyphenated().to_string(),
        "not-an-agent".to_owned(),
    ];
    let plans: Vec<String> = PLANS
        .iter()
        .chain(&[RETIRED, ""])
        .map(|&plan| plan.to_owned())
        .collect();
    prop_oneof![
        select(agents).prop_map(|agent| format!("{AGENT_TAG_PREFIX}{agent}")),
        select(plans).prop_map(|plan| format!("{PLAN_TAG_PREFIX}{plan}")),
        select(vec!["daily", "ladderback-other:x", "ladderback-plan"]).prop_map(str::to_owned),
    ]
}

/// The most snapshots a listing has; answers name IDs up to two past it.
const MOST_SNAPSHOTS: usize = 12;

fn snapshot_id(number: usize) -> SnapshotId {
    SnapshotId::parse(&format!("{number:064x}")).expect("a snapshot ID")
}

/// A listing: each snapshot's ID is its place in it, and it is taken at one
/// of a few times, on one of a few hosts, labels and sets of paths, so that
/// groups share them.
fn snapshots() -> impl Strategy<Value = Vec<Snapshot>> {
    // A third of the snapshots carry both tags of a plan of this agent, in
    // either order and among others, so that most listings have managed
    // groups, several plans' among them; the rest carry any tags.
    let managed = (
        select(PLANS.to_vec()),
        prop::collection::vec(tag(), 0..=2),
        any::<bool>(),
    )
        .prop_map(|(plan, mut tags, agent_first)| {
            let mut owner = vec![
                format!("{AGENT_TAG_PREFIX}{}", own_text()),
                format!("{PLAN_TAG_PREFIX}{plan}"),
            ];
            if !agent_first {
                owner.reverse();
            }
            owner.append(&mut tags);
            owner
        });
    let tags = prop_oneof![
        2 => prop::collection::vec(tag(), 0..=3),
        1 => managed,
    ];
    let snapshot = (
        tags,
        0_i64..4,
        select(vec!["demo-host", "other-host"]),
        select(vec!["", "nightly"]),
        subsequence(vec!["/home/demo", "/srv/photos"], 0..=2),
    );
    prop::collection::vec(snapshot, 0..=MOST_SNAPSHOTS).prop_map(|listing| {
        listing
            .into_iter()
            .enumerate()
            .map(
                |(number, (tags, second, hostname, label, paths))| Snapshot {
                    id: snapshot_id(number),
                    time: Timestamp::from_second(1_800_000_000 + second).expect("a time in range"),
                    offset: crate::Offset::UTC,
                    hostname: hostname.to_owned(),
                    label: label.to_owned(),
                    tags,
                    paths: paths.into_iter().map(str::to_owned).collect(),
                },
            )
            .collect()
    })
}

/// Some of the plans, in any order.
fn configured() -> impl Strategy<Value = Vec<ManagedPlan>> {
    subsequence(PLANS.to_vec(), 0..=PLANS.len())
        .prop_shuffle()
        .prop_map(|plans| {
            plans
                .into_iter()
                .map(|plan| ManagedPlan {
                    plan: plan_id(plan),
                    policy: RetentionPolicy::default(),
                })
                .collect()
        })
}

/// What a retention evaluation may answer: for any plan, configured or not,
/// any snapshots, its own, another plan's, untagged, orphaned or none of
/// the listing's.
fn removals() -> impl Strategy<Value = Vec<(PlanId, Vec<SnapshotId>)>> {
    let plans: Vec<&str> = PLANS.iter().copied().chain([RETIRED]).collect();
    let id = (0..MOST_SNAPSHOTS + 2).prop_map(snapshot_id);
    prop::collection::vec(
        (
            select(plans).prop_map(plan_id),
            prop::collection::vec(id, 0..=6),
        ),
        0..=4,
    )
}

/// The text of `snapshot`'s first tag that starts with `prefix`: the first
/// of two that disagree wins.
fn first_tag<'a>(snapshot: &'a Snapshot, prefix: &str) -> Option<&'a str> {
    snapshot
        .tags
        .iter()
        .find(|tag| tag.starts_with(prefix))
        .map(|tag| &tag[prefix.len()..])
}

/// The agent and plan `snapshot`'s tags name.
fn owner_tags(snapshot: &Snapshot) -> (Option<&str>, Option<&str>) {
    (
        first_tag(snapshot, AGENT_TAG_PREFIX),
        first_tag(snapshot, PLAN_TAG_PREFIX),
    )
}

/// The plan that owns `snapshot` by its tags, if one of `plans` does.
fn owner<'a>(snapshot: &Snapshot, plans: &'a [ManagedPlan]) -> Option<&'a PlanId> {
    let (agent, plan) = owner_tags(snapshot);
    if agent != Some(own_text().as_str()) {
        return None;
    }
    plans
        .iter()
        .map(|configured| &configured.plan)
        .find(|configured| plan == Some(configured.as_str()))
}

fn tagged(snapshot: &Snapshot) -> bool {
    owner_tags(snapshot) != (None, None)
}

/// Every snapshot of every group, by ID.
fn all_of(partition: &Partition) -> Vec<&Snapshot> {
    let mut all: Vec<&Snapshot> = partition
        .managed
        .iter()
        .flat_map(|group| &group.snapshots)
        .chain(partition.orphaned.iter().flat_map(|group| &group.snapshots))
        .chain(
            partition
                .unmanaged
                .iter()
                .flat_map(|group| &group.snapshots),
        )
        .collect();
    all.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    all
}

proptest! {
    #![proptest_config(crate::properties::config())]

    #[test]
    fn every_snapshot_is_in_exactly_one_group_the_one_its_tags_put_it_in(
        snapshots in snapshots(),
        plans in configured(),
    ) {
        let partition = ForgetSelection::partition(&snapshots, &own(), &plans);
        let mut listed: Vec<&Snapshot> = snapshots.iter().collect();
        listed.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        prop_assert_eq!(all_of(&partition), listed);
        for group in &partition.managed {
            for snapshot in &group.snapshots {
                prop_assert_eq!(owner(snapshot, &plans), Some(&group.plan));
            }
        }
        let mut keys: Vec<(Option<&str>, Option<&str>)> = Vec::new();
        for group in &partition.orphaned {
            keys.push((group.agent.as_deref(), group.plan.as_deref()));
            for snapshot in &group.snapshots {
                prop_assert!(tagged(snapshot) && owner(snapshot, &plans).is_none());
                prop_assert_eq!(
                    owner_tags(snapshot),
                    (group.agent.as_deref(), group.plan.as_deref())
                );
            }
        }
        let orphaned_groups = keys.len();
        keys.sort_unstable();
        keys.dedup();
        prop_assert_eq!(keys.len(), orphaned_groups, "two orphaned groups share a key");
        let mut keys: Vec<(&str, &str, &[String])> = Vec::new();
        for group in &partition.unmanaged {
            keys.push((&group.hostname, &group.label, &group.paths));
            for snapshot in &group.snapshots {
                prop_assert!(!tagged(snapshot));
                prop_assert_eq!(
                    (&snapshot.hostname, &snapshot.label, &snapshot.paths),
                    (&group.hostname, &group.label, &group.paths)
                );
            }
        }
        let unmanaged_groups = keys.len();
        keys.sort_unstable();
        keys.dedup();
        prop_assert_eq!(keys.len(), unmanaged_groups, "two unmanaged groups share a key");
        let mut managed: Vec<&PlanId> = partition.managed.iter().map(|group| &group.plan).collect();
        let managed_groups = managed.len();
        managed.sort_unstable();
        managed.dedup();
        prop_assert_eq!(managed.len(), managed_groups, "two managed groups share a plan");
    }

    #[test]
    fn a_forget_names_only_snapshots_a_plan_owns_each_under_that_plan(
        snapshots in snapshots(),
        plans in configured(),
        removals in removals(),
    ) {
        let partition = ForgetSelection::partition(&snapshots, &own(), &plans);
        let preview = ForgetPreview::new(&partition, removals.clone());
        // Exactly the snapshots a plan owns that an answer named for that
        // plan, each once.
        let mut expected: Vec<&SnapshotId> = snapshots
            .iter()
            .filter(|snapshot| {
                owner(snapshot, &plans).is_some_and(|plan| {
                    removals
                        .iter()
                        .any(|(named, ids)| named == plan && ids.contains(&snapshot.id))
                })
            })
            .map(|snapshot| &snapshot.id)
            .collect();
        expected.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        let removal = preview.removal();
        let mut removed: Vec<&SnapshotId> = removal.iter().collect();
        removed.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        prop_assert_eq!(removed, expected);
    }
}
