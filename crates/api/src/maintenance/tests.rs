use serde_json::{from_str, json, to_string, to_value};

use crate::{
    AdoptionFromDto, ErrorCode, ForgetPlanDto, LeftAloneDto, MAX_GROUP_SNAPSHOTS,
    MAX_PREVIEW_SNAPSHOTS, MaintenancePreviewDto, MaintenanceRequestDto, OrphanedGroupDto,
    PlanGroupDto, SnapshotDto, SnapshotGroupsDto, TallyDto, UnmanagedGroupDto, WarningDto,
};

fn snapshot() -> SnapshotDto {
    SnapshotDto {
        id: "ab".repeat(32),
        time: "2026-11-09T10:15:00Z".to_owned(),
        offset: None,
        hostname: "demo-host".to_owned(),
        label: "home".to_owned(),
        tags: vec!["ladderback-plan:demo-home".to_owned()],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
        tags_truncated: false,
    }
}

fn previews() -> [MaintenancePreviewDto; 4] {
    [
        MaintenancePreviewDto::Forget {
            plans: vec![ForgetPlanDto {
                plan: "demo-home".to_owned(),
                total: 3,
                remove: 2,
                listed: vec![snapshot()],
            }],
            left_alone: LeftAloneDto {
                orphaned: TallyDto {
                    groups: 1,
                    snapshots: 4,
                },
                unmanaged: TallyDto::default(),
            },
            selection: "68497038cf038672".to_owned(),
            warnings: Vec::new(),
        },
        MaintenancePreviewDto::Adopt {
            into: "demo-home".to_owned(),
            count: 1,
            listed: vec![snapshot()],
            selection: "68497038cf038672".to_owned(),
            warnings: Vec::new(),
        },
        MaintenancePreviewDto::Prune {
            packs_marked: 2,
            packs_deleted: 1,
            packs_recovered: 0,
            bytes_deleted: 4096,
            keep_delete_hours: 144,
            warnings: Vec::new(),
        },
        MaintenancePreviewDto::Check { read_data: false },
    ]
}

#[test]
fn each_preview_names_its_kind_on_the_wire_and_reads_back() {
    for (preview, kind) in previews()
        .into_iter()
        .zip(["forget", "adopt", "prune", "check"])
    {
        let wire = to_value(&preview).unwrap();
        assert_eq!(wire["kind"], kind);
        assert_eq!(
            from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
            preview
        );
    }
}

#[test]
fn a_preview_carries_kebab_case_field_names() {
    let [forget, adopt, prune, check] = previews();
    let forget = to_value(&forget).unwrap();
    assert_eq!(
        forget["left-alone"],
        json!({
            "orphaned": {"groups": 1, "snapshots": 4},
            "unmanaged": {"groups": 0, "snapshots": 0},
        })
    );
    assert_eq!(forget["plans"][0]["remove"], 2);
    assert_eq!(forget["selection"], "68497038cf038672");
    let adopt = to_value(&adopt).unwrap();
    assert_eq!(adopt["into"], "demo-home");
    assert_eq!(adopt["count"], 1);
    assert_eq!(
        to_value(&prune).unwrap(),
        json!({
            "kind": "prune",
            "packs-marked": 2,
            "packs-deleted": 1,
            "packs-recovered": 0,
            "bytes-deleted": 4096,
            "keep-delete-hours": 144,
        })
    );
    assert_eq!(
        to_value(&check).unwrap(),
        json!({"kind": "check", "read-data": false})
    );
}

#[test]
fn a_preview_warns_of_what_it_is_given_and_a_check_of_nothing() {
    let warned = vec![WarningDto::other_hosts(1, &["demo-desktop".to_owned()])];
    let [forget, adopt, prune, check] = previews();
    for preview in [forget, adopt, prune] {
        assert!(preview.warnings().is_empty(), "{preview:?}");
        let preview = preview.warned(warned.clone());
        assert_eq!(preview.warnings(), warned.as_slice());
        let wire = to_value(&preview).unwrap();
        assert_eq!(
            wire["warnings"],
            json!([{
                "code": "maintenance-other-hosts",
                "args": {
                    "count": {"type": "count", "value": 1},
                    "hosts": {"type": "text", "value": "demo-desktop"},
                },
            }])
        );
        assert_eq!(
            from_str::<MaintenancePreviewDto>(&wire.to_string()).unwrap(),
            preview
        );
    }
    let check = check.warned(warned);
    assert!(check.warnings().is_empty(), "{check:?}");
    assert_eq!(
        to_value(&check).unwrap(),
        json!({"kind": "check", "read-data": false})
    );
}

#[test]
fn a_request_names_only_what_it_holds() {
    let bare = MaintenanceRequestDto {
        kind: "forget".to_owned(),
        into: None,
        from: None,
        selection: None,
    };
    assert_eq!(to_string(&bare).unwrap(), r#"{"kind":"forget"}"#);
    assert_eq!(
        from_str::<MaintenanceRequestDto>(r#"{"kind":"forget"}"#).unwrap(),
        bare
    );
}

#[test]
fn an_adoption_request_names_its_destination_source_and_selection() {
    for (from, wire) in [
        (
            AdoptionFromDto::Orphaned {
                agent: "agent-1".to_owned(),
                plan: "old-home".to_owned(),
            },
            json!({"from": "orphaned", "agent": "agent-1", "plan": "old-home"}),
        ),
        (AdoptionFromDto::Unmanaged, json!({"from": "unmanaged"})),
    ] {
        let request = MaintenanceRequestDto {
            kind: "adopt".to_owned(),
            into: Some("demo-home".to_owned()),
            from: Some(from),
            selection: Some("68497038cf038672".to_owned()),
        };
        assert_eq!(
            to_value(&request).unwrap(),
            json!({
                "kind": "adopt",
                "into": "demo-home",
                "from": wire,
                "selection": "68497038cf038672",
            })
        );
        assert_eq!(
            from_str::<MaintenanceRequestDto>(&to_string(&request).unwrap()).unwrap(),
            request
        );
    }
}

#[test]
fn a_source_that_is_no_kind_of_group_is_not_read() {
    assert!(from_str::<AdoptionFromDto>(r#"{"from":"somewhere"}"#).is_err());
    assert!(from_str::<AdoptionFromDto>(r#"{"agent":"a","plan":"p"}"#).is_err());
}

#[test]
fn a_source_with_a_key_its_kind_does_not_know_is_not_read() {
    for body in [
        r#"{"kind":"adopt","into":"demo-home","from":{"from":"unmanaged","agent":"a"}}"#,
        r#"{"kind":"adopt","into":"demo-home","from":{"from":"orphaned","agent":"a","plan":"p","extra":1}}"#,
    ] {
        assert!(from_str::<MaintenanceRequestDto>(body).is_err(), "{body}");
    }
}

#[test]
fn the_groups_carry_their_own_field_names_and_read_back() {
    let groups = SnapshotGroupsDto {
        plans: vec![PlanGroupDto {
            plan: "demo-home".to_owned(),
            count: 1,
            snapshots: vec![snapshot()],
        }],
        orphaned: vec![OrphanedGroupDto {
            agent: Some("agent-1".to_owned()),
            plan: None,
            count: 1,
            snapshots: vec![snapshot()],
            adopt_into: vec!["demo-home".to_owned()],
        }],
        unmanaged: vec![UnmanagedGroupDto {
            hostname: "demo-host".to_owned(),
            label: String::new(),
            paths: vec!["/home/demo".to_owned()],
            count: 1,
            snapshots: vec![snapshot()],
            adopt_into: vec![],
        }],
        warnings: Vec::new(),
    };
    let wire = to_value(&groups).unwrap();
    assert_eq!(wire["orphaned"][0]["adopt-into"], json!(["demo-home"]));
    assert!(wire["orphaned"][0]["plan"].is_null());
    assert_eq!(wire["unmanaged"][0]["adopt-into"], json!([]));
    assert_eq!(wire["plans"][0]["count"], 1);
    assert_eq!(
        from_str::<SnapshotGroupsDto>(&wire.to_string()).unwrap(),
        groups
    );
}

#[test]
fn the_codes_of_a_changed_selection_and_a_slow_read_take_no_arguments() {
    for (code, wire) in [
        (
            ErrorCode::MaintenanceSelectionChanged,
            r#""maintenance-selection-changed""#,
        ),
        (
            ErrorCode::RepositoryReadTimedOut,
            r#""repository-read-timed-out""#,
        ),
    ] {
        assert_eq!(to_string(&code).unwrap(), wire);
        assert_eq!(from_str::<ErrorCode>(wire).unwrap(), code);
        assert!(code.args().is_empty());
    }
}

/// Snapshot `n` as the wire would show it: its ID is `n`.
fn numbered(n: &u32) -> Option<SnapshotDto> {
    Some(SnapshotDto {
        id: format!("{n:064x}"),
        ..snapshot()
    })
}

fn ids(list: &[SnapshotDto]) -> Vec<String> {
    list.iter().map(|snapshot| snapshot.id.clone()).collect()
}

fn first(count: u32) -> Vec<String> {
    (0..count).map(|n| format!("{n:064x}")).collect()
}

/// More snapshots than any list shows.
fn many() -> Vec<u32> {
    (0..=u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap()).collect()
}

#[test]
fn a_group_lists_its_first_snapshots_and_counts_them_all() {
    let items = many();
    let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();

    let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
    assert_eq!((plan.plan.as_str(), plan.count), ("demo-home", 501));
    assert_eq!(ids(&plan.snapshots), first(cap));

    let orphaned = OrphanedGroupDto::new(
        Some("agent-1".to_owned()),
        None,
        &items,
        vec!["demo-home".to_owned()],
        numbered,
    );
    assert_eq!(orphaned.agent.as_deref(), Some("agent-1"));
    assert_eq!(orphaned.plan, None);
    assert_eq!(orphaned.adopt_into, ["demo-home"]);
    assert_eq!(orphaned.count, 501);
    assert_eq!(ids(&orphaned.snapshots), first(cap));

    let unmanaged = UnmanagedGroupDto::new(
        "demo-host".to_owned(),
        "home".to_owned(),
        vec!["/home/demo".to_owned()],
        &items,
        Vec::new(),
        numbered,
    );
    assert_eq!(unmanaged.hostname, "demo-host");
    assert_eq!(unmanaged.label, "home");
    assert_eq!(unmanaged.paths, ["/home/demo"]);
    assert!(unmanaged.adopt_into.is_empty());
    assert_eq!(unmanaged.count, 501);
    assert_eq!(ids(&unmanaged.snapshots), first(cap));
}

#[test]
fn a_group_that_fits_lists_every_snapshot() {
    let cap = u32::try_from(MAX_GROUP_SNAPSHOTS).unwrap();
    for held in [0, 3, cap] {
        let items: Vec<u32> = (0..held).collect();
        let plan = PlanGroupDto::new("demo-home".to_owned(), &items, numbered);
        assert_eq!(plan.count, u64::from(held));
        assert_eq!(ids(&plan.snapshots), first(held));
    }
}

#[test]
fn a_preview_lists_fewer_snapshots_than_a_group_does() {
    let items = many();
    let cap = u32::try_from(MAX_PREVIEW_SNAPSHOTS).unwrap();

    let forget = ForgetPlanDto::new("demo-home".to_owned(), 900, &items, numbered);
    assert_eq!(forget.plan, "demo-home");
    assert_eq!((forget.total, forget.remove), (900, 501));
    assert_eq!(ids(&forget.listed), first(cap));

    let adopt = MaintenancePreviewDto::adopt(
        "demo-home".to_owned(),
        &items,
        "68497038cf038672".to_owned(),
        numbered,
    );
    let MaintenancePreviewDto::Adopt {
        into,
        count,
        listed,
        selection,
        ..
    } = adopt
    else {
        panic!("an adoption preview");
    };
    assert_eq!(
        (into.as_str(), count, selection.as_str()),
        ("demo-home", 501, "68497038cf038672")
    );
    assert_eq!(ids(&listed), first(cap));

    let few: Vec<u32> = (0..3).collect();
    let forget = ForgetPlanDto::new("demo-home".to_owned(), 3, &few, numbered);
    assert_eq!((forget.remove, ids(&forget.listed)), (3, first(3)));
}

#[test]
fn only_the_snapshots_a_list_shows_are_built() {
    let items = many();
    let mut built = 0;
    let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
        built += 1;
        numbered(n)
    });
    assert_eq!(built, MAX_GROUP_SNAPSHOTS);
    assert_eq!(group.count, 501);
}

#[test]
fn a_snapshot_the_wire_cannot_carry_is_left_out_and_still_counted() {
    let items: Vec<u32> = (0..3).collect();
    let group = PlanGroupDto::new("demo-home".to_owned(), &items, |n| {
        (*n != 1).then(|| numbered(n)).flatten()
    });
    assert_eq!(group.count, 3);
    assert_eq!(
        ids(&group.snapshots),
        [first(1), vec![format!("{:064x}", 2)]].concat()
    );
}

#[test]
fn a_request_with_a_key_it_does_not_know_is_not_read() {
    assert!(
        from_str::<MaintenanceRequestDto>(r#"{"kind":"forget","selectoin":"68497038cf038672"}"#)
            .is_err()
    );
}
