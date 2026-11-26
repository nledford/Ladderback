use super::*;
use crate::AgentId;
use crate::Uuid;
use crate::execution::plan_tags::{MACHINE_TAG_PREFIX, PLAN_TAG_PREFIX};
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
        offset: Offset::UTC,
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
        format!("{MACHINE_TAG_PREFIX}{}", agent.as_uuid().hyphenated()),
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
fn snapshots_taken_at_the_same_time_are_ordered_by_id_however_they_were_listed() {
    let same_time = |number| Snapshot {
        time: at(0),
        ..tagged("home", &agent(), number)
    };
    for listed in [
        vec![same_time(2), same_time(1)],
        vec![same_time(1), same_time(2)],
    ] {
        let partition =
            ForgetSelection::partition(&listed, &agent(), &plans(&[("home", home_policy())]));
        assert_eq!(
            ids(&partition.managed[0]),
            [
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000002"
            ],
        );
    }
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

/// A snapshot still tagged `ladderback-agent:` with this machine's ID, as
/// every backup before §16, decision 163 was, belongs to no plan: the
/// plan's forget never selects it, and it is orphaned under its plan tag
/// alone (I6).
#[test]
fn a_snapshot_with_the_old_machine_tag_is_never_a_plans_to_forget() {
    let old = vec![
        format!("{PLAN_TAG_PREFIX}home"),
        format!("ladderback-agent:{}", own_text()),
    ];
    let snapshots = vec![snapshot(1, &old), snapshot(2, &old), snapshot(3, &old)];
    let partition =
        ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
    assert!(partition.managed.is_empty(), "{:?}", partition.managed);
    let [group] = partition.orphaned.try_into().expect("one orphaned group");
    assert_eq!(group.agent, None);
    assert_eq!(group.plan.as_deref(), Some("home"));
    assert_eq!(group.snapshots.len(), 3);
}

fn own_text() -> String {
    agent().as_uuid().hyphenated().to_string()
}

fn other_text() -> String {
    other_agent().as_uuid().hyphenated().to_string()
}

#[test]
fn two_plan_tags_that_disagree_orphan_the_snapshot_and_a_repeated_one_does_not() {
    let own = format!("{MACHINE_TAG_PREFIX}{}", own_text());
    let home = format!("{PLAN_TAG_PREFIX}home");
    let photos = format!("{PLAN_TAG_PREFIX}photos");
    let snapshots = vec![
        snapshot(1, &[home.clone(), own.clone(), home.clone(), own.clone()]),
        snapshot(2, &[home.clone(), photos.clone(), own.clone()]),
        snapshot(3, &[photos, home, own]),
    ];
    let partition = ForgetSelection::partition(
        &snapshots,
        &agent(),
        &plans(&[("home", home_policy()), ("photos", home_policy())]),
    );
    let [managed] = partition.managed.try_into().expect("one managed group");
    assert_eq!(managed.plan.as_str(), "home");
    assert_eq!(
        ids(&managed),
        ["0000000000000000000000000000000000000000000000000000000000000001"],
        "the same tag twice is no disagreement"
    );
    let [group] = partition.orphaned.try_into().expect("one orphaned group");
    assert_eq!(group.agent, Some(own_text()));
    assert_eq!(group.plan, None, "two plans name no one plan");
    assert_eq!(group.snapshots.len(), 2, "whichever plan tag comes first");
}

#[test]
fn two_agent_tags_that_disagree_orphan_the_snapshot_even_when_one_is_this_agents() {
    let own = format!("{MACHINE_TAG_PREFIX}{}", own_text());
    let other = format!("{MACHINE_TAG_PREFIX}{}", other_text());
    let home = format!("{PLAN_TAG_PREFIX}home");
    let snapshots = vec![
        snapshot(1, &[home.clone(), own.clone(), other.clone()]),
        snapshot(2, &[home, other, own]),
    ];
    let partition =
        ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
    assert!(partition.managed.is_empty(), "no plan's forget selects it");
    let [group] = partition.orphaned.try_into().expect("one orphaned group");
    assert_eq!(group.agent, None, "two agents name no one agent");
    assert_eq!(group.plan.as_deref(), Some("home"));
    assert_eq!(group.snapshots.len(), 2, "whichever agent tag comes first");
}

#[test]
fn owner_tags_that_differ_only_in_case_or_spacing_disagree() {
    let own = format!("{MACHINE_TAG_PREFIX}{}", own_text());
    let home = format!("{PLAN_TAG_PREFIX}home");
    let snapshots = vec![
        snapshot(
            1,
            &[home.clone(), format!("{PLAN_TAG_PREFIX}Home"), own.clone()],
        ),
        snapshot(
            2,
            &[home.clone(), format!("{PLAN_TAG_PREFIX}home "), own.clone()],
        ),
        snapshot(
            3,
            &[home, own, format!("{MACHINE_TAG_PREFIX} {}", own_text())],
        ),
    ];
    let partition =
        ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
    assert!(
        partition.managed.is_empty(),
        "a tag is compared exactly: {:?}",
        partition.managed
    );
    assert_eq!(partition.len(), 3, "each is orphaned instead");
    assert!(partition.unmanaged.is_empty());
}

#[test]
fn owner_tags_that_disagree_on_both_kinds_are_still_orphaned_not_unmanaged() {
    let snapshots = vec![snapshot(
        1,
        &[
            format!("{MACHINE_TAG_PREFIX}{}", own_text()),
            format!("{MACHINE_TAG_PREFIX}{}", other_text()),
            format!("{PLAN_TAG_PREFIX}home"),
            format!("{PLAN_TAG_PREFIX}photos"),
        ],
    )];
    let partition =
        ForgetSelection::partition(&snapshots, &agent(), &plans(&[("home", home_policy())]));
    assert!(partition.managed.is_empty());
    assert!(partition.unmanaged.is_empty(), "it carries owner tags");
    let [group] = partition.orphaned.try_into().expect("one orphaned group");
    assert_eq!((group.agent, group.plan), (None, None));
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
    let partition = ForgetSelection::partition(&[], &agent(), &plans(&[("home", home_policy())]));
    assert_eq!(partition.len(), 0);
    assert!(partition.is_empty());
}

/// The name the engine records on a snapshot when neither the plan nor
/// the agent names a host: this machine's own.
const MACHINE: &str = "demo-machine";

/// An agent whose `hostname` is `hostname`.
fn agent_named(hostname: &str) -> AgentSettings {
    AgentSettings {
        hostname: Some(crate::planning::Hostname::parse(hostname).expect("a hostname")),
        ..AgentSettings::default()
    }
}

/// The plans that claim `group`, as `claimants` tells it of one group,
/// on an agent whose `hostname` is `hostname`.
fn claimants_of(group: &UnmanagedGroup, hostname: &str, plans: &[Plan]) -> Vec<PlanId> {
    AdoptSelection::claimants(
        std::slice::from_ref(group),
        &agent_named(hostname),
        MACHINE,
        plans,
    )
    .remove(0)
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
    let group = AdoptSelection::unmanaged(
        &partition,
        &agent_named("demo-host"),
        MACHINE,
        &home,
        &plans,
    )
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
        AdoptSelection::unmanaged(
            &partition,
            &agent_named("demo-host"),
            MACHINE,
            &photos,
            &plans
        ),
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
        AdoptSelection::unmanaged(
            &partition,
            &agent_named("demo-host"),
            MACHINE,
            &home,
            &plans
        ),
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
        AdoptSelection::unmanaged(
            &partition,
            &agent_named("demo-host"),
            MACHINE,
            &home,
            &plans
        )
        .is_ok(),
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

fn differing(planned: &[PlanForget]) -> Vec<(PlanId, usize, Vec<SnapshotId>)> {
    planned
        .iter()
        .map(|one| (one.plan.clone(), one.total, removed_ids(one)))
        .collect()
}

#[test]
fn a_comparison_names_what_only_the_after_side_forgets_and_what_only_the_before_side_does() {
    let comparison = ForgetComparison {
        before: ForgetSide::Forgets(ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[4]), ("photos", &[5])]),
        )),
        after: ForgetSide::Forgets(ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[2, 3, 4]), ("photos", &[])]),
        )),
    };
    assert_eq!(
        differing(&comparison.forgets_more()),
        [(plan("home"), 4, vec![id(2), id(3)])],
        "only the plans whose removals grew, with only what they add"
    );
    assert_eq!(
        differing(&comparison.forgets_less()),
        [(plan("photos"), 1, vec![id(5)])]
    );
}

/// A side where no forget runs, or where one would be refused, removes
/// nothing.
#[test]
fn a_side_this_agent_does_not_maintain_or_would_refuse_forgets_nothing() {
    let preview = ForgetPreview::new(&busy_repository(), removals(&[("home", &[3, 4])]));
    for nothing in [
        ForgetSide::NotMaintained,
        ForgetSide::KeepsNothing { plan: plan("home") },
    ] {
        let taken_over = ForgetComparison {
            before: nothing.clone(),
            after: ForgetSide::Forgets(preview.clone()),
        };
        assert_eq!(
            differing(&taken_over.forgets_more()),
            [(plan("home"), 4, vec![id(3), id(4)])]
        );
        assert!(taken_over.forgets_less().is_empty());
        let given_up = ForgetComparison {
            before: ForgetSide::Forgets(preview.clone()),
            after: nothing,
        };
        assert!(given_up.forgets_more().is_empty());
        assert_eq!(
            differing(&given_up.forgets_less()),
            [(plan("home"), 4, vec![id(3), id(4)])]
        );
    }
}

#[test]
fn a_snapshot_forgotten_on_both_sides_is_no_difference() {
    let comparison = ForgetComparison {
        before: ForgetSide::Forgets(ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[4]), ("photos", &[5])]),
        )),
        after: ForgetSide::Forgets(ForgetPreview::new(
            &busy_repository(),
            removals(&[("home", &[4]), ("photos", &[5])]),
        )),
    };
    assert!(comparison.forgets_more().is_empty());
    assert!(comparison.forgets_less().is_empty());
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
    let mut labeled = full_plan("labelled", "/home/demo");
    labeled.label = Some("weekly".to_owned());
    let candidates = vec![full_plan("photos", "/srv/photos"), other_host, labeled];
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
            &agent_named("demo-host"),
            MACHINE,
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
    assert!(
        AdoptSelection::claimants(&[], &agent_named("demo-host"), MACHINE, &candidates).is_empty()
    );
}

#[test]
fn a_plan_naming_no_host_on_an_agent_naming_none_claims_by_the_machines_name() {
    let partition = ForgetSelection::partition(&[untagged(1)], &agent(), &plans(&[]));
    let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
    let mut home = full_plan("home", "/home/demo");
    home.host = None;
    let candidates = vec![home];
    let claimed = |machine: &str| {
        AdoptSelection::claimants(
            std::slice::from_ref(&group),
            &AgentSettings::default(),
            machine,
            &candidates,
        )
        .remove(0)
    };
    assert_eq!(claimed("demo-host"), [plan("home")]);
    assert_eq!(claimed("another-host"), Vec::<PlanId>::new());
}

#[test]
fn a_plans_own_host_wins_over_the_agents_hostname_and_the_machines_name() {
    let partition = ForgetSelection::partition(&[untagged(1)], &agent(), &plans(&[]));
    let [group] = partition.unmanaged.try_into().expect("one unmanaged group");
    let candidates = vec![full_plan("home", "/home/demo")];
    assert_eq!(
        AdoptSelection::claimants(
            std::slice::from_ref(&group),
            &agent_named("another-host"),
            "another-machine",
            &candidates,
        )
        .remove(0),
        [plan("home")]
    );
}

#[test]
fn only_a_repository_that_asks_for_this_agent_is_maintained_here() {
    assert!(Ownership::owned_here(Maintenance::Local));
    assert!(!Ownership::owned_here(Maintenance::None));
    assert!(!Ownership::owned_here(Maintenance::Hub));
}

#[test]
fn turning_maintenance_on_takes_over_a_repository_not_maintained_here() {
    assert_eq!(
        Ownership::change(Maintenance::None, true),
        OwnershipChange::TakeOver
    );
    assert_eq!(
        Ownership::change(Maintenance::Hub, true),
        OwnershipChange::TakeOver
    );
    assert_eq!(
        Ownership::change(Maintenance::Local, true),
        OwnershipChange::Unchanged
    );
}

#[test]
fn turning_maintenance_off_gives_up_only_a_repository_maintained_here() {
    assert_eq!(
        Ownership::change(Maintenance::Local, false),
        OwnershipChange::GiveUp
    );
    // A hub request is left as it is: this agent doesn't maintain it
    // either way.
    assert_eq!(
        Ownership::change(Maintenance::Hub, false),
        OwnershipChange::Unchanged
    );
    assert_eq!(
        Ownership::change(Maintenance::None, false),
        OwnershipChange::Unchanged
    );
}

#[test]
fn maintenance_is_given_up_only_while_none_of_it_runs_here() {
    assert_eq!(Ownership::give_up(None), Ok(Maintenance::None));
    let running = OperationId::from_uuid(crate::Uuid::from_u128(7));
    assert_eq!(Ownership::give_up(Some(running)), Err(running));
}
