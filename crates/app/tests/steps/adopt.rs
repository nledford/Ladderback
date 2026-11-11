//! Steps for `features/maintenance/adopt.feature`: they drive the
//! maintenance use case against the scripted engine the forget scenarios
//! use, which answers an identify, a listing and a rewrite, and records
//! what it was asked to rewrite.

use std::time::Duration;

use cucumber::{given, then, when};
use domain::{AgentId, OperationId, PlanId, Uuid};

use super::Agent;
use super::forget_steps::assess_and_start;
use app::maintenance::{Adoption, AdoptionSource, MaintenanceError, MaintenanceKind};
use app::ports::OperationStore;

/// The alias every adopt scenario's repository goes under, as the forget
/// scenarios' own.
fn alias() -> domain::RepositoryAlias {
    domain::RepositoryAlias::parse("demo").expect("an alias")
}

fn self_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0001))
}

fn other_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_00aa))
}

fn operation_id(low: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_0000 | low,
    ))
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s) that backs up the same sources")]
async fn plan_same_sources(agent: &mut Agent, plan: String, keep: i64) {
    agent.forget.repository().await;
    agent
        .forget
        .plan(&format!("{plan}-sibling"), Some(keep))
        .await;
    // The mirror plan carries the same sources and no host of its own, so
    // it matches everything the first plan matches.
    let mut mirror = domain::planning::Plan::new(
        PlanId::parse(&plan).expect("a plan ID"),
        alias(),
        vec![domain::planning::Source::parse("/home/demo").expect("a source")],
    );
    mirror.retention.keep_last = Some(domain::planning::KeepCount::new(keep).expect("a count"));
    agent.forget.plan_raw(mirror).await;
}

#[given(expr = "the repository holds {int} snapshots no plan owns that match the plan {string}")]
async fn snapshots_matching(agent: &mut Agent, count: u64, _plan: String) {
    // The world's untagged snapshots of `/home/demo` carry the host a plan
    // without one of its own falls back to, so they match by construction.
    agent.forget.snapshots(count, None).await;
}

#[given(expr = "the repository holds {int} snapshots no plan owns of another folder")]
async fn snapshots_of_another_folder(agent: &mut Agent, count: u64) {
    agent
        .forget
        .snapshots_of_folder(count, None, "/srv/other")
        .await;
}

#[when(expr = "the owner adopts the other agent's {string} history into the plan {string}")]
async fn adopt_orphaned(agent: &mut Agent, from: String, into: String) {
    run_adoption(
        agent,
        AdoptionSource::Orphaned {
            agent: other_agent().as_uuid().hyphenated().to_string(),
            plan: from,
        },
        into,
    )
    .await;
}

#[when(expr = "the owner adopts the unmanaged history that matches the plan {string}")]
async fn adopt_unmanaged(agent: &mut Agent, into: String) {
    run_adoption(agent, AdoptionSource::Unmanaged, into).await;
}

#[when(
    expr = "the owner adopts the other agent's {string} history into the plan {string} and a forget is asked while it runs"
)]
async fn adopt_then_forget(agent: &mut Agent, from: String, into: String) {
    let use_case = super::forget_steps::use_case(agent);
    let kind = adopt_kind(
        AdoptionSource::Orphaned {
            agent: other_agent().as_uuid().hyphenated().to_string(),
            plan: from,
        },
        &into,
    );
    let prepared = assess_and_start(&use_case, operation_id(0xce), kind)
        .await
        .expect("the repository is free");
    let runner = std::sync::Arc::clone(&use_case);
    let running =
        tokio::spawn(async move { runner.execute(prepared, futures::future::pending()).await });
    wait_for_job(agent, "adopt").await;
    let refused = assess_and_start(&use_case, operation_id(0xcf), MaintenanceKind::Forget).await;
    agent.forget.set_refused(refused.err());
    agent.forget.release_hold();
    agent.finished = Some(running.await.expect("the run ends"));
}

#[when(expr = "the owner adopts the unmanaged history into the plan {string}")]
async fn adopt_unmanaged_into(agent: &mut Agent, into: String) {
    run_adoption(agent, AdoptionSource::Unmanaged, into).await;
}

/// Assesses, starts and runs one adoption, as the route does.
async fn run_adoption(agent: &mut Agent, from: AdoptionSource, into: String) {
    let use_case = super::forget_steps::use_case(agent);
    let kind = adopt_kind(from, &into);
    let prepared = assess_and_start(&use_case, operation_id(0xce), kind).await;
    match prepared {
        Ok(prepared) => {
            assert_eq!(
                prepared.operation().id(),
                operation_id(0xce),
                "the prepared run carries the operation the route answered with"
            );
            let runner = std::sync::Arc::clone(&use_case);
            let running =
                tokio::spawn(
                    async move { runner.execute(prepared, futures::future::pending()).await },
                );
            wait_for_job(agent, "adopt").await;
            agent.forget.release_hold();
            agent.finished = Some(running.await.expect("the run ends"));
        }
        Err(error) => agent.forget.set_refused(Some(error)),
    }
}

fn adopt_kind(from: AdoptionSource, into: &str) -> MaintenanceKind {
    MaintenanceKind::Adopt(Adoption {
        from,
        into: PlanId::parse(into).expect("a plan ID"),
    })
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

#[then(expr = "the {int} snapshots are rewritten with the tags of the plan {string}")]
fn rewritten(agent: &mut Agent, count: u64, plan: String) {
    let Some((ids, tags)) = agent.forget.rewrite_job() else {
        panic!("the engine was never asked to rewrite anything");
    };
    assert_eq!(ids.len(), usize::try_from(count).unwrap_or(0), "{ids:?}");
    let expected = domain::execution::plan_tags::PlanTags::new(
        PlanId::parse(&plan).expect("a plan ID"),
        self_agent(),
    )
    .tags()
    .to_vec();
    assert_eq!(tags, expected, "the plan's own tags, and no others");
}

#[then(expr = "the snapshots the other agent held are gone")]
fn other_agents_gone(agent: &mut Agent) {
    let Some((ids, _)) = agent.forget.rewrite_job() else {
        panic!("the engine was never asked to rewrite anything");
    };
    let foreign = agent.forget.foreign_snapshots("home", other_agent());
    assert_eq!(
        ids, foreign,
        "exactly the other agent's history is rewritten"
    );
}

#[then(expr = "the snapshots no plan owned are gone")]
fn unmanaged_gone(agent: &mut Agent) {
    let Some((ids, _)) = agent.forget.rewrite_job() else {
        panic!("the engine was never asked to rewrite anything");
    };
    let untagged = agent.forget.untagged_snapshots();
    assert_eq!(ids, untagged, "exactly the untagged history is rewritten");
}

#[then(expr = "the adoption is refused because no plan matches that history")]
fn refused_no_match(agent: &mut Agent) {
    let error = agent
        .forget
        .take_refused()
        .expect("the adoption was refused");
    assert_eq!(error, MaintenanceError::AdoptionNoMatch);
}

#[then(expr = "the adoption is refused because the plan {string} is not configured")]
fn refused_unknown_plan(agent: &mut Agent, plan: String) {
    let error = agent
        .forget
        .take_refused()
        .expect("the adoption was refused");
    assert_eq!(
        error,
        MaintenanceError::UnknownPlan {
            plan: PlanId::parse(&plan).expect("a plan ID")
        }
    );
}

#[then(expr = "the adoption is refused because two plans match that history")]
fn refused_ambiguous(agent: &mut Agent) {
    let error = agent
        .forget
        .take_refused()
        .expect("the adoption was refused");
    let MaintenanceError::AdoptionAmbiguous { plans } = error else {
        panic!("an ambiguity, not {error:?}")
    };
    let named: Vec<&str> = plans.iter().map(|plan| plan.as_str()).collect();
    assert!(
        named.contains(&"home") && named.contains(&"mirror"),
        "{named:?}"
    );
}

#[then(expr = "the engine was never asked to rewrite anything")]
fn no_rewrite_job(agent: &mut Agent) {
    assert!(
        !agent.forget.jobs().contains(&"adopt"),
        "the engine ran: {:?}",
        agent.forget.jobs()
    );
}

#[then(expr = "the forget is refused while the adoption holds the repository")]
fn forget_refused(agent: &mut Agent) {
    let error = agent.forget.take_refused().expect("the forget was refused");
    assert!(matches!(error, MaintenanceError::Busy { .. }), "{error:?}");
}

#[then(expr = "the log holds no operation of this adoption")]
async fn no_operation(agent: &mut Agent) {
    let found = agent
        .store
        .get(operation_id(0xce))
        .await
        .expect("the log answers");
    assert_eq!(found, None, "a refused adoption leaves nothing in the log");
}
