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
        let redactor = redactor_of(repository);
        let policy = policy.clone();
        let wanted: std::collections::HashSet<String> =
            group.iter().map(|s| s.id.as_str().to_owned()).collect();
        blocking(&redactor, move |redactor| {
            evaluate(&access, redactor, &policy, &wanted)
        })
        .await
    }
}

/// Runs `work` on a blocking thread. What the thread panics with is masked
/// like any other text of the open: the panic message may quote what the
/// open was reading (I4).
async fn blocking<T: Send + 'static>(
    redactor: &domain::secrets::Redactor,
    work: impl FnOnce(&domain::secrets::Redactor) -> Result<T, RetentionError> + Send + 'static,
) -> Result<T, RetentionError> {
    let owned = redactor.clone();
    tokio::task::spawn_blocking(move || work(&owned))
        .await
        .map_err(|error| engine(redactor, error))?
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
    redactor: &domain::secrets::Redactor,
    policy: &RetentionPolicy,
    wanted: &std::collections::HashSet<String>,
) -> Result<Vec<SnapshotId>, RetentionError> {
    let backends = BackendOptions::default()
        .repository(spec.repository.clone())
        .options(spec.options.clone())
        .options_hot(spec.options_hot.clone())
        .options_cold(spec.options_cold.clone());
    let backends = match &spec.repo_hot {
        Some(hot) => backends.repo_hot(hot.clone()),
        None => backends,
    };
    // An `rclone:` backend starts `rclone` children, and reads the server's
    // output until it is up: built through the spawn gate, held until then.
    // Other backends start nothing, and a slow one must not hold up every
    // start (decision 58). This runs on a blocking thread already.
    let starts_rclone = std::iter::once(&spec.repository)
        .chain(&spec.repo_hot)
        .any(|location| is_rclone(location));
    let backends = if starts_rclone {
        spawn_gate::hold(|| backends.to_backends())
    } else {
        backends.to_backends()
    }
    .map_err(|error| engine(redactor, error))?;
    let repository = Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .map_err(|error| engine(redactor, error))?
        .open(&Credentials::password(spec.password.clone()))
        .map_err(|error| engine(redactor, error))?;
    // The repository's own snapshot files, not the listing's: a calendar
    // rule counts each snapshot on the calendar of the offset its file
    // records, which the listing's `Timestamp` has lost (§16, decision 56).
    let snapshots: Vec<SnapshotFile> = repository
        .get_all_snapshots()
        .map_err(|error| engine(redactor, error))?
        .iter()
        .filter(|snapshot| wanted.contains(&*snapshot.id.to_hex().to_string()))
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

/// Whether rustic reads `location` as an `rclone:` repository.
fn is_rclone(location: &str) -> bool {
    rustic_backend::util::location_to_type_and_path(location)
        .is_ok_and(|(kind, _)| kind == rustic_backend::choose::SupportedBackend::Rclone)
}

fn engine(redactor: &domain::secrets::Redactor, error: impl std::fmt::Display) -> RetentionError {
    RetentionError::Engine {
        detail: redactor.redact(&error.to_string()),
    }
}

/// Everything the open of this repository could let slip into an error
/// (I4), so the adapter's errors are redacted like the worker's own: the
/// repository's secrets, what its references resolved to, and the proxy
/// credentials of this process's environment, which the in-process opener
/// reads.
fn redactor_of(access: &RepositoryAccess) -> domain::secrets::Redactor {
    let environment: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let resolved: Vec<domain::secrets::SecretValue> = access
        .secrets
        .iter()
        .cloned()
        .chain(crate::environment::proxy_credentials(&environment))
        .collect();
    domain::secrets::Redactor::for_repository(domain::secrets::RepositorySecrets {
        resolved: &resolved,
        ..access.known_secrets()
    })
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

/// Normalizes a plan's sources the way rustic records them: dots removed,
/// symlinks resolved, sorted and deduplicated, so adoption's matching
/// compares a plan with its own snapshots' paths.
#[derive(Default)]
pub struct RusticSources;

#[async_trait::async_trait]
impl app::ports::NormalizeSources for RusticSources {
    async fn normalized(
        &self,
        sources: &[domain::planning::Source],
    ) -> Result<Vec<domain::planning::Source>, app::ports::SourcesError> {
        let texts: Vec<String> = sources.iter().map(|s| s.as_str().to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let list = rustic_core::PathList::from_iter(texts)
                .sanitize()
                .map_err(|error| app::ports::SourcesError::Unreadable {
                    detail: error.to_string(),
                })?;
            (0..list.len())
                .map(|index| &list[index])
                .map(|path| {
                    domain::planning::Source::parse(path.to_str().ok_or_else(|| {
                        app::ports::SourcesError::Unreadable {
                            detail: "a source is not UTF-8".to_owned(),
                        }
                    })?)
                    .map_err(|error| app::ports::SourcesError::Unreadable {
                        detail: error.to_string(),
                    })
                })
                .collect()
        })
        .await
        .map_err(|error| app::ports::SourcesError::Unreadable {
            detail: error.to_string(),
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::secrets::{Redactor, SecretValue};

    #[tokio::test]
    async fn what_a_blocking_task_panics_with_is_masked() {
        let canary = "CANARY-quoted-by-a-panic";
        let redactor = Redactor::new(&[SecretValue::new(canary)]);
        let error = blocking::<()>(&redactor, move |_| {
            panic!("the open read {canary} and gave up");
        })
        .await
        .expect_err("a panicking task is an error");
        let RetentionError::Engine { detail } = &error else {
            panic!("an engine failure, not {error:?}");
        };
        assert!(!detail.contains(canary), "the detail leaks: {detail}");
        assert!(detail.contains("gave up"), "the rest stays: {detail}");
    }
}
