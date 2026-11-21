//! The history fixture builder (#1122) against rustic itself: the repository
//! it builds passes rustic's check, and rustic lists exactly the declared
//! snapshots, at the declared times, tagged as their plans' own backups.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use domain::{AgentId, Uuid};
use engine::history_fixture::{self, History, Start, Target};
use rustic_backend::BackendOptions;
use rustic_core::repofile::{RusticTime, SnapshotFile};
use rustic_core::{CheckOptions, Credentials, Repository, RepositoryOptions};

/// A canary, never a real password.
const PASSWORD: &str = "canary-history-fixture";

const AGENT: Uuid = Uuid::from_u128(0x0193_0000_0000_7000_8000_0000_0000_0001);

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/history")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Builds `declaration` into a new repository under `scratch`.
fn build(declaration: &str, scratch: &Path) -> Vec<history_fixture::Built> {
    let history: History = toml::from_str(declaration).unwrap();
    let target = Target {
        repository: scratch.join("repo"),
        start: Start::Init,
        password: PASSWORD.to_owned(),
        sources: scratch.join("sources"),
        agent: AgentId::from_uuid(AGENT),
    };
    history_fixture::build(&history, &target).unwrap()
}

fn open(scratch: &Path) -> Repository<rustic_core::OpenStatus> {
    let backends = BackendOptions::default()
        .repository(scratch.join("repo").to_str().unwrap())
        .to_backends()
        .unwrap();
    Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .unwrap()
        .open(&Credentials::password(PASSWORD))
        .unwrap()
}

/// rustic's own check, reading every pack, as `rustic check --read-data`:
/// each finding, which a sound repository has none of.
fn findings(scratch: &Path) -> Vec<String> {
    let results = open(scratch)
        .check(CheckOptions::default().read_data(true))
        .unwrap();
    results
        .0
        .iter()
        .map(|(level, finding)| format!("{level:?}: {finding}"))
        .collect()
}

/// Each snapshot as rustic lists it: its plan tag, its time with the offset
/// it was taken at, and its other tags, oldest first.
fn listed(scratch: &Path) -> Vec<(String, String, Vec<String>, String)> {
    let mut snapshots: Vec<SnapshotFile> = open(scratch).get_all_snapshots().unwrap();
    snapshots.sort_by_key(|snapshot| snapshot.time.timestamp());
    snapshots
        .iter()
        .map(|snapshot| {
            let tags: Vec<String> = snapshot
                .tags
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect();
            let plan = tags
                .iter()
                .find_map(|tag| tag.strip_prefix("ladderback-plan:"))
                .unwrap_or("(none)")
                .to_owned();
            (
                plan,
                RusticTime::to_string(&snapshot.time),
                tags,
                snapshot.hostname.clone(),
            )
        })
        .collect()
}

/// The `[[snapshot]]` tables of a declaration, read without the builder:
/// each one's plan and time, as rustic reads the time.
fn declared(declaration: &str) -> Vec<(String, String)> {
    let table: toml::Table = toml::from_str(declaration).unwrap();
    let mut declared: Vec<(String, String)> = table
        .get("snapshot")
        .and_then(toml::Value::as_array)
        .unwrap()
        .iter()
        .map(|snapshot| {
            let field = |name: &str| snapshot.get(name).and_then(toml::Value::as_str).unwrap();
            let time = RusticTime::parse_utc(field("time")).unwrap();
            (field("plan").to_owned(), RusticTime::to_string(&time))
        })
        .collect();
    declared.sort_by_key(|(_, time)| RusticTime::parse_utc(time).unwrap().timestamp());
    declared
}

#[test]
fn the_small_history_is_exactly_its_declared_snapshots_in_a_repository_rustic_checks_clean() {
    let scratch = tempfile::tempdir().unwrap();
    let declaration = fixture("small.toml");
    build(&declaration, scratch.path());

    let listed = listed(scratch.path());
    let seen: Vec<(String, String)> = listed
        .iter()
        .map(|(plan, time, _, _)| (plan.clone(), time.clone()))
        .collect();
    assert_eq!(seen, declared(&declaration), "the snapshots rustic lists");
    let agent = format!("ladderback-agent:{}", AGENT.hyphenated());
    for (plan, time, tags, host) in &listed {
        // A backup's tags, as the daemon's own backups have them.
        assert_eq!(
            tags,
            &[agent.clone(), format!("ladderback-plan:{plan}")],
            "the tags of the snapshot at {time}"
        );
        assert_eq!(host, "demo-host", "the host of the snapshot at {time}");
    }
    assert_eq!(
        findings(scratch.path()),
        Vec::<String>::new(),
        "rustic's check"
    );
}

#[test]
fn a_failed_run_saves_no_snapshot_and_leaves_a_repository_rustic_checks_clean() {
    let scratch = tempfile::tempdir().unwrap();
    let declaration = r#"
        host = "demo-host"

        [[series]]
        plan = "home"
        first = "2026-08-01T02:00:00+02:00"
        every = "1 day"
        runs = 3
        files = { count = 2, bytes = "4 KiB" }
        failed = [2]

        [[snapshot]]
        plan = "photos"
        time = "2026-08-02T12:00:00+00:00"
        files = { count = 1, bytes = 100 }
    "#;
    let built = build(declaration, scratch.path());

    let seen: Vec<(String, String)> = listed(scratch.path())
        .into_iter()
        .map(|(plan, time, _, _)| (plan, time))
        .collect();
    let at = |plan: &str, time: &str| (plan.to_owned(), time.to_owned());
    assert_eq!(
        seen,
        [
            at("home", "2026-08-01T02:00:00+02:00"),
            at("photos", "2026-08-02T12:00:00+00:00"),
            at("home", "2026-08-03T02:00:00+02:00"),
        ]
    );
    let failed: Vec<String> = built
        .iter()
        .filter(|snapshot| snapshot.id.is_none())
        .map(|snapshot| RusticTime::to_string(&snapshot.time))
        .collect();
    assert_eq!(failed, ["2026-08-02T02:00:00+02:00"], "the failed runs");
    assert_eq!(
        findings(scratch.path()),
        Vec::<String>::new(),
        "rustic's check"
    );
}

#[test]
fn a_repository_that_is_already_there_gets_the_history_added_to_it() {
    let scratch = tempfile::tempdir().unwrap();
    let declaration = fixture("small.toml");
    build(&declaration, scratch.path());
    let history: History = toml::from_str(&declaration).unwrap();
    let target = Target {
        repository: scratch.path().join("repo"),
        start: Start::Existing,
        password: PASSWORD.to_owned(),
        sources: scratch.path().join("more"),
        agent: AgentId::from_uuid(AGENT),
    };
    history_fixture::build(&history, &target).unwrap();
    assert_eq!(listed(scratch.path()).len(), 6);
}

/// Every entry under `folder`, by its path below it, with the time it was
/// last modified.
fn dated(folder: &Path) -> BTreeMap<String, SystemTime> {
    let mut dated = BTreeMap::new();
    let mut left = vec![folder.to_path_buf()];
    while let Some(at) = left.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            let below = path.strip_prefix(folder).unwrap().display().to_string();
            dated.insert(below, path.metadata().unwrap().modified().unwrap());
            if path.is_dir() {
                left.push(path);
            }
        }
    }
    dated
}

#[test]
fn a_plan_whose_files_shrink_loses_them_and_each_entry_is_dated_by_the_run_that_last_changed_it() {
    let scratch = tempfile::tempdir().unwrap();
    let declaration = r#"
        host = "demo-host"

        [[snapshot]]
        plan = "home"
        time = "2026-08-01T02:00:00+00:00"
        files = { count = 4, bytes = 400 }

        [[snapshot]]
        plan = "home"
        time = "2026-08-02T02:00:00+00:00"
        files = { count = 2, bytes = 200 }
    "#;
    build(declaration, scratch.path());

    let at = |time: &str| SystemTime::from(time.parse::<jiff::Timestamp>().unwrap());
    let (first, second) = (at("2026-08-01T02:00:00Z"), at("2026-08-02T02:00:00Z"));
    // The second run keeps the sizes of the files it keeps, rewrites one of
    // them, and removes the two past its count, which changes their folder.
    assert_eq!(
        dated(&scratch.path().join("sources/home")),
        BTreeMap::from([
            ("folder-000".to_owned(), second),
            ("folder-000/demo-00000.dat".to_owned(), first),
            ("folder-000/demo-00001.dat".to_owned(), second),
        ])
    );
    assert_eq!(
        findings(scratch.path()),
        Vec::<String>::new(),
        "rustic's check"
    );
}

#[test]
fn every_checked_in_declaration_is_valid() {
    for name in ["small.toml", "rich.toml"] {
        let history: History =
            toml::from_str(&fixture(name)).unwrap_or_else(|error| panic!("{name}: {error}"));
        history
            .check()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}
