//! The daemon's log while the daemon opens a repository itself, to evaluate
//! retention: rustic, its backends and OpenDAL log through the logger the
//! daemon installs, and no record of theirs shows a credential of the
//! repository (I4, #330).

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use app::ports::{RepositoryAccess, Retention};
use domain::planning::{KeepCount, RetentionPolicy};
use domain::secrets::SecretValue;
use engine::RusticRetention;
use env_logger::{Target, WriteStyle};
use rustic_backend::BackendOptions;
use rustic_core::{ConfigOptions, Credentials, KeyOptions, Repository, RepositoryOptions};

use super::Pipe;
use crate::logging::{logger, set};

const PASSWORD: &str = "demo-scratch-password";

/// The credential in the rclone location: S3's secret key, written in the
/// connection string, as rclone allows.
const CANARY: &str = "CANARY-rclone-secret-key";

/// A repository made in `dir` with rustic itself, holding no snapshot.
fn init(dir: &Path) {
    let backends = BackendOptions::default()
        .repository(dir.to_string_lossy().into_owned())
        .to_backends()
        .unwrap();
    Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .unwrap()
        .init(
            &Credentials::password(PASSWORD),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .unwrap();
}

/// What `rclone serve restic` serves, for the requests an open and a
/// snapshot listing make: the repository in `root` over rustic's REST
/// protocol (version 2 listings), one request per connection, whatever the
/// credentials.
fn rest_server(root: PathBuf) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            answer(&root, &stream);
        }
    });
    address
}

fn answer(root: &Path, stream: &TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
        header.clear();
    }
    let mut parts = request.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return;
    };
    let wanted = root.join(path.trim_start_matches('/'));
    let (status, body) = if path.ends_with('/') {
        ("200 OK", listing(&wanted).into_bytes())
    } else {
        match std::fs::read(&wanted) {
            Ok(bytes) => ("200 OK", bytes),
            Err(_) => ("404 Not Found", Vec::new()),
        }
    };
    let body = if method == "HEAD" { Vec::new() } else { body };
    let mut writer = stream;
    let _ = writer.write_all(
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/vnd.x.restic.rest.v2\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    );
    let _ = writer.write_all(&body);
}

/// A folder's files as a version 2 listing: `[{"name": …, "size": …}]`.
fn listing(folder: &Path) -> String {
    let entries: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| {
            format!(
                r#"{{"name":"{}","size":{}}}"#,
                entry.file_name().to_string_lossy(),
                entry.metadata().unwrap().len()
            )
        })
        .collect();
    format!("[{}]", entries.join(","))
}

/// An `rclone:` repository whose connection string holds `credential`, as
/// the daemon opens it for a forget, with the daemon's logger installed at
/// `filter` (`RUST_LOG`): the stand-in for `rclone` says what it was asked
/// to serve, as rclone does when it fails to (`Failed to create file system
/// for ":s3,…"`), then that it serves the scratch repository, the open goes
/// on to the end, and once rustic has stopped the server, the stand-in says
/// it again while rustic drops the backend. Every record of it reaches the
/// daemon's own logger,
/// whose text this returns once the open succeeded.
async fn open_logged(filter: Option<&str>, credential: &str) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("repo");
    init(&root);
    let address = rest_server(root);
    let pipe = Pipe::default();
    set(logger(
        filter,
        Target::Pipe(Box::new(pipe.clone())),
        WriteStyle::Never,
    ));
    // rustic appends what follows `rclone:` as the last argument; `exec`
    // makes the shell the server rustic stops when it is done. A child of it
    // holds rclone's stderr, and once the server is stopped (the write end
    // of the FIFO, which only the server holds, closes) says what was asked
    // of it again: rustic logs that while it drops the backend, waiting for
    // the thread that reads the stderr (I4 holds until the open is gone).
    let fifo = scratch.path().join("stopped");
    let stand_in = format!(
        "sh -c 'mkfifo \"{fifo}\"; \
         (cat \"{fifo}\" >/dev/null; echo \"rclone was stopped serving $1\" >&2) & \
         echo \"rclone was asked to serve $1\" >&2; \
         echo \"Serving restic REST API on http://{address}/\" >&2; \
         exec sleep 600 3>\"{fifo}\"' sh",
        fifo = fifo.display()
    );
    let access = RepositoryAccess {
        repository: format!(
            "rclone::s3,access_key_id=demo-key-id,secret_access_key={credential}:demo-bucket/repo"
        ),
        repo_hot: None,
        password: SecretValue::new(PASSWORD),
        id: None,
        entry: None,
        options: [("rclone-command".to_owned(), stand_in)].into(),
        options_hot: Default::default(),
        options_cold: Default::default(),
        secrets: Vec::new(),
    };
    let removed = RusticRetention
        .removes(
            &access,
            &RetentionPolicy {
                keep_last: Some(KeepCount::new(1).unwrap()),
                ..RetentionPolicy::default()
            },
            &[],
        )
        .await;
    let log = pipe.text();
    assert_eq!(removed, Ok(Vec::new()), "the open succeeds: {log}");
    // The records that name the location were written, so what a test
    // asserts of the log is about them.
    for written in [
        "password is correct",
        "rclone was asked to serve",
        "rclone was stopped serving",
    ] {
        assert!(
            log.contains(written),
            "the open's records reached the daemon's log: {log}"
        );
    }
    log
}

/// At the daemon's default level, none of the open's records shows the
/// credential.
#[tokio::test]
async fn an_open_by_the_daemon_never_logs_a_credential_of_an_rclone_location() {
    let log = open_logged(None, CANARY).await;
    assert!(!log.contains(CANARY), "the daemon's log leaks: {log}");
}

/// At `debug`, rustic logs the command it starts rclone with as `{:?}`
/// prints it, the location last, so a credential's `"` shows as `\"`: the
/// credential is masked in that form too.
#[tokio::test]
async fn at_debug_an_open_by_the_daemon_never_logs_a_credential_its_debug_form_escapes() {
    let log = open_logged(Some("debug"), "CANARY-rclone\"quoted-key").await;
    assert!(
        log.contains("starting rclone via"),
        "the record that quotes rclone's command was written: {log}"
    );
    assert!(!log.contains("CANARY"), "the daemon's log leaks: {log}");
}
