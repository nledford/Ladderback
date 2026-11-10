//! Steps for `features/maintenance/preview.feature`: they drive the
//! maintenance use case's previews, confirmed runs and group listing against
//! the scripted engine the forget scenarios use, which records every job it
//! was given.

use std::sync::Arc;
use std::time::Duration;

use cucumber::{given, then, when};
use domain::maintenance::{ForgetPreview, LeftAlone, PlanForget};
use domain::planning::{KeepCount, Plan, Source};
use domain::{AgentId, OperationId, PlanId, RepositoryAlias, SnapshotId, Uuid};

use super::Agent;
use app::maintenance::{
    AdoptPreview, Adoption, AdoptionSource, MaintenanceError, MaintenanceKind, MaintenancePreview,
    PlanHistory, PrunePreview, RepositoryGroups, default_keep_delete,
};
use app::ports::{HistoryQuery, OperationStore};

/// The alias every maintenance scenario's repository goes under.
fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

fn other_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_00aa))
}

fn self_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0001))
}

fn operation_id(low: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_0000 | low,
    ))
}

/// The operation a preview or a groups listing runs its reads under: the
/// log never holds it.
fn preview_operation() -> OperationId {
    operation_id(0xdd)
}

/// What the preview scenarios were told.
#[derive(Default)]
pub struct PreviewWorld {
    /// What the last preview answered.
    previewed: Option<MaintenancePreview>,
    /// What the last groups listing answered.
    groups: Option<RepositoryGroups>,
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s) of the folder {string}")]
async fn plan_of_folder(agent: &mut Agent, plan: String, keep: i64, folder: String) {
    agent.forget.repository().await;
    let mut draft = Plan::new(
        PlanId::parse(&plan).expect("a plan ID"),
        alias(),
        vec![Source::parse(&folder).expect("a source")],
    );
    draft.retention.keep_last = Some(KeepCount::new(keep).expect("a count"));
    agent.forget.plan_raw(draft).await;
}

#[when(expr = "the repository gains {int} snapshot(s) of the plan {string}")]
async fn gains_own(agent: &mut Agent, count: u64, plan: String) {
    agent
        .forget
        .snapshots(count, Some((plan, self_agent())))
        .await;
}

#[when(expr = "the repository gains {int} snapshot(s) another agent's plan {string} holds")]
async fn gains_foreign(agent: &mut Agent, count: u64, plan: String) {
    agent
        .forget
        .snapshots(count, Some((plan, other_agent())))
        .await;
}

#[given(expr = "the repository holds {int} snapshots only another agent's tag marks")]
fn only_the_agent_tag(agent: &mut Agent, count: u64) {
    let tags = [format!(
        "{}{}",
        domain::execution::plan_tags::AGENT_TAG_PREFIX,
        other_agent().as_uuid().hyphenated()
    )];
    agent.forget.snapshots_tagged(count, &tags, "/home/demo");
}

#[given(expr = "the repository holds {int} snapshots only the plan tag {string} marks")]
fn only_the_plan_tag(agent: &mut Agent, count: u64, plan: String) {
    let tags = [format!(
        "{}{plan}",
        domain::execution::plan_tags::PLAN_TAG_PREFIX
    )];
    agent.forget.snapshots_tagged(count, &tags, "/home/demo");
}

#[given(expr = "the repository holds {int} snapshots no plan owns of the folder {string}")]
async fn untagged_of_the_folder(agent: &mut Agent, count: u64, folder: String) {
    agent.forget.snapshots_of_folder(count, None, &folder).await;
}

#[when("the owner previews forgetting the repository")]
async fn preview_forget(agent: &mut Agent) {
    preview(agent, MaintenanceKind::Forget).await;
}

#[when("the owner previews checking the repository")]
async fn preview_check(agent: &mut Agent) {
    preview(agent, MaintenanceKind::Check).await;
}

#[when("the owner previews pruning the repository")]
async fn preview_prune(agent: &mut Agent) {
    preview(agent, MaintenanceKind::Prune).await;
}

#[when(
    expr = "the owner previews adopting the other agent's {string} history into the plan {string}"
)]
async fn preview_adopt_orphaned(agent: &mut Agent, from: String, into: String) {
    let from = AdoptionSource::Orphaned {
        agent: other_agent().as_uuid().hyphenated().to_string(),
        plan: from,
    };
    preview(agent, adoption(from, &into)).await;
}

#[when(expr = "the owner previews adopting the unmanaged history into the plan {string}")]
async fn preview_adopt_unmanaged(agent: &mut Agent, into: String) {
    preview(agent, adoption(AdoptionSource::Unmanaged, &into)).await;
}

#[when("the owner forgets the repository with the preview's selection")]
async fn forget_with_selection(agent: &mut Agent) {
    confirm(agent, MaintenanceKind::Forget, operation_id(0xcc), "forget").await;
}

#[when(
    expr = "the owner adopts the other agent's {string} history into the plan {string} with the preview's selection"
)]
async fn adopt_orphaned_with_selection(agent: &mut Agent, from: String, into: String) {
    let from = AdoptionSource::Orphaned {
        agent: other_agent().as_uuid().hyphenated().to_string(),
        plan: from,
    };
    confirm(agent, adoption(from, &into), operation_id(0xce), "adopt").await;
}

#[when("the owner lists the repository's groups")]
async fn list_groups(agent: &mut Agent) {
    let use_case = super::forget_steps::use_case(agent);
    // The scenario's clock jumps to a timer's deadline, so a listing of a
    // repository that never answers ends at once; one that does not time
    // out would hang, and is cut off here.
    let listed = tokio::time::timeout(
        Duration::from_secs(5),
        use_case.groups(preview_operation(), &alias()),
    )
    .await
    .expect("the listing returns instead of waiting for ever");
    match listed {
        Ok(groups) => agent.preview.groups = Some(groups),
        Err(error) => agent.forget.set_refused(Some(error)),
    }
}

fn adoption(from: AdoptionSource, into: &str) -> MaintenanceKind {
    MaintenanceKind::Adopt(Adoption {
        from,
        into: PlanId::parse(into).expect("a plan ID"),
    })
}

/// Previews `kind`, keeping what it answered or why it refused.
async fn preview(agent: &mut Agent, kind: MaintenanceKind) {
    let use_case = super::forget_steps::use_case(agent);
    let previewed = tokio::time::timeout(
        Duration::from_secs(5),
        use_case.preview(preview_operation(), &alias(), &kind),
    )
    .await
    .expect("the preview returns instead of waiting for ever");
    match previewed {
        Ok(preview) => agent.preview.previewed = Some(preview),
        Err(error) => agent.forget.set_refused(Some(error)),
    }
}

/// Assesses and starts `kind` held to the selection the last preview named,
/// then runs it, as the route does, or keeps why it was refused.
async fn confirm(agent: &mut Agent, kind: MaintenanceKind, op: OperationId, job: &str) {
    let selection = agent
        .preview
        .previewed
        .as_ref()
        .and_then(MaintenancePreview::selection)
        .expect("a preview that names a selection was made first");
    let use_case = super::forget_steps::use_case(agent);
    let assessed = use_case
        .assess_previewed(op, &alias(), kind, &selection)
        .await;
    let prepared = match assessed {
        Ok(assessment) => use_case.start(assessment).await,
        Err(error) => Err(error),
    };
    match prepared {
        Ok(prepared) => {
            assert_eq!(
                prepared.operation().id(),
                op,
                "the prepared run carries the operation the route answered with"
            );
            let runner = Arc::clone(&use_case);
            let running =
                tokio::spawn(
                    async move { runner.execute(prepared, futures::future::pending()).await },
                );
            wait_for_job(agent, job).await;
            agent.forget.release().notify_waiters();
            agent.finished = Some(running.await.expect("the run ends"));
        }
        Err(error) => agent.forget.set_refused(Some(error)),
    }
}

/// Waits until the scripted engine was given a job of this kind.
async fn wait_for_job(agent: &Agent, name: &str) {
    for _ in 0..400 {
        if agent.forget.jobs().contains(&name) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the engine was never given the {name} job");
}

fn forget_preview(agent: &Agent) -> &ForgetPreview {
    match &agent.preview.previewed {
        Some(MaintenancePreview::Forget(preview)) => preview,
        other => panic!("a forget was previewed, not {other:?}"),
    }
}

fn adopt_preview(agent: &Agent) -> &AdoptPreview {
    match &agent.preview.previewed {
        Some(MaintenancePreview::Adopt(preview)) => preview,
        other => panic!("an adoption was previewed, not {other:?}"),
    }
}

fn plan_forget<'a>(preview: &'a ForgetPreview, plan: &str) -> &'a PlanForget {
    preview
        .plans
        .iter()
        .find(|entry| entry.plan.as_str() == plan)
        .unwrap_or_else(|| panic!("the preview lists the plan {plan:?}: {preview:?}"))
}

fn groups(agent: &Agent) -> &RepositoryGroups {
    agent
        .preview
        .groups
        .as_ref()
        .expect("the groups were listed")
}

fn history_of<'a>(groups: &'a RepositoryGroups, plan: &str) -> &'a PlanHistory {
    groups
        .plans
        .iter()
        .find(|history| history.plan.as_str() == plan)
        .unwrap_or_else(|| panic!("the groups list the plan {plan:?}: {groups:?}"))
}

/// What a refused preview, run or listing answered.
fn refusal(agent: &mut Agent) -> MaintenanceError {
    agent
        .forget
        .take_refused()
        .expect("the request was refused")
}

fn names(plans: &[PlanId]) -> String {
    plans
        .iter()
        .map(PlanId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[then(expr = "the preview says the plan {string} would lose {int} of its {int} snapshots")]
fn plan_would_lose(agent: &mut Agent, plan: String, remove: usize, total: usize) {
    let entry = plan_forget(forget_preview(agent), &plan);
    assert_eq!(entry.remove.len(), remove, "{entry:?}");
    assert_eq!(entry.total, total, "{entry:?}");
}

#[then(expr = "the preview names the {int} oldest snapshots of the plan {string}")]
fn names_the_oldest(agent: &mut Agent, count: usize, plan: String) {
    let mut owned = agent.forget.plan_snapshots(&plan);
    let oldest = owned.split_off(owned.len().saturating_sub(count));
    let named: Vec<SnapshotId> = plan_forget(forget_preview(agent), &plan)
        .remove
        .iter()
        .map(|snapshot| snapshot.id.clone())
        .collect();
    assert_eq!(named, oldest, "the plan's oldest, newest first");
}

#[then(expr = "the preview lists the plans {string} in that order")]
fn lists_the_plans(agent: &mut Agent, plans: String) {
    let listed: Vec<&str> = forget_preview(agent)
        .plans
        .iter()
        .map(|entry| entry.plan.as_str())
        .collect();
    assert_eq!(listed.join(", "), plans);
}

#[then(
    expr = "the preview says it leaves alone {int} group(s) of orphaned history holding {int} snapshots"
)]
fn leaves_orphaned_alone(agent: &mut Agent, groups: usize, snapshots: usize) {
    let left = forget_preview(agent).left_alone;
    assert_eq!(
        (left.orphaned.groups, left.orphaned.snapshots),
        (groups, snapshots),
        "{left:?}"
    );
}

#[then(
    expr = "the preview says it leaves alone {int} group(s) of unmanaged history holding {int} snapshots"
)]
fn leaves_unmanaged_alone(agent: &mut Agent, groups: usize, snapshots: usize) {
    let left = forget_preview(agent).left_alone;
    assert_eq!(
        (left.unmanaged.groups, left.unmanaged.snapshots),
        (groups, snapshots),
        "{left:?}"
    );
}

#[then("the preview says it leaves nothing alone")]
fn leaves_nothing_alone(agent: &mut Agent) {
    assert_eq!(forget_preview(agent).left_alone, LeftAlone::default());
}

#[then(expr = "the preview says {int} snapshots would be rewritten into the plan {string}")]
fn adoption_would_rewrite(agent: &mut Agent, count: usize, plan: String) {
    let preview = adopt_preview(agent);
    assert_eq!(preview.into.as_str(), plan);
    assert_eq!(preview.snapshots.len(), count, "{preview:?}");
}

#[then("the preview names the snapshots the other agent held")]
fn names_the_other_agents(agent: &mut Agent) {
    let named: Vec<SnapshotId> = adopt_preview(agent)
        .snapshots
        .iter()
        .map(|snapshot| snapshot.id.clone())
        .collect();
    assert_eq!(
        named,
        agent.forget.foreign_snapshots("home", other_agent()),
        "exactly the other agent's history"
    );
}

#[then("the preview names the snapshots no plan owned")]
fn names_the_untagged(agent: &mut Agent) {
    let named: Vec<SnapshotId> = adopt_preview(agent)
        .snapshots
        .iter()
        .map(|snapshot| snapshot.id.clone())
        .collect();
    assert_eq!(
        named,
        agent.forget.untagged_snapshots(),
        "exactly the untagged history"
    );
}

#[then("the preview says the check does not read the repository's data")]
fn check_reads_no_data(agent: &mut Agent) {
    match &agent.preview.previewed {
        Some(MaintenancePreview::Check(check)) => assert!(!check.read_data, "{check:?}"),
        other => panic!("a check was previewed, not {other:?}"),
    }
}

fn prune_preview(agent: &Agent) -> &PrunePreview {
    match &agent.preview.previewed {
        Some(MaintenancePreview::Prune(prune)) => prune,
        other => panic!("a prune was previewed, not {other:?}"),
    }
}

#[then(
    expr = "the preview says the prune would mark {int} packs, delete {int} and bring back {int}, freeing {int} bytes"
)]
fn prune_figures(agent: &mut Agent, marked: u64, deleted: u64, recovered: u64, bytes: u64) {
    let prune = prune_preview(agent);
    assert_eq!(
        (
            prune.packs_marked,
            prune.packs_deleted,
            prune.packs_recovered,
            prune.bytes_deleted
        ),
        (marked, deleted, recovered, bytes),
        "{prune:?}"
    );
}

#[then("the preview says marked packs wait the default time before a later prune deletes them")]
fn prune_waits(agent: &mut Agent) {
    assert_eq!(prune_preview(agent).keep_delete, default_keep_delete());
}

#[then("the engine was only asked to plan a prune")]
fn only_a_plan(agent: &mut Agent) {
    assert_eq!(
        agent.forget.jobs(),
        ["prune-plan"],
        "the engine was asked for more than a plan"
    );
}

#[then("the engine was never given a job")]
fn no_job_at_all(agent: &mut Agent) {
    assert_eq!(
        agent.forget.jobs(),
        Vec::<&str>::new(),
        "the engine was asked for something"
    );
}

#[then("the forget removed exactly the snapshots the preview named")]
fn removed_what_the_preview_named(agent: &mut Agent) {
    let mut named = forget_preview(agent).removal();
    named.sort();
    let mut removed = agent.forget.removed();
    removed.sort();
    assert_eq!(removed, named, "the removal is the preview's selection");
}

#[then("the log holds no operation of this preview")]
async fn no_operation_of_the_preview(agent: &mut Agent) {
    let found = agent
        .store
        .get(preview_operation())
        .await
        .expect("the log answers");
    assert_eq!(found, None, "a preview leaves nothing in the log");
}

#[then("the log holds only the operation that was already running")]
async fn only_the_running_operation(agent: &mut Agent) {
    let page = agent
        .store
        .list(&HistoryQuery::default())
        .await
        .expect("the log answers");
    let held: Vec<OperationId> = page
        .operations
        .iter()
        .map(|operation| operation.id)
        .collect();
    assert_eq!(held, [operation_id(0xbb)]);
}

#[then("the preview is refused because the plan's policy keeps nothing")]
fn preview_keeps_nothing(agent: &mut Agent) {
    assert_eq!(
        refusal(agent),
        MaintenanceError::KeepsNothing {
            plan: PlanId::parse("home").expect("a plan ID")
        }
    );
}

#[then("the preview is refused because no plan matches that history")]
fn preview_no_match(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::AdoptionNoMatch);
}

#[then("the preview is refused because two plans match that history")]
fn preview_ambiguous(agent: &mut Agent) {
    let error = refusal(agent);
    let MaintenanceError::AdoptionAmbiguous { plans } = error else {
        panic!("an ambiguity, not {error:?}")
    };
    let named = names(&plans);
    assert!(
        named.contains("home") && named.contains("mirror"),
        "{named}"
    );
}

#[then(expr = "the preview is refused because the plan {string} is not configured")]
fn preview_unknown_plan(agent: &mut Agent, plan: String) {
    assert_eq!(
        refusal(agent),
        MaintenanceError::UnknownPlan {
            plan: PlanId::parse(&plan).expect("a plan ID")
        }
    );
}

#[then("the preview is refused because the repository is not configured")]
fn preview_unknown_repository(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::UnknownRepository);
}

#[then("the preview is refused because the engine cannot be reached")]
fn preview_engine(agent: &mut Agent) {
    let error = refusal(agent);
    assert!(
        matches!(error, MaintenanceError::Engine { .. }),
        "an engine failure, not {error:?}"
    );
}

#[then("the preview is refused because the repository did not answer in time")]
fn preview_silent(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::TimedOut);
}

#[then("the run is refused because the snapshots changed since the preview")]
fn run_selection_changed(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::SelectionChanged);
}

#[then("the groups are refused because the repository is not configured")]
fn groups_unknown_repository(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::UnknownRepository);
}

#[then("the groups are refused because the engine cannot be reached")]
fn groups_engine(agent: &mut Agent) {
    let error = refusal(agent);
    assert!(
        matches!(error, MaintenanceError::Engine { .. }),
        "an engine failure, not {error:?}"
    );
}

#[then("the groups are refused because the repository did not answer in time")]
fn groups_silent(agent: &mut Agent) {
    assert_eq!(refusal(agent), MaintenanceError::TimedOut);
}

#[then(expr = "the groups list the plans {string} in that order")]
fn groups_plans(agent: &mut Agent, plans: String) {
    let listed: Vec<&str> = groups(agent)
        .plans
        .iter()
        .map(|history| history.plan.as_str())
        .collect();
    assert_eq!(listed.join(", "), plans);
}

#[then(expr = "the groups list the plan {string} with {int} snapshots")]
fn groups_plan_snapshots(agent: &mut Agent, plan: String, count: usize) {
    let history = history_of(groups(agent), &plan);
    assert_eq!(history.snapshots.len(), count, "{history:?}");
}

#[then(expr = "the groups list the plan {string} with {int} snapshots, newest first")]
fn groups_plan_newest_first(agent: &mut Agent, plan: String, count: usize) {
    let history = history_of(groups(agent), &plan);
    assert_eq!(history.snapshots.len(), count, "{history:?}");
    assert!(
        history
            .snapshots
            .windows(2)
            .all(|pair| pair[0].time > pair[1].time),
        "newest first: {history:?}"
    );
}

#[then(expr = "the groups list the plan {string} with no snapshots")]
fn groups_plan_empty(agent: &mut Agent, plan: String) {
    let history = history_of(groups(agent), &plan);
    assert!(history.snapshots.is_empty(), "{history:?}");
}

#[then("the groups list no orphaned history")]
fn groups_no_orphaned(agent: &mut Agent) {
    assert!(groups(agent).orphaned.is_empty(), "{:?}", groups(agent));
}

#[then(expr = "the sources of {int} plan(s) were read from the disk")]
fn sources_read(agent: &mut Agent, count: usize) {
    assert_eq!(
        agent.forget.source_reads(),
        count,
        "only an unmanaged group is matched by a plan's sources"
    );
}

#[then("the groups list no unmanaged history")]
fn groups_no_unmanaged(agent: &mut Agent) {
    assert!(groups(agent).unmanaged.is_empty(), "{:?}", groups(agent));
}

#[then(
    expr = "the groups list an orphaned group of {int} snapshots that can be adopted into {string}"
)]
fn groups_orphaned_group(agent: &mut Agent, count: usize, into: String) {
    let [orphaned] = groups(agent).orphaned.as_slice() else {
        panic!("one orphaned group: {:?}", groups(agent));
    };
    assert_eq!(orphaned.group.snapshots.len(), count, "{orphaned:?}");
    assert_eq!(names(&orphaned.adopt_into), into, "{orphaned:?}");
}

#[then(
    expr = "the groups list an unmanaged group of {int} snapshots that can be adopted into {string}"
)]
fn groups_unmanaged_group(agent: &mut Agent, count: usize, into: String) {
    let [unmanaged] = groups(agent).unmanaged.as_slice() else {
        panic!("one unmanaged group: {:?}", groups(agent));
    };
    assert_eq!(unmanaged.group.snapshots.len(), count, "{unmanaged:?}");
    assert_eq!(names(&unmanaged.adopt_into), into, "{unmanaged:?}");
}

#[then(expr = "the groups list an orphaned group of {int} snapshots that no plan could adopt")]
fn groups_unadoptable_orphaned_group(agent: &mut Agent, count: usize) {
    let [orphaned] = groups(agent).orphaned.as_slice() else {
        panic!("one orphaned group: {:?}", groups(agent));
    };
    assert_eq!(orphaned.group.snapshots.len(), count, "{orphaned:?}");
    assert!(orphaned.adopt_into.is_empty(), "{orphaned:?}");
}

#[then(expr = "the groups list an unmanaged group of {int} snapshots that no plan would adopt")]
fn groups_unclaimed_group(agent: &mut Agent, count: usize) {
    let [unmanaged] = groups(agent).unmanaged.as_slice() else {
        panic!("one unmanaged group: {:?}", groups(agent));
    };
    assert_eq!(unmanaged.group.snapshots.len(), count, "{unmanaged:?}");
    assert!(unmanaged.adopt_into.is_empty(), "{unmanaged:?}");
}
