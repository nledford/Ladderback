//! The real jobs. Each one opens the repository, tells the daemon it has
//! `Started` right before the step the deadline counts from, and returns what
//! `rustic_core` reported, converted by `convert`.

use domain::execution::plan_tags::PlanTags;
use std::path::PathBuf;

use rustic_core::repofile::{SnapshotFile, SnapshotId as RusticSnapshotId};
use rustic_core::{
    CheckOptions, ConfigOptions, Credentials, KeyOptions, LocalSource, ParentOptions, PathList,
    SnapshotOptions, StringList,
};

use super::cancel::CancellableSource;
use super::convert::{
    JobError, backup_options, backup_result, check_result, errors_first, prune_options,
    prune_plan_result, prune_result, snapshot_host, snapshot_id, snapshot_ref, snapshot_tags,
};
use super::listing;
use super::repository::{self as location, new_repository, open, repository_id};
use super::{Context, Done};
use crate::job::{
    BackupJobSpec, CheckJobSpec, ForgetJobSpec, ForgetResult, OpResult, ProbeResult, PruneJobSpec,
    RepositorySpec, WorkerEvent,
};
use crate::job::{RewriteJobSpec, RewriteResult};

pub(crate) fn identify(ctx: &Context, spec: &RepositorySpec) -> Result<Done, JobError> {
    let repository = open(ctx, spec)?;
    let id = repository_id(&repository)?;
    ctx.start(None, Some(&id))?;
    Ok(Done::Result(OpResult::Repository { id }))
}

/// Tries one location of a repository reached at several and says what is
/// there (decision 80). It tells the daemon it has started once it has an
/// answer, with the ID when it found one.
pub(crate) fn probe(ctx: &Context, spec: &RepositorySpec) -> Result<Done, JobError> {
    let found = location::probe(ctx, spec)?;
    let id = match &found {
        ProbeResult::Holds { id } => Some(id),
        ProbeResult::Refused | ProbeResult::NotARepository | ProbeResult::PasswordNotAccepted => {
            None
        }
    };
    ctx.start(None, id)?;
    Ok(Done::Result(OpResult::Probed(found)))
}

/// Lists the repository's snapshots, newest first. A read: it emits `Started`
/// after the open, as `identify` does, then the listing as a series of
/// `snapshots` events, then one final result that counts them (§4.1).
pub(crate) fn ls(ctx: &Context, spec: &RepositorySpec) -> Result<Done, JobError> {
    let repository = open(ctx, spec)?;
    ctx.start(None, Some(&repository_id(&repository)?))?;
    let snapshots = repository.get_all_snapshots()?;
    let count = listing::send(&snapshots, |snapshots| {
        ctx.events.emit(&WorkerEvent::Snapshots { snapshots })
    })?;
    Ok(Done::Result(OpResult::Snapshots { count }))
}

/// Removes exactly the snapshots the job names. Which snapshots those are
/// was decided before the job ran, by the owner's own selection (I6); the
/// job deletes them and nothing else.
pub(crate) fn forget(ctx: &Context, job: &ForgetJobSpec) -> Result<Done, JobError> {
    // Parsed before the repository is opened, so a malformed ID is refused
    // before anything is touched.
    let ids: Vec<RusticSnapshotId> = job
        .ids
        .iter()
        .map(|id| {
            id.as_str()
                .parse()
                .map_err(|_| JobError::failed(format!("not a snapshot ID: {}", id.as_str())))
        })
        .collect::<Result<_, _>>()?;
    let repository = open(ctx, &job.repo)?;
    let repository_id = repository_id(&repository)?;
    ctx.start(None, Some(&repository_id))?;
    repository.delete_snapshots(&ids)?;
    let removed = ids.iter().map(snapshot_id).collect::<Result<_, _>>()?;
    Ok(Done::Result(OpResult::Forget(ForgetResult {
        repository: repository_id,
        removed,
    })))
}

/// Rewrites exactly the snapshots the job names with the tags given (§7.3,
/// step 3): each becomes a new snapshot file over the same tree, the
/// originals are removed, and the user's own tags survive. Which snapshots
/// those are was decided before the job ran, by the owner's own selection.
///
/// Saving the copies and removing the originals are two writes, so a worker
/// stopped between them leaves both. A copy is known by the original's tree
/// and time and the job's tags; one already there is kept rather than made
/// again, so the next adoption of that history finishes the first.
pub(crate) fn rewrite(ctx: &Context, job: &RewriteJobSpec) -> Result<Done, JobError> {
    // Parsed before the repository is opened, so a malformed ID is refused
    // before anything is touched.
    let ids: Vec<RusticSnapshotId> = job
        .ids
        .iter()
        .map(|id| {
            id.as_str()
                .parse()
                .map_err(|_| JobError::failed(format!("not a snapshot ID: {id}")))
        })
        .collect::<Result<_, _>>()?;
    let new_tags: StringList = job.tags.join(",").parse().map_err(|_| {
        JobError::failed("the adoption's tags are not a list the repository can store")
    })?;
    let repository = open(ctx, &job.repo)?;
    let repository_id = repository_id(&repository)?;
    ctx.start(None, Some(&repository_id))?;
    let originals = repository.get_snapshots(&job.ids)?;
    if originals.len() != ids.len() {
        return Err(JobError::failed(
            "a snapshot the selection named is not in the repository",
        ));
    }
    let before = repository.get_all_snapshots()?;
    let mut copies = Vec::new();
    for original in &originals {
        if copy_of(original, &before, &ids, &new_tags, &[]).is_some() {
            continue;
        }
        // The user's own tags survive; every `ladderback-*` tag is replaced
        // by the two the job carries.
        let mut copy = original.clone();
        let mut tags = new_tags.clone();
        for tag in original.tags.iter() {
            if !tag.starts_with("ladderback-") {
                tags.add(tag.clone());
            }
        }
        copy.tags = tags;
        copies.push(copy);
    }
    repository.save_snapshots(copies)?;
    repository.delete_snapshots(&ids)?;
    // What the rewrite leaves for each original, in the order the job named
    // them: never a snapshot that was there for another reason.
    let after = repository.get_all_snapshots()?;
    let mut created: Vec<RusticSnapshotId> = Vec::with_capacity(originals.len());
    for original in &originals {
        let copy = copy_of(original, &after, &ids, &new_tags, &created).ok_or_else(|| {
            JobError::failed("a rewritten snapshot is not in the repository after the save")
        })?;
        created.push(copy);
    }
    let created = created.iter().map(snapshot_id).collect::<Result<_, _>>()?;
    Ok(Done::Result(OpResult::Rewrite(RewriteResult {
        repository: repository_id,
        created,
    })))
}

/// The copy of `original` among `snapshots`: same tree, same time, both of
/// the job's tags, and neither an original nor a copy already counted.
fn copy_of(
    original: &SnapshotFile,
    snapshots: &[SnapshotFile],
    originals: &[RusticSnapshotId],
    tags: &StringList,
    counted: &[RusticSnapshotId],
) -> Option<RusticSnapshotId> {
    snapshots
        .iter()
        .find(|snapshot| {
            snapshot.tree == original.tree
                && snapshot.time == original.time
                && snapshot.tags.contains_all(tags)
                && !originals.contains(&snapshot.id)
                && !counted.contains(&snapshot.id)
        })
        .map(|snapshot| snapshot.id)
}

/// Initialises a repository at the location, with default key options and
/// both halves of a hot/cold repository. It refuses when a repository
/// already exists there (`repository-exists`), and never overwrites one.
pub(crate) fn init(ctx: &Context, spec: &RepositorySpec) -> Result<Done, JobError> {
    let repository = new_repository(ctx, spec)?;
    // The deadline counts from here: the backends are open, so this is the
    // step that can now take its time, as `identify` does. It names no
    // repository opened: its result names the one it makes.
    ctx.start(None, None)?;
    let repository = repository
        .init(
            &Credentials::password(spec.password.expose()),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .map_err(|error| init_failure(spec, *error))?;
    // A hot/cold repository keeps its config in both halves: `init` wrote
    // the cold half's, `init_hot` writes the hot half's.
    if spec.repo_hot.is_some() {
        repository.init_hot()?;
    }
    Ok(Done::Result(OpResult::Repository {
        id: repository_id(&repository)?,
    }))
}

/// The refusals `init` gives that the agent reports as codes of their own: a
/// repository already exists at the location (rustic says so when either
/// half holds a config file), and nothing was written; or a REST server
/// refused the credentials, told by the status check opening makes (#484).
/// Every other failure keeps rustic's words, as `operation-failed`.
fn init_failure(spec: &RepositorySpec, error: rustic_core::RusticError) -> JobError {
    let detail = error.display_log();
    if detail.contains("Config file already exists") {
        JobError::Failed {
            code: api::ErrorCode::RepositoryExists,
            detail,
        }
    } else if let Some(refused) = location::credentials_refused(spec, &error) {
        refused
    } else {
        JobError::failed(detail)
    }
}
pub(crate) fn backup(ctx: &Context, job: &BackupJobSpec) -> Result<Done, JobError> {
    // Checked before the repository is opened, so a host the daemon would
    // refuse touches nothing.
    let host = snapshot_host(job.host.as_deref())?;
    let repository = open(ctx, &job.repo)?;
    let id = repository_id(&repository)?;

    // The parent is chosen from the plan's own snapshots (§7.3), not by
    // rustic's host, label and paths match, so moving a source folder doesn't
    // start a full backup. `None` leaves the choice to rustic.
    let known = repository
        .get_all_snapshots()?
        .iter()
        .map(snapshot_ref)
        .collect::<Result<Vec<_>, _>>()?;
    let plan_tags = PlanTags::new(job.plan.clone(), job.agent);
    let mut parent_options = ParentOptions::default();
    if let Some(parent) = plan_tags.parent(&known) {
        parent_options = parent_options.parents(vec![parent.as_str().to_owned()]);
    }

    let tags = snapshot_tags(&plan_tags, &job.extra_tags)?;
    let mut snapshot_options = SnapshotOptions::default().tags(vec![tags]).host(host);
    if let Some(label) = &job.label {
        snapshot_options = snapshot_options.label(label.clone());
    }
    let snapshot = SnapshotFile::from_options(&snapshot_options)?;
    let paths = PathList::from_iter(job.sources.iter().map(PathBuf::from))
        .sanitize()
        .map_err(|error| JobError::failed(error.to_string()))?
        .paths();
    let options = backup_options(&job.filter).parent_opts(parent_options);
    // What `Repository::backup` does for folders, with a source a cancel
    // cuts short: the backup then winds down through rustic's own end,
    // which saves the index of the packs written, and the backends refuse
    // its snapshot (§4.1).
    let source = CancellableSource::new(
        LocalSource::new(
            options.ignore_save_opts,
            &options.excludes,
            &options.ignore_filter_opts,
            &paths,
        )?,
        ctx.cancel().clone(),
    );

    // The deadline counts from here: the index read is what comes next.
    ctx.start(Some(job.deadline_budget), Some(&id))?;
    let repository = repository.to_indexed_ids()?;
    let Some(snapshot) = ctx
        .cancel()
        .saving(|| repository.archive(&options, &source, snapshot, &paths))
    else {
        return Ok(Done::Cancelled);
    };
    let snapshot = snapshot?;
    // The snapshot is saved: nothing is left that the deadline guards. The
    // result goes out before the repository, and its index, is dropped,
    // which a deadline passing meanwhile must not turn into a failure.
    ctx.disarm();
    ctx.report(OpResult::Backup(backup_result(id, &snapshot)?))?;
    Ok(Done::Reported)
}

pub(crate) fn prune(ctx: &Context, job: &PruneJobSpec) -> Result<Done, JobError> {
    let options = prune_options(job.keep_delete)?;
    let repository = open(ctx, &job.repo)?;
    let id = repository_id(&repository)?;
    ctx.start(None, Some(&id))?;
    let plan = repository.prune_plan(&options)?;
    let result = prune_result(id, &plan.stats);
    repository.prune(&options, plan)?;
    Ok(Done::Result(OpResult::Prune(result)))
}

/// What [`prune`] would do, worked out and not done: the plan's figures, and
/// nothing deleted or marked.
pub(crate) fn prune_plan(ctx: &Context, job: &PruneJobSpec) -> Result<Done, JobError> {
    let options = prune_options(job.keep_delete)?;
    let repository = open(ctx, &job.repo)?;
    let id = repository_id(&repository)?;
    ctx.start(None, Some(&id))?;
    let plan = repository.prune_plan(&options)?;
    Ok(Done::Result(OpResult::PrunePlan(prune_plan_result(
        id,
        &plan.stats,
    ))))
}

/// Checks the repository. A check that ran to its end succeeded, whatever it
/// found: each finding goes to the daemon as a warning of its level in
/// rustic's own words (`emit` masks the job's secrets in them and cuts a long
/// one), the errors first, and the result counts them by level.
pub(crate) fn check(ctx: &Context, job: &CheckJobSpec) -> Result<Done, JobError> {
    let repository = open(ctx, &job.repo)?;
    let id = repository_id(&repository)?;
    ctx.start(None, Some(&id))?;
    let results = repository.check(CheckOptions::default().read_data(job.read_data))?;
    for (code, finding) in errors_first(&results.0) {
        ctx.events.emit(&WorkerEvent::Warning {
            item: None,
            during: "check".to_owned(),
            code,
            detail: finding.to_string(),
        })?;
    }
    Ok(Done::Result(OpResult::Check(check_result(id, &results))))
}
