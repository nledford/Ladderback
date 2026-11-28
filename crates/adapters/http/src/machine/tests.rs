//! The machine routes with sign-in off; `api_tokens/tests.rs` checks that no
//! API token reaches them.

use app::fakes::{FakeThisMachine, InMemoryConfigStore, InMemoryOperationStore};
use app::machine::{MachineStanding, check_at_start};
use axum::body::Body;
use axum::http::{Method, Request, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use crate::request_checks::tests::{NoUi, test_state};
use crate::{RequestChecks, router};

/// A machine started on this computer, whose data folder recorded
/// `recorded` (another computer's for a copy), as the daemon checks it
/// before it serves, with `config` its configuration and `operations` its
/// operation log.
fn started_with(
    recorded: &str,
    config: Arc<InMemoryConfigStore>,
    operations: Arc<InMemoryOperationStore>,
) -> (MachineRoute, Arc<FakeThisMachine>) {
    let machine = Arc::new(FakeThisMachine::new(Some("this-computer")).with_recorded(recorded));
    let standing = Arc::new(MachineStanding::new(
        check_at_start(machine.as_ref()).standing(),
    ));
    let take = TakeNewId::new(
        Arc::clone(&machine) as Arc<dyn app::ports::ThisMachine>,
        standing,
        config,
        operations,
        Arc::default(),
    );
    (MachineRoute::new(Arc::new(take)), machine)
}

fn started(recorded: &str) -> (MachineRoute, Arc<FakeThisMachine>) {
    started_with(recorded, Arc::default(), Arc::default())
}

/// A configuration that asks this agent to maintain one repository.
async fn maintaining_one() -> Arc<InMemoryConfigStore> {
    use domain::planning::{AgentConfig, Maintenance, RepositoryConfig, RepositorySettings};
    use domain::secrets::{SecretRef, SecretValue};
    let entry = RepositoryConfig {
        locations: Default::default(),
        id: None,
        settings: RepositorySettings {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: SecretRef::Inline(SecretValue::new("demo-password")),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        },
        maintenance: Maintenance::Local,
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let alias = domain::RepositoryAlias::parse("demo").unwrap();
    let config = AgentConfig::new(
        Default::default(),
        Default::default(),
        std::collections::BTreeMap::from([(alias, entry)]),
        Vec::new(),
    )
    .unwrap();
    let store = Arc::new(InMemoryConfigStore::new());
    app::ports::ConfigStore::save(store.as_ref(), &config)
        .await
        .unwrap();
    store
}

/// A request as the page sends it: a change is JSON, from the page's own
/// origin.
fn request(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:8080")
        .header(header::ORIGIN, "http://127.0.0.1:8080")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap()
}

async fn send(route: &MachineRoute, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let mut state = test_state();
    state.machine = route.clone();
    let response = router(Arc::new(NoUi), RequestChecks::default(), state)
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn a_machine_that_never_checked_stands_as_usual_and_keeps_its_id() {
    let route = MachineRoute::default();
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "same" }));
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "machine-not-copied");
}

/// The route tells who this machine is beside where it stands, and keeps
/// telling it after a new ID is taken, which is used from the next start
/// (§16, decision 175).
#[tokio::test]
async fn the_machine_route_tells_who_this_machine_is() {
    let (route, _) = started("other-computer");
    let route = route.with_identity(MachineIdentity::new(
        "018f0000-0000-7000-8000-0000000000aa".to_owned(),
        "demo-hub",
        Some("Europe/Berlin".to_owned()),
    ));
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "standing": "copied",
            "machine-id": "018f0000-0000-7000-8000-0000000000aa",
            "hostname": "demo-hub",
            "zone": "Europe/Berlin",
        })
    );
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["standing"], "new-id-taken");
    assert_eq!(body["machine-id"], "018f0000-0000-7000-8000-0000000000aa");
    assert_eq!(body["hostname"], "demo-hub");
}

/// The hub's own hostname passes the rule a pairing hostname does, or the
/// route leaves it out, as a hub never shows a hostname it would refuse
/// from a machine (decision 120, "Hostnames"; decision 175).
#[tokio::test]
async fn the_machine_route_leaves_out_a_hostname_a_hub_would_refuse() {
    for refused in ["", " demo-hub", "demo\u{202e}buh", "demo\u{200b}hub"] {
        let identity = MachineIdentity::new(
            "018f0000-0000-7000-8000-0000000000aa".to_owned(),
            refused,
            None,
        );
        let route = MachineRoute::default().with_identity(identity);
        let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({
                "standing": "same",
                "machine-id": "018f0000-0000-7000-8000-0000000000aa",
            }),
            "{refused:?}"
        );
    }
    let identity = MachineIdentity::new(
        "018f0000-0000-7000-8000-0000000000aa".to_owned(),
        "Nick's MacBook Pro",
        None,
    );
    let route = MachineRoute::default().with_identity(identity);
    let (_, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(body["hostname"], "Nick's MacBook Pro");
}

#[tokio::test]
async fn an_unchanged_machine_reads_same_and_can_t_take_a_new_id() {
    let (route, machine) = started("this-computer");
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "same" }));
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "machine-not-copied");
    assert_eq!(machine.renewed_id(), None);
}

#[tokio::test]
async fn a_copy_reads_copied_and_takes_one_new_id() {
    let (route, machine) = started("other-computer");
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "copied" }));

    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "new-id-taken" }));
    let taken = machine.renewed_id();
    assert!(taken.is_some());

    // The read tells it, and a second request takes no other.
    let (_, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(body, serde_json::json!({ "standing": "new-id-taken" }));
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "machine-not-copied");
    assert_eq!(machine.renewed_id(), taken);
}

/// A machine its hub refused, signed, as a second computer: its data folder
/// recorded this computer's identifier, so only the refusal tells it.
#[tokio::test]
async fn a_machine_its_hub_refused_reads_refused_by_hub_and_takes_one_new_id() {
    let machine =
        Arc::new(FakeThisMachine::new(Some("this-computer")).with_recorded("this-computer"));
    let take = TakeNewId::new(
        Arc::clone(&machine) as Arc<dyn app::ports::ThisMachine>,
        Arc::new(MachineStanding::new(Standing::RefusedByHub)),
        maintaining_one().await,
        Arc::new(InMemoryOperationStore::default()),
        Arc::default(),
    );
    let route = MachineRoute::new(Arc::new(take));
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "refused-by-hub" }));

    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "new-id-taken" }));
    let taken = machine.renewed_id();
    assert!(taken.is_some());
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "machine-not-copied");
    assert_eq!(machine.renewed_id(), taken);
}

/// A machine its hub removed, or rejected while it waited, signed (#1489).
#[tokio::test]
async fn a_machine_its_hub_removed_reads_removed_by_hub_and_takes_one_new_id() {
    let machine =
        Arc::new(FakeThisMachine::new(Some("this-computer")).with_recorded("this-computer"));
    let take = TakeNewId::new(
        Arc::clone(&machine) as Arc<dyn app::ports::ThisMachine>,
        Arc::new(MachineStanding::new(Standing::RemovedByHub)),
        maintaining_one().await,
        Arc::new(InMemoryOperationStore::default()),
        Arc::default(),
    );
    let route = MachineRoute::new(Arc::new(take));
    let (status, body) = send(&route, request(Method::GET, MACHINE_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "removed-by-hub" }));

    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "standing": "new-id-taken" }));
    assert!(machine.renewed_id().is_some());
}

#[tokio::test]
async fn a_new_id_asked_for_from_another_origin_is_refused() {
    let (route, machine) = started("other-computer");
    let mut cross = request(Method::POST, NEW_ID_PATH);
    cross.headers_mut().insert(
        header::ORIGIN,
        header::HeaderValue::from_static("http://evil.example"),
    );
    let (status, _) = send(&route, cross).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(machine.renewed_id(), None);
}

#[tokio::test]
async fn a_copy_keeps_its_id_while_maintenance_runs_and_says_which() {
    let operations = Arc::new(InMemoryOperationStore::default());
    let at = domain::Timestamp::from_second(1_800_000_000).unwrap();
    let id = domain::OperationId::from_uuid(domain::Uuid::from_u128(0x0296));
    let mut check = domain::execution::Operation::queue(
        id,
        domain::execution::OperationKind::Check,
        domain::execution::Trigger::Manual,
        domain::execution::Target {
            plan: None,
            repository: Some(domain::RepositoryId::parse(&"07".repeat(32)).unwrap()),
        },
        at,
    );
    check.start(at).unwrap();
    app::ports::OperationStore::try_start(operations.as_ref(), &check)
        .await
        .unwrap();
    let (route, machine) = started_with("other-computer", maintaining_one().await, operations);
    let (status, body) = send(&route, request(Method::POST, NEW_ID_PATH)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "maintenance-busy");
    assert_eq!(
        body["args"]["operation"]["value"],
        id.as_uuid().hyphenated().to_string()
    );
    assert_eq!(machine.renewed_id(), None);
}
