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
    todo!()
}

pub(crate) fn backup(ctx: &Context, job: &BackupJobSpec) -> Result<Done, JobError> {
    todo!()
}

pub(crate) fn prune(ctx: &Context, job: &PruneJobSpec) -> Result<Done, JobError> {
    todo!()
}

pub(crate) fn check(ctx: &Context, job: &CheckJobSpec) -> Result<Done, JobError> {
    todo!()
}
