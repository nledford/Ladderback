//! Steps for the `@e2e` scenarios of `features/restore/restore.feature`: the
//! restore use case runs against the real worker and a scratch repository
//! made with `rustic_core` (see `snapshots.rs`), and what it wrote is read
//! straight from the disk.
//!
//! No step restores in place onto a path outside the scenario's own scratch
//! folder: an in-place restore writes over the entry's original path, so the
//! steps check that path is inside the scratch folder before restoring, and
//! refuse a snapshot that wasn't backed up at its own path there.

use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use app::execution::{Finished, RunOperation};
use app::fakes::InMemoryConfigStore;
use app::ports::{ConfigStore, RestorePlanSummary};
use app::restore::{RestoreAsk, RestoreError, RestoreSetup, Restores};
use app::secrets::ResolveRepository;
use cucumber::{given, then, when};
use domain::access::step_up::short_id;
use domain::browse::FolderPath;
use domain::execution::{FailureReason, OperationStatus};
use domain::planning::{AgentConfig, AgentSettings, Hooks};
use domain::restore::{Destination, RestoreOrigin};
use domain::{RepositoryAlias, TimeZone};
use platform::SystemClock;
use secrets::SystemSecrets;

use super::Agent;

/// What a restore scenario's restores gave.
#[derive(Default)]
pub struct RestoreE2e {
    finished: Option<Finished>,
    preview: Option<Result<RestorePlanSummary, RestoreError>>,
}

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

fn text(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 scratch path")
}

impl Agent {
    /// The agent's restore folder: `restored` in the scratch folder.
    fn restore_folder(&self) -> PathBuf {
        self.scratch().join("restored")
    }

    /// The restore use case on the real worker, configured with the
    /// scenario's repository and sign-in off.
    async fn restores(&mut self) -> Restores {
        let config = InMemoryConfigStore::new();
        let repositories = [(alias(), self.snapshot_entry())].into_iter().collect();
        let agent_config = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            repositories,
            Vec::new(),
        )
        .expect("the configuration is valid");
        config
            .save(&agent_config)
            .await
            .expect("the configuration saves");
        let store = self.store().await;
        let clock = Arc::new(SystemClock);
        Restores::new(
            self.engine.clone(),
            clock.clone(),
            Arc::new(config),
            ResolveRepository::new(Arc::new(SystemSecrets::new())),
            RunOperation::new(self.engine.clone(), clock, store),
            RestoreSetup {
                default_folder: text(&self.restore_folder()).to_owned(),
                zone: TimeZone::UTC,
            },
        )
    }

    /// The original path of `relative`, a path in the scratch folder.
    fn original(&self, relative: &str) -> PathBuf {
        self.scratch().join(relative)
    }

    /// The snapshot's path of `entry`, an original path relative to the
    /// scratch folder: itself when the source was backed up as `/source`,
    /// the scratch folder's real path joined with it when it was backed up
    /// at its own path.
    fn snapshot_path(&self, entry: &str) -> FolderPath {
        let path = match self.snapshot_backed_up_from() {
            Some(root) => {
                let original = root.join(entry);
                original
                    .strip_prefix("/")
                    .expect("an absolute real path")
                    .to_str()
                    .expect("a UTF-8 scratch path")
                    .to_owned()
            }
            None => entry.to_owned(),
        };
        FolderPath::parse(&path).expect("an entry path")
    }

    /// The snapshot's path of `entry` for a restore in place, which writes
    /// over `/` joined with it: refused unless that is inside the
    /// scenario's own scratch folder.
    fn in_place_path(&self, entry: &str) -> FolderPath {
        let root = self.snapshot_backed_up_from().expect(
            "a restore in place restores only a source backed up at its own path in the \
             scratch folder",
        );
        let scratch = fs::canonicalize(self.scratch()).expect("the scratch folder's real path");
        let original = root.join(entry);
        assert!(
            original
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
                && original.starts_with(&scratch)
                && original != scratch,
            "a restore in place writes only inside the scenario's scratch folder {scratch:?}, \
             not over {original:?}"
        );
        let path = self.snapshot_path(entry);
        assert_eq!(
            Path::new("/").join(path.to_string()),
            original,
            "the restore writes over the path checked"
        );
        path
    }

    /// Confirms and runs a restore of `entry` into `destination`, asked for
    /// in the agent's own web UI with sign-in off, until it ends.
    async fn restore_entry(&mut self, entry: FolderPath, destination: Destination) {
        let restores = self.restores().await;
        let snapshot = self.snapshot_made();
        let confirmation = short_id(&snapshot).to_owned();
        let ask = RestoreAsk {
            alias: alias(),
            snapshot,
            entry,
            destination,
            origin: RestoreOrigin::LocalUi,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            confirmation,
        };
        let op = self.next_snapshot_op();
        let prepared = match restores.start(op, ask).await {
            Ok(prepared) => prepared,
            Err(error) => panic!("the restore didn't start: {error:?}"),
        };
        self.restore.finished = Some(restores.run(prepared, futures::future::pending()).await);
    }

    fn restore_status(&self) -> &OperationStatus {
        self.restore
            .finished
            .as_ref()
            .expect("a restore ran")
            .operation
            .status()
    }
}

/// Only runs where file permissions work: `tests/e2e.rs` skips scenarios
/// tagged `@needs-file-permissions` elsewhere, visibly.
#[given("the agent's restore folder can't be written")]
fn restore_folder_unwritable(agent: &mut Agent) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let folder = agent.restore_folder();
        fs::create_dir_all(&folder).expect("the restore folder");
        // Read and search, no write: no folder can be made in it.
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o500))
            .expect("the restore folder's permissions change");
    }
    #[cfg(not(unix))]
    {
        let _ = agent;
        panic!("@needs-file-permissions scenarios are skipped off Unix");
    }
}

#[given(expr = "the original {string} now holds {string}")]
fn the_original_now_holds(agent: &mut Agent, path: String, content: String) {
    fs::write(agent.original(&path), content).expect("the original changes");
}

#[given(expr = "the original {string} now also holds {string}")]
fn the_original_now_also_holds(agent: &mut Agent, folder: String, name: String) {
    fs::write(agent.original(&folder).join(name), "extra").expect("a file the snapshot lacks");
}

#[when(expr = "the agent restores {string} of the snapshot into a new folder")]
async fn restores_into_a_new_folder(agent: &mut Agent, entry: String) {
    let path = agent.snapshot_path(&entry);
    agent.restore_entry(path, Destination::NewFolder).await;
}

#[when(expr = "the agent previews restoring {string} of the snapshot in place")]
async fn previews_in_place(agent: &mut Agent, entry: String) {
    let path = agent.in_place_path(&entry);
    let restores = agent.restores().await;
    let snapshot = agent.snapshot_made();
    let op = agent.next_snapshot_op();
    agent.restore.preview = Some(
        restores
            .preview_in_place(op, &alias(), snapshot, path, RestoreOrigin::LocalUi)
            .await,
    );
}

#[when(expr = "the agent restores {string} of the snapshot in place")]
async fn restores_in_place(agent: &mut Agent, entry: String) {
    let path = agent.in_place_path(&entry);
    agent.restore_entry(path, Destination::InPlace).await;
}

#[then("the restore ends as succeeded")]
fn ends_succeeded(agent: &mut Agent) {
    assert_eq!(agent.restore_status(), &OperationStatus::Succeeded);
}

#[then("the restore fails because its target can't be written")]
fn fails_unwritable(agent: &mut Agent) {
    assert_eq!(
        agent.restore_status(),
        &OperationStatus::Failed {
            reason: FailureReason::RestoreTargetUnwritable
        }
    );
}

#[then(expr = "the new folder holds {string} with {string} and {string} with {string}")]
fn the_new_folder_holds(
    agent: &mut Agent,
    first: String,
    first_content: String,
    second: String,
    second_content: String,
) {
    let made: Vec<PathBuf> = fs::read_dir(agent.restore_folder())
        .expect("the restore folder was made")
        .map(|entry| entry.expect("an entry of the restore folder").path())
        .collect();
    let [folder] = made.as_slice() else {
        panic!("one new folder in the restore folder: {made:?}");
    };
    for (name, content) in [(first, first_content), (second, second_content)] {
        let read = fs::read_to_string(folder.join(&name))
            .unwrap_or_else(|error| panic!("{name} in {folder:?}: {error}"));
        assert_eq!(read, content, "{name}");
    }
}

#[then("the agent's restore folder is still empty")]
fn restore_folder_empty(agent: &mut Agent) {
    let held: Vec<_> = fs::read_dir(agent.restore_folder())
        .expect("the restore folder is there")
        .collect();
    assert!(held.is_empty(), "the restore wrote {held:?}");
}

#[then(regex = r"^the preview says (\d+) files? would be added, (\d+) overwritten and (\d+) left as they are$")]
fn preview_counts(agent: &mut Agent, added: u64, overwritten: u64, unchanged: u64) {
    let preview = match agent.restore.preview.as_ref().expect("a preview was asked for") {
        Ok(preview) => preview,
        Err(error) => panic!("the preview failed: {error:?}"),
    };
    assert_eq!(
        (
            preview.files_added,
            preview.files_overwritten,
            preview.files_unchanged
        ),
        (added, overwritten, unchanged)
    );
}

#[then(expr = "the original {string} holds {string}")]
fn the_original_holds(agent: &mut Agent, path: String, content: String) {
    let read = fs::read_to_string(agent.original(&path)).expect("the original is there");
    assert_eq!(read, content);
}

#[then(expr = "the original {string} is still there")]
fn the_original_is_still_there(agent: &mut Agent, path: String) {
    assert!(agent.original(&path).is_file(), "{path} is gone");
}
