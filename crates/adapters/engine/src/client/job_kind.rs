//! A job as the worker takes it: each of `app`'s jobs mapped to its spec.

use api::WarningCode;
use app::ports::{
    EngineError, FakeEnd, FakeJob, ForgetJob, Job, MAX_JOB_IDS, RepositoryAccess, RestoreJob,
    RestoreTarget, RewriteJob,
};
use domain::execution::WarningKind;
use domain::planning::{GlobFile, SourceFilter};

use crate::job::{
    BackupJobSpec, BrowseJobSpec, CheckJobSpec, FakeEndSpec, FakeJobSpec, FakeWarning,
    ForgetJobSpec, JobKind, PruneJobSpec, RepositorySpec, RestoreJobSpec, RestoreTargetSpec,
    RewriteJobSpec, SourceFilterSpec,
};

fn repository_spec(access: RepositoryAccess) -> RepositorySpec {
    RepositorySpec {
        repository: access.repository,
        repo_hot: access.repo_hot,
        password: access.password,
        options: access.options,
        options_hot: access.options_hot,
        options_cold: access.options_cold,
        secrets: access.secrets,
        id: access.id,
    }
}

/// A plan's source filter as the worker takes it, field for field.
fn filter_spec(filter: SourceFilter) -> SourceFilterSpec {
    let SourceFilter {
        globs,
        iglobs,
        glob_files,
        iglob_files,
        git_ignore,
        no_require_git,
        custom_ignorefiles,
        exclude_if_present,
        exclude_if_xattr,
        one_file_system,
    } = filter;
    SourceFilterSpec {
        globs,
        iglobs,
        glob_files: glob_files.into_iter().map(GlobFile::into_string).collect(),
        iglob_files: iglob_files.into_iter().map(GlobFile::into_string).collect(),
        git_ignore,
        no_require_git,
        custom_ignorefiles,
        exclude_if_present,
        exclude_if_xattr,
        one_file_system,
    }
}

/// A restore as the worker takes it: the entry by its keys, as a browse
/// names its folder.
fn restore_spec(job: RestoreJob) -> RestoreJobSpec {
    RestoreJobSpec {
        repo: repository_spec(job.repository),
        snapshot: job.snapshot,
        entry: job
            .entry
            .names()
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect(),
        target: match job.target {
            RestoreTarget::NewFolder(path) => RestoreTargetSpec::NewFolder { path },
            RestoreTarget::InPlace => RestoreTargetSpec::InPlace,
        },
        deadline_budget: job.deadline_budget,
    }
}

pub(super) fn job_kind(job: Job) -> Result<JobKind, EngineError> {
    // A spec names the snapshots it works on, each about 70 bytes of a
    // message that is capped: the caller splits a longer list into jobs.
    if let Job::Forget(ForgetJob { ids, .. }) | Job::Rewrite(RewriteJob { ids, .. }) = &job
        && ids.len() > MAX_JOB_IDS
    {
        return Err(EngineError::Unavailable {
            detail: format!(
                "a job names {} snapshots, over the {MAX_JOB_IDS} it takes",
                ids.len()
            ),
        });
    }
    Ok(match job {
        Job::Fake(FakeJob {
            steps,
            step_delay,
            warnings,
            end,
            deadline_budget,
        }) => JobKind::Fake(FakeJobSpec {
            steps,
            step_delay_ms: u64::try_from(step_delay.as_millis()).map_err(|_| {
                EngineError::Unavailable {
                    detail: "step delay out of range".to_owned(),
                }
            })?,
            warnings: warnings
                .into_iter()
                .map(|w| FakeWarning {
                    code: match w.kind {
                        WarningKind::SourceUnreadable => WarningCode::SourceUnreadable,
                        WarningKind::Hook => WarningCode::HookWarning,
                        WarningKind::MaintenancePartial => WarningCode::MaintenancePartial,
                        WarningKind::CheckWarning => WarningCode::CheckWarning,
                        WarningKind::CheckError => WarningCode::CheckError,
                        WarningKind::OtherHosts => WarningCode::MaintenanceOtherHosts,
                        WarningKind::LocationSkipped => WarningCode::LocationSkipped,
                        WarningKind::SecretFileReadable => WarningCode::SecretFileReadable,
                        WarningKind::FullDiskAccessMissing => WarningCode::FullDiskAccessMissing,
                        WarningKind::Other => WarningCode::Unknown,
                    },
                    item: w.item,
                    during: w.during,
                    detail: w.detail,
                })
                .collect(),
            end: match end {
                FakeEnd::Succeed => FakeEndSpec::Succeed,
                FakeEnd::Fail { detail } => FakeEndSpec::Fail { detail },
                FakeEnd::Exit {
                    after_steps,
                    exit_code,
                } => FakeEndSpec::Exit {
                    after_steps,
                    exit_code,
                },
            },
            deadline_budget,
        }),
        Job::Identify(access) => JobKind::Identify(repository_spec(access)),
        Job::Probe(access) => JobKind::Probe(repository_spec(access)),
        Job::Init(access) => JobKind::Init(repository_spec(access)),
        Job::Ls(access) => JobKind::Ls(repository_spec(access)),
        Job::Browse(job) => JobKind::Browse(BrowseJobSpec {
            repo: repository_spec(job.repository),
            snapshot: job.snapshot,
            folder: job
                .folder
                .names()
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
            from: job.from,
        }),
        Job::Stats(access) => JobKind::Stats(repository_spec(access)),
        Job::Restore(job) => JobKind::Restore(restore_spec(job)),
        Job::RestorePlan(job) => JobKind::RestorePlan(restore_spec(job)),
        Job::Dump(_) => todo!(),
        Job::Backup(job) => JobKind::Backup(BackupJobSpec {
            repo: repository_spec(job.repository),
            sources: job.sources,
            plan: job.tags.plan().clone(),
            agent: job.tags.agent(),
            extra_tags: job
                .extra_tags
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect(),
            label: job.label,
            host: job.host.map(|host| host.as_str().to_owned()),
            filter: filter_spec(job.filter),
            deadline_budget: job.deadline_budget,
        }),
        Job::Forget(job) => JobKind::Forget(ForgetJobSpec {
            repo: repository_spec(job.repository),
            ids: job.ids.iter().map(|id| id.as_str().to_owned()).collect(),
        }),
        Job::Rewrite(job) => JobKind::Rewrite(RewriteJobSpec {
            repo: repository_spec(job.repository),
            ids: job.ids.iter().map(|id| id.as_str().to_owned()).collect(),
            tags: job.tags.tags().to_vec(),
        }),
        Job::Prune(job) => JobKind::Prune(PruneJobSpec {
            repo: repository_spec(job.repository),
            keep_delete: job.keep_delete.get(),
        }),
        Job::PrunePlan(job) => JobKind::PrunePlan(PruneJobSpec {
            repo: repository_spec(job.repository),
            keep_delete: job.keep_delete.get(),
        }),
        Job::Check(job) => JobKind::Check(CheckJobSpec {
            repo: repository_spec(job.repository),
            read_data: job.read_data,
        }),
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use app::ports::Engine;
    use domain::{OperationId, Uuid};

    use super::*;
    use crate::client::WorkerEngine;
    use crate::client::test_support::access;
    use crate::job::MAX_MESSAGE_BYTES;

    fn snapshot_ids(count: usize) -> Vec<domain::SnapshotId> {
        (0..count)
            .map(|n| domain::SnapshotId::parse(&format!("{n:064x}")).unwrap())
            .collect()
    }

    #[test]
    fn a_forget_of_the_most_one_job_may_name_fits_one_message_with_room_to_spare() {
        let job = Job::Forget(app::ports::ForgetJob {
            repository: access(),
            ids: snapshot_ids(app::ports::MAX_JOB_IDS),
        });
        let spec = serde_json::to_vec(&job_kind(job).unwrap()).unwrap();
        assert!(
            spec.len() < MAX_MESSAGE_BYTES / 2,
            "{} bytes for {} IDs",
            spec.len(),
            app::ports::MAX_JOB_IDS
        );
    }

    #[tokio::test]
    async fn a_forget_or_rewrite_naming_more_than_a_job_takes_is_refused_before_a_worker_starts() {
        let too_many = snapshot_ids(app::ports::MAX_JOB_IDS + 1);
        let forget = Job::Forget(app::ports::ForgetJob {
            repository: access(),
            ids: too_many.clone(),
        });
        let rewrite = Job::Rewrite(app::ports::RewriteJob {
            repository: access(),
            ids: too_many,
            tags: domain::execution::plan_tags::PlanTags::new(
                domain::PlanId::parse("home").unwrap(),
                domain::AgentId::from_uuid(Uuid::from_u128(1)),
            ),
        });
        // A program that can't start, so only the check before spawning can
        // produce the message.
        let engine = WorkerEngine::new("/nonexistent/ladderback-worker", ["worker"]);
        for job in [forget, rewrite] {
            let error = engine
                .run(OperationId::from_uuid(Uuid::from_u128(1)), job)
                .await
                .err()
                .expect("a job naming too many snapshots is refused");
            assert_matches!(
                &error,
                EngineError::Unavailable { detail } if detail.contains(&app::ports::MAX_JOB_IDS.to_string()),
                "the error should name the limit: {error:?}"
            );
        }
    }
}
