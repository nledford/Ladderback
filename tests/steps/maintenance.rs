//! Steps for `features/maintenance/api.feature`: the real `ladderback serve`
//! process, asked what the Snapshots page asks it: a repository's history
//! grouped by owner, and a preview of each maintenance run before it starts.
//! The repositories are made and inspected with `rustic_core` itself, never
//! through Ladderback, so the checks don't trust the code under test.
//!
//! The daemon owns the plan the serving step names; history the test makes
//! in process (`the plan has been backed up`) carries the test's own agent
//! tag, which the daemon doesn't own, so it is orphaned from the daemon's
//! view. The daemon reads its configuration on every request, so a step may
//! rewrite it between requests.

use std::collections::BTreeSet;
use std::path::Path;

use api::{
    MaintenancePreviewDto, OperationDetailDto, OperationResultDto, SnapshotDto, SnapshotGroupsDto,
    WarningCode,
};
use cucumber::{given, then, when};

use super::events_steps::save_daemon_config;
use super::forget_steps::snapshots_of;
use super::operations_steps::{exchange_at, finished_backups, wait_ended};
use super::repositories_steps::api_post;
use super::{Agent, blocking};

/// The alias the serving step gives the scenario's repository.
const ALIAS: &str = "demo";

/// The ID the stray pack is given: one no index lists.
const STRAY_PACK: &str = "5ca1ab1e5ca1ab1e5ca1ab1e5ca1ab1e5ca1ab1e5ca1ab1e5ca1ab1e5ca1ab1e";

/// How rustic names that pack in its words: by the first 8 hex digits of
/// its ID, as its own `check` prints it.
const STRAY_PACK_NAMED: &str = "5ca1ab1e";

/// What the maintenance scenarios keep between steps.
#[derive(Default)]
pub struct MaintenanceE2e {
    /// How many times the plan's folder has been changed: each change is
    /// unique, so each backup is a snapshot of its own contents.
    changes: usize,
    /// The preview a start named the selection of.
    previewed: Option<MaintenancePreviewDto>,
    /// Every snapshot ID the repository held when the start was sent.
    before: Vec<String>,
    /// The operation a start was accepted as.
    started: Option<String>,
    /// The names of the files under the repository's `index` and `data`
    /// folders, as noted.
    noted: Option<BTreeSet<String>>,
}

/// Asks the daemon, remembering the answer as the last one. The wait is on a
/// blocking thread: see [`blocking`].
async fn api_get(agent: &mut Agent, path: &str) {
    let addr = agent.events.addr();
    let path = path.to_owned();
    let answer = blocking(move || exchange_at(addr, "GET", &path, "")).await;
    agent.repositories.api_last = Some(answer);
}

/// The body of the last answer, which must be a 200.
fn ok_body(agent: &Agent, what: &str) -> String {
    let (status, body) = agent
        .repositories
        .api_last
        .clone()
        .expect("the owner has asked for something");
    assert_eq!(status, 200, "{what} was not answered: {body}");
    body
}

fn groups_answer(agent: &Agent) -> SnapshotGroupsDto {
    let body = ok_body(agent, "the groups");
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("the groups are the documented JSON: {error}: {body}"))
}

fn preview_answer(agent: &Agent) -> MaintenancePreviewDto {
    let body = ok_body(agent, "the preview");
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("the preview is the documented JSON: {error}: {body}"))
}

fn ids(list: &[SnapshotDto]) -> Vec<String> {
    list.iter().map(|snapshot| snapshot.id.clone()).collect()
}

/// The tag text a snapshot of the test's own agent carries after
/// `ladderback-agent:`.
fn test_agent_text(agent: &Agent) -> String {
    agent.agent_id.as_uuid().hyphenated().to_string()
}

/// Every snapshot ID the repository holds, newest first, read with rustic
/// itself.
async fn repository_ids_newest_first(agent: &Agent) -> Vec<String> {
    snapshots_of(agent)
        .await
        .into_iter()
        .rev()
        .map(|(id, _, _)| id.as_str().to_owned())
        .collect()
}

/// Is `selection` what a preview names: 16 lower-case hex digits.
fn is_selection_token(selection: &str) -> bool {
    selection.len() == 16
        && selection
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The selection a forget or an adoption preview named.
fn selection_of(preview: &MaintenancePreviewDto) -> String {
    match preview {
        MaintenancePreviewDto::Forget { selection, .. }
        | MaintenancePreviewDto::Adopt { selection, .. } => selection.clone(),
        other => panic!("only a forget or an adoption preview names a selection: {other:?}"),
    }
}

/// Changes every file of the plan's folder to contents it never had.
fn change_source(agent: &mut Agent) {
    agent.maintenance.changes += 1;
    let n = agent.maintenance.changes;
    let source = agent.backup.source.clone().expect("a plan source");
    for entry in std::fs::read_dir(&source).expect("the source folder") {
        let path = entry.expect("a file").path();
        std::fs::write(&path, format!("changed {n} of {}\n", path.display()))
            .expect("the file changes");
    }
}

/// The names of the files under `folder`, below `prefix`.
fn walk(folder: &Path, prefix: &str, names: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a repository entry").path();
        let name = format!(
            "{prefix}/{}",
            path.file_name().expect("a name").to_string_lossy()
        );
        if path.is_dir() {
            walk(&path, &name, names);
        } else {
            names.insert(name);
        }
    }
}

/// The names of the files under the repository's `index` and `data` folders.
fn repository_files(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for folder in ["index", "data"] {
        walk(&root.join(folder), folder, &mut names);
    }
    names
}

/// Posts a run to the daemon's start route, remembering the operation when it
/// was accepted.
async fn start_run(agent: &mut Agent, body: &str) {
    api_post(
        agent,
        &format!("/api/v1/repositories/{ALIAS}/maintenance"),
        body,
    )
    .await;
    let (status, answer) = agent
        .repositories
        .api_last
        .clone()
        .expect("the start was answered");
    agent.maintenance.started = (status == 202)
        .then(|| serde_json::from_str::<serde_json::Value>(&answer).ok())
        .flatten()
        .and_then(|json| {
            json.get("operation")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
}

/// The status of the daemon's operation `id`.
async fn operation_status(agent: &Agent, id: &str) -> String {
    let addr = agent.events.addr();
    let path = format!("/api/v1/operations/{id}");
    let (status, body) = blocking(move || exchange_at(addr, "GET", &path, "")).await;
    assert_eq!(status, 200, "the operation is known: {body}");
    serde_json::from_str::<serde_json::Value>(&body)
        .unwrap_or_else(|error| panic!("the operation is JSON: {error}: {body}"))["operation"]
        ["status"]
        .as_str()
        .expect("the operation has a status")
        .to_owned()
}

/// The daemon's operation `id`, as its detail route answers.
async fn operation_detail(agent: &Agent, id: &str) -> OperationDetailDto {
    let addr = agent.events.addr();
    let path = format!("/api/v1/operations/{id}");
    let (status, body) = blocking(move || exchange_at(addr, "GET", &path, "")).await;
    assert_eq!(status, 200, "the operation is known: {body}");
    serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("the operation is the documented JSON: {error}: {body}"))
}

/// The operation the last start was accepted as.
fn started(agent: &Agent) -> String {
    agent
        .maintenance
        .started
        .clone()
        .expect("the start named its operation")
}

#[given(expr = "the daemon holds {int} snapshot(s) of the plan {string}")]
async fn daemon_holds(agent: &mut Agent, count: usize, plan: String) {
    // The folder changes before each backup, so no two snapshots share
    // contents.
    for _ in 0..count {
        change_source(agent);
        finished_backups(agent, 1, plan.clone()).await;
    }
}

#[given(expr = "the daemon's plan keeps the last {int} snapshot(s)")]
async fn plan_keeps_last(agent: &mut Agent, keep: i64) {
    save_daemon_config(agent, Some(keep)).await;
}

#[given("the daemon's plan keeps nothing")]
async fn plan_keeps_nothing(agent: &mut Agent) {
    save_daemon_config(agent, Some(0)).await;
}

#[given(expr = "the daemon has forgotten all but the last {int} snapshot(s)")]
async fn daemon_forgot(agent: &mut Agent, keep: i64) {
    save_daemon_config(agent, Some(keep)).await;
    start_run(agent, r#"{"kind":"forget"}"#).await;
    let id = agent
        .maintenance
        .started
        .clone()
        .expect("the daemon accepted the forget");
    wait_ended(agent, &id).await;
    assert_eq!(operation_status(agent, &id).await, "succeeded");
}

/// A backup killed before it saved its index has written packs that no index
/// lists. A copy of a pack of the repository, under an ID of its own, is one.
#[given(
    "a backup killed before it saved its index left a pack in the repository that no index lists"
)]
async fn killed_backup_left_a_pack(agent: &mut Agent) {
    let root = agent.backup.repository.clone().expect("a repository");
    blocking(move || {
        let mut names = BTreeSet::new();
        walk(&root.join("data"), "data", &mut names);
        let pack = names.first().expect("the repository holds a pack to copy");
        let folder = root.join("data").join(&STRAY_PACK[..2]);
        std::fs::create_dir_all(&folder).expect("the pack's folder");
        std::fs::copy(root.join(pack), folder.join(STRAY_PACK)).expect("the pack is copied");
    })
    .await;
}

#[given("the names of the files in the repository's index and data folders are noted")]
async fn notes_files(agent: &mut Agent) {
    let root = agent.backup.repository.clone().expect("a repository");
    agent.maintenance.noted = Some(blocking(move || repository_files(&root)).await);
}

#[when("the owner asks for the repository's groups")]
async fn asks_for_groups(agent: &mut Agent) {
    api_get(agent, &format!("/api/v1/repositories/{ALIAS}/groups")).await;
}

#[then(expr = "the answer warns of snapshots from the other host {string}")]
fn answer_warns_of_other_host(agent: &mut Agent, host: String) {
    let (status, json) = super::repositories_steps::api_answer(agent);
    assert_eq!(status, 200, "the answer: {json}");
    assert_eq!(
        json["warnings"],
        serde_json::json!([{
            "code": "maintenance-other-hosts",
            "args": {
                "count": {"type": "count", "value": 1},
                "hosts": {"type": "text", "value": host},
            },
        }]),
        "the answer: {json}"
    );
}

#[when(expr = "the owner asks for the groups of the repository {string}")]
async fn asks_for_groups_of(agent: &mut Agent, alias: String) {
    api_get(agent, &format!("/api/v1/repositories/{alias}/groups")).await;
}

async fn preview(agent: &mut Agent, alias: &str, body: &str) {
    api_post(
        agent,
        &format!("/api/v1/repositories/{alias}/maintenance/preview"),
        body,
    )
    .await;
}

#[when("the owner previews forgetting")]
async fn previews_forgetting(agent: &mut Agent) {
    preview(agent, ALIAS, r#"{"kind":"forget"}"#).await;
}

#[when("the owner previews pruning")]
async fn previews_pruning(agent: &mut Agent) {
    preview(agent, ALIAS, r#"{"kind":"prune"}"#).await;
}

#[when("the owner previews a check")]
async fn previews_a_check(agent: &mut Agent) {
    preview(agent, ALIAS, r#"{"kind":"check"}"#).await;
}

#[when(expr = "the owner previews adopting orphaned history into the plan {string}")]
async fn previews_adopting_orphaned(agent: &mut Agent, into: String) {
    let from_plan = agent.backup.plan.clone().expect("a plan");
    let body = serde_json::json!({
        "kind": "adopt",
        "into": into,
        "from": {
            "from": "orphaned",
            "agent": test_agent_text(agent),
            "plan": from_plan.as_str(),
        },
    })
    .to_string();
    preview(agent, ALIAS, &body).await;
}

#[when(expr = "the owner previews adopting unmanaged history into the plan {string}")]
async fn previews_adopting_unmanaged(agent: &mut Agent, into: String) {
    let body = serde_json::json!({
        "kind": "adopt",
        "into": into,
        "from": {"from": "unmanaged"},
    })
    .to_string();
    preview(agent, ALIAS, &body).await;
}

#[when(
    regex = r#"^the owner previews maintenance of the repository "([^"]+)" with the request (.+)$"#
)]
async fn previews_raw(agent: &mut Agent, alias: String, body: String) {
    preview(agent, &alias, &body).await;
}

#[when(
    regex = r#"^the owner starts maintenance of the repository "([^"]+)" with the request (.+)$"#
)]
async fn starts_raw(agent: &mut Agent, alias: String, body: String) {
    api_post(
        agent,
        &format!("/api/v1/repositories/{alias}/maintenance"),
        &body,
    )
    .await;
}

#[when("the owner checks the repository and the check ends")]
async fn checks_until_the_end(agent: &mut Agent) {
    start_run(agent, r#"{"kind":"check"}"#).await;
    wait_ended(agent, &started(agent)).await;
}

#[when(expr = "the daemon makes another snapshot of the plan {string}")]
async fn daemon_makes_another(agent: &mut Agent, plan: String) {
    change_source(agent);
    finished_backups(agent, 1, plan).await;
}

#[when("the owner confirms the forget with the preview's selection")]
async fn confirms_the_forget(agent: &mut Agent) {
    let preview = preview_answer(agent);
    let selection = selection_of(&preview);
    agent.maintenance.previewed = Some(preview);
    agent.maintenance.before = repository_ids_newest_first(agent).await;
    let body = serde_json::json!({"kind": "forget", "selection": selection}).to_string();
    start_run(agent, &body).await;
}

#[then(expr = "the groups list the plan {string} with {int} snapshot(s) newest first")]
async fn groups_list_plan(agent: &mut Agent, plan: String, count: u64) {
    let groups = groups_answer(agent);
    let group = groups
        .plans
        .iter()
        .find(|group| group.plan == plan)
        .unwrap_or_else(|| panic!("the plan {plan} is listed: {groups:?}"));
    assert_eq!(group.count, count, "how many snapshots the plan owns");
    assert_eq!(
        ids(&group.snapshots),
        repository_ids_newest_first(agent).await,
        "the plan's snapshots, newest first"
    );
}

#[then(expr = "the groups list the plan {string} with no snapshots")]
fn groups_list_plan_empty(agent: &mut Agent, plan: String) {
    let groups = groups_answer(agent);
    let group = groups
        .plans
        .iter()
        .find(|group| group.plan == plan)
        .unwrap_or_else(|| panic!("the plan {plan} is listed: {groups:?}"));
    assert_eq!(group.count, 0);
    assert!(group.snapshots.is_empty(), "{group:?}");
}

#[then("the groups name no orphaned and no unmanaged history")]
fn groups_name_no_other_history(agent: &mut Agent) {
    let groups = groups_answer(agent);
    assert!(groups.orphaned.is_empty(), "{:?}", groups.orphaned);
    assert!(groups.unmanaged.is_empty(), "{:?}", groups.unmanaged);
}

#[then(
    expr = "the groups name one orphaned group of {int} snapshot(s), from the agent that made them and the plan {string}, adoptable into the plan {string}"
)]
fn groups_name_orphaned(agent: &mut Agent, count: u64, plan: String, into: String) {
    let groups = groups_answer(agent);
    let [group] = groups.orphaned.as_slice() else {
        panic!("one orphaned group: {:?}", groups.orphaned);
    };
    assert_eq!(
        group.agent.as_deref(),
        Some(test_agent_text(agent).as_str())
    );
    assert_eq!(group.plan.as_deref(), Some(plan.as_str()));
    assert_eq!(group.count, count);
    assert_eq!(group.snapshots.len() as u64, count);
    assert_eq!(group.adopt_into, vec![into]);
}

#[then(
    expr = "the groups name one unmanaged group of {int} snapshot(s), adoptable into the plan {string}"
)]
fn groups_name_unmanaged(agent: &mut Agent, count: u64, into: String) {
    let groups = groups_answer(agent);
    let [group] = groups.unmanaged.as_slice() else {
        panic!("one unmanaged group: {:?}", groups.unmanaged);
    };
    assert_eq!(group.count, count);
    assert_eq!(group.snapshots.len() as u64, count);
    assert_eq!(group.adopt_into, vec![into]);
}

#[then(
    expr = "the forget preview names {int} of the {int} snapshots of the plan {string} for removal, the oldest ones"
)]
async fn forget_preview_names(agent: &mut Agent, remove: u64, total: u64, plan: String) {
    let MaintenancePreviewDto::Forget { plans, .. } = preview_answer(agent) else {
        panic!("the preview is a forget preview");
    };
    let group = plans
        .iter()
        .find(|group| group.plan == plan)
        .unwrap_or_else(|| panic!("the plan {plan} is previewed: {plans:?}"));
    assert_eq!(group.total, total, "how many snapshots the plan owns");
    assert_eq!(group.remove, remove, "how many its policy lets go");
    let keeps = usize::try_from(total - remove).expect("a small count");
    let oldest: Vec<String> = repository_ids_newest_first(agent)
        .await
        .into_iter()
        .skip(keeps)
        .collect();
    assert_eq!(
        ids(&group.listed),
        oldest,
        "the snapshots going, newest first"
    );
}

#[then("the preview names a selection of 16 lower-case hex digits")]
fn preview_names_a_selection(agent: &mut Agent) {
    let selection = selection_of(&preview_answer(agent));
    assert!(is_selection_token(&selection), "the selection: {selection}");
}

#[then(
    expr = "the forget preview leaves alone {int} orphaned group(s) of {int} snapshot(s) and {int} unmanaged group(s) of {int} snapshot(s)"
)]
fn forget_preview_leaves_alone(
    agent: &mut Agent,
    orphaned_groups: u64,
    orphaned_snapshots: u64,
    unmanaged_groups: u64,
    unmanaged_snapshots: u64,
) {
    let MaintenancePreviewDto::Forget { left_alone, .. } = preview_answer(agent) else {
        panic!("the preview is a forget preview");
    };
    assert_eq!(left_alone.orphaned.groups, orphaned_groups);
    assert_eq!(left_alone.orphaned.snapshots, orphaned_snapshots);
    assert_eq!(left_alone.unmanaged.groups, unmanaged_groups);
    assert_eq!(left_alone.unmanaged.snapshots, unmanaged_snapshots);
}

#[then(expr = "the adoption preview names {int} snapshot(s) to rewrite into the plan {string}")]
async fn adoption_preview_names(agent: &mut Agent, count: u64, into: String) {
    let MaintenancePreviewDto::Adopt {
        into: named,
        count: rewritten,
        listed,
        ..
    } = preview_answer(agent)
    else {
        panic!("the preview is an adoption preview");
    };
    assert_eq!(named, into);
    assert_eq!(rewritten, count);
    assert_eq!(
        ids(&listed),
        repository_ids_newest_first(agent).await,
        "the snapshots it would rewrite, newest first"
    );
}

#[then("every snapshot in the repository still carries the tag of the agent that made it")]
async fn snapshots_keep_their_agent_tag(agent: &mut Agent) {
    let tag = format!("ladderback-agent:{}", test_agent_text(agent));
    let snapshots = snapshots_of(agent).await;
    assert!(!snapshots.is_empty(), "the repository holds history");
    for (id, _, tags) in snapshots {
        assert!(
            tags.contains(&tag),
            "{} lost its agent tag: {tags:?}",
            id.as_str()
        );
    }
}

#[then(expr = "the prune preview marks at least {int} pack(s)")]
fn prune_preview_marks(agent: &mut Agent, least: u64) {
    let MaintenancePreviewDto::Prune { packs_marked, .. } = preview_answer(agent) else {
        panic!("the preview is a prune preview");
    };
    assert!(
        packs_marked >= least,
        "the prune would mark {packs_marked} packs"
    );
}

#[then(expr = "the prune preview waits {int} hours before anything marked is deleted")]
fn prune_preview_waits(agent: &mut Agent, hours: u64) {
    let MaintenancePreviewDto::Prune {
        keep_delete_hours, ..
    } = preview_answer(agent)
    else {
        panic!("the preview is a prune preview");
    };
    assert_eq!(keep_delete_hours, hours);
}

#[then("the repository's index and data files are as they were noted")]
async fn files_are_as_noted(agent: &mut Agent) {
    let noted = agent
        .maintenance
        .noted
        .clone()
        .expect("the files were noted");
    let root = agent.backup.repository.clone().expect("a repository");
    let now = blocking(move || repository_files(&root)).await;
    assert_eq!(now, noted, "the repository's files changed");
}

#[then("the check preview reads the repository's structure only")]
fn check_preview_reads_structure_only(agent: &mut Agent) {
    let MaintenancePreviewDto::Check { read_data } = preview_answer(agent) else {
        panic!("the preview is a check preview");
    };
    assert!(!read_data, "a check preview reads no pack data");
}

#[then(
    expr = "the check ended as succeeded with warnings, counting {int} warning(s) and no errors"
)]
async fn check_ended_with_warnings(agent: &mut Agent, warnings: u64) {
    let detail = operation_detail(agent, &started(agent)).await;
    assert_eq!(detail.operation.status, "succeeded-with-warnings");
    assert!(
        matches!(
            detail.operation.result,
            Some(OperationResultDto::Check { errors: 0, warnings: counted, .. }) if counted == warnings
        ),
        "the check's result: {:?}",
        detail.operation.result
    );
}

#[then("the check's warnings name the pack no index lists, in rustic's words")]
async fn check_warnings_name_the_pack(agent: &mut Agent) {
    let detail = operation_detail(agent, &started(agent)).await;
    let naming: Vec<_> = detail
        .warnings
        .iter()
        .filter(|warning| warning.detail.contains(STRAY_PACK_NAMED))
        .collect();
    assert_eq!(
        naming.len(),
        1,
        "the check's warnings: {:?}",
        detail.warnings
    );
    assert_eq!(naming[0].code, WarningCode::CheckWarning);
    assert_eq!(naming[0].during, "check");
    assert!(
        naming[0].detail.contains("not referenced in index"),
        "rustic's own words: {}",
        naming[0].detail
    );
}

#[then("the operation log holds no maintenance operation")]
async fn log_holds_no_maintenance(agent: &mut Agent) {
    api_get(agent, "/api/v1/operations?limit=50").await;
    let body = ok_body(agent, "the operation log");
    let page: api::OperationsPageDto = serde_json::from_str(&body)
        .unwrap_or_else(|error| panic!("the log is the documented JSON: {error}: {body}"));
    let kinds: Vec<String> = page
        .operations
        .iter()
        .map(|operation| operation.kind.clone())
        .filter(|kind| kind != "backup")
        .collect();
    assert!(kinds.is_empty(), "the log holds {kinds:?}");
}

#[then("the confirmed forget runs to a success")]
async fn confirmed_forget_succeeds(agent: &mut Agent) {
    let (status, body) = agent
        .repositories
        .api_last
        .clone()
        .expect("the start was answered");
    assert_eq!(status, 202, "the start was accepted: {body}");
    let id = agent
        .maintenance
        .started
        .clone()
        .expect("the start named its operation");
    wait_ended(agent, &id).await;
    assert_eq!(operation_status(agent, &id).await, "succeeded");
}

#[then("the forget removed exactly the snapshots the preview listed and kept the newest")]
async fn forget_removed_what_was_listed(agent: &mut Agent) {
    let Some(MaintenancePreviewDto::Forget { plans, .. }) = agent.maintenance.previewed.clone()
    else {
        panic!("a forget preview was confirmed");
    };
    let listed: BTreeSet<String> = plans.iter().flat_map(|plan| ids(&plan.listed)).collect();
    let before = agent.maintenance.before.clone();
    let after: BTreeSet<String> = repository_ids_newest_first(agent)
        .await
        .into_iter()
        .collect();
    let removed: BTreeSet<String> = before
        .iter()
        .filter(|id| !after.contains(*id))
        .cloned()
        .collect();
    assert_eq!(removed, listed, "the removed snapshots are the listed ones");
    let newest = before.first().expect("the repository held snapshots");
    assert!(after.contains(newest), "the newest snapshot remains");
}
