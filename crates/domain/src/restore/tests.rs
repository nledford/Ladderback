use jiff::civil::date;

use super::*;

fn snapshot() -> SnapshotId {
    SnapshotId::parse(&format!("3f2a1b4c{}", "0".repeat(56))).expect("a snapshot ID")
}

fn entry() -> FolderPath {
    FolderPath::parse("home/docs").expect("a path")
}

fn admit(
    policy: RestorePolicy,
    destination: Destination,
    origin: RestoreOrigin,
) -> Result<RestoreRequest, RestoreRefused> {
    RestoreRequest::admit(policy, snapshot(), entry(), destination, origin)
}

const HUB_ALLOWED: RestorePolicy = RestorePolicy {
    allow_hub_restore: true,
};

#[test]
fn the_agents_own_ui_restores_into_a_new_folder_or_in_place() {
    for destination in [Destination::NewFolder, Destination::InPlace] {
        let request = admit(RestorePolicy::default(), destination, RestoreOrigin::LocalUi)
            .expect("the agent's own UI may restore");
        assert_eq!(request.destination(), destination);
        assert_eq!(request.origin(), RestoreOrigin::LocalUi);
        assert_eq!(request.snapshot(), &snapshot());
        assert_eq!(request.entry(), &entry());
    }
}

#[test]
fn the_hub_never_restores_in_place_even_where_hub_restores_are_allowed() {
    assert_eq!(
        admit(HUB_ALLOWED, Destination::InPlace, RestoreOrigin::Hub),
        Err(RestoreRefused::InPlaceFromHub)
    );
    assert_eq!(
        admit(
            RestorePolicy::default(),
            Destination::InPlace,
            RestoreOrigin::Hub
        ),
        Err(RestoreRefused::InPlaceFromHub)
    );
}

#[test]
fn the_hub_restores_into_a_new_folder_only_where_the_agent_allows_it() {
    assert_eq!(
        admit(
            RestorePolicy::default(),
            Destination::NewFolder,
            RestoreOrigin::Hub
        ),
        Err(RestoreRefused::HubNotAllowed)
    );
    let request = admit(HUB_ALLOWED, Destination::NewFolder, RestoreOrigin::Hub)
        .expect("an agent that allows hub restores takes one into a new folder");
    assert_eq!(request.origin(), RestoreOrigin::Hub);
}

fn name(host: &str) -> String {
    restore_folder_name(host, &snapshot(), date(2027, 1, 15).at(8, 0, 0, 0))
}

#[test]
fn a_restore_folder_is_named_for_the_host_the_short_id_and_the_time() {
    assert_eq!(name("laptop"), "laptop-3f2a1b4c-20270115-080000");
    assert_eq!(name("nas.lan"), "nas.lan-3f2a1b4c-20270115-080000");
    assert_eq!(name("café_mac"), "café_mac-3f2a1b4c-20270115-080000");
}

#[test]
fn a_host_that_isnt_a_safe_folder_name_is_made_one() {
    assert_eq!(name("../evil/host"), "_evil_host-3f2a1b4c-20270115-080000");
    assert_eq!(name(".hidden"), "hidden-3f2a1b4c-20270115-080000");
    assert_eq!(name("a\\b c"), "a_b_c-3f2a1b4c-20270115-080000");
    assert_eq!(name("abc\u{202e}txt"), "abc_txt-3f2a1b4c-20270115-080000");
    assert_eq!(name("new\nline"), "new_line-3f2a1b4c-20270115-080000");
}

#[test]
fn a_host_that_leaves_no_name_is_called_host() {
    assert_eq!(name(""), "host-3f2a1b4c-20270115-080000");
    assert_eq!(name(".."), "host-3f2a1b4c-20270115-080000");
}

#[test]
fn a_long_host_is_cut_at_64_characters() {
    let long = "é".repeat(100);
    assert_eq!(
        name(&long),
        format!("{}-3f2a1b4c-20270115-080000", "é".repeat(64))
    );
}
