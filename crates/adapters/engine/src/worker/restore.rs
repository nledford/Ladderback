//! A restore of one entry of a snapshot (technical design §10.4), and its dry
//! run. The entry is reached through the snapshot's trees one name at a
//! time, as a browse reaches a folder, and restored with rustic's own
//! restore: `delete` stays off, so a file the snapshot doesn't hold is never
//! removed. A new folder is made by the worker and refused when it is there
//! already, so no file that was there is touched.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use api::ErrorCode;
use jiff::SignedDuration;
use rustic_core::repofile::{Metadata, Node, NodeType, SnapshotId as RusticSnapshotId};
use rustic_core::{
    IndexedTree, LocalDestination, LsOptions, OpenStatus, Repository, RestoreOptions, RestoreStats,
    RusticError, TreeId,
};

use super::convert::JobError;
use super::repository::{open, repository_id};
use super::{Context, Done};
use crate::job::{OpResult, RestoreJobSpec, RestorePlanResult, RestoreResult, RestoreTargetSpec};

/// Restores the job's entry where its target says. The deadline counts from
/// the index read, as a backup's does (I3: a prune may delete the packs it
/// reads once `keep-delete` has passed).
pub(crate) fn restore(ctx: &Context, job: &RestoreJobSpec) -> Result<Done, JobError> {
    let (repository, tree) = snapshot_tree(ctx, job, Some(job.deadline_budget))?;
    let repository = repository.to_indexed()?;
    let (node, names) = entry(&repository, tree, &job.entry)?;
    let destination = match &job.target {
        RestoreTargetSpec::NewFolder { path } => {
            new_folder(Path::new(path))?;
            LocalDestination::new(path, false, false)?
        }
        RestoreTargetSpec::InPlace => {
            let path = original_path(&names)?;
            LocalDestination::new(&path, true, !node.is_dir())
                .map_err(|error| restore_failure(&error))?
        }
    };
    let options = RestoreOptions::default();
    let listing = repository.ls(&node, &LsOptions::default())?;
    let plan = repository
        .prepare_restore(&options, listing.clone(), &destination, false)
        .map_err(|error| restore_failure(&error))?;
    let result = restore_result(&plan.stats, plan.restore_size);
    repository
        .restore(plan, &options, listing, &destination)
        .map_err(|error| restore_failure(&error))?;
    // Everything is written: nothing is left that the deadline guards.
    ctx.disarm();
    ctx.report(OpResult::Restore(result))?;
    Ok(Done::Reported)
}

/// What [`restore`] would do, worked out by rustic's dry run, which writes
/// nothing: a read, with no deadline.
pub(crate) fn plan(ctx: &Context, job: &RestoreJobSpec) -> Result<Done, JobError> {
    let (repository, tree) = snapshot_tree(ctx, job, None)?;
    let repository = repository.to_indexed()?;
    let (node, names) = entry(&repository, tree, &job.entry)?;
    // A new folder doesn't exist yet: everything in the entry is added.
    let path = match &job.target {
        RestoreTargetSpec::NewFolder { path } => path.clone(),
        RestoreTargetSpec::InPlace => original_path(&names)?,
    };
    let expect_file = matches!(job.target, RestoreTargetSpec::InPlace) && !node.is_dir();
    let destination = LocalDestination::new(&path, false, expect_file)?;
    let listing = repository.ls(&node, &LsOptions::default())?;
    let plan =
        repository.prepare_restore(&RestoreOptions::default(), listing, &destination, true)?;
    Ok(Done::Result(OpResult::RestorePlan(plan_result(
        &plan.stats,
        plan.restore_size,
    ))))
}

/// The repository, opened, and the tree of the job's snapshot. It tells the
/// daemon it has started, with `budget` as the deadline, right before the
/// index read that comes next. The snapshot is listed rather than looked up,
/// so one that isn't there fails before anything is read or written.
fn snapshot_tree(
    ctx: &Context,
    job: &RestoreJobSpec,
    budget: Option<SignedDuration>,
) -> Result<(Repository<OpenStatus>, TreeId), JobError> {
    let id: RusticSnapshotId =
        job.snapshot.as_str().parse().map_err(|_| {
            JobError::failed(format!("not a snapshot ID: {}", job.snapshot.as_str()))
        })?;
    let repository = open(ctx, &job.repo)?;
    let repository_id = repository_id(&repository)?;
    let unknown = || JobError::failed(format!("the repository holds no snapshot {id}"));
    if !repository
        .list::<RusticSnapshotId>()?
        .any(|listed| listed == id)
    {
        return Err(unknown());
    }
    let snapshot = repository
        .get_snapshots(&[job.snapshot.as_str()])?
        .into_iter()
        .next()
        .ok_or_else(unknown)?;
    ctx.start(budget, Some(&repository_id))?;
    Ok((repository, snapshot.tree))
}

/// The node `names` leads to from the tree `root`, and each name on the way
/// as a file system writes it. Each name is matched against the escaped names
/// the trees record, one level at a time, as a browse does.
fn entry<S: IndexedTree>(
    repository: &Repository<S>,
    root: TreeId,
    names: &[String],
) -> Result<(Node, Vec<OsString>), JobError> {
    let mut node = Node::new_node(OsStr::new(""), NodeType::Dir, Metadata::default());
    node.subtree = Some(root);
    let mut written = Vec::new();
    for name in names {
        let Some(subtree) = node.subtree.filter(|_| node.is_dir()) else {
            return Err(entry_unknown(names));
        };
        let Some(found) = repository
            .get_tree(&subtree)?
            .nodes
            .into_iter()
            .find(|node| node.name == *name)
        else {
            return Err(entry_unknown(names));
        };
        written.push(found.name().into_owned());
        node = found;
    }
    Ok((node, written))
}

fn entry_unknown(names: &[String]) -> JobError {
    JobError::failed(format!("the snapshot holds no entry {}", names.join("/")))
}

/// The entry's original path: `/` joined with its names. rustic takes a
/// destination as text, so a name that isn't UTF-8 can't be restored in
/// place.
fn original_path(names: &[OsString]) -> Result<String, JobError> {
    let path: PathBuf = std::iter::once(OsStr::new("/"))
        .chain(names.iter().map(OsString::as_os_str))
        .collect();
    path.into_os_string().into_string().map_err(|_| {
        JobError::failed("the entry's original path isn't UTF-8, which rustic can't restore to")
    })
}

/// Makes the restore's new folder, with the folders above it it needs. The
/// folder itself is made exclusively: one that is there already is refused,
/// so nothing in it is touched. Each folder it makes is the owner's alone
/// (0700 on Unix), whatever the umask: rustic sets the modes of the entry's
/// contents but not of the folder they land in, and what a snapshot holds
/// may be private, so another user of the machine never reads it there.
fn new_folder(path: &Path) -> Result<(), JobError> {
    if let Some(parent) = path.parent() {
        private_folder()
            .recursive(true)
            .create(parent)
            .map_err(|error| folder_failure(path, &error))?;
    }
    private_folder()
        .create(path)
        .map_err(|error| folder_failure(path, &error))
}

fn private_folder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
}

fn folder_failure(path: &Path, error: &io::Error) -> JobError {
    let detail = format!("the folder {} couldn't be made: {error}", path.display());
    if unwritable(error) {
        JobError::Failed {
            code: ErrorCode::RestoreTargetUnwritable,
            detail,
        }
    } else {
        JobError::failed(detail)
    }
}

/// The failure of rustic's restore: `restore-target-unwritable` when the
/// file system refused a write, by the I/O error in its chain; rustic's own
/// words otherwise.
fn restore_failure(error: &RusticError) -> JobError {
    let refused =
        std::iter::successors(Some(error as &(dyn std::error::Error + 'static)), |error| {
            error.source()
        })
        .filter_map(|error| error.downcast_ref::<io::Error>())
        .any(unwritable);
    let detail = error.display_log();
    if refused {
        JobError::Failed {
            code: ErrorCode::RestoreTargetUnwritable,
            detail,
        }
    } else {
        JobError::failed(detail)
    }
}

fn unwritable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}

/// What a restore wrote, from its plan's counts: a file to restore and a file
/// to modify are both written.
fn restore_result(stats: &RestoreStats, bytes: u64) -> RestoreResult {
    RestoreResult {
        files_restored: stats.files.restore.saturating_add(stats.files.modify),
        files_unchanged: stats.files.unchanged.saturating_add(stats.files.verified),
        folders_restored: stats.dirs.restore,
        bytes_restored: bytes,
    }
}

/// What a dry run found. rustic counts what the destination holds and the
/// snapshot doesn't as additional, even with `delete` off, and doesn't
/// descend into such a folder: it counts once.
fn plan_result(stats: &RestoreStats, bytes: u64) -> RestorePlanResult {
    RestorePlanResult {
        files_added: stats.files.restore,
        files_overwritten: stats.files.modify,
        files_unchanged: stats.files.unchanged.saturating_add(stats.files.verified),
        files_kept: stats.files.additional.saturating_add(stats.dirs.additional),
        bytes_to_write: bytes,
    }
}
