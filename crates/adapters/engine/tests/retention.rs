//! The real `Retention` adapter against a scratch repository: it removes
//! exactly the snapshots rustic's own retention lets go, and it agrees with
//! the port's fake on the same group (§13.3: fakes don't drift).

use std::sync::Arc;

use app::fakes::FakeRetention;
use app::ports::{EngineEvent, EngineResult, Job, RepositoryAccess, Retention, RetentionError};
use domain::SnapshotId;
use domain::Timestamp;
use domain::maintenance::Snapshot;
use domain::planning::{KeepCount, RetentionPolicy};
use engine::{RusticRetention, WorkerEngine};
use rustic_core::{BackupOptions, PathList, SnapshotOptions};

mod support;

use support::{PASSWORD, Scratch, engine, run_to_end, write_file};

/// The repository's snapshots as the use case sees them: the listing's,
/// newest first.
async fn listed(engine: &WorkerEngine, access: &RepositoryAccess) -> Vec<Snapshot> {
    let events = run_to_end(engine, Job::Ls(access.clone())).await;
    let list = events.into_iter().find_map(|event| match event {
        EngineEvent::Succeeded(EngineResult::Snapshots(list)) => Some(list),
        _ => None,
    });
    let Some(list) = list else {
        panic!("the listing never answered");
    };
    let mut group: Vec<Snapshot> = list
        .iter()
        .map(|snapshot| Snapshot {
            id: snapshot.id.clone(),
            time: snapshot.time,
            hostname: snapshot.hostname.clone(),
            label: snapshot.label.clone(),
            tags: snapshot.tags.clone(),
            paths: snapshot.paths.clone(),
        })
        .collect();
    group.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.time));
    group
}

/// Four backups of changing contents: four distinct snapshots, each made
/// at least a second of wall-clock time after the last, so no two share a
/// timestamp and the oldest are unambiguous.
async fn four_snapshots(_scratch: &Scratch, access: &RepositoryAccess) -> Vec<Snapshot> {
    let group = listed(&engine(), access).await;
    assert_eq!(group.len(), 4, "{group:?}");
    group
}

#[tokio::test]
async fn keep_last_removes_the_oldest_as_the_fake_does() {
    let scratch = Scratch::new();
    let location = scratch.local();
    location.init();
    write_file(scratch.source().join("a.txt"), b"hello");
    let repo = location.open().to_indexed_ids().unwrap();
    let paths = PathList::from_iter([scratch.source()]).sanitize().unwrap();
    for number in 1..=4 {
        write_file(
            scratch.source().join("a.txt"),
            format!("content {number}").as_bytes(),
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        repo.backup(
            &BackupOptions::default(),
            &paths,
            SnapshotOptions::default().to_snapshot().unwrap(),
        )
        .unwrap();
    }
    let access = location.access(PASSWORD);
    let group = four_snapshots(&scratch, &access).await;

    let policy = RetentionPolicy {
        keep_last: Some(KeepCount::new(2).unwrap()),
        ..RetentionPolicy::default()
    };
    let real = RusticRetention
        .removes(&access, &policy, &group)
        .await
        .expect("the real adapter answers");
    let expected = Arc::new(FakeRetention)
        .removes(&access, &policy, &group)
        .await
        .expect("the fake answers");

    assert_eq!(real, expected, "the fake and the real adapter agree");
    let oldest: Vec<_> = group[2..].iter().map(|s| s.id.clone()).collect();
    assert_eq!(real, oldest, "the oldest two go: the group is newest first");
}

#[tokio::test]
async fn an_error_never_carries_the_repository_credentials() {
    // A repository string with credentials in it, pointed at a port
    // nothing listens on: the open fails, and its detail must come back
    // redacted (I4), like the worker's own errors.
    let access = RepositoryAccess {
        repository: "sftp://demo:hunter2@127.0.0.1:1/repo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new("e2e-scratch-password"),
        id: None,
        entry: None,
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: Vec::new(),
    };
    let group = vec![Snapshot {
        id: SnapshotId::parse(&"07".repeat(32)).unwrap(),
        time: Timestamp::from_second(1_800_000_000).unwrap(),
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: Vec::new(),
        paths: vec!["/home/demo".to_owned()],
    }];
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &group,
        )
        .await
        .expect_err("the unreachable repository fails");
    let detail = match &error {
        RetentionError::Engine { detail } => detail.clone(),
        other => panic!("an engine failure, not {other:?}"),
    };
    assert!(
        !detail.contains("hunter2") && !detail.contains("e2e-scratch-password"),
        "the detail leaks a credential: {detail}"
    );
}

/// The groups of a repository whose open failed: any one snapshot will do.
fn one_snapshot() -> Vec<Snapshot> {
    vec![Snapshot {
        id: SnapshotId::parse(&"07".repeat(32)).unwrap(),
        time: Timestamp::from_second(1_800_000_000).unwrap(),
        hostname: "demo-host".to_owned(),
        label: String::new(),
        tags: Vec::new(),
        paths: vec!["/home/demo".to_owned()],
    }]
}

#[tokio::test]
async fn an_error_never_carries_what_a_reference_resolved_to() {
    // The secret sits in the URL's path, not its userinfo, so only the
    // resolved list masks it (I4): the open fails on a port nothing listens
    // on, and its error quotes the URL.
    let canary = "CANARY-resolved-path-secret";
    let access = RepositoryAccess {
        repository: format!("rest:http://127.0.0.1:1/{canary}/"),
        repo_hot: None,
        password: domain::secrets::SecretValue::new("e2e-scratch-password"),
        id: None,
        entry: None,
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: vec![domain::secrets::SecretValue::new(canary)],
    };
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &one_snapshot(),
        )
        .await
        .expect_err("the unreachable repository fails");
    let RetentionError::Engine { detail } = &error else {
        panic!("an engine failure, not {error:?}");
    };
    assert!(
        !detail.contains(canary),
        "the detail leaks a resolved secret: {detail}"
    );
}

/// The `rclone` that serves an `rclone:` repository inherits no descriptor
/// another start in the daemon has open: it starts through the spawn gate,
/// so it waits while a pipe that is not yet close-on-exec is open (decision
/// 58). The stand-in for `rclone` writes the number of each of the pipe's
/// ends it holds to a file before any redirection of its own can open one,
/// then exits, so the open fails.
#[cfg(unix)]
#[tokio::test]
async fn an_rclone_repository_s_server_inherits_no_pipe_another_start_has_open() {
    let scratch = tempfile::tempdir().unwrap();
    let found = scratch.path().join("found");
    let pipe = testkit::inheritable::InheritablePipe::open();
    let [read, write] = pipe.numbers();
    // rustic appends the repository's path as the last argument.
    let stand_in = format!(
        "sh -c 'found=; for n in \"$1\" \"$2\"; do if [ -e /dev/fd/$n ]; then found=\"$found $n\"; fi; done; echo \"$found\" > \"$3\"' sh {read} {write} '{}'",
        found.display()
    );
    let access = rclone_repository(stand_in);
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &one_snapshot(),
        )
        .await
        .expect_err("the stand-in exits before it serves");
    drop(pipe);
    let inherited = std::fs::read_to_string(&found)
        .unwrap_or_else(|_| panic!("the stand-in never ran: {error:?}"));
    assert_eq!(
        inherited.trim(),
        "",
        "the rclone stand-in inherited another start's pipe"
    );
}

/// An `rclone:` repository whose `rclone` is started by `stand_in`, the
/// `rclone-command` rustic runs in place of `rclone serve restic`, with the
/// repository's path appended as its last argument.
fn rclone_repository(stand_in: String) -> RepositoryAccess {
    RepositoryAccess {
        repository: "rclone:demo-remote:repo".to_owned(),
        repo_hot: None,
        password: domain::secrets::SecretValue::new(PASSWORD),
        id: None,
        entry: None,
        options: [("rclone-command".to_owned(), stand_in)].into(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: Vec::new(),
    }
}

/// An `rclone` that never says it is serving holds up no other start: the
/// spawn gate is held while it starts, not while the daemon waits for it to
/// serve (decision 58, #319). The stand-in marks that it has started, then
/// waits in silence until the test lets it go, and exits without serving.
#[cfg(unix)]
#[tokio::test]
async fn an_rclone_that_never_serves_holds_up_no_other_start() {
    use std::time::{Duration, Instant};
    let scratch = tempfile::tempdir().unwrap();
    let started = scratch.path().join("started");
    let let_go = scratch.path().join("let-go");
    // At most 30 s, so a run whose gate stays held still ends.
    let stand_in = format!(
        "sh -c ': > \"$1\"; n=0; while [ ! -e \"$2\" ] && [ $n -lt 600 ]; do sleep 0.05; n=$((n + 1)); done' sh '{}' '{}'",
        started.display(),
        let_go.display()
    );
    let access = rclone_repository(stand_in);
    let evaluation = tokio::spawn(async move {
        RusticRetention
            .removes(
                &access,
                &RetentionPolicy {
                    keep_last: Some(KeepCount::new(1).unwrap()),
                    ..RetentionPolicy::default()
                },
                &one_snapshot(),
            )
            .await
    });
    let waiting_since = Instant::now();
    while !started.exists() {
        assert!(
            waiting_since.elapsed() < Duration::from_secs(10),
            "the rclone stand-in never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Another start, as a hook's would: it takes the gate as soon as it is
    // free. Nothing else in this test's process holds it, so a free gate is
    // taken at once; 2 s is slack for a loaded machine.
    let (taken, took) = std::sync::mpsc::channel();
    let other_start = std::thread::spawn(move || {
        spawn_gate::hold(|| {
            let _ = taken.send(());
        });
    });
    let free = took.recv_timeout(Duration::from_secs(2)).is_ok();
    std::fs::write(&let_go, b"").unwrap();
    let outcome = evaluation.await.unwrap();
    other_start.join().unwrap();
    assert!(free, "another start waited for an rclone that never served");
    outcome.expect_err("the stand-in exits without serving");
}

/// An `rclone:` repository is read through the address its server says it
/// serves on, as the user and with the password the server was handed in
/// its environment, and the server is stopped once the evaluation ends.
/// The stand-in names a listener of the test's, which answers that there
/// is no such file, so the open fails, and its error names the repository
/// as it is written, not the server's address, as rustic's own does.
#[cfg(unix)]
#[tokio::test]
async fn an_rclone_repository_is_read_where_its_server_serves_and_the_server_stops() {
    use base64::Engine;
    use std::io::{BufRead, BufReader, Write};
    let scratch = tempfile::tempdir().unwrap();
    let handed = scratch.path().join("handed");
    let pid = scratch.path().join("pid");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (authorized, authorizations) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut request = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while request.read_line(&mut line).is_ok_and(|read| read > 2) {
                if let Some(value) = line.trim_end().strip_prefix("authorization: ") {
                    let _ = authorized.send(value.to_owned());
                }
                line.clear();
            }
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            );
        }
    });
    let stand_in = format!(
        "sh -c 'echo \"$RCLONE_USER:$RCLONE_PASS\" > \"$1\"; echo $$ > \"$2\"; echo \"NOTICE: Serving restic REST API on [http://127.0.0.1:$3/]\" >&2; exec sleep 30' sh '{}' '{}' {port}",
        handed.display(),
        pid.display()
    );
    let mut access = rclone_repository(stand_in);
    access
        .options
        .insert("retry".to_owned(), "false".to_owned());
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &one_snapshot(),
        )
        .await
        .expect_err("the listener has no repository");
    let RetentionError::Engine { detail } = &error else {
        panic!("an engine failure, not {error:?}");
    };
    assert!(detail.contains("`rclone:demo-remote:repo`"), "{detail}");
    assert!(
        !detail.contains(&format!(":{port}")),
        "the detail shows the server's address: {detail}"
    );
    let handed = std::fs::read_to_string(&handed).unwrap();
    let (user, password) = handed.trim().split_once(':').unwrap();
    assert!(!user.is_empty() && !password.is_empty() && user != password);
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(handed.trim())
    );
    let sent = authorizations
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the repository was read through the server's address");
    assert_eq!(sent, expected);
    let pid = std::fs::read_to_string(&pid).unwrap();
    let mut probe = std::process::Command::new("kill");
    probe
        .args(["-0", pid.trim()])
        .stderr(std::process::Stdio::null());
    let alive = spawn_gate::hold(|| probe.spawn())
        .unwrap()
        .wait()
        .unwrap()
        .success();
    assert!(!alive, "the rclone stand-in is still running, or unreaped");
}

/// The user and password an `rclone` server is started with never show in
/// an error (I4), though rustic's quotes the address they are written in:
/// here, a `rest-url` that is no URL. They are 64 hex digits each.
#[cfg(unix)]
#[tokio::test]
async fn an_error_never_carries_the_rclone_server_s_credentials() {
    let mut access = rclone_repository("sh -c 'exec sleep 30' sh".to_owned());
    access
        .options
        .insert("rest-url".to_owned(), "http://bad host:1".to_owned());
    let error = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &one_snapshot(),
        )
        .await
        .expect_err("the address is no URL");
    let RetentionError::Engine { detail } = &error else {
        panic!("an engine failure, not {error:?}");
    };
    assert!(detail.contains("bad host:1"), "{detail}");
    let longest_hex = detail
        .split(|c: char| !c.is_ascii_hexdigit())
        .map(str::len)
        .max()
        .unwrap_or(0);
    assert!(longest_hex < 64, "the detail leaks a credential: {detail}");
}
