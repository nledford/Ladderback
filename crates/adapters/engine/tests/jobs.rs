//! The real jobs against scratch repositories, through the real worker
//! process.

mod support;

use std::assert_matches;
use std::fs::{self, File, FileTimes};
use std::time::{Duration, SystemTime};

use app::ports::{
    Engine, EngineEvent, EngineResult, ForgetJob, Job, OperationStep, ProgressEvent, RunningJob,
};
use domain::SignedDuration;
use domain::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::execution::{FailureReason, Warning, WarningKind};
use domain::secrets::SecretValue;
use futures::StreamExt;
use rustic_core::repofile::SnapshotFile;
use rustic_core::{BackupOptions, PathList, SnapshotOptions};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

fn tags_of(snapshot: &SnapshotFile) -> Vec<String> {
    snapshot.tags.iter().cloned().collect()
}

#[tokio::test]
async fn identify_reports_the_full_repository_id() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let repository = location.init();
    let expected = repository.config().id.to_hex().to_string();
    let events = run_to_end(
        &engine(),
        app::ports::Job::Identify(location.access(PASSWORD)),
    )
    .await;
    let EngineResult::Repository { id } = succeeded(&events) else {
        panic!("not an identify result: {events:?}");
    };
    assert_eq!(id.as_str().len(), 64);
    assert_eq!(id.as_str(), expected);
}

#[tokio::test]
async fn ls_lists_the_snapshots_newest_first() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let repo = location.open().to_indexed_ids().unwrap();
    let paths = PathList::from_iter([scratch.source()]).sanitize().unwrap();
    for label in ["first", "second", "third"] {
        repo.backup(
            &BackupOptions::default(),
            &paths,
            SnapshotOptions::default()
                .label(label.to_owned())
                .to_snapshot()
                .unwrap(),
        )
        .unwrap();
    }
    let events = run_to_end(&engine(), app::ports::Job::Ls(location.access(PASSWORD))).await;
    let EngineResult::Snapshots(listed) = succeeded(&events) else {
        panic!("not a snapshots result: {events:?}");
    };
    assert_eq!(listed.len(), 3, "{listed:?}");
    let labels: Vec<&str> = listed.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["third", "second", "first"]);
}

#[tokio::test]
async fn a_backup_writes_a_snapshot_with_the_plan_tags_extra_tags_and_label() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let summary = backed_up(&events);
    assert_eq!(summary.files_new, 1);
    let snapshots = location.snapshots();
    let [snapshot] = snapshots.as_slice() else {
        panic!("expected one snapshot: {snapshots:?}");
    };
    assert_eq!(snapshot.id.to_hex().as_str(), summary.snapshot.as_str());
    let tags = tags_of(snapshot);
    assert!(
        tags.contains(&format!("{PLAN_TAG_PREFIX}daily")),
        "{tags:?}"
    );
    assert!(
        tags.iter().any(|t| t.starts_with(AGENT_TAG_PREFIX)),
        "{tags:?}"
    );
    assert!(tags.contains(&"extra".to_owned()), "{tags:?}");
    assert_eq!(snapshot.label, "home");
    // Started came before the backup's own progress.
    assert_matches!(
        events
            .iter()
            .find(|e| !matches!(e, EngineEvent::Progress(_))),
        Some(EngineEvent::Started { .. })
    );
}

#[tokio::test]
async fn a_backup_reports_the_bytes_of_the_source_it_went_through() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    write_file(scratch.source().join("b.txt"), b"hello, again");
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    assert_eq!(backed_up(&events).bytes_processed, Some(17));
}

#[tokio::test]
async fn a_backup_after_the_source_folder_moved_has_the_previous_plan_snapshot_as_parent() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let before = scratch.dir.path().join("before");
    let after = scratch.dir.path().join("after");
    write_file(before.join("a.txt"), b"hello");
    let engine = engine();
    let first = run_to_end(&engine, backup_job(&location, &before, "daily", HOUR)).await;
    fs::rename(&before, &after).unwrap();
    let second = run_to_end(&engine, backup_job(&location, &after, "daily", HOUR)).await;
    assert_eq!(
        backed_up(&second).parent.as_ref(),
        Some(&backed_up(&first).snapshot),
        "rustic's own match would have found no parent: the paths differ"
    );
}

#[tokio::test]
async fn a_first_backup_with_an_untagged_earlier_snapshot_gets_rustics_default_parent() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let repository = location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let paths = PathList::from_iter([scratch.source()]).sanitize().unwrap();
    let untagged = repository
        .to_indexed_ids()
        .unwrap()
        .backup(
            &BackupOptions::default(),
            &paths,
            SnapshotOptions::default()
                .label("home".to_owned())
                .to_snapshot()
                .unwrap(),
        )
        .unwrap();
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    assert_eq!(
        backed_up(&events)
            .parent
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        Some(untagged.id.to_hex().to_string()),
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_file_is_a_warning_and_counted_as_an_error() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("ok.txt"), b"fine");
    let locked = scratch.source().join("locked.txt");
    write_file(&locked, b"secret");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if File::open(&locked).is_ok() {
        testkit::skip!("running as a user that can read mode 000 files");
    }
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let warning = events.iter().find_map(|event| match event {
        EngineEvent::Warning(warning) => Some(warning),
        _ => None,
    });
    let warning = warning.unwrap_or_else(|| panic!("no warning in {events:?}"));
    assert_eq!(warning.kind, WarningKind::SourceUnreadable);
    assert!(
        warning
            .item
            .as_deref()
            .is_some_and(|item| item.ends_with("locked.txt")),
        "{warning:?}"
    );
    assert_eq!(backed_up(&events).error_count, 1);
}

#[tokio::test]
async fn a_budget_of_zero_fails_before_anything_is_written() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", SignedDuration::ZERO),
    )
    .await;
    let after_started: Vec<_> = events
        .iter()
        .skip_while(|e| !matches!(e, EngineEvent::Started { .. }))
        .skip(1)
        .collect();
    assert_matches!(
        after_started.as_slice(),
        [EngineEvent::Failed(FailureReason::DeadlinePassed)],
        "{events:?}"
    );
    assert!(location.snapshots().is_empty(), "a snapshot was written");
}

#[tokio::test]
async fn stopping_a_slow_backup_leaves_no_result_and_no_snapshot() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let mut rng = Rng(11);
    for n in 0..8 {
        write_file(
            scratch.source().join(format!("big{n}.bin")),
            &rng.bytes(8 << 20),
        );
    }
    let RunningJob {
        mut events,
        mut control,
    } = engine()
        .run(
            op(),
            backup_job(&location, &scratch.source(), "daily", HOUR),
        )
        .await
        .unwrap();
    loop {
        match events.next().await {
            Some(EngineEvent::Started { .. }) => break,
            Some(_) => {}
            None => panic!("the job ended before it started"),
        }
    }
    control.stop().await;
    assert!(events.next().await.is_none(), "events followed the stop");
    assert!(
        location.snapshots().is_empty(),
        "a stopped backup left a snapshot"
    );
}

#[tokio::test]
async fn a_wrong_password_never_appears_in_the_failure() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let wrong = "hunter2-not-the-password";
    let events = run_to_end(&engine(), app::ports::Job::Identify(location.access(wrong))).await;
    let Some(EngineEvent::Failed(FailureReason::PasswordNotAccepted)) = events.last() else {
        panic!("expected the password to be refused: {events:?}");
    };
    assert!(
        !format!("{events:?}").contains(wrong),
        "the password leaked: {events:?}"
    );
}

#[tokio::test]
async fn a_check_counts_what_it_found_by_level() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let engine = engine();
    run_to_end(
        &engine,
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let clean = run_to_end(&engine, check_job(&location, true)).await;
    let EngineResult::Check(summary) = succeeded(&clean) else {
        panic!("not a check result: {clean:?}");
    };
    assert_eq!((summary.errors, summary.warnings), (0, 0));

    // A pack file nothing refers to is a warning, not an error.
    let stray = scratch
        .repo_dir()
        .join("data")
        .join("ab")
        .join("ab".repeat(32));
    write_file(&stray, b"not a pack");
    let dirty = run_to_end(&engine, check_job(&location, false)).await;
    let EngineResult::Check(summary) = succeeded(&dirty) else {
        panic!("a check that ran to its end must succeed: {dirty:?}");
    };
    assert_eq!(summary.errors, 0, "{summary:?}");
    assert!(summary.warnings >= 1, "{summary:?}");
}

/// The warnings among a run's `events`.
fn warnings(events: &[EngineEvent]) -> Vec<&Warning> {
    events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::Warning(warning) => Some(warning),
            _ => None,
        })
        .collect()
}

/// How many of `found` are of `kind`, as a result counts them.
fn count(found: &[&Warning], kind: WarningKind) -> u64 {
    found.iter().filter(|w| w.kind == kind).count() as u64
}

/// A repository with one snapshot and a pack file no index lists, as a
/// backup killed before it saved its index leaves: its ID starts `abababab`.
async fn with_a_stray_pack(scratch: &Scratch, location: &Location, engine: &engine::WorkerEngine) {
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    run_to_end(
        engine,
        backup_job(location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let stray = scratch
        .repo_dir()
        .join("data")
        .join("ab")
        .join("ab".repeat(32));
    write_file(&stray, b"not a pack");
}

#[tokio::test]
async fn a_check_reports_each_thing_it_found_as_a_warning_in_rustics_words() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let engine = engine();
    with_a_stray_pack(&scratch, &location, &engine).await;
    let events = run_to_end(&engine, check_job(&location, false)).await;
    let EngineResult::Check(summary) = succeeded(&events) else {
        panic!("not a check result: {events:?}");
    };
    let found = warnings(&events);
    assert!(
        found.iter().all(|w| w.kind == WarningKind::CheckWarning
            && w.during == "check"
            && w.item.is_none()),
        "{found:?}"
    );
    assert_eq!(count(&found, WarningKind::CheckWarning), summary.warnings);
    assert_eq!(summary.errors, 0, "{summary:?}");
    assert!(
        found
            .iter()
            .any(|w| w.detail.contains("abababab") && w.detail.contains("not referenced in index")),
        "{found:?}"
    );
}

#[tokio::test]
async fn a_check_that_finds_damage_reports_each_error_and_still_ends() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let engine = engine();
    run_to_end(
        &engine,
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    // Every pack the index names is gone.
    for folder in fs::read_dir(scratch.repo_dir().join("data")).unwrap() {
        for pack in fs::read_dir(folder.unwrap().path()).unwrap() {
            fs::remove_file(pack.unwrap().path()).unwrap();
        }
    }
    let events = run_to_end(&engine, check_job(&location, false)).await;
    let EngineResult::Check(summary) = succeeded(&events) else {
        panic!("a check that ran to its end must succeed: {events:?}");
    };
    assert!(summary.errors >= 1, "{summary:?}");
    let found = warnings(&events);
    assert_eq!(count(&found, WarningKind::CheckError), summary.errors);
    assert_eq!(count(&found, WarningKind::CheckWarning), summary.warnings);
    assert_eq!(
        found.len() as u64,
        summary.errors + summary.warnings,
        "{found:?}"
    );
}

#[tokio::test]
async fn what_a_check_found_hides_the_jobs_secrets() {
    let scratch = Scratch::new();
    let location = scratch.local();
    let engine = engine();
    with_a_stray_pack(&scratch, &location, &engine).await;
    let mut job = check_job(&location, false);
    let Job::Check(check) = &mut job else {
        panic!("not a check job");
    };
    check.repository.secrets = vec![SecretValue::new("abababab")];
    let events = run_to_end(&engine, job).await;
    succeeded(&events);
    let found = warnings(&events);
    assert!(
        found.iter().any(|w| w.detail.contains("pack ***")),
        "{found:?}"
    );
    assert!(
        !format!("{events:?}").contains("abababab"),
        "a secret leaked: {events:?}"
    );
}

#[tokio::test]
async fn a_prune_runs_and_reports_its_plan() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let engine = engine();
    run_to_end(
        &engine,
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let events = run_to_end(&engine, prune_job(&location)).await;
    let EngineResult::Prune(summary) = succeeded(&events) else {
        panic!("not a prune result: {events:?}");
    };
    assert_eq!(summary.packs_deleted, 0);
}

/// The names of the files under the repository's `index` and `data` folders.
fn pack_files(root: &std::path::Path) -> Vec<String> {
    fn walk(folder: &std::path::Path, prefix: &str, names: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                walk(&entry.path(), &name, names);
            } else {
                names.push(name);
            }
        }
    }
    let mut names = Vec::new();
    for folder in ["index", "data"] {
        walk(&root.join(folder), folder, &mut names);
    }
    names.sort();
    names
}

#[tokio::test]
async fn a_prune_plan_reports_what_a_prune_would_do_and_changes_nothing() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let engine = engine();
    write_file(scratch.source().join("a.txt"), b"the first contents");
    let first = run_to_end(
        &engine,
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    let first = backed_up(&first).snapshot.clone();
    write_file(
        scratch.source().join("a.txt"),
        b"contents the first snapshot never held",
    );
    run_to_end(
        &engine,
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    // The first snapshot's data is now used by nothing.
    run_to_end(
        &engine,
        Job::Forget(ForgetJob {
            repository: location.access(PASSWORD),
            ids: vec![first],
        }),
    )
    .await;
    let before = pack_files(&scratch.repo_dir());

    let events = run_to_end(&engine, prune_plan_job(&location)).await;
    let EngineResult::PrunePlan(plan) = succeeded(&events) else {
        panic!("not a prune plan: {events:?}");
    };
    assert!(plan.packs_marked >= 1, "{plan:?}");
    assert_eq!(plan.packs_deleted, 0, "{plan:?}");
    assert_eq!(
        before,
        pack_files(&scratch.repo_dir()),
        "a plan changed the repository"
    );

    // What the plan said is what a prune then does.
    let events = run_to_end(&engine, prune_job(&location)).await;
    let EngineResult::Prune(pruned) = succeeded(&events) else {
        panic!("not a prune result: {events:?}");
    };
    assert_eq!(pruned.packs_marked, plan.packs_marked);
}

#[tokio::test]
async fn only_old_temporary_files_are_removed_before_a_job() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let repo = scratch.repo_dir();
    let hex = "ab".repeat(32);
    let old_tmp = repo.join("data").join("ab").join(format!("{hex}-tmp-1"));
    let old_config_tmp = repo.join("config-tmp-1");
    let fresh_tmp = repo.join("data").join("ab").join(format!("{hex}-tmp-2"));
    let old_other = repo.join("data").join("ab").join("notes.txt");
    for path in [&old_tmp, &old_config_tmp, &fresh_tmp, &old_other] {
        write_file(path, b"x");
    }
    let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 24 * 3600);
    for path in [&old_tmp, &old_config_tmp, &old_other] {
        fs::set_times(path, FileTimes::new().set_modified(two_days_ago)).unwrap();
    }
    let events = run_to_end(
        &engine(),
        app::ports::Job::Identify(location.access(PASSWORD)),
    )
    .await;
    succeeded(&events);
    assert!(!old_tmp.exists() && !old_config_tmp.exists());
    assert!(fresh_tmp.exists() && old_other.exists());
}

#[tokio::test]
async fn a_mistyped_repository_path_loses_no_files() {
    let scratch = Scratch::new();
    // Not a repository: somebody's own folder, with something named like a
    // leftover deep inside.
    let folder = scratch.dir.path().join("home");
    let lookalike = folder
        .join("projects")
        .join("deep")
        .join(format!("{}-tmp-1", "ab".repeat(32)));
    write_file(&lookalike, b"mine");
    two_days_old(&lookalike);
    let events = run_to_end(
        &engine(),
        app::ports::Job::Identify(Location::local(&folder).access(PASSWORD)),
    )
    .await;
    assert_matches!(events.last(), Some(EngineEvent::Failed(_)), "{events:?}");
    assert!(
        lookalike.exists(),
        "a file in a folder that is no repository was deleted"
    );
}

#[tokio::test]
async fn a_repository_that_cannot_be_opened_loses_no_files() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let leftover = scratch
        .repo_dir()
        .join("data")
        .join("ab")
        .join(format!("{}-tmp-1", "ab".repeat(32)));
    write_file(&leftover, b"x");
    two_days_old(&leftover);
    let events = run_to_end(
        &engine(),
        app::ports::Job::Identify(location.access("not the password")),
    )
    .await;
    assert_matches!(events.last(), Some(EngineEvent::Failed(_)), "{events:?}");
    assert!(
        leftover.exists(),
        "a job that failed to open still cleaned up"
    );
}

#[tokio::test]
async fn folders_of_a_repository_that_rustic_does_not_write_to_are_left_alone() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let repo = scratch.repo_dir();
    let name = format!("{}-tmp-1", "ab".repeat(32));
    let unrelated = [
        repo.join("notes").join(&name),
        repo.join("data").join("ab").join("deeper").join(&name),
    ];
    for path in &unrelated {
        write_file(path, b"mine");
        two_days_old(path);
    }
    let events = run_to_end(
        &engine(),
        app::ports::Job::Identify(location.access(PASSWORD)),
    )
    .await;
    succeeded(&events);
    for path in &unrelated {
        assert!(path.exists(), "{path:?} was deleted");
    }
}

/// What the job said when it started: the ID of the repository it opened.
fn started_with(events: &[EngineEvent]) -> Option<domain::RepositoryId> {
    match events
        .iter()
        .find(|e| !matches!(e, EngineEvent::Progress(_)))
    {
        Some(EngineEvent::Started { repository, .. }) => repository.clone(),
        _ => panic!("the job reports its start first: {events:?}"),
    }
}

/// Every job that opens a repository names, as it starts, the ID of the one
/// it opened, so the daemon can record it in an entry that records none
/// (§6.1, decision 57). Both halves of a hot/cold repository hold one ID. An
/// initialisation opens none: the repository it makes is new, and its ID is
/// in the result.
#[tokio::test]
async fn every_job_that_opens_a_repository_names_its_id_when_it_starts() {
    for hot_cold in [false, true] {
        let scratch = Scratch::new();
        let location = if hot_cold {
            scratch.hot_cold()
        } else {
            scratch.local()
        };
        let made = run_to_end(&engine(), Job::Init(location.access(PASSWORD))).await;
        assert_eq!(started_with(&made), None, "init, hot/cold: {hot_cold}");
        let EngineResult::Repository { id: opened } = succeeded(&made).clone() else {
            panic!("not an init result: {made:?}");
        };
        write_file(scratch.source().join("a.txt"), b"hello");
        let jobs = [
            Job::Identify(location.access(PASSWORD)),
            Job::Ls(location.access(PASSWORD)),
            backup_job(&location, &scratch.source(), "daily", HOUR),
            check_job(&location, false),
            Job::Forget(ForgetJob {
                repository: location.access(PASSWORD),
                ids: Vec::new(),
            }),
            prune_plan_job(&location),
            prune_job(&location),
        ];
        for job in jobs {
            let kind = job.kind();
            let events = run_to_end(&engine(), job).await;
            succeeded(&events);
            assert_eq!(
                started_with(&events),
                Some(opened.clone()),
                "{kind:?}, hot/cold: {hot_cold}"
            );
        }
    }
}

/// The steps a run's progress names, in the order it names them, each once
/// where it repeats.
fn steps_of(events: &[EngineEvent]) -> Vec<OperationStep> {
    let mut steps: Vec<OperationStep> = Vec::new();
    for event in events {
        if let EngineEvent::Progress(
            ProgressEvent::Begin {
                step: Some(step), ..
            }
            | ProgressEvent::Title {
                step: Some(step), ..
            },
        ) = event
            && steps.last() != Some(step)
        {
            steps.push(*step);
        }
    }
    steps
}

/// Whether `expected` comes in `steps` in its order, with others between.
fn named_in_order(steps: &[OperationStep], expected: &[OperationStep]) -> bool {
    let mut rest = steps.iter();
    expected.iter().all(|want| rest.any(|step| step == want))
}

/// The pinned rustic's texts name the steps a person sees (decision 82): a
/// first backup reads the index, looks for its parent among the snapshots,
/// then backs up. A rustic that renamed one would show it as working.
#[tokio::test]
async fn a_backup_names_its_steps_from_reading_the_index_to_backing_up() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let events = run_to_end(
        &engine(),
        backup_job(&location, &scratch.source(), "daily", HOUR),
    )
    .await;
    backed_up(&events);
    assert_eq!(
        steps_of(&events),
        [
            OperationStep::ReadingIndex,
            OperationStep::GettingSnapshots,
            OperationStep::BackingUp
        ]
    );
}

#[tokio::test]
async fn a_check_and_a_prune_name_their_steps() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let engine = engine();
    backed_up(
        &run_to_end(
            &engine,
            backup_job(&location, &scratch.source(), "daily", HOUR),
        )
        .await,
    );

    let checked = run_to_end(&engine, check_job(&location, true)).await;
    succeeded(&checked);
    let steps = steps_of(&checked);
    assert!(
        named_in_order(
            &steps,
            &[
                OperationStep::GettingSnapshots,
                OperationStep::ReadingIndex,
                OperationStep::ListingPacks,
                OperationStep::CheckingTrees,
                OperationStep::ReadingPackData,
            ]
        ),
        "{steps:?}"
    );

    let pruned = run_to_end(&engine, prune_job(&location)).await;
    succeeded(&pruned);
    let steps = steps_of(&pruned);
    assert!(
        named_in_order(
            &steps,
            &[
                OperationStep::ReadingIndex,
                OperationStep::GettingSnapshots,
                OperationStep::FindingUsedData,
                OperationStep::ListingPacks,
            ]
        ),
        "{steps:?}"
    );
}
