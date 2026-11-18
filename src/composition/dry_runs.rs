//! The dry-run routes' side of the wiring (§16, decision 89, point 13): a
//! draft plan's table read by the config adapter as the file is read when
//! it loads, then dry-run by the use case of `app`, and the dry-run
//! allowlist read and set; each refusal said as the routes answer it,
//! never with a path. A walk that failed is said in the log only: the
//! engine's words can name a path, which a client may not learn. The
//! warnings it raises while it walks are counted, not logged, since
//! `GET /api/v1/log` serves the log at the `read` scope: one line gives
//! their count and the draft's plan ID.

use api::{DryRunStopDto, ErrorCode, ExcludeTallyDto, PlanDryRunDto};
use app::dry_run::{DryRunAllowlists, DryRunError, PlanDryRun, PlanDryRuns};
use app::ports::{ConfigError, WalkStop};
use async_trait::async_trait;
use domain::planning::{DryRunAllowlist, DryRunFolder, MAX_DRY_RUN_FOLDERS, Proposer};
use http_adapter::{DryRunsError, PlansError};

use super::config_unusable;

/// The daemon's dry runs, and the allowlist they read within.
pub(super) struct DryRunService {
    dry_runs: PlanDryRuns,
    allowlists: DryRunAllowlists,
    /// The daemon's home folder, which "Add home folder" adds, when it has
    /// one written in UTF-8.
    home: Option<String>,
}

impl DryRunService {
    pub(super) fn new(
        dry_runs: PlanDryRuns,
        allowlists: DryRunAllowlists,
        home: Option<String>,
    ) -> Self {
        Self {
            dry_runs,
            allowlists,
            home,
        }
    }

    fn shown(&self, allowlist: &DryRunAllowlist) -> api::DryRunAllowlistDto {
        api::DryRunAllowlistDto {
            folders: allowlist
                .folders()
                .iter()
                .map(|folder| folder.as_str().to_owned())
                .collect(),
            home: self.home.clone(),
        }
    }
}

#[async_trait]
impl http_adapter::DryRuns for DryRunService {
    async fn dry_run(
        &self,
        asker: Proposer,
        plan: api::PlanDto,
    ) -> Result<PlanDryRunDto, DryRunsError> {
        let plan = super::plans::read(&plan).map_err(|error| match error {
            PlansError::InvalidBody => DryRunsError::InvalidBody,
            error => DryRunsError::Plan(error),
        })?;
        let counted = self.dry_runs.run(&asker, &plan).await.map_err(refused)?;
        if counted.warnings > 0 {
            log::warn!(
                "A plan dry run of the draft plan {} met {} warnings of the engine while it walked; they aren't logged, as they can name a path.",
                plan.id.as_str(),
                counted.warnings
            );
        }
        Ok(shown(counted))
    }

    async fn allowlist(&self) -> Result<api::DryRunAllowlistDto, DryRunsError> {
        let allowlist = self.allowlists.allowlist().await.map_err(unusable)?;
        Ok(self.shown(&allowlist))
    }

    async fn set_allowlist(
        &self,
        folders: Vec<String>,
    ) -> Result<api::DryRunAllowlistDto, DryRunsError> {
        // The count before the folders are read, so a list of too many is
        // refused as one, whatever a folder beyond the limit holds.
        if folders.len() > MAX_DRY_RUN_FOLDERS {
            return Err(DryRunsError::TooLong {
                limit: MAX_DRY_RUN_FOLDERS,
            });
        }
        let folders = folders
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                DryRunFolder::parse(folder).map_err(|_| DryRunsError::FolderInvalid {
                    entry: index.saturating_add(1),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let allowlist = DryRunAllowlist::new(folders).map_err(|_| DryRunsError::TooLong {
            limit: MAX_DRY_RUN_FOLDERS,
        })?;
        self.allowlists
            .set(allowlist.clone())
            .await
            .map_err(unusable)?;
        Ok(self.shown(&allowlist))
    }
}

/// What a dry run counted, as the route answers it: each exclude's entry
/// from 1.
fn shown(counted: PlanDryRun) -> PlanDryRunDto {
    PlanDryRunDto {
        files: counted.files,
        bytes: counted.bytes,
        unreadable: counted.unreadable,
        excludes: counted
            .excludes
            .into_iter()
            .map(|tally| ExcludeTallyDto {
                setting: tally.exclude.setting.to_owned(),
                entry: tally
                    .exclude
                    .index
                    .map(|index| u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1)),
                files: tally.files,
                bytes: tally.bytes,
            })
            .collect(),
        stopped: counted.stopped.map(|stop| match stop {
            WalkStop::Entries => DryRunStopDto::Entries,
            WalkStop::Time => DryRunStopDto::Time,
            WalkStop::GaveWay => DryRunStopDto::GaveWay,
        }),
    }
}

/// Why a dry run didn't run, as the route answers it.
fn refused(error: DryRunError) -> DryRunsError {
    match error {
        DryRunError::Running => DryRunsError::Busy,
        DryRunError::BackupGoing => DryRunsError::GivesWay,
        DryRunError::TooSoon { until } => DryRunsError::TooSoon {
            seconds: wait_seconds(until),
        },
        DryRunError::NotAllowed { setting, index } => DryRunsError::NotAllowed {
            setting,
            entry: index.saturating_add(1),
        },
        DryRunError::ReadsAbove { setting } => DryRunsError::ReadsAbove { setting },
        DryRunError::Filter => DryRunsError::Filter,
        DryRunError::KeyFilesNotLeftOut => DryRunsError::KeyFilesNotLeftOut,
        DryRunError::Config(error) => unusable(*error),
        DryRunError::Walk { .. } => {
            // The engine's words can name a path the walk met, and the log
            // is served at the `read` scope: they are left out. A walk
            // fails only when its thread panics, and the panic is written
            // to stderr as it happens, outside the log the route serves.
            log::warn!("A plan dry run's walk failed.");
            DryRunsError::Failed
        }
    }
}

/// A proposer's wait until `until`, in whole seconds rounded up, at least
/// one, as a proposal's is told.
fn wait_seconds(until: domain::Timestamp) -> u64 {
    let left = until
        .as_millisecond()
        .saturating_sub(domain::Timestamp::now().as_millisecond());
    u64::try_from(left.saturating_add(999) / 1000)
        .unwrap_or(0)
        .max(1)
}

/// The configuration couldn't be read or written, as the routes say it.
fn unusable(error: ConfigError) -> DryRunsError {
    config_unusable(error, |code: ErrorCode, args, detail| {
        DryRunsError::Unavailable { code, args, detail }
    })
}
