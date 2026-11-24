use super::captured_log::{self, capture};
use std::sync::{Mutex, PoisonError};

use api::{ErrorCode, MsgArg};
use app::fakes::InMemoryConfigStore;

use super::service_support::*;
use super::*;
use crate::composition::catalog::{folder_refusal, listing_refusal};
use crate::composition::repositories::settings_of;

/// The lines logged so far about the snapshots of `alias`.
fn logged_about(alias: &str) -> Vec<String> {
    captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .filter(|line| line.contains(&format!("snapshots of {alias} ")))
        .collect()
}

/// A catalog whose repository `alias` has the password from `command`, a
/// credential written inline, and one read from a file.
async fn catalog_with_a_password_command(
    alias: &str,
    command: &str,
    engine: Arc<dyn Engine>,
) -> (ConfigCatalog, tempfile::TempDir) {
    catalog_at(alias, "/srv/demo/repo", command, engine).await
}

/// A catalog as [`catalog_with_a_password_command`] makes it, with its
/// repository at `location`.
async fn catalog_at(
    alias: &str,
    location: &str,
    command: &str,
    engine: Arc<dyn Engine>,
) -> (ConfigCatalog, tempfile::TempDir) {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let referenced = scratch.path().join("token");
    std::fs::write(&referenced, "canary-from-a-file\n").expect("the file is written");
    let config = Arc::new(InMemoryConfigStore::new());
    let settings = RepositorySettings {
        repository: location.to_owned(),
        repo_hot: None,
        password: SecretRef::Command(
            domain::secrets::CommandLine::parse(command).expect("a command"),
        ),
        options: [
            ("secret_access_key".to_owned(), "canary-inline".to_owned()),
            (
                "application_key".to_owned(),
                format!("${{file:{}}}", referenced.display()),
            ),
        ]
        .into(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };
    config
        .save(
            &domain::planning::AgentConfig::new(
                Default::default(),
                Default::default(),
                std::collections::BTreeMap::from([(
                    RepositoryAlias::parse(alias).expect("an alias"),
                    domain::planning::RepositoryConfig {
                        locations: Default::default(),
                        id: None,
                        settings,
                        maintenance: domain::planning::Maintenance::None,
                        group_by: Default::default(),
                        hooks: Default::default(),
                    },
                )]),
                Vec::new(),
            )
            .expect("a valid configuration"),
        )
        .await
        .expect("the configuration saves");
    let catalog = ConfigCatalog {
        config,
        engine,
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        leases: None,
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    (catalog, scratch)
}

/// The plans list carries the scheduler's planned run of each scheduled
/// plan, and none for a plan without a schedule, even where a run is
/// still kept for it (§5, #195).
#[tokio::test]
async fn the_plans_list_carries_the_planned_run_of_each_scheduled_plan() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let file = scratch.path().join("config.toml");
    std::fs::write(
        &file,
        "schema = 1\n\n[repositories.demo]\nrepository = \"/srv/demo\"\npassword = \"canary\"\n\n\
             [[plans]]\nid = \"home\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n\
             schedule = { cron = \"0 3 * * *\" }\n\n\
             [[plans]]\nid = \"docs\"\nrepository = \"demo\"\nsources = [\"/home/demo\"]\n",
    )
    .expect("the config file");
    let schedules = Arc::new(app::fakes::InMemoryScheduleStore::new());
    for (plan, second) in [("home", 1_794_200_400), ("docs", 1_794_200_000)] {
        schedules
            .plan_run(&app::ports::PlannedRun {
                plan: PlanId::try_from(plan.to_owned()).expect("a plan ID"),
                at: domain::Timestamp::from_second(second).expect("a time"),
            })
            .await
            .expect("the run is kept");
    }
    let catalog = ConfigCatalog {
        config: Arc::new(FileConfigStore::new(file, Arc::new(SystemClock))),
        engine: engine(),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules,
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        leases: None,
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    let listed = catalog.plans().await.expect("the plans");
    let shown: Vec<(&str, bool, Option<&str>)> = listed
        .plans
        .iter()
        .map(|plan| (plan.id.as_str(), plan.scheduled, plan.next_run.as_deref()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("home", true, Some("2026-11-09T05:00:00Z")),
            ("docs", false, None),
        ]
    );
}

/// On a paired machine, the repositories list says since when the
/// maintenance of each repository maintained here is paused for want of a
/// lease, as RFC 3339, and nothing for a repository maintained elsewhere or
/// one whose pause isn't noted yet (decision 158).
#[tokio::test]
async fn the_repositories_list_says_since_when_maintenance_here_is_paused() {
    use domain::fleet::{HubAddress, HubPin, PublicKey, TokenTls};
    use domain::maintenance::{HeldLeases, Pause};
    use domain::planning::Maintenance;
    let id =
        |digit: char| domain::RepositoryId::parse(&digit.to_string().repeat(64)).expect("an ID");
    let entry = |digit, maintenance| domain::planning::RepositoryConfig {
        locations: Default::default(),
        id: Some(id(digit)),
        settings: RepositorySettings {
            repository: format!("/srv/{digit}"),
            repo_hot: None,
            password: SecretRef::Inline(domain::secrets::SecretValue::new("canary")),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        },
        maintenance,
        group_by: Default::default(),
        hooks: Default::default(),
    };
    let alias = |name| RepositoryAlias::parse(name).expect("an alias");
    let config = Arc::new(InMemoryConfigStore::new());
    config
        .save(
            &domain::planning::AgentConfig::new(
                Default::default(),
                Default::default(),
                std::collections::BTreeMap::from([
                    (alias("photos"), entry('a', Maintenance::Local)),
                    (alias("music"), entry('b', Maintenance::None)),
                    (alias("docs"), entry('c', Maintenance::Local)),
                ]),
                Vec::new(),
            )
            .expect("a valid configuration"),
        )
        .await
        .expect("the configuration saves");
    let identity = Arc::new(app::fakes::InMemoryIdentity::default());
    let key = |byte| PublicKey::from_bytes([byte; 32]).expect("a key");
    let pin = HubPin::new(
        key(7),
        TokenTls::Ca,
        key(9),
        HubAddress::parse("https://hub.example:9898").expect("an address"),
    );
    app::ports::Identity::write_pin(identity.as_ref(), &pin)
        .await
        .expect("the pin is kept");
    let since = domain::Timestamp::from_second(1_800_000_000).expect("a time");
    let pause = Pause {
        since,
        alerted: false,
    };
    let cache = Arc::new(app::fakes::InMemoryLeaseCache::default());
    app::ports::LeaseCache::save_pauses(
        cache.as_ref(),
        &HeldLeases::default().with_pauses([(id('a'), pause), (id('b'), pause)]),
    )
    .await
    .expect("the pauses are kept");
    let clock = Arc::new(app::fakes::FakeClock::new(since));
    let catalog = ConfigCatalog {
        config,
        engine: engine(),
        operations: Arc::new(app::fakes::InMemoryOperationStore::new()),
        schedules: Arc::new(app::fakes::InMemoryScheduleStore::new()),
        sources: Arc::new(app::fakes::FakeSources),
        reads: Arc::default(),
        listings: Arc::default(),
        leases: Some(Arc::new(app::leases::MaintenanceLeases::new(
            identity, cache, clock,
        ))),
        link: Arc::new(NameWatch::new(
            Arc::new(app::fakes::FakeNetworkLink::new()),
            || {},
        )),
    };
    let listed = catalog.repositories().await.expect("the list");
    let shown: Vec<(&str, Option<&str>)> = listed
        .repositories
        .iter()
        .map(|row| (row.alias.as_str(), row.maintenance_paused_since.as_deref()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("docs", None),
            ("music", None),
            ("photos", Some("2027-01-15T08:00:00Z")),
        ]
    );
}

/// A listing whose password command fails with the other secrets of the
/// repository in its stderr is refused with the code of the failure,
/// and neither the answer nor the log holds one of them (I4).
#[tokio::test]
async fn a_listing_whose_password_command_fails_is_refused_with_its_code_and_masks_the_secrets() {
    capture();
    let (catalog, _scratch) = catalog_with_a_password_command(
        "failing-pass",
        "sh -c 'echo canary-inline canary-from-a-file >&2; exit 3'",
        engine(),
    )
    .await;
    let refused = catalog
        .snapshots("failing-pass".to_owned())
        .await
        .expect_err("the listing is refused");
    let SnapshotsError::SecretRefused(refusal) = refused else {
        panic!("a secret refusal, not {refused:?}");
    };
    assert_eq!(refusal.code, ErrorCode::SecretCommandFailed);
    let said = format!("{refusal:?}");
    let logged = logged_about("failing-pass").join("\n");
    for line in [&said, &logged] {
        assert!(!line.contains("canary-inline"), "{line}");
        assert!(!line.contains("canary-from-a-file"), "{line}");
    }
    assert!(logged.contains("SecretCommandFailed"), "{logged}");
    assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
}

/// An engine that can't be started says what it likes: what reaches the
/// log of the listing has the secrets of the repository masked, and the
/// debug text of the error is not logged as it is.
#[tokio::test]
async fn the_log_of_a_listing_the_engine_refused_masks_the_secrets_of_the_repository() {
    capture();
    let engine = engine();
    *engine
        .refuses
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::Unavailable {
        detail: "could not start with canary-inline and canary-from-a-file".to_owned(),
    });
    let (catalog, _scratch) =
        catalog_with_a_password_command("engine-down", "echo not-a-real-password", engine).await;
    let refused = catalog.snapshots("engine-down".to_owned()).await;
    assert_eq!(refused, Err(engine_unavailable()));
    let logged = logged_about("engine-down").join("\n");
    assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
    assert!(!logged.contains("canary-inline"), "{logged}");
    assert!(!logged.contains("canary-from-a-file"), "{logged}");
}

/// A listing the engine refused for more secrets than it masks is
/// refused with that code, in the answer and in the log (§10.2).
#[tokio::test]
async fn a_listing_the_engine_refused_for_too_much_to_mask_is_refused_with_its_code() {
    capture();
    let engine = engine();
    *engine
        .refuses
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::SecretsTooLargeToMask {
        field: Some("HTTPS_PROXY".to_owned()),
        limit: 262_144,
    });
    let (catalog, _scratch) =
        catalog_with_a_password_command("too-much", "echo not-a-real-password", engine).await;
    let refused = catalog.snapshots("too-much".to_owned()).await;
    assert_eq!(
        refused,
        Err(SnapshotsError::SecretRefused(SecretRefusal {
            code: ErrorCode::SecretsTooLargeToMask,
            args: vec![
                ("limit", MsgArg::Bytes(262_144)),
                ("field", MsgArg::Text("HTTPS_PROXY".to_owned())),
            ],
            detail: None,
        }))
    );
    let logged = logged_about("too-much").join("\n");
    assert!(logged.contains("SecretsTooLargeToMask"), "{logged}");
}

/// What the engine says of an rclone location is logged without the
/// credential written in it (I4).
#[tokio::test]
async fn the_log_of_a_listing_masks_the_credential_written_in_an_rclone_location() {
    capture();
    let engine = engine();
    let location = "rclone::sftp,host=nas.example.com,pass=CANARY-rclone-log:/backups";
    *engine
        .refuses
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::Unavailable {
        detail: format!("rclone could not open {location}"),
    });
    let (catalog, _scratch) =
        catalog_at("rclone-down", location, "echo not-a-real-password", engine).await;
    let refused = catalog.snapshots("rclone-down".to_owned()).await;
    assert_eq!(refused, Err(engine_unavailable()));
    let logged = logged_about("rclone-down").join("\n");
    assert!(
        logged.contains("rclone::sftp,host=nas.example.com,pass=***:/backups"),
        "{logged}"
    );
    assert!(!logged.contains("CANARY-rclone-log"), "{logged}");
}

/// A listing or a folder read the engine ran and failed is told by the
/// code of its failure, as a test of the repository tells it: a wrong
/// password is `password-not-accepted`, never an engine that can't run
/// (#637), and no repository at the location is told as such (#525).
#[tokio::test]
async fn a_listing_or_folder_read_that_failed_is_told_by_the_code_of_its_failure() {
    use domain::execution::RepositoryHalf;
    let said = "No repository config file found for `/srv/demo/repo`.";
    for (reason, expected) in [
        (
            FailureReason::PasswordNotAccepted,
            SnapshotsError::Unavailable {
                code: ErrorCode::PasswordNotAccepted,
                args: Vec::new(),
                detail: None,
            },
        ),
        (
            FailureReason::CredentialsRefused {
                half: Some(RepositoryHalf::Hot),
            },
            SnapshotsError::Unavailable {
                code: ErrorCode::CredentialsRefused,
                args: vec![("half", MsgArg::Text("hot".to_owned()))],
                detail: None,
            },
        ),
        (
            FailureReason::RepositoryNotFound,
            SnapshotsError::RepositoryNotFound,
        ),
        (
            FailureReason::RepositoryConfigNotFound {
                detail: said.to_owned(),
            },
            SnapshotsError::RepositoryConfigNotFound {
                detail: said.to_owned(),
            },
        ),
        // An engine that can't run is told by its code alone, its words in
        // the log (#836).
        (
            FailureReason::EngineUnavailable {
                detail: "no worker program".to_owned(),
            },
            engine_unavailable_alone(),
        ),
        (
            FailureReason::JobRejected {
                detail: "the worker refused the job".to_owned(),
            },
            engine_unavailable_alone(),
        ),
    ] {
        let engine = engine();
        *engine
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Err(reason.clone());
        let (catalog, _scratch) =
            catalog_with_a_password_command("failing", "echo not-a-real-password", engine).await;
        assert_eq!(
            catalog.snapshots("failing".to_owned()).await,
            Err(expected.clone()),
            "{reason:?}"
        );
        assert_eq!(
            catalog
                .folder("failing".to_owned(), a_snapshot(), docs(), 0, 1_000)
                .await,
            Err(FolderError::Repository(expected)),
            "{reason:?}"
        );
    }
}

/// The refusal of a read whose engine can't run: its code alone.
fn engine_unavailable_alone() -> SnapshotsError {
    SnapshotsError::Unavailable {
        code: ErrorCode::EngineUnavailable,
        args: Vec::new(),
        detail: None,
    }
}

/// A listing, a folder read or a search the engine ran and failed in its own
/// words, with no more specific code, is told as the maintenance routes tell
/// such a failure: a failed operation with those words, which the engine
/// masked (I4), never an engine that couldn't be reached (#836).
#[tokio::test]
async fn a_listing_folder_read_or_search_the_engine_failed_in_its_own_words_is_a_failed_operation()
{
    let said = "error sending request for url (http://demo:***@127.0.0.1:9/config)";
    let engine = engine();
    *engine
        .outcome
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Err(FailureReason::EngineFailed {
        detail: said.to_owned(),
    });
    let (catalog, _scratch) =
        catalog_with_a_password_command("rustic-said", "echo not-a-real-password", engine).await;
    let expected = SnapshotsError::OperationFailed {
        detail: said.to_owned(),
    };
    assert_eq!(
        catalog.snapshots("rustic-said".to_owned()).await,
        Err(expected.clone())
    );
    assert_eq!(
        catalog
            .folder("rustic-said".to_owned(), a_snapshot(), docs(), 0, 1_000)
            .await,
        Err(FolderError::Repository(expected.clone()))
    );
    let pattern = domain::find::NamePattern::parse("tax*").expect("a pattern");
    assert_eq!(
        catalog
            .find(
                "rustic-said".to_owned(),
                pattern,
                None,
                domain::find::SearchCaps::clamped(None, None),
            )
            .await
            .map(|_| ()),
        Err(expected)
    );
}

use app::ports::FolderAnswer;

/// An engine that answers every browse with `answer`, and keeps the
/// jobs it was given.
struct Browsing {
    answer: FolderAnswer,
    seen: Mutex<Vec<Job>>,
}

#[async_trait]
impl Engine for Browsing {
    async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(job);
        Ok(RunningJob {
            events: futures::stream::iter([EngineEvent::Succeeded(EngineResult::Folder(
                self.answer.clone(),
            ))])
            .boxed(),
            control: Box::new(Stop),
        })
    }
}

fn browsing(answer: FolderAnswer) -> Arc<Browsing> {
    Arc::new(Browsing {
        answer,
        seen: Mutex::new(Vec::new()),
    })
}

fn a_snapshot() -> SnapshotId {
    SnapshotId::parse(&"ab".repeat(32)).expect("a snapshot ID")
}

fn docs() -> FolderPath {
    FolderPath::parse("source/docs").expect("a folder path")
}

/// The lines logged so far about a browse of `alias`.
fn logged_about_browsing(alias: &str) -> Vec<String> {
    captured_log::lines()
        .into_iter()
        .map(|(_, line)| line)
        .filter(|line| line.contains(&format!("a snapshot of {alias} could not be read")))
        .collect()
}

/// A page is told with its folder's path, each entry's name, kind, size
/// and modification time, the folder's total and where the next page
/// starts; the engine is asked for that snapshot, folder and position.
#[tokio::test]
async fn a_page_of_a_snapshot_s_folder_is_told_entry_by_entry() {
    use domain::browse::{DisplayName, Entry, EntryKind, EntryName, FolderPage};
    // Each shown name differs from its key, so the page can't swap them.
    let entry = |name: &str, kind, size, modified: Option<i64>| Entry {
        name: EntryName::parse(name).expect("a name"),
        display: DisplayName::from(name.to_uppercase()),
        kind,
        size,
        modified: modified.map(|second| domain::Timestamp::from_second(second).expect("a time")),
    };
    let engine = browsing(FolderAnswer::Page(FolderPage {
        entries: vec![
            entry("drafts", EntryKind::Folder, None, None),
            entry("notes.md", EntryKind::File, Some(5), Some(1_793_534_400)),
            entry("latest", EntryKind::Link, None, None),
            entry("pipe", EntryKind::Other, None, None),
        ],
        total: 1_004,
        next: Some(1_004),
    }));
    let (catalog, _scratch) =
        catalog_with_a_password_command("demo", "echo not-a-real-password", engine.clone()).await;
    let page = catalog
        .folder("demo".to_owned(), a_snapshot(), docs(), 1_000, 1_000)
        .await
        .expect("the page");
    let entry = |name: &str, kind, size, modified: Option<&str>| api::EntryDto {
        name: name.to_owned(),
        display: name.to_uppercase(),
        kind,
        size,
        modified: modified.map(str::to_owned),
    };
    assert_eq!(
        page,
        api::FolderDto {
            path: "source/docs".to_owned(),
            entries: vec![
                entry("drafts", api::EntryKindDto::Folder, None, None),
                entry(
                    "notes.md",
                    api::EntryKindDto::File,
                    Some(5),
                    Some("2026-11-01T12:00:00Z")
                ),
                entry("latest", api::EntryKindDto::Link, None, None),
                entry("pipe", api::EntryKindDto::Other, None, None),
            ],
            total: 1_004,
            next: Some(1_004),
        }
    );
    let seen = engine.seen.lock().unwrap_or_else(PoisonError::into_inner);
    let [Job::Browse(job)] = seen.as_slice() else {
        panic!("one browse, not {seen:?}");
    };
    assert_eq!(
        (&job.snapshot, &job.folder, job.from),
        (&a_snapshot(), &docs(), 1_000)
    );
}

/// A snapshot or a folder that isn't there is told as such, not as an
/// engine that failed.
#[tokio::test]
async fn a_snapshot_or_folder_that_is_not_there_is_told_as_unknown() {
    for (answer, refused) in [
        (FolderAnswer::SnapshotUnknown, FolderError::SnapshotUnknown),
        (FolderAnswer::FolderUnknown, FolderError::FolderUnknown),
    ] {
        let (catalog, _scratch) =
            catalog_with_a_password_command("demo", "echo not-a-real-password", browsing(answer))
                .await;
        let read = catalog
            .folder("demo".to_owned(), a_snapshot(), docs(), 0, 1_000)
            .await;
        assert_eq!(read, Err(refused));
    }
}

/// An alias the configuration doesn't hold, or no alias could be, is an
/// unknown repository; the engine isn't asked.
#[tokio::test]
async fn a_folder_of_a_repository_the_configuration_does_not_hold_is_unknown() {
    let engine = browsing(FolderAnswer::FolderUnknown);
    let (catalog, _scratch) =
        catalog_with_a_password_command("demo", "echo not-a-real-password", engine.clone()).await;
    for alias in ["other", "not an alias!"] {
        let read = catalog
            .folder(alias.to_owned(), a_snapshot(), docs(), 0, 1_000)
            .await;
        assert_eq!(
            read,
            Err(FolderError::Repository(SnapshotsError::Unknown)),
            "{alias}"
        );
    }
    assert!(
        engine
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
}

/// The settings of a repository whose read ran out of time.
fn timed_out_settings() -> RepositorySettings {
    RepositorySettings {
        repository: "/srv/slow".to_owned(),
        repo_hot: None,
        password: SecretRef::Command(
            domain::secrets::CommandLine::parse("echo not-a-real-password").expect("a command"),
        ),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    }
}

/// A listing that ran out of time is told as a read that timed out,
/// not as an engine that can't be started (#710).
#[tokio::test]
async fn a_listing_that_ran_out_of_time_is_refused_as_a_read_that_timed_out() {
    let alias = RepositoryAlias::parse("slow").expect("an alias");
    let refused = listing_refusal(&alias, &timed_out_settings(), SnapshotError::TimedOut).await;
    assert_eq!(refused, SnapshotsError::TimedOut);
}

/// A folder read that ran out of time is told as a read that timed
/// out, not as an engine that can't be started.
#[tokio::test]
async fn a_folder_read_that_ran_out_of_time_is_refused_as_a_read_that_timed_out() {
    let alias = RepositoryAlias::parse("slow").expect("an alias");
    let refused = folder_refusal(
        "a folder of a snapshot",
        &alias,
        &timed_out_settings(),
        BrowseError::Read(app::maintenance::MaintenanceError::TimedOut),
    )
    .await;
    assert_eq!(refused, FolderError::Repository(SnapshotsError::TimedOut));
}

/// A browse whose password command fails is refused with the code of
/// the failure, and neither the answer nor the log holds a secret of
/// the repository (I4).
#[tokio::test]
async fn a_browse_whose_password_command_fails_is_refused_with_its_code_and_masks_the_secrets() {
    capture();
    let (catalog, _scratch) = catalog_with_a_password_command(
        "failing-browse",
        "sh -c 'echo canary-inline canary-from-a-file >&2; exit 3'",
        browsing(FolderAnswer::FolderUnknown),
    )
    .await;
    let refused = catalog
        .folder("failing-browse".to_owned(), a_snapshot(), docs(), 0, 1_000)
        .await
        .expect_err("the browse is refused");
    let FolderError::Repository(SnapshotsError::SecretRefused(refusal)) = refused else {
        panic!("a secret refusal, not {refused:?}");
    };
    assert_eq!(refusal.code, ErrorCode::SecretCommandFailed);
    let said = format!("{refusal:?}");
    let logged = logged_about_browsing("failing-browse").join("\n");
    for line in [&said, &logged] {
        assert!(!line.contains("canary-inline"), "{line}");
        assert!(!line.contains("canary-from-a-file"), "{line}");
    }
    assert!(logged.contains("SecretCommandFailed"), "{logged}");
    assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
}

/// An engine that can't be started is told by its code alone, and what
/// it said reaches the log with the repository's secrets masked (I4).
#[tokio::test]
async fn the_log_of_a_browse_the_engine_refused_masks_the_secrets_of_the_repository() {
    capture();
    let engine = engine();
    *engine
        .refuses
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::Unavailable {
        detail: "could not start with canary-inline and canary-from-a-file".to_owned(),
    });
    let (catalog, _scratch) =
        catalog_with_a_password_command("browse-down", "echo not-a-real-password", engine).await;
    let refused = catalog
        .folder("browse-down".to_owned(), a_snapshot(), docs(), 0, 1_000)
        .await;
    assert_eq!(refused, Err(FolderError::Repository(engine_unavailable())));
    let logged = logged_about_browsing("browse-down").join("\n");
    assert!(logged.contains(domain::secrets::REDACTED), "{logged}");
    assert!(!logged.contains("canary-inline"), "{logged}");
    assert!(!logged.contains("canary-from-a-file"), "{logged}");
}

/// A browse the engine refused for more secrets than it masks is
/// refused with that code (§10.2).
#[tokio::test]
async fn a_browse_the_engine_refused_for_too_much_to_mask_is_refused_with_its_code() {
    let engine = engine();
    *engine
        .refuses
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(EngineError::SecretsTooLargeToMask {
        field: Some("HTTPS_PROXY".to_owned()),
        limit: 262_144,
    });
    let (catalog, _scratch) =
        catalog_with_a_password_command("too-much", "echo not-a-real-password", engine).await;
    let refused = catalog
        .folder("too-much".to_owned(), a_snapshot(), docs(), 0, 1_000)
        .await;
    assert_eq!(
        refused,
        Err(FolderError::Repository(SnapshotsError::SecretRefused(
            SecretRefusal {
                code: ErrorCode::SecretsTooLargeToMask,
                args: vec![
                    ("limit", MsgArg::Bytes(262_144)),
                    ("field", MsgArg::Text("HTTPS_PROXY".to_owned())),
                ],
                detail: None,
            }
        )))
    );
}

/// A command's line cut at the cap can end in the beginning of a secret
/// the mask would not find: the first characters of it are not shown
/// either (I4).
#[tokio::test]
async fn the_words_of_a_failed_command_cut_at_the_cap_show_no_start_of_a_secret() {
    use domain::secrets::Problem;
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.options = [("secret_access_key".to_owned(), "canary-inline".to_owned())].into();
    let stderr = format!("{}canary-inline", "x".repeat(505));
    let error = unresolved(
        Field::Password,
        domain::secrets::SourceName::Command {
            program: "pass".to_owned(),
        },
        Problem::command_failed(Some(1), stderr.as_bytes()),
    );
    let detail = secret_refusal(&error, &settings, &SystemSecrets::default())
        .await
        .detail
        .expect("the words are kept");
    assert!(detail.ends_with('…'), "{detail}");
    assert!(!detail.contains("canary"), "{detail}");
}

/// A whole secret that ends just before the cap of a command's line is
/// masked whole: nothing is cut away to leave its start behind (I4).
#[tokio::test]
async fn the_words_of_a_failed_command_show_no_start_of_a_secret_just_before_the_cap() {
    use domain::secrets::{MAX_DETAIL_BYTES, Problem};
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.options = [("secret_access_key".to_owned(), "canary-inline".to_owned())].into();
    let before = "x".repeat(MAX_DETAIL_BYTES - "canary-inline".len() - 2);
    let stderr = format!("{before}canary-inlineyyyyyyyy");
    let error = unresolved(
        Field::Password,
        domain::secrets::SourceName::Command {
            program: "pass".to_owned(),
        },
        Problem::command_failed(Some(1), stderr.as_bytes()),
    );
    let detail = secret_refusal(&error, &settings, &SystemSecrets::default())
        .await
        .detail
        .expect("the words are kept");
    assert!(detail.ends_with('…'), "{detail}");
    assert!(!detail.contains("ca"), "{detail}");
}

/// A keyring with no service to ask is refused with a code of its own,
/// whose words say what is missing and what to use instead; what the
/// platform said is the detail, masked of the repository's secrets (I4).
#[tokio::test]
async fn a_keyring_with_no_service_is_refused_with_what_to_use_instead() {
    use domain::secrets::{Problem, SourceName};
    let mut settings = settings_of(settings_dto()).expect("settings");
    settings.options = [("secret_access_key".to_owned(), "canary-inline".to_owned())].into();
    let error = unresolved(
        Field::Password,
        SourceName::Keyring {
            entry: "ladderback/nas".to_owned(),
        },
        Problem::KeyringUnavailable {
            detail: "DBus error: no bus canary-inline".to_owned(),
            cause: domain::secrets::KeyringCause::NoService,
        },
    );
    let refusal = secret_refusal(&error, &settings, &SystemSecrets::default()).await;
    assert_eq!(
        refusal,
        SecretRefusal {
            code: ErrorCode::SecretKeyringUnavailable,
            args: vec![
                ("field", MsgArg::Text("password".to_owned())),
                ("source", MsgArg::Text("ladderback/nas".to_owned())),
            ],
            detail: Some("DBus error: no bus ***".to_owned()),
        }
    );
    let shown =
        l10n::Localizer::new(&[]).error(refusal.code, &refusal.args, refusal.detail.as_deref());
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    let text = shown.text();
    for expected in [
        "could not read the keyring entry ladderback/nas",
        "no keyring service it can use (DBus error: no bus ***)",
        "keep the secret in a file, a command or an environment variable instead",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
}
