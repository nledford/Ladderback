//! Conversions between rustic's types and the protocol's, and the ways a job
//! can end badly.

use std::io;

use api::{ErrorCode, WarningCode};
use domain::execution::deadline::KeepDelete;
use domain::execution::plan_tags::{PlanTags, UserTag};
use domain::planning::Hostname;
use domain::snapshot::SnapshotRef;
use domain::{RepositoryId, SignedDuration, SnapshotId};
use jiff::Span;
use rustic_core::repofile::{SnapshotFile, SnapshotId as RusticSnapshotId, StringList};
use rustic_core::{
    BackupOptions, CheckResults, Excludes, LocalSourceFilterOptions, PruneOptions, PruneStats,
    RusticError,
};

use crate::job::{BackupResult, CheckResult, PrunePlanResult, PruneResult, SourceFilterSpec};

/// How a job ended badly.
#[derive(Debug)]
pub(crate) enum JobError {
    /// The job failed, and the daemon is told why.
    Failed { code: ErrorCode, detail: String },
    /// The event stream broke, so there is no one to tell.
    Events(io::Error),
}

impl JobError {
    /// A failure the engine reports with its own words.
    pub(crate) fn failed(detail: impl Into<String>) -> Self {
        Self::Failed {
            code: ErrorCode::OperationFailed,
            detail: detail.into(),
        }
    }
}

impl From<Box<RusticError>> for JobError {
    fn from(error: Box<RusticError>) -> Self {
        // The one-line form: the plain `Display` is several lines and a
        // backtrace. The event stream redacts secrets in it.
        Self::failed(error.display_log())
    }
}

impl From<io::Error> for JobError {
    fn from(error: io::Error) -> Self {
        Self::Events(error)
    }
}

/// What Ladderback knows of a snapshot rustic listed.
pub(crate) fn snapshot_ref(snapshot: &SnapshotFile) -> Result<SnapshotRef, JobError> {
    let mut paths: Vec<String> = snapshot.paths.iter().cloned().collect();
    paths.sort();
    Ok(SnapshotRef {
        id: snapshot_id(&snapshot.id)?,
        time: snapshot.time.timestamp(),
        host: snapshot.hostname.clone(),
        label: snapshot.label.clone(),
        paths,
        tags: snapshot.tags.iter().cloned().collect(),
    })
}

/// Ladderback's form of a snapshot ID: the full 64 hex characters.
pub(crate) fn snapshot_id(id: &RusticSnapshotId) -> Result<SnapshotId, JobError> {
    SnapshotId::parse(id.to_hex().as_str())
        .map_err(|_| JobError::failed("rustic reported a snapshot ID that isn't 64 hex characters"))
}

/// A finished backup: what rustic recorded in the snapshot it saved.
pub(crate) fn backup_result(
    repository: RepositoryId,
    snapshot: &SnapshotFile,
) -> Result<BackupResult, JobError> {
    let summary = snapshot
        .summary
        .as_ref()
        .ok_or_else(|| JobError::failed("the snapshot rustic saved has no summary"))?;
    Ok(BackupResult {
        repository,
        snapshot: snapshot_id(&snapshot.id)?,
        parent: snapshot.parent.as_ref().map(snapshot_id).transpose()?,
        files_new: summary.files_new,
        files_changed: summary.files_changed,
        files_unmodified: summary.files_unmodified,
        bytes_processed: summary.total_bytes_processed,
        data_added: summary.data_added,
        data_added_packed: summary.data_added_packed,
        error_count: summary.error_count,
    })
}

/// rustic's prune options for a job. Prune only marks packs and deletes them
/// in a later run, so `instant_delete` and `early_delete_index` stay off
/// whatever the job says: they would let a concurrent backup lose data (I2).
/// A `keep_delete` under rustic's default is refused like the daemon refuses
/// it ([`KeepDelete`]).
pub(crate) fn prune_options(keep_delete: SignedDuration) -> Result<PruneOptions, JobError> {
    let keep_delete = KeepDelete::new(keep_delete).map_err(|error| invalid_spec(&error))?;
    let keep_delete = Span::try_from(keep_delete.get())
        .map_err(|error| JobError::failed(format!("keep_delete is out of range: {error}")))?;
    Ok(PruneOptions::default()
        .keep_delete(keep_delete)
        .instant_delete(false)
        .early_delete_index(false))
}

/// A snapshot's tags: the plan's, then the job's extra tags, one list entry
/// each, since `SnapshotOptions::add_tags` would split them on commas. An
/// extra tag with the reserved prefix is refused like the daemon refuses it
/// ([`UserTag`]).
pub(crate) fn snapshot_tags(plan: &PlanTags, extra: &[String]) -> Result<StringList, JobError> {
    let mut tags = StringList::default();
    for tag in plan.tags() {
        tags.add(tag);
    }
    for tag in extra {
        let tag = UserTag::new(tag.as_str()).map_err(|error| invalid_spec(&error))?;
        tags.add(tag.as_str().to_owned());
    }
    Ok(tags)
}

/// rustic's backup options with the plan's source filter: its globs and glob
/// files as `Excludes`, the rest as `LocalSourceFilterOptions`, each field
/// under its own name. What the filter doesn't set is rustic's default, and
/// `exclude-larger-than`, which a plan never sets (§6.1), stays off. The
/// filter is taken apart whole, so a field added to it fails to compile here
/// until it is mapped.
pub(crate) fn backup_options(filter: &SourceFilterSpec) -> BackupOptions {
    let SourceFilterSpec {
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
    let excludes = Excludes::default()
        .globs(globs.clone())
        .iglobs(iglobs.clone())
        .glob_files(glob_files.clone())
        .iglob_files(iglob_files.clone());
    let filter_options = LocalSourceFilterOptions::default()
        .git_ignore(*git_ignore)
        .no_require_git(*no_require_git)
        .custom_ignorefiles(custom_ignorefiles.clone())
        .exclude_if_present(exclude_if_present.clone())
        .exclude_if_xattr(exclude_if_xattr.clone())
        .one_file_system(*one_file_system);
    BackupOptions::default()
        .excludes(excludes)
        .ignore_filter_opts(filter_options)
}

/// The host a snapshot records, checked as the daemon's [`Hostname`] checks
/// it: one to 255 bytes, no control character. A host the daemon would
/// refuse is an invalid spec, like a reserved extra tag.
pub(crate) fn snapshot_host(host: Option<&str>) -> Result<Option<String>, JobError> {
    host.map(|host| {
        Hostname::parse(host)
            .map(|host| host.as_str().to_owned())
            .map_err(|error| invalid_spec(&error))
    })
    .transpose()
}

fn invalid_spec(error: &dyn std::error::Error) -> JobError {
    JobError::Failed {
        code: ErrorCode::InvalidJobSpec,
        detail: error.to_string(),
    }
}

/// What a prune plan will do, counted in packs.
///
/// - `packs_marked`: unused packs and packs to repack. Neither is deleted by
///   this run; both are marked, and a later run deletes them once
///   `keep_delete` has passed. A pack younger than rustic's `keep_pack`
///   (zero here) is counted although rustic leaves it alone.
/// - `packs_deleted`: packs marked by an earlier run that rustic deletes now.
/// - `packs_recovered`: marked packs a newer snapshot uses again; rustic
///   unmarks them.
pub(crate) fn prune_result(repository: RepositoryId, stats: &PruneStats) -> PruneResult {
    PruneResult {
        repository,
        packs_marked: stats.packs.unused.saturating_add(stats.packs.repack),
        packs_deleted: stats.packs_to_delete.remove,
        packs_recovered: stats.packs_to_delete.recover,
    }
}

/// What a prune would do, for a read: the figures of [`prune_result`], and the
/// bytes of the packs rustic would delete now.
pub(crate) fn prune_plan_result(repository: RepositoryId, stats: &PruneStats) -> PrunePlanResult {
    let PruneResult {
        repository,
        packs_marked,
        packs_deleted,
        packs_recovered,
    } = prune_result(repository, stats);
    PrunePlanResult {
        repository,
        packs_marked,
        packs_deleted,
        packs_recovered,
        bytes_deleted: stats.size_to_delete.remove,
    }
}

/// The warning code of a check's finding at `level`. `CheckErrorLevel`
/// isn't exported, so the level is read from its `Debug` form: `Warn` is a
/// warning, anything else an error, so a level a newer rustic adds is told
/// as an error rather than passed over.
pub(crate) fn finding_code(level: &impl std::fmt::Debug) -> WarningCode {
    if format!("{level:?}") == "Warn" {
        WarningCode::CheckWarning
    } else {
        WarningCode::CheckError
    }
}

/// What a finished check found, counted by level as [`finding_code`] tells
/// them.
pub(crate) fn check_result(repository: RepositoryId, results: &CheckResults) -> CheckResult {
    let (mut errors, mut warnings) = (0_u64, 0_u64);
    for (level, _) in &results.0 {
        if finding_code(level) == WarningCode::CheckWarning {
            warnings = warnings.saturating_add(1);
        } else {
            errors = errors.saturating_add(1);
        }
    }
    CheckResult {
        repository,
        errors,
        warnings,
    }
}

/// A check's findings, each with its warning code: the errors first, then the
/// rest, each in the order rustic found them. An operation keeps only its
/// first warnings (§4.1), and rustic finds the packs to tidy before the damage
/// in the trees, so a check that found more than that would otherwise keep
/// none of the errors it counts.
pub(crate) fn errors_first<L: std::fmt::Debug, F>(
    findings: &[(L, F)],
) -> impl Iterator<Item = (WarningCode, &F)> {
    let coded = findings
        .iter()
        .map(|(level, finding)| (finding_code(level), finding));
    let errors = coded
        .clone()
        .filter(|(code, _)| *code == WarningCode::CheckError);
    let rest = coded.filter(|(code, _)| *code != WarningCode::CheckError);
    errors.chain(rest)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use domain::Timestamp;
    use jiff::tz::TimeZone;
    use rustic_core::repofile::{SnapshotId as RusticSnapshotId, SnapshotSummary, StringList};

    use super::*;

    #[derive(Debug)]
    enum Level {
        Warn,
        Error,
    }

    #[test]
    fn a_checks_errors_come_before_what_it_found_to_tidy_each_in_the_order_found() {
        let found = [
            (Level::Warn, "a"),
            (Level::Error, "b"),
            (Level::Warn, "c"),
            (Level::Error, "d"),
        ];
        let sent: Vec<_> = errors_first(&found)
            .map(|(code, finding)| (code, *finding))
            .collect();
        assert_eq!(
            sent,
            [
                (WarningCode::CheckError, "b"),
                (WarningCode::CheckError, "d"),
                (WarningCode::CheckWarning, "a"),
                (WarningCode::CheckWarning, "c"),
            ]
        );
    }

    fn repository() -> RepositoryId {
        RepositoryId::parse(&"cd".repeat(32)).unwrap()
    }

    fn rustic_id(byte: &str) -> RusticSnapshotId {
        byte.repeat(32).parse().unwrap()
    }

    fn list(items: &[&str]) -> StringList {
        let mut list = StringList::default();
        for item in items {
            list.add((*item).to_owned());
        }
        list
    }

    fn snapshot() -> SnapshotFile {
        SnapshotFile {
            id: rustic_id("ab"),
            time: Timestamp::from_second(1_800_000_123)
                .unwrap()
                .to_zoned(TimeZone::UTC),
            hostname: "laptop".to_owned(),
            label: "home".to_owned(),
            paths: list(&["/home/demo", "/etc"]),
            tags: list(&["ladderback-plan:home", "weekly,odd"]),
            ..SnapshotFile::default()
        }
    }

    #[test]
    fn a_snapshot_becomes_a_reference_with_its_full_id() {
        let reference = snapshot_ref(&snapshot()).unwrap();
        assert_eq!(reference.id.as_str(), "ab".repeat(32));
        assert_eq!(
            reference.time,
            Timestamp::from_second(1_800_000_123).unwrap()
        );
        assert_eq!(reference.host, "laptop");
        assert_eq!(reference.label, "home");
        assert_eq!(reference.paths, ["/etc", "/home/demo"]);
        // A tag with a comma stays one tag.
        assert_eq!(reference.tags, ["ladderback-plan:home", "weekly,odd"]);
    }

    fn summarized(parent: Option<RusticSnapshotId>) -> SnapshotFile {
        let mut summary = SnapshotSummary::default();
        summary.files_new = 3;
        summary.files_changed = 2;
        summary.files_unmodified = 11;
        summary.total_bytes_processed = 65_536;
        summary.data_added = 4096;
        summary.data_added_packed = 1024;
        summary.error_count = 1;
        SnapshotFile {
            parent,
            summary: Some(summary),
            ..snapshot()
        }
    }

    #[test]
    fn a_backup_result_carries_the_summary_and_the_parent_rustic_recorded() {
        let result = backup_result(repository(), &summarized(Some(rustic_id("ef")))).unwrap();
        assert_eq!(
            result,
            BackupResult {
                repository: repository(),
                snapshot: domain::SnapshotId::parse(&"ab".repeat(32)).unwrap(),
                parent: Some(domain::SnapshotId::parse(&"ef".repeat(32)).unwrap()),
                files_new: 3,
                files_changed: 2,
                files_unmodified: 11,
                bytes_processed: 65_536,
                data_added: 4096,
                data_added_packed: 1024,
                error_count: 1,
            }
        );
    }

    #[test]
    fn a_first_backup_has_no_parent() {
        let result = backup_result(repository(), &summarized(None)).unwrap();
        assert_eq!(result.parent, None);
    }

    #[test]
    fn a_backup_result_carries_the_bytes_the_backup_went_through() {
        let result = backup_result(repository(), &summarized(None)).unwrap();
        assert_eq!(result.bytes_processed, 65_536);
    }

    #[test]
    fn a_snapshot_without_a_summary_is_an_error_not_a_guess() {
        assert_matches!(
            backup_result(repository(), &snapshot()),
            Err(JobError::Failed { .. })
        );
    }

    #[test]
    fn prune_never_deletes_instantly_and_keeps_what_the_job_says() {
        let options = prune_options(SignedDuration::from_hours(144)).unwrap();
        assert!(!options.instant_delete);
        assert!(!options.early_delete_index);
        assert_eq!(options.keep_delete.get_seconds(), 144 * 3600);
        // The seconds are the whole span: no calendar units to bend it.
        assert_eq!(options.keep_delete.get_days(), 0);
    }

    #[test]
    fn a_keep_delete_shorter_than_rustics_default_is_refused() {
        assert!(prune_options(SignedDuration::from_hours(23)).is_ok());
        for short in [
            SignedDuration::from_hours(23) - SignedDuration::from_secs(1),
            SignedDuration::ZERO,
            SignedDuration::from_hours(-1),
        ] {
            assert_matches!(
                prune_options(short),
                Err(JobError::Failed { .. }),
                "{short:?}"
            );
        }
    }

    fn plan_tags() -> PlanTags {
        PlanTags::new(
            domain::PlanId::parse("home").unwrap(),
            domain::AgentId::from_uuid(domain::Uuid::from_u128(1)),
        )
    }

    #[test]
    fn a_snapshot_gets_the_plan_tags_and_each_extra_tag_whole() {
        let tags = snapshot_tags(&plan_tags(), &["a,b".to_owned()]).unwrap();
        let mut tags: Vec<&str> = tags.iter().map(String::as_str).collect();
        tags.sort_unstable();
        assert_eq!(
            tags,
            [
                "a,b",
                "ladderback-agent:00000000-0000-0000-0000-000000000001",
                "ladderback-plan:home",
            ]
        );
    }

    #[test]
    fn an_extra_tag_with_the_reserved_prefix_is_refused() {
        assert_matches!(
            snapshot_tags(&plan_tags(), &["ladderback-plan:other".to_owned()]),
            Err(JobError::Failed { .. })
        );
    }

    #[test]
    fn a_prune_plan_is_counted_in_packs() {
        let mut stats = PruneStats::default();
        stats.packs.unused = 3;
        stats.packs.repack = 2;
        stats.packs.partly_used = 40;
        stats.packs.used = 400;
        stats.packs_to_delete.remove = 4;
        stats.packs_to_delete.recover = 1;
        stats.packs_to_delete.keep = 9;
        assert_eq!(
            prune_result(repository(), &stats),
            PruneResult {
                repository: repository(),
                packs_marked: 5,
                packs_deleted: 4,
                packs_recovered: 1,
            }
        );
    }

    #[test]
    fn a_prune_plan_adds_the_bytes_it_would_delete() {
        let mut stats = PruneStats::default();
        stats.packs.unused = 3;
        stats.packs.repack = 2;
        stats.packs_to_delete.remove = 4;
        stats.packs_to_delete.recover = 1;
        stats.size_to_delete.remove = 8192;
        assert_eq!(
            prune_plan_result(repository(), &stats),
            PrunePlanResult {
                repository: repository(),
                packs_marked: 5,
                packs_deleted: 4,
                packs_recovered: 1,
                bytes_deleted: 8192,
            }
        );
    }

    fn every_filter_field() -> SourceFilterSpec {
        SourceFilterSpec {
            globs: vec!["!*.tmp".to_owned()],
            iglobs: vec!["!node_modules".to_owned()],
            glob_files: vec!["/home/demo/.globs".to_owned()],
            iglob_files: vec!["/home/demo/.iglobs".to_owned()],
            git_ignore: true,
            no_require_git: true,
            custom_ignorefiles: vec![".rusticignore".to_owned()],
            exclude_if_present: vec![".nobackup".to_owned()],
            exclude_if_xattr: vec!["user.nobackup".to_owned()],
            one_file_system: true,
        }
    }

    /// rustic's options as a rustic profile would set them, under rustic's
    /// own keys, so a field that lands on the wrong rustic field is caught.
    fn as_rustic_reads(value: serde_json::Value) -> serde_json::Value {
        let filter: rustic_core::LocalSourceFilterOptions = serde_json::from_value(value).unwrap();
        serde_json::to_value(filter).unwrap()
    }

    #[test]
    fn every_field_of_a_source_filter_reaches_rustic_under_its_own_name() {
        let options = backup_options(&every_filter_field());
        let excludes: rustic_core::Excludes = serde_json::from_value(serde_json::json!({
            "globs": ["!*.tmp"],
            "iglobs": ["!node_modules"],
            "glob-files": ["/home/demo/.globs"],
            "iglob-files": ["/home/demo/.iglobs"],
        }))
        .unwrap();
        assert_eq!(options.excludes, excludes);
        assert_eq!(
            serde_json::to_value(&options.ignore_filter_opts).unwrap(),
            as_rustic_reads(serde_json::json!({
                "git-ignore": true,
                "no-require-git": true,
                "custom-ignorefiles": [".rusticignore"],
                "exclude-if-present": [".nobackup"],
                "exclude-if-xattr": ["user.nobackup"],
                "one-file-system": true,
            }))
        );
    }

    #[test]
    fn an_empty_source_filter_leaves_nothing_out() {
        let options = backup_options(&SourceFilterSpec::default());
        assert!(options.excludes.is_empty());
        assert_eq!(
            serde_json::to_value(&options.ignore_filter_opts).unwrap(),
            as_rustic_reads(serde_json::json!({}))
        );
    }

    #[test]
    fn a_snapshot_host_is_kept_as_written_and_none_stays_none() {
        for host in ["demo-laptop", "Demo's MacBook Pro", &"é".repeat(127)] {
            assert_eq!(snapshot_host(Some(host)).unwrap().as_deref(), Some(host));
        }
        assert_eq!(snapshot_host(None).unwrap(), None);
    }

    #[test]
    fn a_host_the_daemon_would_refuse_is_an_invalid_spec() {
        for host in [String::new(), "a".repeat(256), "demo\nlaptop".to_owned()] {
            assert_matches!(
                snapshot_host(Some(&host)),
                Err(JobError::Failed {
                    code: ErrorCode::InvalidJobSpec,
                    ..
                }),
                "{host:?}"
            );
        }
    }
}
