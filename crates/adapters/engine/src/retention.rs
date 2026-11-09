//! The `Retention` port's real adapter: rustic's own retention rules, run
//! in-process with `ForgetGroups` (technical design §3.2). The daemon — not
//! a worker — evaluates which of a managed group's snapshots the plan's
//! policy lets go, because the decision is the owner's (I6) and the worker
//! that deletes them must be handed a finished list of IDs.

use async_trait::async_trait;
use domain::SnapshotId;
use domain::maintenance::Snapshot;
use domain::planning::{KeepCount, KeepWithin, RetentionPolicy};
use jiff::Zoned;
use rustic_backend::BackendOptions;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{Credentials, Repository, RepositoryOptions};
use rustic_core::{Group, Grouped, SnapshotGroup, SnapshotGroupCriterion};

use app::ports::{RepositoryAccess, Retention, RetentionError};

/// Evaluates retention exactly as rustic's `forget` does, on the repository
/// the use case already resolved.
#[derive(Default)]
pub struct RusticRetention;

#[async_trait]
impl Retention for RusticRetention {
    async fn removes(
        &self,
        repository: &RepositoryAccess,
        policy: &RetentionPolicy,
        group: &[Snapshot],
    ) -> Result<Vec<SnapshotId>, RetentionError> {
        let access = RepositorySpec::of(repository);
        let policy = policy.clone();
        let wanted: Vec<String> = group.iter().map(|s| s.id.as_str().to_owned()).collect();
        tokio::task::spawn_blocking(move || evaluate(&access, &policy, &wanted))
            .await
            .map_err(|error| RetentionError::Engine {
                detail: error.to_string(),
            })?
    }
}

/// The repository as the in-process opener reads it. Spelled out here, like
/// the worker protocol's `RepositorySpec`, so only this crate sees rustic's
/// own strings.
struct RepositorySpec {
    repository: String,
    repo_hot: Option<String>,
    password: String,
    options: std::collections::BTreeMap<String, String>,
    options_hot: std::collections::BTreeMap<String, String>,
    options_cold: std::collections::BTreeMap<String, String>,
}

impl RepositorySpec {
    fn of(access: &RepositoryAccess) -> Self {
        Self {
            repository: access.repository.clone(),
            repo_hot: access.repo_hot.clone(),
            password: access.password.expose().to_owned(),
            options: access.options.clone(),
            options_hot: access.options_hot.clone(),
            options_cold: access.options_cold.clone(),
        }
    }
}

/// Opens the repository, picks the group's snapshots out of it, applies the
/// policy to them as one group — as an exported profile's `group-by = ""`
/// would — and names the ones the policy lets go.
fn evaluate(
    spec: &RepositorySpec,
    policy: &RetentionPolicy,
    wanted: &[String],
) -> Result<Vec<SnapshotId>, RetentionError> {
    let backends = BackendOptions::default()
        .repository(spec.repository.clone())
        .options(spec.options.clone())
        .options_hot(spec.options_hot.clone())
        .options_cold(spec.options_cold.clone());
    let backends = match &spec.repo_hot {
        Some(hot) => backends.repo_hot(hot.clone()),
        None => backends,
    }
    .to_backends()
    .map_err(engine)?;
    let repository = Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .map_err(engine)?
        .open(&Credentials::password(spec.password.clone()))
        .map_err(engine)?;
    let snapshots: Vec<SnapshotFile> = repository
        .get_all_snapshots()
        .map_err(engine)?
        .iter()
        .filter(|snapshot| {
            let hex = snapshot.id.to_hex();
            wanted.contains(&hex.to_string())
        })
        .cloned()
        .collect();
    // One group: the policy belongs to the plan, and every snapshot here is
    // the plan's own.
    let grouped = Grouped {
        criterion: SnapshotGroupCriterion::default(),
        groups: vec![Group {
            group_key: SnapshotGroup::default(),
            items: snapshots,
        }],
    };
    let forget = rustic_core::ForgetGroups::from_grouped_snapshots_with_retention(
        grouped,
        &keep_options(policy),
        &Zoned::now(),
    )
    .map_err(|_| RetentionError::InvalidPolicy)?;
    Ok(forget
        .into_forget_ids()
        .into_iter()
        .filter_map(|id| SnapshotId::parse(id.to_hex().as_str()).ok())
        .collect())
}

fn engine(error: impl std::fmt::Display) -> RetentionError {
    RetentionError::Engine {
        detail: error.to_string(),
    }
}

/// The policy as rustic's `KeepOptions` read it. Every field is named, so a
/// field added later is a compile error here until it is mapped.
fn keep_options(policy: &RetentionPolicy) -> rustic_core::KeepOptions {
    let RetentionPolicy {
        keep_last,
        keep_minutely,
        keep_hourly,
        keep_daily,
        keep_weekly,
        keep_monthly,
        keep_quarter_yearly,
        keep_half_yearly,
        keep_yearly,
        keep_within,
        keep_within_minutely,
        keep_within_hourly,
        keep_within_daily,
        keep_within_weekly,
        keep_within_monthly,
        keep_within_quarter_yearly,
        keep_within_half_yearly,
        keep_within_yearly,
    } = policy;
    let count = |keep: &Option<KeepCount>| keep.map(|count| count.get());
    let span = |keep: &Option<KeepWithin>| keep.map(|keep| keep.span());
    use rustic_core::KeepOptions;
    KeepOptions::default()
        .keep_last(count(keep_last))
        .keep_minutely(count(keep_minutely))
        .keep_hourly(count(keep_hourly))
        .keep_daily(count(keep_daily))
        .keep_weekly(count(keep_weekly))
        .keep_monthly(count(keep_monthly))
        .keep_quarter_yearly(count(keep_quarter_yearly))
        .keep_half_yearly(count(keep_half_yearly))
        .keep_yearly(count(keep_yearly))
        .keep_within(span(keep_within))
        .keep_within_minutely(span(keep_within_minutely))
        .keep_within_hourly(span(keep_within_hourly))
        .keep_within_daily(span(keep_within_daily))
        .keep_within_weekly(span(keep_within_weekly))
        .keep_within_monthly(span(keep_within_monthly))
        .keep_within_quarter_yearly(span(keep_within_quarter_yearly))
        .keep_within_half_yearly(span(keep_within_half_yearly))
        .keep_within_yearly(span(keep_within_yearly))
}
