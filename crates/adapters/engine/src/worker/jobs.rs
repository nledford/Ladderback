//! The real jobs. Each one opens the repository, tells the daemon it has
//! `Started` right before the step the deadline counts from, and returns what
//! `rustic_core` reported, converted by `convert`.

use std::path::PathBuf;

use domain::execution::plan_tags::PlanTags;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{BackupOptions, CheckOptions, ParentOptions, PathList, SnapshotOptions};

use super::convert::{
    JobError, backup_result, check_result, prune_options, prune_result, snapshot_ref, snapshot_tags,
};
use super::repository::{open, repository_id};
use super::{Context, Done};
use crate::job::{BackupJobSpec, CheckJobSpec, OpResult, PruneJobSpec, RepositorySpec};

pub(crate) fn identify(ctx: &Context, spec: &RepositorySpec) -> Result<Done, JobError> {
    let repository = open(ctx, spec)?;
    ctx.start(None)?;
    Ok(Done::Result(OpResult::Repository {
        id: repository_id(&repository)?,
    }))
}

pub(crate) fn backup(ctx: &Context, job: &BackupJobSpec) -> Result<Done, JobError> {
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
    let mut snapshot_options = SnapshotOptions::default().tags(vec![tags]);
    if let Some(label) = &job.label {
        snapshot_options = snapshot_options.label(label.clone());
    }
    let snapshot = SnapshotFile::from_options(&snapshot_options)?;
    let paths = PathList::from_iter(job.sources.iter().map(PathBuf::from))
        .sanitize()
        .map_err(|error| JobError::failed(error.to_string()))?;

    // The deadline counts from here: the index read is what comes next.
    ctx.start(Some(job.deadline_budget))?;
    let repository = repository.to_indexed_ids()?;
    let snapshot = repository.backup(
        &BackupOptions::default().parent_opts(parent_options),
        &paths,
        snapshot,
    )?;
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
    ctx.start(None)?;
    let plan = repository.prune_plan(&options)?;
    let result = prune_result(id, &plan.stats);
    repository.prune(&options, plan)?;
    Ok(Done::Result(OpResult::Prune(result)))
}

pub(crate) fn check(ctx: &Context, job: &CheckJobSpec) -> Result<Done, JobError> {
    let repository = open(ctx, &job.repo)?;
    let id = repository_id(&repository)?;
    ctx.start(None)?;
    // A check that ran to its end succeeded, whatever it found: the findings
    // are the result.
    let results = repository.check(CheckOptions::default().read_data(job.read_data))?;
    Ok(Done::Result(OpResult::Check(check_result(id, &results))))
}
