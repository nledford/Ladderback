mod hardening;

use std::collections::HashMap;

use axum::http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;
use app::fakes::InMemoryEventBus;

struct Files(HashMap<&'static str, &'static str>);

impl AssetSource for Files {
    fn get(&self, path: &str) -> Option<Asset> {
        self.0.get(path).map(|s| Asset {
            bytes: Cow::Borrowed(s.as_bytes()),
        })
    }
}

fn ui() -> Arc<dyn AssetSource> {
    Arc::new(Files(HashMap::from([
        ("index.html", "<!doctype html><title>Ladderback</title>"),
        ("web-1a2b.js", "export default 1;"),
        ("web-1a2b_bg.wasm", "\0asm"),
    ])))
}

/// Answers every plan but `missing`.
struct Plans;

/// The one operation the fake log holds, as the wire shows it.
pub(crate) fn canned_summary() -> api::OperationSummaryDto {
    api::OperationSummaryDto::new(
        "018f0000-0000-7000-8000-000000000001".to_owned(),
        "home".to_owned(),
        "backup".to_owned(),
        "succeeded".to_owned(),
        "2026-11-09T10:00:00Z".to_owned(),
        Some("2026-11-09T10:00:01Z".to_owned()),
        Some("2026-11-09T10:00:02Z".to_owned()),
        Vec::new(),
        None,
        Some(api::OperationResultDto::Backup {
            files: 3,
            bytes: 4096,
            duration_ms: 1000,
            snapshot: Some("cd".repeat(32)),
            bytes_processed: Some(65_536),
        }),
        None,
    )
    .unwrap()
}

/// The fake's reading of the stats of "demo", taken at `taken_at`.
fn demo_reading(taken_at: &str) -> api::StatsReadingDto {
    api::StatsReadingDto {
        taken_at: taken_at.to_owned(),
        snapshots: 3,
        total_size: 3000,
        stored_size: 1200,
        compression_saved_percent: Some(50),
    }
}

/// The refusal the fake stands for `repository` with: `None` for a
/// repository it answers.
fn refusal_of(repository: &str) -> Option<MaintenanceRefusal> {
    let text = |value: &str| value.to_owned();
    Some(match repository {
        "missing" => MaintenanceRefusal::UnknownRepository,
        "not-owner" => MaintenanceRefusal::NotOwner,
        "busy" => MaintenanceRefusal::Busy {
            running: text("018f0000-0000-7000-8000-000000000003"),
        },
        "keeps-nothing" => MaintenanceRefusal::KeepsNothing { plan: text("home") },
        "unknown-plan" => MaintenanceRefusal::UnknownPlan {
            plan: text("nowhere"),
        },
        "no-match" => MaintenanceRefusal::AdoptionNoMatch,
        "ambiguous" => MaintenanceRefusal::AdoptionAmbiguous {
            plans: vec![text("home"), text("mirror")],
        },
        "bad-token" => MaintenanceRefusal::InvalidBody,
        "stale" => MaintenanceRefusal::SelectionChanged,
        "no-secret" => MaintenanceRefusal::SecretRefused(SecretRefusal {
            code: api::ErrorCode::SecretMissing,
            args: vec![
                ("field", api::MsgArg::Text(text("password"))),
                ("source", api::MsgArg::Text(text("PW"))),
            ],
            detail: None,
        }),
        "not-found" => MaintenanceRefusal::RepositoryNotFound,
        "no-config" => MaintenanceRefusal::RepositoryConfigNotFound {
            detail: text("No repository config file found for `/media/usb/restic`."),
        },
        "failed" => MaintenanceRefusal::OperationFailed {
            detail: text("wrong password or no repository at the location"),
        },
        "slow" => MaintenanceRefusal::TimedOut,
        "unreachable" => MaintenanceRefusal::Unavailable {
            code: api::ErrorCode::ConfigUnavailable,
            args: Vec::new(),
            detail: None,
        },
        "engine-down" => MaintenanceRefusal::Unavailable {
            code: api::ErrorCode::EngineUnavailable,
            args: Vec::new(),
            detail: Some(text("no worker program")),
        },
        _ => return None,
    })
}

#[async_trait]
impl Operations for Plans {
    async fn start_backup(
        &self,
        plan: String,
        _trigger: domain::execution::Trigger,
    ) -> Result<String, StartError> {
        match plan.as_str() {
            "missing" => Err(StartError::UnknownPlan),
            "no-secret" => Err(StartError::SecretRefused(SecretRefusal {
                code: api::ErrorCode::SecretCommandFailed,
                args: vec![
                    ("field", api::MsgArg::Text("password".to_owned())),
                    ("source", api::MsgArg::Text("pass".to_owned())),
                ],
                detail: Some("no such entry".to_owned()),
            })),
            "keys" => Err(StartError::KeyFilesNotLeftOut),
            "waiting" => Err(StartError::RunWaiting),
            "mistaken" => {
                let (code, args, detail) = mistake();
                Err(StartError::Unavailable { code, args, detail })
            }
            _ => Ok("018f0000-0000-7000-8000-000000000001".to_owned()),
        }
    }

    async fn start_maintenance(
        &self,
        repository: String,
        request: MaintenanceRequest,
        selection: Option<String>,
    ) -> Result<String, MaintenanceRefusal> {
        if let Some(refusal) = refusal_of(&repository) {
            return Err(refusal);
        }
        if repository != "demo" {
            return Err(MaintenanceRefusal::UnknownRepository);
        }
        let adopt = matches!(request, MaintenanceRequest::Adopt { .. });
        match (adopt, selection.as_deref()) {
            (false, None) => Ok("018f0000-0000-7000-8000-000000000002".to_owned()),
            (true, None) => Ok("018f0000-0000-7000-8000-000000000004".to_owned()),
            // The selection a preview named: the run goes ahead.
            (false, Some(SELECTION)) => Ok("018f0000-0000-7000-8000-000000000005".to_owned()),
            (true, Some(SELECTION)) => Ok("018f0000-0000-7000-8000-000000000006".to_owned()),
            // Any other: the repository has changed since.
            (_, Some(_)) => Err(MaintenanceRefusal::SelectionChanged),
        }
    }

    async fn preview(
        &self,
        repository: String,
        request: MaintenanceRequest,
    ) -> Result<api::MaintenancePreviewDto, MaintenanceRefusal> {
        if let Some(refusal) = refusal_of(&repository) {
            return Err(refusal);
        }
        if repository != "demo" {
            return Err(MaintenanceRefusal::UnknownRepository);
        }
        let shown = |snapshot: &api::SnapshotDto| Some(snapshot.clone());
        let snapshots = [demo_snapshot()];
        Ok(match request {
            MaintenanceRequest::Forget => api::MaintenancePreviewDto::Forget {
                plans: vec![api::ForgetPlanDto::new(
                    "home".to_owned(),
                    3,
                    &snapshots,
                    shown,
                )],
                left_alone: api::LeftAloneDto::default(),
                selection: SELECTION.to_owned(),
                warnings: Vec::new(),
            },
            MaintenanceRequest::Adopt { into, .. } => {
                api::MaintenancePreviewDto::adopt(into, &snapshots, SELECTION.to_owned(), shown)
            }
            MaintenanceRequest::Prune => api::MaintenancePreviewDto::Prune {
                packs_marked: 2,
                packs_deleted: 1,
                packs_recovered: 0,
                bytes_deleted: 4096,
                keep_delete_hours: 144,
                warnings: Vec::new(),
            },
            MaintenanceRequest::Check => api::MaintenancePreviewDto::Check { read_data: false },
        })
    }

    async fn groups(
        &self,
        repository: String,
    ) -> Result<api::SnapshotGroupsDto, MaintenanceRefusal> {
        if let Some(refusal) = refusal_of(&repository) {
            return Err(refusal);
        }
        if repository != "demo" {
            return Err(MaintenanceRefusal::UnknownRepository);
        }
        Ok(api::SnapshotGroupsDto {
            plans: vec![api::PlanGroupDto::new(
                "home".to_owned(),
                &[demo_snapshot()],
                |snapshot| Some(snapshot.clone()),
            )],
            orphaned: Vec::new(),
            unmanaged: Vec::new(),
            warnings: Vec::new(),
        })
    }

    async fn stats(
        &self,
        repository: String,
    ) -> Result<api::RepositoryStatsDto, MaintenanceRefusal> {
        match refusal_of(&repository) {
            Some(refusal) => Err(refusal),
            // A repository never read has no reading.
            None if repository == "never-read" => Ok(api::RepositoryStatsDto { latest: None }),
            None if repository == "demo" => Ok(api::RepositoryStatsDto {
                latest: Some(demo_reading("2026-11-14T09:00:00Z")),
            }),
            None => Err(MaintenanceRefusal::UnknownRepository),
        }
    }

    async fn read_stats(
        &self,
        repository: String,
    ) -> Result<api::StatsReadingDto, MaintenanceRefusal> {
        match refusal_of(&repository) {
            Some(refusal) => Err(refusal),
            None if repository == "demo" => Ok(demo_reading("2026-11-15T09:00:00Z")),
            None => Err(MaintenanceRefusal::UnknownRepository),
        }
    }

    async fn history(
        &self,
        _plan: Option<String>,
        after: Option<String>,
        _limit: Option<u64>,
    ) -> Result<api::OperationsPageDto, HistoryError> {
        // A cursor that names no place to continue from is refused, as
        // the real log refuses it.
        if after.is_some_and(|cursor| cursor.parse::<uuid::Uuid>().is_err()) {
            return Err(HistoryError::BadCursor);
        }
        Ok(api::OperationsPageDto {
            operations: vec![canned_summary()],
            next: None,
        })
    }

    async fn operation(&self, id: String) -> Result<api::OperationDetailDto, LookupError> {
        if id == "018f0000-0000-7000-8000-000000000001" {
            Ok(api::OperationDetailDto {
                operation: canned_summary(),
                warnings: Vec::new(),
            })
        } else {
            Err(LookupError::Unknown)
        }
    }

    async fn cancel(&self, id: String, _by: Canceller) -> Result<(), CancelError> {
        if id == "018f0000-0000-7000-8000-000000000001" {
            // The one logged operation finished long ago.
            Err(CancelError::NotRunning)
        } else {
            Err(CancelError::Unknown)
        }
    }
}

/// Answers every repository but `missing`, with one snapshot.
struct DemoCatalog;

fn demo_snapshot() -> api::SnapshotDto {
    api::SnapshotDto::new(
        "ab".repeat(32),
        "2026-11-09T10:15:00Z".to_owned(),
        "demo-host".to_owned(),
        "home".to_owned(),
        vec!["ladderback-plan:demo-home".to_owned()],
        vec!["/home/demo".to_owned()],
    )
    .unwrap()
}

/// What the fake catalog says of the repository `empty` (an empty
/// folder) and of `no-config` (no config file found).
fn no_repository(alias: &str) -> SnapshotsError {
    match alias {
        "empty" => SnapshotsError::RepositoryNotFound,
        _ => SnapshotsError::RepositoryConfigNotFound {
            detail: "No repository config file found for `/media/usb/restic`.".to_owned(),
        },
    }
}

#[async_trait]
impl Catalog for DemoCatalog {
    async fn status(
        &self,
        full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, CatalogError> {
        Ok(api::StatusDto {
            plans: Vec::new(),
            running: Vec::new(),
            unreadable_repositories: Vec::new(),
            full_disk_access,
        })
    }

    async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
        Ok(api::PlansDto {
            plans: vec![api::PlanSummaryDto {
                id: "home".to_owned(),
                repository: "demo".to_owned(),
                scheduled: false,
                next_run: None,
                warnings: Vec::new(),
            }],
        })
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
        Ok(api::RepositoriesDto {
            repositories: vec![api::RepositoryDto {
                alias: "demo".to_owned(),
                repository: "/srv/demo/repo".to_owned(),
                repo_hot: None,
                password: api::PasswordSourceDto::Inline,
                maintained_here: true,
                create_offered: false,
                warnings: Vec::new(),
                locations: Vec::new(),
            }],
        })
    }

    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
        match alias.as_str() {
            "missing" => Err(SnapshotsError::Unknown),
            "engine-down" => Err(SnapshotsError::Unavailable {
                code: api::ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: None,
            }),
            "empty" | "no-config" => Err(no_repository(&alias)),
            "slow" => Err(SnapshotsError::TimedOut),
            _ => Ok(api::SnapshotsDto {
                snapshots: vec![demo_snapshot()],
            }),
        }
    }

    /// The folder `source/docs` of the snapshot `ab…` holds one file of
    /// a folder of 1,001 entries; the top holds `source`. The page
    /// echoes the path and continues from where it started, after as many
    /// entries as it was let hold.
    async fn folder(
        &self,
        alias: String,
        snapshot: domain::SnapshotId,
        folder: domain::browse::FolderPath,
        from: u64,
        most: u64,
    ) -> Result<api::FolderDto, FolderError> {
        match alias.as_str() {
            "missing" => return Err(FolderError::Repository(SnapshotsError::Unknown)),
            "empty" | "no-config" => {
                return Err(FolderError::Repository(no_repository(&alias)));
            }
            "engine-down" => {
                return Err(FolderError::Repository(SnapshotsError::Unavailable {
                    code: api::ErrorCode::EngineUnavailable,
                    args: Vec::new(),
                    detail: None,
                }));
            }
            "slow" => return Err(FolderError::Repository(SnapshotsError::TimedOut)),
            "no-secret" => {
                return Err(FolderError::Repository(SnapshotsError::SecretRefused(
                    SecretRefusal {
                        code: api::ErrorCode::SecretMissing,
                        args: vec![
                            ("field", api::MsgArg::Text("password".to_owned())),
                            ("source", api::MsgArg::Text("PW".to_owned())),
                        ],
                        detail: None,
                    },
                )));
            }
            _ => {}
        }
        if snapshot.as_str() != "ab".repeat(32) {
            return Err(FolderError::SnapshotUnknown);
        }
        let entry = |name: &str, kind, size| api::EntryDto {
            name: name.to_owned(),
            display: name.to_owned(),
            kind,
            size,
            modified: Some("2026-11-01T12:00:00Z".to_owned()),
        };
        let (entries, total) = match folder.to_string().as_str() {
            "" => (vec![entry("source", api::EntryKindDto::Folder, None)], 1),
            "source/docs" => (
                vec![entry("notes.md", api::EntryKindDto::File, Some(5))],
                1_001,
            ),
            _ => return Err(FolderError::FolderUnknown),
        };
        let next = from
            .checked_add(most.min(1_000))
            .filter(|next| *next < total);
        Ok(api::FolderDto {
            path: folder.to_string(),
            entries,
            total,
            next,
        })
    }

    /// Refuses the repositories the listing refuses; otherwise finds
    /// nothing, and echoes what it was asked for as the pattern, so a test
    /// sees what the route handed on.
    async fn find(
        &self,
        alias: String,
        pattern: domain::find::NamePattern,
        plan: Option<domain::PlanId>,
        caps: domain::find::SearchCaps,
    ) -> Result<api::FindDto, SnapshotsError> {
        match alias.as_str() {
            "missing" => Err(SnapshotsError::Unknown),
            "slow" => Err(SnapshotsError::TimedOut),
            _ => Ok(api::FindDto {
                pattern: format!(
                    "{} plan {} snapshots {} matches {}",
                    pattern.as_str(),
                    plan.as_ref().map_or("-", domain::PlanId::as_str),
                    caps.snapshots(),
                    caps.matches()
                ),
                ..api::FindDto::default()
            }),
        }
    }
}

fn state() -> AppState {
    AppState {
        events: Arc::new(InMemoryEventBus::new()),
        operations: Arc::new(Plans),
        catalog: Arc::new(DemoCatalog),
        repositories: Arc::new(Repos::default()),
        plans: Arc::new(crate::request_checks::tests::NoPlans),
        profiles: Arc::new(crate::request_checks::tests::NoProfiles),
        restores: Arc::new(crate::request_checks::tests::NoRestores),
        proposals: Arc::new(crate::request_checks::tests::NoProposals),
        dry_runs: Arc::new(crate::request_checks::tests::NoDryRuns),
        downloads: Arc::new(crate::request_checks::tests::NoDownloads),
        sign_in: crate::request_checks::tests::sign_in_off(),
        exposure: Exposure::Loopback,
        build: crate::request_checks::tests::test_build(),
        restart: Restart::default(),
        updates: Arc::default(),
        recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
        without_token: crate::request_checks::tests::without_token_over(Arc::new(
            app::fakes::InMemoryConfigStore::new(),
        )),
        full_disk_access: FullDiskAccessRoute::default(),
        sync: SyncHub::default(),
    }
}

async fn request(assets: Arc<dyn AssetSource>, method: Method, path: &str, body: &str) -> Response {
    router(assets, RequestChecks::default(), state())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "localhost:9898")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Serves the router on a loopback port, and returns where and the bus
/// its state holds.
async fn serving() -> (std::net::SocketAddr, Arc<app::fakes::InMemoryEventBus>) {
    let loopback = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
    let listener = GatedListener::bind(loopback).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let events = Arc::new(InMemoryEventBus::new());
    let state = AppState {
        events: events.clone(),
        operations: Arc::new(Plans),
        catalog: Arc::new(DemoCatalog),
        repositories: Arc::new(Repos::default()),
        plans: Arc::new(crate::request_checks::tests::NoPlans),
        profiles: Arc::new(crate::request_checks::tests::NoProfiles),
        restores: Arc::new(crate::request_checks::tests::NoRestores),
        proposals: Arc::new(crate::request_checks::tests::NoProposals),
        dry_runs: Arc::new(crate::request_checks::tests::NoDryRuns),
        downloads: Arc::new(crate::request_checks::tests::NoDownloads),
        sign_in: crate::request_checks::tests::sign_in_off(),
        exposure: Exposure::Loopback,
        build: crate::request_checks::tests::test_build(),
        restart: Restart::default(),
        updates: Arc::default(),
        recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
        without_token: crate::request_checks::tests::without_token_over(Arc::new(
            app::fakes::InMemoryConfigStore::new(),
        )),
        full_disk_access: FullDiskAccessRoute::default(),
        sync: SyncHub::default(),
    };
    let app = router(ui(), RequestChecks::default(), state);
    tokio::spawn(async move {
        let serving = axum::serve(listener, app);
        let _ = serving.await;
    });
    (addr, events)
}

async fn get(path: &str) -> Response {
    request(ui(), Method::GET, path, "").await
}

fn content_type(response: &Response) -> &str {
    response.headers()[header::CONTENT_TYPE].to_str().unwrap()
}

async fn body(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn the_events_handshake_upgrades_and_carries_events() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::header::ORIGIN;

    let (addr, events) = serving().await;
    let mut request = format!("ws://{addr}/api/v1/events")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert(ORIGIN, format!("http://{addr}").parse().unwrap());
    let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert!(
        response
            .headers()
            .contains_key(header::SEC_WEBSOCKET_ACCEPT),
        "an upgrade without the accept key is not a WebSocket"
    );
    // Nothing arrives before any event does; what is published arrives
    // as it is, in order.
    events
        .publish(UiEvent::OperationStarted {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            repository: None,
            started_at: "2026-11-09T10:00:00Z".into(),
        })
        .await;
    let text = match socket.next().await {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => text,
        other => panic!("the first event arrives as text: {other:?}"),
    };
    let event: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
    assert_eq!(
        event,
        serde_json::json!({
            "type": "operation-started",
            "operation": "018f0000-0000-7000-8000-000000000001",
            "repository": null,
            "started_at": "2026-11-09T10:00:00Z",
        })
    );
}

#[tokio::test]
async fn a_named_plan_is_accepted_with_its_operation() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        r#"{"plan":"home"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body(response).await,
        r#"{"operation":"018f0000-0000-7000-8000-000000000001"}"#
    );
}

#[tokio::test]
async fn a_plan_the_configuration_does_not_have_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        r#"{"plan":"missing"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // The refusal names the plan, as `ErrorCode::PlanUnknown` declares.
    assert_eq!(
        body(response).await,
        r#"{"code":"plan-unknown","args":{"plan":{"type":"text","value":"missing"}}}"#
    );
}

/// A backup that would take key files no glob can leave out is refused
/// with its own code, and nothing else: the data folder's path, not
/// being UTF-8, can't be shown as written.
#[tokio::test]
async fn a_plan_that_would_back_up_the_key_files_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        r#"{"plan":"keys"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(body(response).await, r#"{"code":"key-files-not-left-out"}"#);
}

/// A plan that has a run waiting already is refused with its own code,
/// naming the plan, and nothing is queued (§16, decision 75, point 4).
#[tokio::test]
async fn a_plan_that_has_a_run_waiting_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        r#"{"plan":"waiting"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(response).await,
        r#"{"code":"plan-run-waiting","args":{"plan":{"type":"text","value":"waiting"}}}"#
    );
}

/// A secret of the plan's repository that can't be resolved is refused
/// with its own code, its arguments and what the source said, as the
/// repository routes refuse it: not as a configuration that can't be
/// read (#309).
#[tokio::test]
async fn a_plan_whose_secret_cannot_be_resolved_is_refused_with_the_secret_s_code() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations",
        r#"{"plan":"no-secret"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
        serde_json::json!({
            "code": "secret-command-failed",
            "args": {
                "field": {"type": "text", "value": "password"},
                "source": {"type": "text", "value": "pass"},
            },
            "detail": "no such entry",
        })
    );
}

#[tokio::test]
async fn maintenance_on_a_repository_is_accepted_with_its_operation() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/maintenance",
        r#"{"kind":"forget"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body(response).await,
        r#"{"operation":"018f0000-0000-7000-8000-000000000002"}"#
    );
}

#[tokio::test]
async fn a_repository_the_configuration_does_not_have_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/missing/maintenance",
        r#"{"kind":"forget"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body(response).await,
        r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
    );
}

#[tokio::test]
async fn a_maintenance_kind_this_version_does_not_run_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/maintenance",
        r#"{"kind":"rewrite"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body(response).await,
        r#"{"code":"maintenance-kind-unknown","args":{"kind":{"type":"text","value":"rewrite"}}}"#
    );
}

#[tokio::test]
async fn a_forget_that_names_a_plan_to_adopt_into_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/maintenance",
        r#"{"kind":"forget","into":"home"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
}

#[tokio::test]
async fn a_busy_repository_is_refused_with_the_operation_that_holds_it() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/busy/maintenance",
        r#"{"kind":"prune"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(response).await,
        r#"{"code":"maintenance-busy","args":{"operation":{"type":"text","value":"018f0000-0000-7000-8000-000000000003"}}}"#
    );
}

#[tokio::test]
async fn a_policy_that_keeps_nothing_is_refused_with_its_plan() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/keeps-nothing/maintenance",
        r#"{"kind":"forget"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body(response).await,
        r#"{"code":"retention-keeps-nothing","args":{"plan":{"type":"text","value":"home"}}}"#
    );
}

#[tokio::test]
async fn an_adoption_is_accepted_with_its_operation() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/maintenance",
        r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body(response).await,
        r#"{"operation":"018f0000-0000-7000-8000-000000000004"}"#
    );
}

#[tokio::test]
async fn an_adoption_that_matches_no_plan_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/no-match/maintenance",
        r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body(response).await, r#"{"code":"adoption-no-match"}"#);
}

#[tokio::test]
async fn an_adoption_two_plans_match_is_refused_with_both() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/ambiguous/maintenance",
        r#"{"kind":"adopt","into":"home","from":{"from":"unmanaged"}}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body(response).await,
        r#"{"code":"adoption-ambiguous","args":{"plans":{"type":"text","value":"home, mirror"}}}"#
    );
}

#[tokio::test]
async fn an_adoption_without_its_group_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/maintenance",
        r#"{"kind":"adopt","into":"home"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
}

#[tokio::test]
async fn a_repository_that_cannot_be_read_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/unreachable/maintenance",
        r#"{"kind":"forget"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(response).await, r#"{"code":"config-unavailable"}"#);
}

/// The selection every preview of the fake names.
const SELECTION: &str = "0123456789abcdef";

#[tokio::test]
async fn a_repositorys_groups_are_listed_by_owner() {
    let response = get("/api/v1/repositories/demo/groups").await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(wire["plans"][0]["plan"], "home");
    assert_eq!(wire["plans"][0]["count"], 1);
    assert_eq!(wire["plans"][0]["snapshots"][0]["id"], "ab".repeat(32));
    assert_eq!(wire["orphaned"], serde_json::json!([]));
    assert_eq!(wire["unmanaged"], serde_json::json!([]));
}

#[tokio::test]
async fn a_repositorys_stats_are_the_reading_kept_last_or_none() {
    use serde_json::json;
    for (alias, expected) in [
        (
            "demo",
            json!({"latest": {
                "taken-at": "2026-11-14T09:00:00Z",
                "snapshots": 3,
                "total-size": 3000,
                "stored-size": 1200,
                "compression-saved-percent": 50,
            }}),
        ),
        ("never-read", json!({})),
    ] {
        let response = get(&format!("/api/v1/repositories/{alias}/stats")).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::OK, "{alias}");
        assert_eq!(answer, expected, "{alias}");
    }
}

#[tokio::test]
async fn reading_a_repositorys_stats_answers_the_new_reading() {
    let response = request(ui(), Method::POST, "/api/v1/repositories/demo/stats", "{}").await;
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["taken-at"], "2026-11-15T09:00:00Z");
    assert_eq!(answer["snapshots"], 3);
}

#[tokio::test]
async fn a_read_of_the_stats_with_a_body_that_is_not_json_is_refused() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/repositories/demo/stats",
        "not json",
    )
    .await;
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(answer, serde_json::json!({"code": "invalid-body"}));
}

#[tokio::test]
async fn each_preview_names_its_kind_and_what_the_run_would_do() {
    use serde_json::json;
    let snapshot = serde_json::to_value(demo_snapshot()).unwrap();
    for (request_body, expected) in [
        (
            r#"{"kind":"forget"}"#,
            json!({
                "kind": "forget",
                "plans": [{"plan": "home", "total": 3, "remove": 1, "listed": [snapshot]}],
                "left-alone": {
                    "orphaned": {"groups": 0, "snapshots": 0},
                    "unmanaged": {"groups": 0, "snapshots": 0},
                },
                "selection": SELECTION,
            }),
        ),
        (
            r#"{"kind":"adopt","into":"home","from":{"from":"orphaned","agent":"a","plan":"p"}}"#,
            json!({
                "kind": "adopt",
                "into": "home",
                "count": 1,
                "listed": [snapshot],
                "selection": SELECTION,
            }),
        ),
        (
            r#"{"kind":"prune"}"#,
            json!({
                "kind": "prune",
                "packs-marked": 2,
                "packs-deleted": 1,
                "packs-recovered": 0,
                "bytes-deleted": 4096,
                "keep-delete-hours": 144,
            }),
        ),
        (
            r#"{"kind":"check"}"#,
            json!({"kind": "check", "read-data": false}),
        ),
    ] {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance/preview",
            request_body,
        )
        .await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::OK, "{request_body}");
        assert_eq!(answer, expected, "{request_body}");
    }
}

#[tokio::test]
async fn a_start_hands_the_selection_it_carries_to_the_daemon() {
    use serde_json::json;
    for (request_body, status, expected) in [
        (
            r#"{"kind":"forget"}"#.to_owned(),
            202,
            json!({"operation": "018f0000-0000-7000-8000-000000000002"}),
        ),
        (
            format!(r#"{{"kind":"forget","selection":"{SELECTION}"}}"#),
            202,
            json!({"operation": "018f0000-0000-7000-8000-000000000005"}),
        ),
        (
            format!(
                r#"{{"kind":"adopt","into":"home","from":{{"from":"unmanaged"}},"selection":"{SELECTION}"}}"#
            ),
            202,
            json!({"operation": "018f0000-0000-7000-8000-000000000006"}),
        ),
        (
            r#"{"kind":"forget","selection":"ffffffffffffffff"}"#.to_owned(),
            409,
            json!({"code": "maintenance-selection-changed"}),
        ),
    ] {
        let response = request(
            ui(),
            Method::POST,
            "/api/v1/repositories/demo/maintenance",
            &request_body,
        )
        .await;
        let (actual, answer) = json_of(response).await;
        assert_eq!(actual.as_u16(), status, "{request_body}");
        assert_eq!(answer, expected, "{request_body}");
    }
}

#[tokio::test]
async fn every_maintenance_refusal_is_typed_with_the_status_of_its_kind() {
    use serde_json::json;
    let text = |value: &str| json!({"type": "text", "value": value});
    for (alias, status, expected) in [
        (
            "missing",
            404,
            json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
        ),
        (
            "unknown-plan",
            404,
            json!({"code": "plan-unknown", "args": {"plan": text("nowhere")}}),
        ),
        (
            "busy",
            409,
            json!({
                "code": "maintenance-busy",
                "args": {"operation": text("018f0000-0000-7000-8000-000000000003")},
            }),
        ),
        (
            "stale",
            409,
            json!({"code": "maintenance-selection-changed"}),
        ),
        (
            "keeps-nothing",
            422,
            json!({"code": "retention-keeps-nothing", "args": {"plan": text("home")}}),
        ),
        ("no-match", 422, json!({"code": "adoption-no-match"})),
        (
            "ambiguous",
            422,
            json!({"code": "adoption-ambiguous", "args": {"plans": text("home, mirror")}}),
        ),
        (
            "no-secret",
            422,
            json!({"code": "secret-missing", "args": {"field": text("password"), "source": text("PW")}}),
        ),
        (
            "failed",
            422,
            json!({
                "code": "operation-failed",
                "detail": "wrong password or no repository at the location",
            }),
        ),
        ("not-found", 404, json!({"code": "repository-not-found"})),
        (
            "no-config",
            404,
            json!({
                "code": "repository-config-not-found",
                "detail": "No repository config file found for `/media/usb/restic`.",
            }),
        ),
        ("bad-token", 400, json!({"code": "invalid-body"})),
        ("slow", 504, json!({"code": "repository-read-timed-out"})),
        ("unreachable", 503, json!({"code": "config-unavailable"})),
        (
            "engine-down",
            503,
            json!({"code": "engine-unavailable", "detail": "no worker program"}),
        ),
    ] {
        // The groups, the stats, a preview and a start refuse alike.
        for (method, route, request_body) in [
            (
                Method::GET,
                format!("/api/v1/repositories/{alias}/groups"),
                "",
            ),
            (
                Method::GET,
                format!("/api/v1/repositories/{alias}/stats"),
                "",
            ),
            (
                Method::POST,
                format!("/api/v1/repositories/{alias}/stats"),
                "{}",
            ),
            (
                Method::POST,
                format!("/api/v1/repositories/{alias}/maintenance/preview"),
                r#"{"kind":"forget"}"#,
            ),
            (
                Method::POST,
                format!("/api/v1/repositories/{alias}/maintenance"),
                r#"{"kind":"forget"}"#,
            ),
        ] {
            let response = request(ui(), method, &route, request_body).await;
            let (actual, answer) = json_of(response).await;
            assert_eq!(actual.as_u16(), status, "{alias} on {route}");
            assert_eq!(answer, expected, "{alias} on {route}");
        }
    }
}

#[tokio::test]
async fn maintenance_of_a_repository_this_agent_does_not_maintain_is_a_conflict_naming_it() {
    use serde_json::json;
    let expected = json!({
        "code": "maintenance-not-owner",
        "args": {"alias": {"type": "text", "value": "not-owner"}},
    });
    for route in [
        "/api/v1/repositories/not-owner/maintenance/preview",
        "/api/v1/repositories/not-owner/maintenance",
    ] {
        for kind in ["forget", "prune", "check"] {
            let body = format!(r#"{{"kind":"{kind}"}}"#);
            let response = request(ui(), Method::POST, route, &body).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status.as_u16(), 409, "{kind} on {route}");
            assert_eq!(answer, expected, "{kind} on {route}");
        }
    }
}

#[tokio::test]
async fn a_maintenance_request_that_takes_other_fields_is_refused_before_the_daemon_sees_it() {
    // The `demo` repository answers every request it is given, so each
    // refusal here is the route's own.
    const PREVIEW: &str = "/api/v1/repositories/demo/maintenance/preview";
    const START: &str = "/api/v1/repositories/demo/maintenance";
    let adopt = format!(
        r#"{{"kind":"adopt","into":"home","from":{{"from":"unmanaged"}},"selection":"{SELECTION}"}}"#
    );
    let selected = |kind: &str| format!(r#"{{"kind":"{kind}","selection":"{SELECTION}"}}"#);
    for (what, route, request_body) in [
        ("a selection on a prune", START, selected("prune")),
        ("a selection on a check", START, selected("check")),
        (
            "a selection on a forget preview",
            PREVIEW,
            selected("forget"),
        ),
        ("a selection on an adoption preview", PREVIEW, adopt),
        (
            "a key it does not know",
            START,
            r#"{"kind":"forget","selectoin":"0123456789abcdef"}"#.to_owned(),
        ),
        (
            "a key a preview does not know",
            PREVIEW,
            r#"{"kind":"forget","extra":1}"#.to_owned(),
        ),
        (
            "a forget that names a plan",
            PREVIEW,
            r#"{"kind":"forget","into":"home"}"#.to_owned(),
        ),
        (
            "a prune that names a group",
            PREVIEW,
            r#"{"kind":"prune","from":{"from":"unmanaged"}}"#.to_owned(),
        ),
        (
            "an adoption without its group",
            PREVIEW,
            r#"{"kind":"adopt","into":"home"}"#.to_owned(),
        ),
        (
            "an adoption without its plan",
            START,
            r#"{"kind":"adopt","from":{"from":"unmanaged"}}"#.to_owned(),
        ),
        (
            "a group of no kind",
            PREVIEW,
            r#"{"kind":"adopt","into":"home","from":{"from":"elsewhere"}}"#.to_owned(),
        ),
        ("a body that is not JSON", PREVIEW, "not json".to_owned()),
        ("an empty body", START, String::new()),
    ] {
        let response = request(ui(), Method::POST, route, &request_body).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{what} to {route}");
        assert_eq!(
            answer,
            serde_json::json!({"code": "invalid-body"}),
            "{what} to {route}"
        );
    }
    // A kind this version doesn't run is told as such, whatever else the
    // body holds.
    for (route, request_body) in [
        (PREVIEW, r#"{"kind":"wipe"}"#.to_owned()),
        (START, selected("wipe")),
    ] {
        let response = request(ui(), Method::POST, route, &request_body).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{request_body} to {route}");
        assert_eq!(
            answer,
            serde_json::json!({
                "code": "maintenance-kind-unknown",
                "args": {"kind": {"type": "text", "value": "wipe"}},
            }),
            "{request_body} to {route}"
        );
    }
}

#[tokio::test]
async fn the_maintenance_routes_want_json_and_bound_their_bodies() {
    for route in [
        "/api/v1/repositories/demo/maintenance",
        "/api/v1/repositories/demo/maintenance/preview",
    ] {
        let post = |content_type: &'static str, request_body: String| {
            router(ui(), RequestChecks::default(), state()).oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(route)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(request_body))
                    .unwrap(),
            )
        };
        let (status, answer) = json_of(
            post("text/plain", r#"{"kind":"forget"}"#.to_owned())
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{route}");
        assert_eq!(answer["code"], "json-required", "{route}");
        let oversize = format!(r#"{{"kind":"{}"}}"#, "a".repeat(3 * 1024 * 1024));
        let (status, answer) = json_of(post("application/json", oversize).await.unwrap()).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{route}");
        assert_eq!(answer["code"], "body-too-large", "{route}");
    }
}

#[tokio::test]
async fn the_maintenance_routes_take_nothing_but_their_own_method() {
    for (method, route) in [
        (Method::GET, "/api/v1/repositories/demo/maintenance"),
        (Method::GET, "/api/v1/repositories/demo/maintenance/preview"),
        (Method::POST, "/api/v1/repositories/demo/groups"),
    ] {
        let response = request(ui(), method.clone(), route, "{}").await;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {route}"
        );
    }
}
#[tokio::test]
async fn the_history_route_answers_one_page() {
    let response = get("/api/v1/operations").await;
    assert_eq!(response.status(), StatusCode::OK);
    let page: api::OperationsPageDto = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(page.operations.len(), 1);
    assert_eq!(page.next, None);
}

#[tokio::test]
async fn a_cursor_that_is_not_an_operation_is_refused() {
    let response = get("/api/v1/operations?after=not-an-operation").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body(response).await,
        r#"{"code":"invalid-cursor","args":{"cursor":{"type":"text","value":"not-an-operation"}}}"#
    );
}

#[tokio::test]
async fn an_unknown_operation_is_refused_by_detail_and_cancel() {
    let unknown = "018f0000-0000-7000-8000-00000000dead";
    let refusal = format!(
        r#"{{"code":"operation-unknown","args":{{"operation":{{"type":"text","value":"{unknown}"}}}}}}"#
    );
    let detail = get(&format!("/api/v1/operations/{unknown}")).await;
    assert_eq!(detail.status(), StatusCode::NOT_FOUND);
    assert_eq!(body(detail).await, refusal);
    let cancel = request(
        ui(),
        Method::POST,
        &format!("/api/v1/operations/{unknown}/cancel"),
        "{}",
    )
    .await;
    assert_eq!(cancel.status(), StatusCode::NOT_FOUND);
    assert_eq!(body(cancel).await, refusal);
}

#[tokio::test]
async fn a_finished_operation_is_refused_by_cancel() {
    let response = request(
        ui(),
        Method::POST,
        "/api/v1/operations/018f0000-0000-7000-8000-000000000001/cancel",
        "{}",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(response).await,
        r#"{"code":"operation-not-running","args":{"operation":{"type":"text","value":"018f0000-0000-7000-8000-000000000001"}}}"#
    );
}

#[tokio::test]
async fn a_body_that_is_not_the_json_the_route_takes_is_refused() {
    let response = request(ui(), Method::POST, "/api/v1/operations", "not json").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(response).await, r#"{"code":"invalid-body"}"#);
}

#[tokio::test]
async fn the_root_serves_the_ui_entry_page() {
    let response = get("/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(content_type(&response), "text/html; charset=utf-8");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-cache",
        "a new release must not be hidden behind a cached entry page"
    );
    assert_eq!(
        body(response).await,
        "<!doctype html><title>Ladderback</title>"
    );
}

#[tokio::test]
async fn assets_are_served_with_their_content_type() {
    let js = get("/web-1a2b.js").await;
    assert_eq!(js.status(), StatusCode::OK);
    assert_eq!(content_type(&js), "text/javascript");
    let wasm = get("/web-1a2b_bg.wasm").await;
    assert_eq!(content_type(&wasm), "application/wasm");
}

#[tokio::test]
async fn only_files_named_by_their_content_are_cached_for_good() {
    // Names as `trunk build` writes them: a content hash of 16 hex digits,
    // or fewer for the script and the module when the hash starts with
    // zeros, since Trunk doesn't pad theirs.
    let hashed = [
        "web-3fb4055f36b2f8e4.js",
        "web-10133c38bd221a27_bg.wasm",
        "web-fb4055f36b2f8e4.js",
        "web-fb405512.js",
        "style-d60c77a651c609fd.css",
        "favicon-31b6d255c3714cba.svg",
        "favicon-32-73b3bd9fb011307a.png",
    ];
    let unhashed = [
        "favicon.svg",
        "favicon-32.png",
        "icon-maskable-512.png",
        "web-fb40551.js",
        "web-03fb4055f36b2f8e4.js",
        "icon-deadbeefxl.png",
        "snippets/web-a1b2c3d4e5f60718/inline0.js",
    ];
    let assets: Arc<dyn AssetSource> = Arc::new(Files(
        hashed
            .iter()
            .chain(&unhashed)
            .map(|name| (*name, "x"))
            .collect(),
    ));
    for name in hashed {
        let response = request(Arc::clone(&assets), Method::GET, &format!("/{name}"), "").await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable",
            "{name}: a new build gives a changed file a new name"
        );
    }
    for name in unhashed {
        let response = request(Arc::clone(&assets), Method::GET, &format!("/{name}"), "").await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-cache",
            "{name}: a file that keeps its name across builds must be revalidated"
        );
    }
}

/// Every path but the API's is the UI's, so a client-side route whose
/// name only starts like the API's gets the page too.
#[tokio::test]
async fn client_side_routes_get_the_entry_page() {
    for route in ["/machines/laptop", "/plans", "/apiary", "/machines/api"] {
        let response = get(route).await;
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_eq!(content_type(&response), "text/html; charset=utf-8");
    }
}

#[tokio::test]
async fn client_side_routes_may_contain_dots() {
    for route in ["/machines/laptop.local", "/machines/nas.example.com"] {
        let response = get(route).await;
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_eq!(content_type(&response), "text/html; charset=utf-8");
    }
}

#[tokio::test]
async fn a_missing_file_is_not_found() {
    for path in ["/missing.js", "/api.js"] {
        let response = get(path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(body(response).await.is_empty(), "{path}");
    }
}

/// The API's paths no route serves: the router's fallback must never take
/// them for the UI's.
const NO_SUCH_ROUTES: &[&str] = &[
    "/api/v1/no-such-route",
    "/api/v2/plans",
    "/api/v1/",
    "/api",
    "//api/v1/no-such-route",
];

#[tokio::test]
async fn a_path_no_route_serves_is_refused_whatever_the_method() {
    use serde_json::json;
    for path in NO_SUCH_ROUTES {
        for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
            let response = request(ui(), method.clone(), path, "").await;
            assert_eq!(
                content_type(&response),
                "application/json",
                "{method} {path}"
            );
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "no-store",
                "{method} {path}"
            );
            assert_eq!(
                json_of(response).await,
                (StatusCode::NOT_FOUND, json!({"code": "route-unknown"})),
                "{method} {path}"
            );
        }
        let head = request(ui(), Method::HEAD, path, "").await;
        assert_eq!(head.status(), StatusCode::NOT_FOUND, "HEAD {path}");
        assert_eq!(content_type(&head), "application/json", "HEAD {path}");
    }
}

#[tokio::test]
async fn only_reads_are_allowed() {
    let response = request(ui(), Method::POST, "/", "").await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
}

#[tokio::test]
async fn a_binary_without_the_ui_says_so() {
    let response = request(Arc::new(Files(HashMap::new())), Method::GET, "/", "").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body(response).await.contains("web UI"));
}

/// Answers every path, as a careless file source would.
struct Anything;

impl AssetSource for Anything {
    fn get(&self, _path: &str) -> Option<Asset> {
        Some(Asset {
            bytes: Cow::Borrowed(b"secret"),
        })
    }
}

#[tokio::test]
async fn paths_never_leave_the_ui_folder() {
    for path in [
        "/../Cargo.toml",
        "/assets/../../secret.txt",
        "/%2e%2e/Cargo.toml",
        "/..%2fCargo.toml",
        "/..\\Cargo.toml",
    ] {
        let response = request(Arc::new(Anything), Method::GET, path, "").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

/// What a debug build reads the UI from: the folder, as it is when a file
/// is asked for.
#[cfg(debug_assertions)]
mod ui_folder {
    use std::fs;

    use super::*;

    #[test]
    fn files_built_after_the_binary_are_served() {
        let ui = tempfile::tempdir().unwrap();
        assert!(read_ui_file(ui.path(), "index.html").is_none());
        fs::create_dir(ui.path().join("assets")).unwrap();
        fs::write(ui.path().join("index.html"), "<title>built</title>").unwrap();
        fs::write(ui.path().join("assets/app.js"), "export default 1;").unwrap();

        let index = read_ui_file(ui.path(), "index.html").unwrap();
        assert_eq!(index.bytes.as_ref(), b"<title>built</title>");
        let script = read_ui_file(ui.path(), "assets/app.js").unwrap();
        assert_eq!(script.bytes.as_ref(), b"export default 1;");
    }

    #[test]
    fn an_unbuilt_ui_has_no_files() {
        let scratch = tempfile::tempdir().unwrap();
        let missing = scratch.path().join("dist");
        assert!(read_ui_file(&missing, "index.html").is_none());
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let ui = scratch.path().join("dist");
        fs::create_dir_all(ui.join("assets")).unwrap();
        let secret = scratch.path().join("secret.txt");
        fs::write(&secret, "secret").unwrap();
        let absolute = secret.to_str().unwrap();
        for path in ["../secret.txt", "assets/../../secret.txt", absolute] {
            assert!(read_ui_file(&ui, path).is_none(), "{path}");
        }
    }
}

#[tokio::test]
async fn the_plans_and_repositories_are_listed() {
    let plans = get("/api/v1/plans").await;
    assert_eq!(plans.status(), StatusCode::OK);
    assert_eq!(
        body(plans).await,
        r#"{"plans":[{"id":"home","repository":"demo","scheduled":false,"next-run":null}]}"#
    );

    let repositories = get("/api/v1/repositories").await;
    assert_eq!(repositories.status(), StatusCode::OK);
    assert_eq!(
        body(repositories).await,
        r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"},"maintained-here":true}]}"#
    );
}

/// A catalog whose configuration has a mistake on line 6: both lists and
/// every repository's snapshots are refused as a save of it is.
struct MistakenCatalog;

/// The code, the arguments and the detail a mistake on line 6 of the
/// configuration is refused with.
fn mistake() -> (
    api::ErrorCode,
    Vec<(&'static str, api::MsgArg)>,
    Option<String>,
) {
    (
        api::ErrorCode::ConfigInvalid,
        vec![
            (
                "file",
                api::MsgArg::Text("/etc/ladderback/config.toml".to_owned()),
            ),
            ("line", api::MsgArg::Text("6".to_owned())),
        ],
        Some("repositories.demo.colour".to_owned()),
    )
}

#[async_trait]
impl Catalog for MistakenCatalog {
    async fn status(
        &self,
        _full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, CatalogError> {
        let (code, args, detail) = mistake();
        Err(CatalogError::Unavailable { code, args, detail })
    }

    async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
        let (code, args, detail) = mistake();
        Err(CatalogError::Unavailable { code, args, detail })
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
        let (code, args, detail) = mistake();
        Err(CatalogError::Unavailable { code, args, detail })
    }

    async fn snapshots(&self, _alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
        let (code, args, detail) = mistake();
        Err(SnapshotsError::Unavailable { code, args, detail })
    }

    async fn folder(
        &self,
        _alias: String,
        _snapshot: domain::SnapshotId,
        _folder: domain::browse::FolderPath,
        _from: u64,
        _most: u64,
    ) -> Result<api::FolderDto, FolderError> {
        let (code, args, detail) = mistake();
        Err(FolderError::Repository(SnapshotsError::Unavailable {
            code,
            args,
            detail,
        }))
    }

    async fn find(
        &self,
        _alias: String,
        _pattern: domain::find::NamePattern,
        _plan: Option<domain::PlanId>,
        _caps: domain::find::SearchCaps,
    ) -> Result<api::FindDto, SnapshotsError> {
        let (code, args, detail) = mistake();
        Err(SnapshotsError::Unavailable { code, args, detail })
    }
}

/// Every route that reads the configuration refuses a mistake in it with
/// its code, its file, its line and its setting (#347). The maintenance
/// routes carry theirs through `MaintenanceRefusal::Unavailable`, tested
/// with the other refusals of maintenance.
#[tokio::test]
async fn a_configuration_that_cannot_be_used_is_refused_with_its_code_and_where_it_is() {
    for (method, path, sent) in [
        (Method::GET, "/api/v1/plans", ""),
        (Method::GET, "/api/v1/status", ""),
        (Method::GET, "/api/v1/repositories", ""),
        (Method::GET, "/api/v1/repositories/demo/snapshots", ""),
        (
            Method::GET,
            "/api/v1/repositories/demo/snapshots/abababababababababababababababababababababababababababababababab/folder",
            "",
        ),
        (Method::POST, "/api/v1/operations", r#"{"plan":"mistaken"}"#),
    ] {
        let state = AppState {
            catalog: Arc::new(MistakenCatalog),
            ..state()
        };
        let response = router(ui(), RequestChecks::default(), state)
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(sent))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
        let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "code": "config-invalid",
                "args": {
                    "file": {"type": "text", "value": "/etc/ladderback/config.toml"},
                    "line": {"type": "text", "value": "6"},
                },
                "detail": "repositories.demo.colour",
            }),
            "{path}"
        );
    }
}

/// An engine that can't be started is told by its code alone: its words
/// may hold a location, and go to the log.
#[tokio::test]
async fn a_listing_the_engine_cannot_start_is_refused_with_its_code_alone() {
    let response = get("/api/v1/repositories/engine-down/snapshots").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(response).await, r#"{"code":"engine-unavailable"}"#);
}

/// No repository at the location is a configuration to correct, not a
/// daemon that is unavailable: a listing and a folder read are refused
/// as 404, an empty folder as `repository-not-found` and a location with
/// no config file as `repository-config-not-found` with the engine's
/// masked words (#525).
#[tokio::test]
async fn a_listing_or_folder_read_of_no_repository_is_refused_as_not_found() {
    for (alias, expected) in [
        ("empty", r#"{"code":"repository-not-found"}"#),
        (
            "no-config",
            r#"{"code":"repository-config-not-found","detail":"No repository config file found for `/media/usb/restic`."}"#,
        ),
    ] {
        for route in [
            format!("/api/v1/repositories/{alias}/snapshots"),
            folder_route(alias, ""),
        ] {
            let response = get(&route).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
            assert_eq!(body(response).await, expected, "{route}");
        }
    }
}

/// A listing that ran out of time is told as a read that timed out.
#[tokio::test]
async fn a_listing_that_ran_out_of_time_is_refused_as_a_read_that_timed_out() {
    let response = get("/api/v1/repositories/slow/snapshots").await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        body(response).await,
        r#"{"code":"repository-read-timed-out"}"#
    );
}

#[tokio::test]
async fn a_repository_snapshots_are_listed_newest_first() {
    let response = get("/api/v1/repositories/demo/snapshots").await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(
        wire["snapshots"][0]["id"],
        "ab".repeat(32),
        "the snapshot the fake catalog holds"
    );
}

#[tokio::test]
async fn a_repository_the_configuration_does_not_have_is_a_typed_refusal() {
    let response = get("/api/v1/repositories/missing/snapshots").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body(response).await,
        r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
    );
}

/// The snapshot every folder test opens.
fn folder_route(alias: &str, query: &str) -> String {
    format!(
        "/api/v1/repositories/{alias}/snapshots/{}/folder{query}",
        "ab".repeat(32)
    )
}

/// A page of a folder carries its path, its entries with their kind,
/// size and modification time, the folder's total and where the next
/// page starts, from the position the request named.
#[tokio::test]
async fn a_page_of_a_snapshot_s_folder_starts_where_the_request_says() {
    let response = get(&folder_route("demo", "?path=source/docs&from=0")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "path": "source/docs",
            "entries": [{
                "name": "notes.md",
                "display": "notes.md",
                "kind": "file",
                "size": 5,
                "modified": "2026-11-01T12:00:00Z",
            }],
            "total": 1001,
            "next": 1000,
        })
    );
    let last = get(&folder_route("demo", "?path=source/docs&from=1000")).await;
    assert_eq!(last.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(last).await).unwrap();
    assert_eq!(wire.get("next"), None, "the last page names no next");
}

/// No path is the snapshot's top folder, and no position its first page.
#[tokio::test]
async fn a_folder_request_naming_nothing_opens_the_top_folder_s_first_page() {
    let response = get(&folder_route("demo", "")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(wire["path"], "");
    assert_eq!(wire["entries"][0]["name"], "source");
    assert_eq!(wire["entries"][0]["kind"], "folder");
    assert_eq!(wire["entries"][0].get("size"), None);
}

/// A position that is no count of entries is refused as an invalid
/// cursor, naming it.
#[tokio::test]
async fn a_folder_position_that_is_no_count_is_an_invalid_cursor() {
    for from in ["abc", "-1", "1.5", "18446744073709551616"] {
        let response = get(&folder_route("demo", &format!("?from={from}"))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{from}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            serde_json::json!({
                "code": "invalid-cursor",
                "args": {"cursor": {"type": "text", "value": from}},
            }),
            "{from}"
        );
    }
}

/// An ID no snapshot could have is as unknown as one the repository
/// doesn't hold; both are named.
#[tokio::test]
async fn a_snapshot_the_repository_does_not_hold_is_a_typed_refusal() {
    for snapshot in ["nope".to_owned(), "cd".repeat(32)] {
        let response = get(&format!(
            "/api/v1/repositories/demo/snapshots/{snapshot}/folder"
        ))
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{snapshot}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            serde_json::json!({
                "code": "snapshot-unknown",
                "args": {"snapshot": {"type": "text", "value": snapshot}},
            }),
        );
    }
}

/// A path no folder could have (an empty name) is as unknown as one the
/// snapshot doesn't hold; both are named as they were asked for.
#[tokio::test]
async fn a_folder_the_snapshot_does_not_hold_is_a_typed_refusal() {
    for (query, path) in [
        ("?path=source//docs", "source//docs"),
        ("?path=/source", "/source"),
        ("?path=source/gone", "source/gone"),
    ] {
        let response = get(&folder_route("demo", query)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            serde_json::json!({
                "code": "folder-unknown",
                "args": {"path": {"type": "text", "value": path}},
            }),
        );
    }
}

/// The repository is refused as its listing would be: unknown, a secret
/// that can't be resolved, or an engine that can't start.
#[tokio::test]
async fn a_folder_of_a_repository_that_cannot_be_read_is_refused_as_its_listing_is() {
    let missing = get(&folder_route("missing", "")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body(missing).await,
        r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
    );
    let secret = get(&folder_route("no-secret", "")).await;
    assert_eq!(secret.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let wire: serde_json::Value = serde_json::from_str(&body(secret).await).unwrap();
    assert_eq!(wire["code"], "secret-missing");
    let down = get(&folder_route("engine-down", "")).await;
    assert_eq!(down.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(down).await, r#"{"code":"engine-unavailable"}"#);
    let slow = get(&folder_route("slow", "")).await;
    assert_eq!(slow.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body(slow).await, r#"{"code":"repository-read-timed-out"}"#);
}

/// A page holds no more entries than the request's `limit`, and the next
/// page starts after them.
#[tokio::test]
async fn a_folder_page_holds_no_more_than_the_limit_asked_for() {
    let response = get(&folder_route("demo", "?path=source/docs&from=0&limit=10")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(wire["next"], 10, "the next page starts after ten entries");
}

/// A limit that is no count of entries is refused as an invalid argument,
/// naming the parameter and never the value given.
#[tokio::test]
async fn a_folder_limit_that_is_no_count_is_an_invalid_argument() {
    for limit in ["abc", "-1", "18446744073709551616"] {
        let response = get(&folder_route("demo", &format!("?limit={limit}"))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{limit}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            invalid_argument("limit", "invalid"),
            "{limit}"
        );
    }
}

/// The refusal of the query parameter `field` for `problem`.
fn invalid_argument(field: &str, problem: &str) -> serde_json::Value {
    serde_json::json!({
        "code": "invalid-arguments",
        "args": {
            "field": {"type": "text", "value": field},
            "problem": {"type": "text", "value": problem},
        },
    })
}

/// A search hands the catalog the pattern, the plan and the caps as the
/// request names them, a cap past its most lowered to it, and answers
/// what the catalog found.
#[tokio::test]
async fn a_search_hands_on_its_pattern_plan_and_caps() {
    let response = get(
        "/api/v1/repositories/demo/find?pattern=*.PDF&plan=documents&snapshots=5&matches=100000",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(
        wire["pattern"],
        "*.PDF plan documents snapshots 5 matches 500"
    );
    let defaults = get("/api/v1/repositories/demo/find?pattern=tax").await;
    let wire: serde_json::Value = serde_json::from_str(&body(defaults).await).unwrap();
    assert_eq!(wire["pattern"], "tax plan - snapshots 20 matches 100");
}

/// A pattern left out or one no name could match, a plan ID that can't be
/// one and a cap that is no count are each refused, naming the parameter.
#[tokio::test]
async fn a_search_s_parameters_are_checked_where_they_enter() {
    for (query, field, problem) in [
        ("", "pattern", "missing"),
        ("?pattern=", "pattern", "invalid"),
        ("?pattern=tax/2025", "pattern", "invalid"),
        ("?pattern=tax&plan=Not%20A%20Plan", "plan", "invalid"),
        ("?pattern=tax&snapshots=many", "snapshots", "invalid"),
        ("?pattern=tax&matches=-1", "matches", "invalid"),
    ] {
        let response = get(&format!("/api/v1/repositories/demo/find{query}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body(response).await).unwrap(),
            invalid_argument(field, problem),
            "{query}"
        );
    }
}

/// The repository is refused as its listing would be.
#[tokio::test]
async fn a_search_of_a_repository_that_cannot_be_read_is_refused_as_its_listing_is() {
    let missing = get("/api/v1/repositories/missing/find?pattern=tax").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body(missing).await,
        r#"{"code":"repository-unknown","args":{"alias":{"type":"text","value":"missing"}}}"#
    );
    let slow = get("/api/v1/repositories/slow/find?pattern=tax").await;
    assert_eq!(slow.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body(slow).await, r#"{"code":"repository-read-timed-out"}"#);
}

/// Answers by the location a request names, and counts the requests
/// that reach it.
#[derive(Default)]
struct Repos {
    calls: std::sync::atomic::AtomicUsize,
}

impl Repos {
    /// The one answer a location stands for: `Ok` for any other.
    fn answer(&self, settings: &api::RepositorySettingsDto) -> Result<(), RepositoriesError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match settings.repository.as_str() {
            "/invalid" => Err(RepositoriesError::InvalidBody),
            "/alias-invalid" => Err(RepositoriesError::AliasInvalid),
            "/alias-taken" => Err(RepositoriesError::AliasTaken {
                alias: "demo".to_owned(),
            }),
            "/already-configured" => Err(RepositoriesError::AlreadyConfigured {
                alias: "demo".to_owned(),
            }),
            "/exists" => Err(RepositoriesError::Exists {
                location: "/exists".to_owned(),
            }),
            "/not-found" => Err(RepositoriesError::NotFound),
            "/config-not-found" => Err(RepositoriesError::ConfigNotFound {
                detail: "No repository config file found for `/media/usb/restic`.".to_owned(),
            }),
            "/relative" => Err(RepositoriesError::PathRelative {
                field: "repo-hot".to_owned(),
            }),
            "/secret" => Err(RepositoriesError::SecretRefused(SecretRefusal {
                code: api::ErrorCode::SecretMissing,
                args: vec![
                    ("field", api::MsgArg::Text("password".to_owned())),
                    ("source", api::MsgArg::Text("PW".to_owned())),
                ],
                detail: None,
            })),
            "/failed" => Err(RepositoriesError::OperationFailed {
                detail: "wrong password or no repository at the location".to_owned(),
            }),
            "/unavailable" => Err(RepositoriesError::Unavailable {
                code: api::ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: Some("no worker program".to_owned()),
            }),
            "/config" => Err(RepositoriesError::Unavailable {
                code: api::ErrorCode::ConfigUnavailable,
                args: vec![("file", api::MsgArg::Text("/etc/config.toml".to_owned()))],
                detail: Some("permission denied".to_owned()),
            }),
            _ => Ok(()),
        }
    }
}

#[async_trait]
impl Repositories for Repos {
    async fn test(
        &self,
        request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        self.answer(&request.settings)?;
        Ok(api::RepositoryIdDto {
            id: format!("test:{}", request.settings.repository),
        })
    }

    async fn connect(
        &self,
        request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        self.answer(&request.settings)?;
        Ok(api::RepositoryIdDto {
            id: format!(
                "{}:{}:{}",
                request.alias,
                request.settings.repository,
                request.settings.repo_hot.as_deref().unwrap_or("-")
            ),
        })
    }

    async fn init(
        &self,
        request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError> {
        self.answer(&request.settings)?;
        Ok(api::RepositoryInitDto {
            id: format!("init:{}", request.settings.repository),
            saved: request.alias.is_some(),
        })
    }

    async fn create_saved(&self, alias: String) -> Result<api::RepositoryIdDto, RepositoriesError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match alias.as_str() {
            "missing" => Err(RepositoriesError::Unknown { alias }),
            "exists" => Err(RepositoriesError::Exists {
                location: "/srv/exists".to_owned(),
            }),
            "not-offered" => Err(RepositoriesError::CreateNotOffered),
            _ => Ok(api::RepositoryIdDto {
                id: format!("created:{alias}"),
            }),
        }
    }

    async fn test_saved(&self, alias: String) -> Result<api::LocationsTestDto, RepositoriesError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match alias.as_str() {
            "missing" => Err(RepositoriesError::Unknown { alias }),
            _ => Ok(api::LocationsTestDto {
                locations: vec![api::LocationTriedDto {
                    position: 1,
                    address: format!("/srv/{alias}"),
                    outcome: "used".to_owned(),
                    source: None,
                }],
            }),
        }
    }

    async fn change_maintainer(
        &self,
        alias: String,
        request: api::MaintainerDto,
    ) -> Result<(), RepositoriesError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match (alias.as_str(), request.maintain_here) {
            ("missing", _) => Err(RepositoriesError::Unknown { alias }),
            ("busy", _) => Err(RepositoriesError::MaintenanceBusy {
                operation: BUSY_OPERATION.to_owned(),
            }),
            ("demo", true) | ("stopped", false) => Ok(()),
            _ => Err(RepositoriesError::InvalidBody),
        }
    }

    async fn notes(&self) -> api::RepositoryNotesDto {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        api::RepositoryNotesDto {
            notes: vec![api::WarningDto::new(
                api::WarningCode::KeyringNotActivatable,
                [],
            )],
        }
    }
}

/// The page's notes are what the daemon says, each a code with no
/// sentence, and only a session reads them: no token's scope opens the
/// path, and no alias is it.
#[tokio::test]
async fn the_repositories_page_reads_its_notes_as_codes() {
    let (response, calls) =
        send_counted(Method::GET, repositories::NOTES_PATH, String::new()).await;
    assert_eq!(calls, 1);
    assert_eq!(response.status(), StatusCode::OK);
    let wire: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"notes": [{"code": "keyring-not-activatable"}]})
    );
    assert_eq!(
        crate::api_tokens::required_permission(&Method::GET, repositories::NOTES_PATH),
        crate::api_tokens::Needs::Session
    );
}

const TEST: &str = "/api/v1/repositories/_test";
const CONNECT: &str = "/api/v1/repositories";
const INIT: &str = "/api/v1/repositories/_init";
/// The maintenance operation the fake says runs on the repository `busy`.
const BUSY_OPERATION: &str = "018f0000-0000-7000-8000-0000000000cc";

/// A request body for `repository`, typed with a password.
fn settings_json(repository: &str) -> serde_json::Value {
    serde_json::json!({
        "repository": repository,
        "password": {"source": "inline", "value": "correct-horse"},
    })
}

/// Posts `body` to `path` against a fake that counts what reaches it:
/// the answer, and how many requests the fake took.
async fn post_counted(path: &str, body: String) -> (Response, usize) {
    send_counted(Method::POST, path, body).await
}

/// As [`post_counted`], with the request's `method`.
async fn send_counted(method: Method, path: &str, body: String) -> (Response, usize) {
    let repos = Arc::new(Repos::default());
    let state = AppState {
        events: Arc::new(InMemoryEventBus::new()),
        operations: Arc::new(Plans),
        catalog: Arc::new(DemoCatalog),
        repositories: repos.clone(),
        plans: Arc::new(crate::request_checks::tests::NoPlans),
        profiles: Arc::new(crate::request_checks::tests::NoProfiles),
        restores: Arc::new(crate::request_checks::tests::NoRestores),
        proposals: Arc::new(crate::request_checks::tests::NoProposals),
        dry_runs: Arc::new(crate::request_checks::tests::NoDryRuns),
        downloads: Arc::new(crate::request_checks::tests::NoDownloads),
        sign_in: crate::request_checks::tests::sign_in_off(),
        exposure: Exposure::Loopback,
        build: crate::request_checks::tests::test_build(),
        restart: Restart::default(),
        updates: Arc::default(),
        recent_log: Arc::new(app::recent_log::RecentLogRing::new()),
        without_token: crate::request_checks::tests::without_token_over(Arc::new(
            app::fakes::InMemoryConfigStore::new(),
        )),
        full_disk_access: FullDiskAccessRoute::default(),
        sync: SyncHub::default(),
    };
    let response = router(ui(), RequestChecks::default(), state)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "localhost:9898")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response,
        repos.calls.load(std::sync::atomic::Ordering::SeqCst),
    )
}

#[tokio::test]
async fn turning_maintenance_of_a_saved_repository_on_or_off_answers_204_or_a_typed_refusal() {
    use serde_json::json;
    let route = |alias: &str| format!("/api/v1/repositories/{alias}/maintainer");
    for (alias, on) in [("demo", true), ("stopped", false)] {
        let request = json!({"maintain-here": on}).to_string();
        let (response, calls) = send_counted(Method::PUT, &route(alias), request).await;
        assert_eq!(calls, 1, "{alias}");
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{alias}");
        assert_eq!(body(response).await, "", "{alias}");
    }
    let text = |value: &str| json!({"type": "text", "value": value});
    for (alias, status, expected) in [
        (
            "missing",
            404,
            json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
        ),
        (
            "busy",
            409,
            json!({"code": "maintenance-busy", "args": {"operation": text(BUSY_OPERATION)}}),
        ),
    ] {
        let request = json!({"maintain-here": false}).to_string();
        let (response, _) = send_counted(Method::PUT, &route(alias), request).await;
        let (actual, answer) = json_of(response).await;
        assert_eq!(actual.as_u16(), status, "{alias}");
        assert_eq!(answer, expected, "{alias}");
    }
    for request in ["not json", "{}", r#"{"maintain-here": true, "more": 1}"#] {
        let (response, calls) = send_counted(Method::PUT, &route("demo"), request.into()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{request}");
        assert_eq!(
            calls, 0,
            "a body that is not the request reached the daemon"
        );
    }
}

async fn json_of(response: Response) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    (status, serde_json::from_str(&body(response).await).unwrap())
}

#[tokio::test]
async fn a_test_answers_with_the_repositorys_id() {
    let request = serde_json::json!({"settings": settings_json("/ok")});
    let (response, calls) = post_counted(TEST, request.to_string()).await;
    assert_eq!(calls, 1);
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer, serde_json::json!({"id": "test:/ok"}));
}

#[tokio::test]
async fn a_connection_is_created_and_answers_with_the_repositorys_id() {
    let mut settings = settings_json("/ok");
    settings["repo-hot"] = serde_json::json!("/hot");
    let request = serde_json::json!({"alias": "demo", "settings": settings});
    let (response, _) = post_counted(CONNECT, request.to_string()).await;
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(answer, serde_json::json!({"id": "demo:/ok:/hot"}));
}

#[tokio::test]
async fn an_initialisation_says_whether_it_was_saved() {
    for (alias, saved) in [(None, false), (Some("fresh"), true)] {
        let mut request = serde_json::json!({"settings": settings_json("/new")});
        if let Some(alias) = alias {
            request["alias"] = serde_json::json!(alias);
        }
        let (response, _) = post_counted(INIT, request.to_string()).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::CREATED, "{alias:?}");
        assert_eq!(
            answer,
            serde_json::json!({"id": "init:/new", "saved": saved}),
            "{alias:?}"
        );
    }
}

#[tokio::test]
async fn a_saved_repository_is_created_and_answers_with_its_id() {
    let (response, calls) = post_counted("/api/v1/repositories/demo/init", "{}".into()).await;
    assert_eq!(calls, 1);
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(answer, serde_json::json!({"id": "created:demo"}));
}

#[tokio::test]
async fn a_test_of_a_saved_repository_answers_each_location_or_a_typed_refusal() {
    let (response, calls) = post_counted("/api/v1/repositories/nas/test", "{}".into()).await;
    assert_eq!(calls, 1);
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        answer,
        serde_json::json!({"locations": [
            {"position": 1, "address": "/srv/nas", "outcome": "used"}
        ]})
    );
    let (response, _) = post_counted("/api/v1/repositories/missing/test", "{}".into()).await;
    let (status, answer) = json_of(response).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(answer["code"], "repository-unknown");
    let (response, calls) = post_counted("/api/v1/repositories/nas/test", "not json".into()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls, 0, "a body that is not JSON reached the daemon");
}

#[tokio::test]
async fn creating_a_saved_repository_is_refused_by_the_kind_of_its_refusal() {
    use serde_json::json;
    let text = |value: &str| json!({"type": "text", "value": value});
    for (alias, status, expected) in [
        (
            "missing",
            404,
            json!({"code": "repository-unknown", "args": {"alias": text("missing")}}),
        ),
        (
            "exists",
            409,
            json!({"code": "repository-exists", "args": {"location": text("/srv/exists")}}),
        ),
        ("not-offered", 409, json!({"code": "create-not-offered"})),
    ] {
        let route = format!("/api/v1/repositories/{alias}/init");
        let (response, _) = post_counted(&route, "{}".into()).await;
        let (actual, answer) = json_of(response).await;
        assert_eq!(actual.as_u16(), status, "{alias}");
        assert_eq!(answer, expected, "{alias}");
    }
    let (response, calls) = post_counted("/api/v1/repositories/demo/init", "not json".into()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls, 0, "a body that is not JSON reached the daemon");
}

#[tokio::test]
async fn every_refusal_is_typed_with_the_status_of_its_kind() {
    use serde_json::json;
    let text = |value: &str| json!({"type": "text", "value": value});
    for (location, status, expected) in [
        ("/invalid", 400, json!({"code": "invalid-body"})),
        (
            "/alias-invalid",
            422,
            json!({"code": "repository-alias-invalid"}),
        ),
        (
            "/alias-taken",
            409,
            json!({"code": "repository-alias-taken", "args": {"alias": text("demo")}}),
        ),
        (
            "/already-configured",
            409,
            json!({"code": "repository-already-configured", "args": {"alias": text("demo")}}),
        ),
        (
            "/exists",
            409,
            json!({"code": "repository-exists", "args": {"location": text("/exists")}}),
        ),
        ("/not-found", 404, json!({"code": "repository-not-found"})),
        (
            "/config-not-found",
            404,
            json!({
                "code": "repository-config-not-found",
                "detail": "No repository config file found for `/media/usb/restic`.",
            }),
        ),
        (
            "/relative",
            422,
            json!({"code": "repository-path-relative", "args": {"field": text("repo-hot")}}),
        ),
        (
            "/secret",
            422,
            json!({"code": "secret-missing", "args": {"field": text("password"), "source": text("PW")}}),
        ),
        (
            "/failed",
            422,
            json!({
                "code": "operation-failed",
                "detail": "wrong password or no repository at the location",
            }),
        ),
        (
            "/unavailable",
            503,
            json!({"code": "engine-unavailable", "detail": "no worker program"}),
        ),
        (
            "/config",
            503,
            json!({
                "code": "config-unavailable",
                "args": {"file": text("/etc/config.toml")},
                "detail": "permission denied",
            }),
        ),
    ] {
        for (route, body) in [
            (TEST, json!({"settings": settings_json(location)})),
            (
                CONNECT,
                json!({"alias": "demo", "settings": settings_json(location)}),
            ),
            (INIT, json!({"settings": settings_json(location)})),
        ] {
            let (response, _) = post_counted(route, body.to_string()).await;
            let (actual, answer) = json_of(response).await;
            assert_eq!(actual.as_u16(), status, "{location} on {route}");
            assert_eq!(answer, expected, "{location} on {route}");
            assert!(
                !answer.to_string().contains("correct-horse"),
                "an answer holds the password: {answer}"
            );
        }
    }
}

#[tokio::test]
async fn a_body_that_is_not_the_json_the_route_takes_is_refused_before_the_daemon_sees_it() {
    let valid = settings_json("/ok");
    let hot_underscore = {
        let mut settings = valid.clone();
        settings["repo_hot"] = serde_json::json!("/y");
        settings
    };
    let command = serde_json::json!({
        "repository": "/ok",
        "password": {"source": "command", "command": "pass show x"},
    });
    for route in [TEST, CONNECT, INIT] {
        let wrap = |settings: &serde_json::Value| {
            serde_json::json!({"alias": "demo", "settings": settings}).to_string()
        };
        for (what, body) in [
            ("not JSON", "not json".to_owned()),
            ("an empty body", String::new()),
            ("an array", "[]".to_owned()),
            ("no settings", r#"{"alias":"demo"}"#.to_owned()),
            (
                "null settings",
                r#"{"alias":"demo","settings":null}"#.to_owned(),
            ),
            ("an unknown field", wrap(&hot_underscore)),
            ("a source the API refuses", wrap(&command)),
            (
                "an unknown field at the top",
                serde_json::json!({"alias": "demo", "settings": valid, "x": 1}).to_string(),
            ),
        ] {
            let (response, calls) = post_counted(route, body).await;
            let (status, answer) = json_of(response).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{what} to {route}");
            assert_eq!(
                answer,
                serde_json::json!({"code": "invalid-body"}),
                "{what} to {route}"
            );
            assert_eq!(calls, 0, "{what} to {route} reached the daemon");
        }
    }
    // A connection needs the name to save it under.
    let (response, calls) =
        post_counted(CONNECT, serde_json::json!({"settings": valid}).to_string()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls, 0);
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_before_the_daemon_sees_it() {
    let oversize = "a".repeat(3 * 1024 * 1024);
    for route in [TEST, CONNECT, INIT] {
        let request = serde_json::json!({"alias": "demo", "settings": settings_json(&oversize)});
        let (response, calls) = post_counted(route, request.to_string()).await;
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{route}");
        assert_eq!(answer["code"], "body-too-large", "{route}");
        assert_eq!(calls, 0, "{route} reached the daemon");
    }
}

#[tokio::test]
async fn the_routes_want_a_json_content_type() {
    for route in [TEST, CONNECT, INIT] {
        let response = router(ui(), RequestChecks::default(), state())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(route)
                    .header(header::HOST, "localhost:9898")
                    .header(header::CONTENT_TYPE, "text/plain")
                    .body(Body::from(r#"{"settings":{}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let (status, answer) = json_of(response).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{route}");
        assert_eq!(answer["code"], "json-required", "{route}");
    }
}

#[tokio::test]
async fn the_test_and_init_routes_take_nothing_but_a_post() {
    for route in [TEST, INIT] {
        let response = request(ui(), Method::GET, route, "").await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{route}");
    }
}
