//! The parity contract suite (§4.6): every tool both backends offer, asked
//! of both over one daemon's state, tells the same. Connect mode reads it
//! through the router `serve` serves, on loopback; the in-process backend
//! reads the handles that router reads.

use app::ports::OperationStore;
use domain::execution::{Operation, OperationKind, Target, Trigger};
use domain::{OperationId, PlanId, Timestamp};
use mcp::Backend;
use store::SqliteStore;
use uuid::Uuid;

use super::super::{Daemon, daemon_at, wire_name};
use super::*;

/// The configuration both backends read: two plans on one repository.
const CONFIG: &str = "schema = 1\n\n\
    [repositories.demo]\nrepository = \"/srv/backups/demo\"\npassword-env = \"DEMO_PASSWORD\"\n\n\
    [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n\n\
    [[plans]]\nid = \"work\"\nrepository = \"demo\"\nsources = [\"/home/demo/work\"]\n";

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a time")
}

/// A backup of `plan` queued at `second`.
fn queued(id: u128, plan: &str, second: i64) -> Operation {
    Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(id)),
        OperationKind::Backup,
        Trigger::Manual,
        Target {
            plan: Some(PlanId::try_from(plan.to_owned()).expect("a plan ID")),
            repository: None,
        },
        at(second),
    )
}

/// A daemon over `scratch` whose log holds a backup of `home` an earlier run
/// left running, which the start ends as failed, and a backup of `work`
/// still queued.
async fn a_daemon(scratch: &std::path::Path) -> Daemon {
    a_daemon_with(scratch, &[]).await
}

/// [`a_daemon`], its log holding `more` too.
async fn a_daemon_with(scratch: &std::path::Path, more: &[Operation]) -> Daemon {
    a_daemon_over(scratch, CONFIG, more).await
}

/// [`a_daemon_with`], reading `config` as its configuration.
async fn a_daemon_over(scratch: &std::path::Path, config_text: &str, more: &[Operation]) -> Daemon {
    let config = scratch.join("config");
    let data = scratch.join("data");
    std::fs::create_dir_all(&config).expect("the config folder");
    std::fs::create_dir_all(&data).expect("the data folder");
    std::fs::write(config.join("config.toml"), config_text).expect("the config file");
    let earlier = SqliteStore::open(&data.join("ladderback.db"))
        .await
        .expect("the log opens");
    let mut running = queued(1, "home", 1_800_000_000);
    running.start(at(1_800_000_001)).expect("the backup starts");
    earlier
        .save(&running, None)
        .await
        .expect("the running backup is saved");
    earlier
        .save(&queued(2, "work", 1_800_000_002), None)
        .await
        .expect("the queued backup is saved");
    for operation in more {
        earlier
            .save(operation, None)
            .await
            .expect("the operation is saved");
    }
    earlier.close().await;
    daemon_at(Some(config), Some(data), None)
        .await
        .map_err(|refused| refused.message)
        .expect("the daemon starts")
}

/// Connect mode's backend, over the router of `daemon` served on loopback.
async fn connected_to(daemon: &Daemon) -> mcp::Connect {
    connected_over(daemon.state.clone()).await
}

/// Connect mode's backend, over the router of `state` served on loopback.
async fn connected_over(state: http_adapter::AppState) -> mcp::Connect {
    mcp::Connect::new(served(state).await).expect("a client")
}

/// The address of the router of `state`, served on loopback.
async fn served(state: http_adapter::AppState) -> mcp::DaemonUrl {
    let listener = http_adapter::GatedListener::bind("127.0.0.1:0".parse().expect("an address"))
        .await
        .expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    let app = http_adapter::router(
        Arc::new(http_adapter::EmbeddedUi),
        http_adapter::RequestChecks::new([]),
        state,
    );
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<http_adapter::PeerAddr>(),
        )
        .await
    });
    mcp::DaemonUrl::parse(&format!("http://{addr}")).expect("a daemon URL")
}

#[tokio::test]
async fn both_backends_tell_the_status_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon)
        .await
        .status()
        .await
        .expect("a status");
    let in_process = InProcess::over(&daemon.state)
        .status()
        .await
        .expect("a status");

    assert_eq!(in_process.plans, connect.plans);
    assert_eq!(in_process.running, connect.running);
    // What the two are told is the daemon's state, not an empty one.
    let plans: Vec<&str> = in_process
        .plans
        .iter()
        .map(|plan| plan.plan.id.as_str())
        .collect();
    assert_eq!(plans, ["home", "work"]);
    let last: Vec<Option<&str>> = in_process
        .plans
        .iter()
        .map(|plan| plan.last.as_ref().map(|last| last.status.as_str()))
        .collect();
    assert_eq!(last, [Some("failed"), Some("queued")]);
    let running: Vec<&str> = in_process
        .running
        .iter()
        .map(|op| op.plan.as_str())
        .collect();
    assert_eq!(running, ["work"]);
    // Only connect mode drives a daemon, here with sign-in off.
    assert!(connect.daemon.is_some());
    assert_eq!(connect.access, mcp::Access::SignInOff);
    assert_eq!(in_process.daemon, None);
    assert_eq!(in_process.access, mcp::Access::InProcess);
}

#[tokio::test]
async fn both_backends_refuse_alike_when_the_configuration_cant_be_read() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    // Read again on every request, as `serve` reads it.
    std::fs::write(
        scratch.path().join("config").join("config.toml"),
        "schema = 1\n[[plans]]\nid = 3\n",
    )
    .expect("the config file");
    let connect = connected_to(&daemon).await.status().await;
    let in_process = InProcess::over(&daemon.state).status().await;

    let refused = in_process.expect_err("an unreadable configuration is refused");
    assert_eq!(Err(refused.clone()), connect);
    assert!(
        wire_name(refused.code).starts_with("config-"),
        "a configuration's refusal: {refused:?}"
    );
}

#[tokio::test]
async fn both_backends_list_the_plans_and_repositories_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    let plans = in_process.plans().await.expect("the plans");
    assert_eq!(Ok(plans.clone()), connect.plans().await);
    let ids: Vec<&str> = plans.plans.iter().map(|plan| plan.id.as_str()).collect();
    assert_eq!(ids, ["home", "work"]);
    let repositories = in_process.repositories().await.expect("the repositories");
    assert_eq!(Ok(repositories.clone()), connect.repositories().await);
    let aliases: Vec<&str> = repositories
        .repositories
        .iter()
        .map(|repository| repository.alias.as_str())
        .collect();
    assert_eq!(aliases, ["demo"]);
}

#[tokio::test]
async fn both_backends_page_the_history_alike_and_refuse_a_cursor_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    let first = in_process
        .history(None, None, 1)
        .await
        .expect("the first page");
    assert_eq!(Ok(first.clone()), connect.history(None, None, 1).await);
    let next = first.next.clone().expect("a page after the first");
    let second = in_process
        .history(None, Some(next.clone()), 1)
        .await
        .expect("the second page");
    assert_eq!(
        Ok(second.clone()),
        connect.history(None, Some(next), 1).await
    );
    let plans: Vec<&str> = first
        .operations
        .iter()
        .chain(&second.operations)
        .map(|operation| operation.plan.as_str())
        .collect();
    assert_eq!(plans, ["work", "home"]);
    let of_home = in_process
        .history(Some("home".to_owned()), None, 5)
        .await
        .expect("home's page");
    assert_eq!(
        Ok(of_home.clone()),
        connect.history(Some("home".to_owned()), None, 5).await
    );
    assert_eq!(of_home.operations.len(), 1);

    let cursor = Some("not-an-operation".to_owned());
    let refused = in_process
        .history(None, cursor.clone(), 1)
        .await
        .expect_err("a cursor that names no operation is refused");
    assert_eq!(Err(refused.clone()), connect.history(None, cursor, 1).await);
    assert_eq!(
        serde_json::to_value(&refused).expect("JSON"),
        serde_json::json!({
            "code": "invalid-cursor",
            "args": {"cursor": {"type": "text", "value": "not-an-operation"}},
        })
    );
}

#[tokio::test]
async fn both_backends_tell_an_operation_alike_and_refuse_one_they_dont_hold_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    let id = Uuid::from_u128(1).hyphenated().to_string();
    let detail = in_process
        .operation(id.clone())
        .await
        .expect("the operation");
    assert_eq!(Ok(detail.clone()), connect.operation(id.clone()).await);
    assert_eq!(detail.operation.id, id);
    assert_eq!(detail.operation.status, "failed");

    for unknown in [
        Uuid::from_u128(9).hyphenated().to_string(),
        "..".to_owned(),
        "a/b".to_owned(),
    ] {
        let refused = in_process
            .operation(unknown.clone())
            .await
            .expect_err("an operation the log doesn't hold is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.operation(unknown.clone()).await,
            "{unknown}"
        );
        assert_eq!(wire_name(refused.code), "operation-unknown", "{unknown}");
    }
}

#[tokio::test]
async fn both_backends_refuse_the_snapshots_of_a_repository_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    for alias in ["nowhere", "..", "a/b"] {
        let refused = in_process
            .snapshots(alias.to_owned())
            .await
            .expect_err("a repository the configuration doesn't hold is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.snapshots(alias.to_owned()).await,
            "{alias}"
        );
        assert_eq!(wire_name(refused.code), "repository-unknown", "{alias}");
    }
    // `demo`'s password is in a variable the daemon doesn't have: refused
    // before the repository is opened, and alike.
    let refused = in_process
        .snapshots("demo".to_owned())
        .await
        .expect_err("a password that can't be read is refused");
    assert_eq!(
        Err(refused.clone()),
        connect.snapshots("demo".to_owned()).await
    );
    assert!(
        wire_name(refused.code).starts_with("secret-"),
        "a secret's refusal: {refused:?}"
    );
}

/// The daemon's catalog, but listing `snapshots` as the repository `demo`'s,
/// which the parity daemon has no real repository for, and `slow`'s as a
/// listing that ran out of time.
struct WithSnapshots {
    catalog: Arc<dyn http_adapter::Catalog>,
    snapshots: Vec<api::SnapshotDto>,
}

#[async_trait]
impl http_adapter::Catalog for WithSnapshots {
    async fn plans(&self) -> Result<api::PlansDto, http_adapter::CatalogError> {
        self.catalog.plans().await
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, http_adapter::CatalogError> {
        self.catalog.repositories().await
    }

    async fn snapshots(
        &self,
        alias: String,
    ) -> Result<api::SnapshotsDto, http_adapter::SnapshotsError> {
        if alias == "slow" {
            return Err(http_adapter::SnapshotsError::TimedOut);
        }
        if alias == "demo" {
            return Ok(api::SnapshotsDto {
                snapshots: self.snapshots.clone(),
            });
        }
        self.catalog.snapshots(alias).await
    }

    async fn folder(
        &self,
        alias: String,
        snapshot: domain::SnapshotId,
        folder: domain::browse::FolderPath,
        from: u64,
    ) -> Result<api::FolderDto, http_adapter::FolderError> {
        self.catalog.folder(alias, snapshot, folder, from).await
    }
}

/// Snapshot `n` of `plan`, taken on `host` at a clock six hours behind UTC.
fn snapshot(n: u8, plan: &str, host: &str) -> api::SnapshotDto {
    serde_json::from_value(serde_json::json!({
        "id": format!("{n:064x}"),
        "time": format!("2026-11-{:02}T10:00:00Z", 20 - n),
        "offset": "-06:00",
        "hostname": host,
        "label": "",
        "tags": ["nightly", format!("ladderback-plan:{plan}"), "ladderback-agent:demo"],
        "paths": ["/home/demo"],
    }))
    .expect("a snapshot")
}

#[tokio::test]
async fn both_backends_list_the_snapshots_alike_and_a_filter_keeps_the_same_of_each() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let mut state = daemon.state.clone();
    state.catalog = Arc::new(WithSnapshots {
        catalog: Arc::clone(&daemon.state.catalog),
        snapshots: vec![
            snapshot(1, "home", "laptop"),
            snapshot(2, "work", "laptop"),
            snapshot(3, "home", "desktop"),
        ],
    });
    let connect = connected_over(state.clone()).await;
    let in_process = InProcess::over(&state);

    let listed = in_process
        .snapshots("demo".to_owned())
        .await
        .expect("the snapshots");
    assert_eq!(
        Ok(listed.clone()),
        connect.snapshots("demo".to_owned()).await
    );
    assert_eq!(listed.snapshots.len(), 3);
    let id = |n: u8| format!("{n:064x}");
    for (plan, host, kept) in [
        (Some("home"), None, vec![id(1), id(3)]),
        (None, Some("laptop"), vec![id(1), id(2)]),
        (Some("home"), Some("laptop"), vec![id(1)]),
        (Some("photos"), None, Vec::new()),
    ] {
        let filter = mcp::SnapshotFilter {
            plan: plan.map(str::to_owned),
            host: host.map(str::to_owned),
        };
        let kept_of = |listed: api::SnapshotsDto| -> Vec<String> {
            listed
                .snapshots
                .into_iter()
                .filter(|snapshot| filter.keeps(snapshot))
                .map(|snapshot| snapshot.id)
                .collect()
        };
        let of_connect = connect
            .snapshots("demo".to_owned())
            .await
            .expect("the snapshots");
        assert_eq!(kept_of(listed.clone()), kept, "{filter:?}");
        assert_eq!(kept_of(of_connect), kept, "{filter:?}");
    }
}

#[tokio::test]
async fn both_backends_refuse_a_listing_that_ran_out_of_time_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let mut state = daemon.state.clone();
    state.catalog = Arc::new(WithSnapshots {
        catalog: Arc::clone(&daemon.state.catalog),
        snapshots: Vec::new(),
    });
    let connect = connected_over(state.clone()).await;
    let refused = InProcess::over(&state)
        .snapshots("slow".to_owned())
        .await
        .expect_err("a listing that ran out of time is refused");
    assert_eq!(
        Err(refused.clone()),
        connect.snapshots("slow".to_owned()).await
    );
    assert_eq!(wire_name(refused.code), "repository-read-timed-out");
}

/// Writes `text` as the file `name` in `folder`, readable by its owner
/// alone, and returns its path.
fn token_file(folder: &std::path::Path, name: &str, text: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = folder.join(name);
    std::fs::write(&path, text).expect("the token file");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("its permissions");
    path
}

/// A refusal's message and arguments.
type Refusal = (l10n::MessageId, Vec<(&'static str, MsgArg)>);

/// What `api_token` refused with, or that it gave a token or none.
fn token_outcome(
    token_file: Option<&std::path::Path>,
    config_dir: Option<&std::path::Path>,
) -> Result<bool, Refusal> {
    api_token(token_file, config_dir)
        .map(|token| token.is_some())
        .map_err(|refused| (refused.message, refused.args))
}

#[test]
fn a_named_token_file_must_be_there_and_the_default_one_need_not_be() {
    let folder = tempfile::tempdir().expect("a scratch dir");
    let config = folder.path().join("config");
    std::fs::create_dir_all(&config).expect("the config folder");
    let missing = folder.path().join("missing-token");

    assert_eq!(
        token_outcome(Some(&missing), Some(&config)),
        Err((
            msg::CLI_MCP_TOKEN_FILE_MISSING,
            vec![("file", MsgArg::Text(missing.display().to_string()))]
        ))
    );
    assert_eq!(token_outcome(None, Some(&config)), Ok(false));
    assert_eq!(token_outcome(None, None), Ok(false));

    token_file(&config, "mcp-token", "ladderback_abcdefghijkl_secret\n");
    assert_eq!(token_outcome(None, Some(&config)), Ok(true));
    // A named file is read in place of the default one, never beside it.
    assert!(token_outcome(Some(&missing), Some(&config)).is_err());
    let named = token_file(folder.path(), "named", "ladderback_abcdefghijkl_other");
    assert_eq!(token_outcome(Some(&named), None), Ok(true));
}

#[test]
fn a_token_file_others_can_read_or_that_holds_no_token_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().expect("a scratch dir");
    let shared = token_file(folder.path(), "shared", "ladderback_abcdefghijkl_secret");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o644))
        .expect("its permissions");
    let blank = token_file(folder.path(), "blank", " \n");
    for (path, detail) in [
        (&shared, "it can be read by other users than its owner"),
        (&blank, "it holds no API token"),
    ] {
        assert_eq!(
            token_outcome(Some(path), None),
            Err((
                msg::CLI_MCP_TOKEN_FILE_UNREADABLE,
                vec![
                    ("file", MsgArg::Text(path.display().to_string())),
                    ("detail", MsgArg::Text(detail.to_owned())),
                ]
            ))
        );
    }
}

#[test]
fn a_token_is_never_sent_over_plain_http_to_another_machine() {
    let folder = tempfile::tempdir().expect("a scratch dir");
    let named = token_file(folder.path(), "named", "ladderback_abcdefghijkl_secret");
    let url = |text: &str| mcp::DaemonUrl::parse(text).expect("a daemon URL");
    let refusal =
        |started: Result<mcp::Server, Refused>| started.err().map(|refused| refused.message);

    assert_eq!(
        refusal(connect(
            url("http://nas.example.com:9898"),
            Some(&named),
            None
        )),
        Some(msg::CLI_MCP_TOKEN_OVER_PLAIN_HTTP)
    );
    assert_eq!(
        refusal(connect(url("https://nas.example.com"), Some(&named), None)),
        None
    );
    assert_eq!(
        refusal(connect(url("http://127.0.0.1:9898"), Some(&named), None)),
        None
    );
    // Without a token, plain HTTP to another machine is allowed.
    assert_eq!(
        refusal(connect(url("http://nas.example.com:9898"), None, None)),
        None
    );
}

/// Connect mode offers the proposal tools: the daemon it drives serves the
/// page a person approves a proposal on (§4.6).
#[test]
fn connect_mode_offers_the_proposal_tools() {
    let daemon = mcp::DaemonUrl::parse("http://127.0.0.1:9898").expect("a daemon URL");
    let Ok(server) = connect(daemon, None, None) else {
        panic!("connect mode didn't start");
    };
    assert_eq!(server.offer(), mcp::Offer::Connect);
}

#[tokio::test]
async fn both_backends_refuse_a_run_of_a_plan_the_configuration_doesnt_hold_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    let refused = in_process
        .run("nowhere".to_owned())
        .await
        .expect_err("a plan the configuration doesn't hold is refused");
    assert_eq!(
        Err(refused.clone()),
        connect.run("nowhere".to_owned()).await
    );
    assert_eq!(wire_name(refused.code), "plan-unknown");
}

#[tokio::test]
async fn both_backends_refuse_a_run_whose_password_cant_be_read_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    // `demo`'s password is in a variable the daemon doesn't have: refused
    // before anything is queued, and alike.
    let refused = in_process
        .run("home".to_owned())
        .await
        .expect_err("a password that can't be read is refused");
    assert_eq!(Err(refused.clone()), connect.run("home".to_owned()).await);
    assert!(
        wire_name(refused.code).starts_with("secret-"),
        "a secret's refusal: {refused:?}"
    );
}

#[tokio::test]
async fn both_backends_refuse_to_cancel_what_isnt_a_queued_or_running_backup_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let check = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(3)),
        OperationKind::Check,
        Trigger::Manual,
        Target::default(),
        at(1_800_000_003),
    );
    let daemon = a_daemon_with(scratch.path(), &[check]).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    for (id, code) in [
        // Not in the log.
        (
            Uuid::from_u128(9).hyphenated().to_string(),
            "operation-unknown",
        ),
        ("a/b".to_owned(), "operation-unknown"),
        // The backup the start ended as failed.
        (
            Uuid::from_u128(1).hyphenated().to_string(),
            "operation-not-running",
        ),
        // A check, queued: maintenance runs to its end (decision 93).
        (
            Uuid::from_u128(3).hyphenated().to_string(),
            "api-token-scope",
        ),
    ] {
        let refused = in_process
            .cancel(id.clone())
            .await
            .expect_err("the cancel is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.cancel(id.clone()).await,
            "{id}"
        );
        assert_eq!(wire_name(refused.code), code, "{id}");
    }
    assert_eq!(
        in_process
            .cancel(Uuid::from_u128(3).hyphenated().to_string())
            .await,
        Err(mcp::not_cancellable())
    );
}

#[tokio::test]
async fn both_backends_test_a_repository_alike_and_refuse_one_they_dont_hold_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    for alias in ["nowhere", "..", "a/b"] {
        let refused = in_process
            .test_repository(alias.to_owned())
            .await
            .expect_err("a repository the configuration doesn't hold is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.test_repository(alias.to_owned()).await,
            "{alias}"
        );
        assert_eq!(wire_name(refused.code), "repository-unknown", "{alias}");
    }
    // `demo`'s password is in a variable the daemon doesn't have: refused
    // before any location is tried, and alike.
    let refused = in_process
        .test_repository("demo".to_owned())
        .await
        .expect_err("a password that can't be read is refused");
    assert_eq!(
        Err(refused.clone()),
        connect.test_repository("demo".to_owned()).await
    );
    assert!(
        wire_name(refused.code).starts_with("secret-"),
        "a secret's refusal: {refused:?}"
    );
}

#[tokio::test]
async fn both_backends_refuse_a_forget_preview_of_a_repository_not_maintained_here_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    // `demo` says nothing of its maintenance, so it isn't maintained here
    // (I1): refused before anything is read.
    for (alias, code) in [
        ("nowhere", "repository-unknown"),
        ("..", "repository-unknown"),
        ("a/b", "repository-unknown"),
        ("demo", "maintenance-not-owner"),
    ] {
        let refused = in_process
            .preview_forget(alias.to_owned())
            .await
            .expect_err("the preview is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.preview_forget(alias.to_owned()).await,
            "{alias}"
        );
        assert_eq!(wire_name(refused.code), code, "{alias}");
    }
}

#[tokio::test]
async fn both_backends_export_a_plan_alike_and_refuse_one_they_dont_hold_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    for plan in ["nowhere", "..", "a/b"] {
        let refused = in_process
            .export_profile(plan.to_owned())
            .await
            .expect_err("a plan the configuration doesn't hold is refused");
        assert_eq!(
            Err(refused.clone()),
            connect.export_profile(plan.to_owned()).await,
            "{plan}"
        );
        assert_eq!(wire_name(refused.code), "plan-unknown", "{plan}");
    }
    assert_eq!(
        in_process.export_profile("home".to_owned()).await,
        connect.export_profile("home".to_owned()).await
    );
}

#[tokio::test]
async fn both_backends_refuse_to_export_a_password_the_configuration_holds_alike() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let config = "schema = 1\n\n\
        [repositories.demo]\nrepository = \"/srv/backups/demo\"\n\
        password = \"canary-inline-password\"\n\n\
        [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n";
    let daemon = a_daemon_over(scratch.path(), config, &[]).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);

    let refused = in_process
        .export_profile("home".to_owned())
        .await
        .expect_err("an inline password is refused");
    assert_eq!(
        Err(refused.clone()),
        connect.export_profile("home".to_owned()).await
    );
    assert_eq!(wire_name(refused.code), "inline-secret");
    let shown = serde_json::to_string(&refused).expect("a refusal as JSON");
    assert!(!shown.contains("canary-inline-password"), "{shown}");
}

#[tokio::test]
async fn both_backends_preview_an_import_alike_naming_the_profiles_it_uses() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);
    let profile = "[global]\nuse-profiles = [\"base\"]\n\n\
        [repository]\nrepository = \"/srv/restic-repo\"\n\
        password-file = \"/home/demo/.restic-password\"\n\n\
        [[backup.snapshots]]\nname = \"laptop\"\nsources = [\"/home/demo\"]\n";

    let previewed = in_process
        .preview_import(profile.to_owned(), "nas".to_owned())
        .await
        .expect("the profile previews");
    assert_eq!(
        Ok(previewed.clone()),
        connect
            .preview_import(profile.to_owned(), "nas".to_owned())
            .await
    );
    assert_eq!(previewed.repository, "nas");
    assert_eq!(previewed.plans, ["laptop"]);
    assert!(
        previewed
            .notices
            .iter()
            .any(|notice| notice.code == api::WarningCode::ProfileUsedNotRead),
        "{previewed:?}"
    );
}

#[tokio::test]
async fn both_backends_refuse_a_profile_alike_and_never_with_its_text() {
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon(scratch.path()).await;
    let connect = connected_to(&daemon).await;
    let in_process = InProcess::over(&daemon.state);
    let valid = "[repository]\nrepository = \"/srv/restic-repo\"\n\
        password-file = \"/home/demo/.restic-password\"\n\n\
        [[backup.snapshots]]\nname = \"laptop\"\nsources = [\"/home/demo\"]\n";

    for (profile, alias, code) in [
        (
            "[repository\npassword = \"canary-profile-password\"\n",
            "nas",
            "profile-invalid",
        ),
        (valid, "Not A Slug", "repository-alias-invalid"),
        (valid, "demo", "repository-alias-taken"),
    ] {
        let refused = in_process
            .preview_import(profile.to_owned(), alias.to_owned())
            .await
            .expect_err("the profile is refused");
        assert_eq!(
            Err(refused.clone()),
            connect
                .preview_import(profile.to_owned(), alias.to_owned())
                .await,
            "{alias}"
        );
        assert_eq!(wire_name(refused.code), code, "{alias}");
        let shown = serde_json::to_string(&refused).expect("a refusal as JSON");
        assert!(!shown.contains("canary"), "{shown}");
    }
}

/// What every secret of the canary test holds, and nothing else does.
const CANARY: &str = "i4-canary-5b1e";

/// How long a call of the canary test waits for its answer: a wait of one
/// second, with room for a loopback daemon.
const ANSWER: std::time::Duration = std::time::Duration::from_secs(10);

/// One MCP session with a backend, as its client sees it: every line the
/// server wrote, answers and notifications alike, kept in `transcript`.
struct Session {
    input: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    output: tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
    transcript: Vec<String>,
}

impl Session {
    fn over(backend: impl Backend) -> Self {
        use tokio::io::AsyncBufReadExt as _;
        let (client, server) = tokio::io::duplex(mcp::MAX_MESSAGE_BYTES);
        let (server_input, server_output) = tokio::io::split(server);
        let server = mcp::Server::new(
            Arc::new(backend),
            crate::build_info::running(),
            l10n::Localizer::new(&[]),
        );
        tokio::spawn(server.serve(server_input, server_output));
        let (output, input) = tokio::io::split(client);
        Self {
            input,
            output: tokio::io::BufReader::new(output).lines(),
            next: 0,
            transcript: Vec::new(),
        }
    }

    /// Calls `tool` with `arguments` in the current revision, the call
    /// carrying a progress token, and returns the answer.
    async fn call(&mut self, tool: &str, arguments: serde_json::Value) -> serde_json::Value {
        use tokio::io::AsyncWriteExt as _;
        self.next += 1;
        let id = self.next;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": arguments,
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                    "progressToken": id,
                },
            },
        });
        let mut line = request.to_string();
        line.push('\n');
        self.input
            .write_all(line.as_bytes())
            .await
            .expect("the call is sent");
        loop {
            let line = tokio::time::timeout(ANSWER, self.output.next_line())
                .await
                .expect("an answer in time")
                .expect("the session's output")
                .expect("the session still open");
            let message: serde_json::Value = serde_json::from_str(&line).expect("a message");
            self.transcript.push(line);
            if message["id"] == id {
                return message;
            }
        }
    }

    /// How many progress notifications the server sent.
    fn progress(&self) -> usize {
        self.transcript
            .iter()
            .filter(|line| line.contains("\"notifications/progress\""))
            .count()
    }
}

/// Whether a tool's answer is a result marked as an error, or a protocol
/// error.
fn refused(answer: &serde_json::Value) -> bool {
    answer["result"]["isError"] == true || answer.get("error").is_some()
}

/// The configuration of the canary test, over `scratch`: `demo`'s password
/// in a file, `vault`'s from a command that prints it and fails, `cold`'s in
/// the configuration itself, each a plan's; `remote`, reached at two
/// locations, with a password in each URL and options written inline, one
/// under a credential's name and one under another name; and `hooked`, a
/// plan's, whose password command, hooks and a command its backend runs a
/// person wrote with a token in them, which an export over MCP leaves out
/// (#856).
fn canary_config(scratch: &std::path::Path) -> String {
    let file = scratch.join("password");
    std::fs::write(&file, format!("{CANARY}-file\n")).expect("the password file");
    let held = scratch.join("held");
    std::fs::write(&held, format!("{CANARY}-command\n")).expect("the command's password");
    let file = file.display();
    let held = held.display();
    let source = scratch.display();
    format!(
        "schema = 1\n\n\
        [repositories.demo]\nrepository = \"/srv/backups/demo\"\npassword-file = \"{file}\"\n\n\
        [repositories.vault]\nrepository = \"/srv/backups/vault\"\nmaintenance = \"local\"\n\
        password-command = \"/bin/sh -c 'cat {held}; cat {held} >&2; exit 3'\"\n\n\
        [repositories.cold]\nrepository = \"/srv/backups/cold\"\npassword = \"{CANARY}-inline\"\n\n\
        [repositories.remote]\nrepository = \"rest:https://demo:{CANARY}-url@nas.example/remote\"\n\
        repository-id = \"0707070707070707070707070707070707070707070707070707070707070707\"\n\
        password-env = \"LADDERBACK_DEMO_PASSWORD\"\n\
        options = {{ secret_access_key = \"{CANARY}-option\", region = \"{CANARY}-region\" }}\n\n\
        [[repositories.remote.locations]]\n\
        repository = \"rest:https://demo:{CANARY}-second@backup.example/remote\"\n\
        options = {{ secret_access_key = \"{CANARY}-second-option\" }}\n\n\
        [repositories.hooked]\nrepository = \"/srv/backups/hooked\"\n\
        password-command = \"demo-vault read --token={CANARY}-password-command\"\n\
        options = {{ post-create-command = \"/usr/bin/curl -fsS https://hc.demo/{CANARY}-option-command\" }}\n\
        hooks = {{ run-after = [\"curl -fsS https://hc.demo/{CANARY}-repository-hook\"] }}\n\n\
        [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"{source}\"]\n\n\
        [[plans]]\nid = \"work\"\nrepository = \"vault\"\nsources = [\"{source}\"]\n\n\
        [[plans]]\nid = \"cold\"\nrepository = \"cold\"\nsources = [\"{source}\"]\n\n\
        [[plans]]\nid = \"hooked\"\nrepository = \"hooked\"\nsources = [\"{source}\"]\n\
        hooks = {{ run-failed = [\"curl -fsS https://hc.demo/{CANARY}-plan-hook\"] }}\n"
    )
}

/// Calls every tool, its refusals and invalid input included, and returns
/// the answer to `run_plan` of `home`, whose password the daemon reads.
async fn every_tool(session: &mut Session) -> serde_json::Value {
    use serde_json::json;
    let failed = Uuid::from_u128(1).hyphenated().to_string();
    let profile = format!(
        "[repository]\nrepository = \"/srv/restic-repo\"\npassword = \"{CANARY}-profile\"\n\n\
        [[backup.snapshots]]\nname = \"laptop\"\nsources = [\"/home/demo\"]\n"
    );
    let broken = format!("[repository\npassword = \"{CANARY}-broken\"\n");
    for (tool, arguments) in [
        ("status", json!({})),
        ("status", json!({"verbose": true})),
        ("list_plans", json!({})),
        ("list_plans", json!({"limit": "many"})),
        ("list_repositories", json!({})),
        ("list_snapshots", json!({"repository": "vault"})),
        ("list_snapshots", json!({"repository": "nowhere"})),
        ("list_snapshots", json!({})),
        ("list_operations", json!({})),
        ("list_operations", json!({"cursor": "not-a-cursor"})),
        ("get_operation", json!({"operation": failed})),
        ("get_operation", json!({"operation": "nowhere"})),
        ("get_operation", json!({"id": failed})),
        ("run_plan", json!({"plan": "work"})),
        ("run_plan", json!({"plan": "nowhere"})),
        ("run_plan", json!({"plan": "cold"})),
        ("cancel_operation", json!({"operation": failed})),
        (
            "wait_operation",
            json!({"operation": "nowhere", "timeout": 0}),
        ),
        ("test_repository", json!({"repository": "vault"})),
        ("test_repository", json!({"repository": "nowhere"})),
        ("preview_forget", json!({"repository": "vault"})),
        ("preview_forget", json!({"repository": "demo"})),
        (
            "preview_import",
            json!({"content": profile, "repository": "nas"}),
        ),
        (
            "preview_import",
            json!({"content": broken, "repository": "nas"}),
        ),
        (
            "preview_import",
            json!({"content": profile, "repository": "vault"}),
        ),
        ("export_profile", json!({"plan": "home"})),
        ("export_profile", json!({"plan": "work"})),
        ("export_profile", json!({"plan": "cold"})),
        ("export_profile", json!({"plan": "hooked"})),
    ] {
        session.call(tool, arguments).await;
    }
    let started = session.call("run_plan", json!({"plan": "home"})).await;
    if let Some(id) = started["result"]["structuredContent"]["operation"].as_str() {
        session
            .call("get_operation", json!({"operation": id}))
            .await;
        session
            .call("wait_operation", json!({"operation": id, "timeout": 1}))
            .await;
        session
            .call("cancel_operation", json!({"operation": id}))
            .await;
    }
    started
}

/// I4 over the MCP server (§0, §4.6): with a repository password in a file,
/// one from a command, one in the configuration, an admin password and an
/// API token, no result, refusal, progress notification or log line of
/// either backend holds a secret, whatever tool is called and however it
/// ends: a profile's own password and one it fails to parse around
/// included. The log is read as the daemon writes it by default and with
/// `RUST_LOG=debug`, at which the MCP SDK would log every request whole.
#[tokio::test]
async fn no_tool_of_either_backend_tells_or_logs_a_secret() {
    use domain::access::api_token::{ApiTokenLabel, Scope, Scopes};
    crate::composition::captured_log::capture_at(log::LevelFilter::Debug);
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let daemon = a_daemon_over(scratch.path(), &canary_config(scratch.path()), &[]).await;
    // A run asked for stays queued: its password is read when it is asked
    // for, and no worker starts.
    daemon.background.runs().freeze();
    let access_with = |seed| {
        Arc::new(app::access::Access::new(
            Arc::new(app::fakes::InMemoryAdminPassword::with(&format!(
                "{CANARY}-admin"
            ))),
            Arc::new(app::fakes::SeededRandom::new(seed)),
            Arc::new(platform::SystemClock),
            Arc::new(app::fakes::InMemoryApiTokenStore::new()),
            Arc::new(app::fakes::InMemorySessionStore::new()),
        ))
    };
    let token_of = |access: Arc<app::access::Access>| async move {
        access
            .create_api_token(
                ApiTokenLabel::new("nightly-mcp".to_owned()).expect("a label"),
                Scopes::new([Scope::Read, Scope::Run, Scope::Preview]).expect("scopes"),
            )
            .await
            .expect("a token")
            .text
            .expose()
            .to_owned()
    };
    let access = access_with(7);
    let token = token_of(Arc::clone(&access)).await;
    // A token of another daemon: one this daemon doesn't know.
    let unknown = token_of(access_with(8)).await;
    let mut state = daemon.state.clone();
    state.sign_in = http_adapter::SignIn::On(access);
    let url = served(state.clone()).await;
    let connect_with = |token: &str| {
        mcp::Connect::new(url.clone())
            .expect("a client")
            .with_token(mcp::ApiToken::parse(token).expect("a token"))
            .expect("loopback keeps a token private")
    };

    let mut sessions = [
        ("in-process", Session::over(InProcess::over(&state))),
        ("connect", Session::over(connect_with(&token))),
        (
            "connect, unknown token",
            Session::over(connect_with(&unknown)),
        ),
    ];
    let mut started = Vec::new();
    for (_, session) in &mut sessions {
        started.push(every_tool(session).await);
    }

    let mut secrets = vec![CANARY.to_owned()];
    for text in [&token, &unknown] {
        secrets.push(text.clone());
        // Its secret, after `ladderback_` and its ID.
        secrets.extend(text.splitn(3, '_').nth(2).map(str::to_owned));
    }
    for (backend, session) in &sessions {
        for line in &session.transcript {
            for secret in &secrets {
                assert!(!line.contains(secret.as_str()), "{backend}: {line}");
            }
        }
    }
    let logged = crate::composition::captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .chain(crate::composition::captured_log::infos())
        .chain(crate::composition::captured_log::written_under("debug"))
        // A directive for one of the SDK's modules, which outranks one for
        // the whole SDK.
        .chain(crate::composition::captured_log::written_under(
            "rmcp::service=debug",
        ));
    for line in logged {
        for secret in &secrets {
            assert!(!line.contains(secret.as_str()), "the log: {line}");
        }
    }
    // Debug records were logged, the SDK's among them: what the daemon's log
    // would write under `RUST_LOG=debug` was read, not nothing.
    let debug = crate::composition::captured_log::targets_at(log::Level::Debug);
    assert!(
        debug.iter().any(|target| target.starts_with("rmcp")),
        "{debug:?}"
    );
    // What was asked reached the daemon: the run read its password and
    // queued, the wait told its progress, and the unknown token got nothing.
    let [in_process, connect, unknown_token] = &sessions;
    let [in_process_run, connect_run, unknown_run]: [serde_json::Value; 3] =
        started.try_into().expect("one run per session");
    assert!(!refused(&in_process_run), "{in_process_run}");
    assert!(!refused(&connect_run), "{connect_run}");
    assert!(refused(&unknown_run), "{unknown_run}");
    assert!(in_process.1.progress() > 0, "{:?}", in_process.1.transcript);
    assert!(connect.1.progress() > 0, "{:?}", connect.1.transcript);
    assert_eq!(unknown_token.1.progress(), 0);
    // `remote`'s second location was listed, so the URLs and options
    // checked above were told masked, not left out.
    for (backend, session) in [in_process, connect] {
        assert!(
            session
                .transcript
                .iter()
                .any(|line| line.contains("backup.example")),
            "{backend}: {:?}",
            session.transcript
        );
    }
}

/// What `answer` names as the operation it is about: the one it tells, the
/// one asked to stop, or the one its refusal names.
fn operation_named(answer: &serde_json::Value) -> serde_json::Value {
    let result = &answer["result"];
    if result["isError"] == true {
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        let refusal: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        return refusal["args"]["operation"]["value"].clone();
    }
    match &result["structuredContent"]["operation"] {
        serde_json::Value::Object(operation) => operation["id"].clone(),
        cancelled => cancelled.clone(),
    }
}

/// The tools that take an operation take it under the name `run_plan`
/// returns it by, in both modes (§4.6): the run's result passed back as it
/// came is an operation each tells of or refuses by its ID, and the name
/// they took before is an argument they don't take.
#[tokio::test]
async fn both_backends_take_an_operation_under_the_name_a_run_returns_it() {
    use serde_json::json;
    let scratch = tempfile::tempdir().expect("a scratch dir");
    let password = scratch.path().join("password");
    std::fs::write(&password, "demo-password\n").expect("the password file");
    let config = format!(
        "schema = 1\n\n\
        [repositories.demo]\nrepository = \"/srv/backups/demo\"\npassword-file = \"{}\"\n\n\
        [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n",
        password.display()
    );
    let daemon = a_daemon_over(scratch.path(), &config, &[]).await;
    // A run asked for stays queued, so no worker starts.
    daemon.background.runs().freeze();
    let mut sessions = [
        ("in-process", Session::over(InProcess::over(&daemon.state))),
        ("connect", Session::over(connected_to(&daemon).await)),
    ];
    for (backend, session) in &mut sessions {
        let started = session.call("run_plan", json!({"plan": "home"})).await;
        let run = started["result"]["structuredContent"].clone();
        let id = run["operation"].clone();
        assert!(id.is_string(), "{backend}: {started}");
        let mut wait = run.clone();
        wait["timeout"] = json!(0);
        for (tool, arguments) in [
            ("get_operation", run.clone()),
            ("wait_operation", wait),
            ("cancel_operation", run.clone()),
        ] {
            let answer = session.call(tool, arguments).await;
            assert_eq!(operation_named(&answer), id, "{backend}, {tool}: {answer}");
            let answer = session.call(tool, json!({"id": id})).await;
            let text = answer["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{backend}, {tool}: {answer}"));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(text).expect("a refusal"),
                json!({
                    "code": "invalid-arguments",
                    "args": {
                        "field": {"type": "text", "value": "id"},
                        "problem": {"type": "text", "value": "unknown-key"},
                    },
                }),
                "{backend}, {tool}"
            );
        }
    }
}
