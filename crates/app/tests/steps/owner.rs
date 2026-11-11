//! Steps for `features/maintenance/owner.feature`: a forget, a prune, a check
//! and an adoption, and the preview of each, asked of a repository this agent
//! doesn't maintain, against the scripted engine of the forget steps.

use std::time::Duration;

use cucumber::{given, then, when};
use domain::planning::Maintenance;
use domain::{AgentId, OperationId, PlanId, RepositoryAlias, Uuid};

use super::Agent;
use super::forget_steps::{assess_and_start, use_case};
use app::maintenance::{Adoption, AdoptionSource, MaintenanceError, MaintenanceKind};
use app::ports::{HistoryQuery, OperationStore};

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("a repository alias")
}

fn other_agent() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_00aa))
}

fn operation() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_00cc))
}

/// The run a scenario names: its kind, and whether only its preview is asked.
fn asked(what: &str) -> (MaintenanceKind, bool) {
    let (run, preview) = match what.strip_prefix("a preview of ") {
        Some(run) => (run, true),
        None => (what, false),
    };
    let kind = match run {
        "a forget" => MaintenanceKind::Forget,
        "a prune" => MaintenanceKind::Prune,
        "a check" => MaintenanceKind::Check,
        // The other agent's history, which a plan of this agent could take.
        "an adoption" => MaintenanceKind::Adopt(Adoption {
            from: AdoptionSource::Orphaned {
                agent: other_agent().as_uuid().hyphenated().to_string(),
                plan: "home".to_owned(),
            },
            into: PlanId::parse("home").expect("a plan ID"),
        }),
        other => panic!("the scenario asks for {other}, which no step runs"),
    };
    (kind, preview)
}

#[given(expr = "the repository's maintenance is {string}")]
async fn maintained_by(agent: &mut Agent, maintenance: String) {
    let maintenance = Maintenance::parse(&maintenance).expect("hub, local or none");
    agent.forget.maintained_by(maintenance).await;
}

#[when(regex = r"^this agent is asked for (.+) of the repository$")]
async fn asked_for(agent: &mut Agent, what: String) {
    let (kind, preview) = asked(&what);
    let use_case = use_case(agent);
    // A run that isn't refused would wait for the scripted engine's release,
    // which no step here gives: it is cut off, and the scenario fails.
    let answered = tokio::time::timeout(Duration::from_secs(5), async {
        if preview {
            use_case
                .preview(operation(), &alias(), &kind)
                .await
                .map(drop)
        } else {
            assess_and_start(&use_case, operation(), kind)
                .await
                .map(drop)
        }
    })
    .await
    .expect("the request is answered instead of waiting for ever");
    agent.forget.set_refused(answered.err());
}

#[then("it is refused because this agent does not maintain the repository")]
fn refused_as_not_owner(agent: &mut Agent) {
    assert_eq!(
        agent.forget.take_refused(),
        Some(MaintenanceError::NotOwner),
        "the request is refused as one this agent may not run"
    );
}

#[then("the operation log holds no operation")]
async fn log_empty(agent: &mut Agent) {
    let page = agent
        .store
        .list(&HistoryQuery::default())
        .await
        .expect("the log answers");
    assert!(
        page.operations.is_empty(),
        "nothing was written to the log: {:?}",
        page.operations
    );
}
