//! A plan's backup settings reach rustic through the real worker: the source
//! filter decides what the snapshot holds, and the snapshot host is what the
//! snapshot records (§6.1, §7.3). The snapshots are read back with
//! `rustic_core` itself.

mod support;

use app::ports::{BackupJob, Job};
use domain::SignedDuration;
use domain::planning::{Hostname, SourceFilter};
use support::*;

const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// `job` with what its plan sets: a host and a source filter.
fn with_settings(job: Job, host: Option<&str>, filter: SourceFilter) -> Job {
    let Job::Backup(job) = job else {
        panic!("not a backup: {job:?}");
    };
    Job::Backup(BackupJob {
        host: host.map(|host| Hostname::parse(host).unwrap()),
        filter,
        ..job
    })
}

#[tokio::test]
async fn a_backup_leaves_out_everything_its_source_filter_excludes() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    let source = scratch.source();
    write_file(source.join("keep.txt"), b"kept");
    write_file(source.join("build.tmp"), b"a glob");
    write_file(
        source.join("Node_Modules/pkg/index.js"),
        b"an iglob, any case",
    );
    write_file(source.join("trace.log"), b"a pattern in a glob file");
    write_file(
        source.join("photo.bak"),
        b"a pattern in an iglob file, any case",
    );
    write_file(source.join(".gitignore"), b"ignored-by-git.txt\n");
    write_file(
        source.join("ignored-by-git.txt"),
        b"no git repository needed",
    );
    write_file(source.join(".rusticignore"), b"ignored-by-custom.txt\n");
    write_file(
        source.join("ignored-by-custom.txt"),
        b"a custom ignore file",
    );
    write_file(source.join("cache/.nobackup"), b"");
    write_file(source.join("cache/data.bin"), b"a marked folder");
    // The glob files sit outside the source, as a profile's usually do.
    let globs = scratch.dir.path().join("globs.txt");
    write_file(&globs, b"!*.log\n");
    let iglobs = scratch.dir.path().join("iglobs.txt");
    write_file(&iglobs, b"!*.BAK\n");
    let filter = SourceFilter {
        globs: vec!["!*.tmp".to_owned()],
        iglobs: vec!["!node_modules".to_owned()],
        glob_files: vec![domain::planning::GlobFile::parse(&globs.to_string_lossy()).unwrap()],
        iglob_files: vec![domain::planning::GlobFile::parse(&iglobs.to_string_lossy()).unwrap()],
        git_ignore: true,
        no_require_git: true,
        custom_ignorefiles: vec![".rusticignore".to_owned()],
        exclude_if_present: vec![".nobackup".to_owned()],
        ..SourceFilter::default()
    };

    let job = with_settings(backup_job(&location, &source, "home", HOUR), None, filter);
    let events = run_to_end(&engine(), job).await;
    backed_up(&events);

    let snapshots = location.snapshots();
    let [snapshot] = snapshots.as_slice() else {
        panic!("expected one snapshot: {snapshots:?}");
    };
    let names = location.names_in(snapshot);
    for kept in ["keep.txt", ".gitignore", ".rusticignore"] {
        assert!(names.iter().any(|name| name == kept), "{kept}: {names:?}");
    }
    for left_out in [
        "build.tmp",
        "Node_Modules",
        "index.js",
        "trace.log",
        "photo.bak",
        "ignored-by-git.txt",
        "ignored-by-custom.txt",
        "cache",
        "data.bin",
    ] {
        assert!(
            !names.iter().any(|name| name == left_out),
            "{left_out}: {names:?}"
        );
    }
}

#[tokio::test]
async fn a_backup_records_the_snapshot_host_it_is_given() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");

    let job = with_settings(
        backup_job(&location, &scratch.source(), "home", HOUR),
        Some("demo-laptop"),
        SourceFilter::default(),
    );
    let events = run_to_end(&engine(), job).await;
    backed_up(&events);

    let snapshots = location.snapshots();
    let [snapshot] = snapshots.as_slice() else {
        panic!("expected one snapshot: {snapshots:?}");
    };
    assert_eq!(snapshot.hostname, "demo-laptop");
}
