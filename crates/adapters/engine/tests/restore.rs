//! A restore through the real worker (§10.4): into a new folder the worker
//! makes, refusing one that is there; in place, adding and overwriting and
//! never deleting; and the dry run, which counts and writes nothing.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use app::ports::{EngineEvent, EngineResult, Job, RestoreJob, RestorePlanSummary, RestoreTarget};
use domain::browse::FolderPath;
use domain::execution::FailureReason;
use domain::{SignedDuration, SnapshotId};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// A scratch repository with one snapshot of the source folder, backed up
/// at its real path, which holds `docs/notes.md` and `docs/report.txt`.
struct Restorable {
    scratch: Scratch,
    location: Location,
    source: PathBuf,
    snapshot: SnapshotId,
}

impl Restorable {
    async fn new() -> Self {
        let scratch = Scratch::new();
        let location = scratch.local();
        location.init();
        let source = fs::canonicalize(scratch.source()).unwrap();
        write_file(source.join("docs/notes.md"), b"hello");
        write_file(source.join("docs/report.txt"), b"hello world");
        let events = run_to_end(&engine(), backup_job(&location, &source, "daily", HOUR)).await;
        let snapshot = backed_up(&events).snapshot.clone();
        Self {
            scratch,
            location,
            source,
            snapshot,
        }
    }

    /// The snapshot's path of `relative`, a path in the source folder.
    fn entry(&self, relative: &str) -> FolderPath {
        let path = self.source.join(relative);
        FolderPath::parse(path.strip_prefix("/").unwrap().to_str().unwrap()).unwrap()
    }

    fn job(&self, relative: &str, target: RestoreTarget) -> RestoreJob {
        RestoreJob {
            repository: self.location.access(PASSWORD),
            snapshot: self.snapshot.clone(),
            entry: self.entry(relative),
            target,
            deadline_budget: HOUR,
        }
    }

    fn new_folder(&self) -> PathBuf {
        self.scratch.dir.path().join("restored/new")
    }

    async fn restore(&self, relative: &str, target: RestoreTarget) -> Vec<EngineEvent> {
        run_to_end(&engine(), Job::Restore(self.job(relative, target))).await
    }

    async fn plan(&self, relative: &str) -> RestorePlanSummary {
        let job = Job::RestorePlan(self.job(relative, RestoreTarget::InPlace));
        let events = run_to_end(&engine(), job).await;
        let EngineResult::RestorePlan(plan) = succeeded(&events) else {
            panic!("not a restore plan: {events:?}");
        };
        *plan
    }
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

fn failed(events: &[EngineEvent]) -> &FailureReason {
    match events.last() {
        Some(EngineEvent::Failed(reason)) => reason,
        other => panic!("the job did not fail: {other:?}\nall events: {events:?}"),
    }
}

#[tokio::test]
async fn a_restore_into_a_new_folder_makes_it_and_writes_the_entry_there() {
    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();

    let events = restorable
        .restore("docs", RestoreTarget::NewFolder(text(&folder)))
        .await;

    let EngineResult::Restore(summary) = succeeded(&events) else {
        panic!("not a restore result: {events:?}");
    };
    assert_eq!(
        (
            summary.files_restored,
            summary.files_unchanged,
            summary.bytes_restored
        ),
        (2, 0, 16)
    );
    assert_eq!(
        fs::read_to_string(folder.join("notes.md")).unwrap(),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(folder.join("report.txt")).unwrap(),
        "hello world"
    );
}

#[tokio::test]
async fn a_file_restored_into_a_new_folder_keeps_its_name_there() {
    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();

    let events = restorable
        .restore("docs/notes.md", RestoreTarget::NewFolder(text(&folder)))
        .await;

    succeeded(&events);
    assert_eq!(
        fs::read_to_string(folder.join("notes.md")).unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn a_new_folder_that_is_there_already_is_refused_and_nothing_in_it_is_touched() {
    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();
    write_file(folder.join("notes.md"), b"mine");

    let events = restorable
        .restore("docs", RestoreTarget::NewFolder(text(&folder)))
        .await;

    assert!(
        matches!(failed(&events), FailureReason::EngineFailed { .. }),
        "{events:?}"
    );
    assert_eq!(fs::read_to_string(folder.join("notes.md")).unwrap(), "mine");
    assert!(!folder.join("report.txt").exists());
}

/// The new folder holds what the snapshot holds, which may be private: it
/// and the folders above it the worker makes are the owner's alone, whatever
/// the umask, since rustic sets the modes of the entry's contents only.
#[cfg(unix)]
#[tokio::test]
async fn the_new_folder_and_the_folders_made_above_it_are_the_owner_s_alone() {
    use std::os::unix::fs::PermissionsExt;

    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();

    let events = restorable
        .restore("docs", RestoreTarget::NewFolder(text(&folder)))
        .await;

    succeeded(&events);
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        (mode(&folder), mode(folder.parent().unwrap())),
        (0o700, 0o700)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_new_folder_that_cannot_be_made_fails_as_an_unwritable_target() {
    use std::os::unix::fs::PermissionsExt;

    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();
    let parent = folder.parent().unwrap();
    fs::create_dir_all(parent).unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
    if fs::create_dir(parent.join("probe")).is_ok() {
        testkit::skip!("this user writes in a read-only folder (root?)");
    }

    let events = restorable
        .restore("docs", RestoreTarget::NewFolder(text(&folder)))
        .await;

    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(failed(&events), &FailureReason::RestoreTargetUnwritable);
}

#[tokio::test]
async fn an_entry_the_snapshot_does_not_hold_fails_and_makes_no_folder() {
    let restorable = Restorable::new().await;
    let folder = restorable.new_folder();

    let events = restorable
        .restore("docs/missing", RestoreTarget::NewFolder(text(&folder)))
        .await;

    assert!(
        matches!(failed(&events), FailureReason::EngineFailed { detail } if detail.contains("no entry")),
        "{events:?}"
    );
    assert!(!folder.exists());
}

#[tokio::test]
async fn a_dry_run_in_place_counts_what_it_would_add_overwrite_and_keep_and_writes_nothing() {
    let restorable = Restorable::new().await;
    let docs = restorable.source.join("docs");
    fs::write(docs.join("notes.md"), "changed").unwrap();
    fs::remove_file(docs.join("report.txt")).unwrap();
    fs::write(docs.join("extra.txt"), "extra").unwrap();

    let plan = restorable.plan("docs").await;

    assert_eq!(
        plan,
        RestorePlanSummary {
            files_added: 1,
            files_overwritten: 1,
            files_unchanged: 0,
            files_kept: 1,
            bytes_to_write: 16,
        }
    );
    assert_eq!(
        fs::read_to_string(docs.join("notes.md")).unwrap(),
        "changed"
    );
    assert!(!docs.join("report.txt").exists());
}

#[tokio::test]
async fn a_restore_in_place_overwrites_and_adds_and_deletes_nothing() {
    let restorable = Restorable::new().await;
    let docs = restorable.source.join("docs");
    fs::write(docs.join("notes.md"), "changed").unwrap();
    fs::remove_file(docs.join("report.txt")).unwrap();
    fs::write(docs.join("extra.txt"), "extra").unwrap();

    let events = restorable.restore("docs", RestoreTarget::InPlace).await;

    succeeded(&events);
    assert_eq!(fs::read_to_string(docs.join("notes.md")).unwrap(), "hello");
    assert_eq!(
        fs::read_to_string(docs.join("report.txt")).unwrap(),
        "hello world"
    );
    assert_eq!(fs::read_to_string(docs.join("extra.txt")).unwrap(), "extra");
}

/// rustic writes a file's contents on threads of its own and panics when a
/// write is refused; the worker reports that as the target refusing the
/// restore, not as an engine that stopped.
#[cfg(unix)]
#[tokio::test]
async fn a_read_only_original_that_differs_fails_the_restore_in_place_as_an_unwritable_target() {
    use std::os::unix::fs::PermissionsExt;

    let restorable = Restorable::new().await;
    let notes = restorable.source.join("docs/notes.md");
    fs::write(&notes, "changed").unwrap();
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&notes).is_ok() {
        testkit::skip!("this user writes a read-only file (root?)");
    }

    let events = restorable.restore("docs", RestoreTarget::InPlace).await;

    assert_eq!(failed(&events), &FailureReason::RestoreTargetUnwritable);
    assert_eq!(fs::read_to_string(&notes).unwrap(), "changed");
}
