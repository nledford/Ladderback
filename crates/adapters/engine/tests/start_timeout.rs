//! The start timeout through the real worker: it is for a worker to take its
//! job, not to open the repository. A backend that takes its time, or never
//! answers, must not make the daemon kill a healthy worker.

mod support;

use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app::ports::{Engine, Job, RepositoryAccess, RunningJob};
use domain::secrets::SecretValue;
use futures::StreamExt;
use support::{engine, op};

/// A server that takes every connection and never answers one, and how many
/// connections it has taken.
fn a_server_that_never_answers() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&accepted);
    std::thread::spawn(move || {
        // The connections stay open for as long as the test runs.
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept() {
            held.push(stream);
            count.fetch_add(1, Ordering::SeqCst);
        }
    });
    (port, accepted)
}

#[tokio::test]
async fn a_worker_stuck_opening_its_repository_is_not_timed_out_as_one_that_never_started() {
    let (port, accepted) = a_server_that_never_answers();
    let start_timeout = Duration::from_secs(1);
    let job = Job::Identify(RepositoryAccess {
        repository: format!("rest:http://127.0.0.1:{port}/"),
        password: SecretValue::new("demo-password"),
        options: Default::default(),
        secrets: Vec::new(),
        id: None,
    });
    let RunningJob {
        mut events,
        mut control,
    } = engine()
        // A proxy in the environment would take the call meant for the
        // server on loopback.
        .with_environment(std::env::vars_os().filter(|(name, _)| {
            !name
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("proxy")
        }))
        .with_start_timeout(start_timeout)
        .run(op(), job)
        .await
        .unwrap();

    // The worker is in the open when it has called the server.
    let mut waited = 0;
    while accepted.load(Ordering::SeqCst) == 0 && waited < 400 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += 1;
    }
    assert!(
        accepted.load(Ordering::SeqCst) > 0,
        "the worker never called the server"
    );

    // Well past the start timeout, which counts from the job's write.
    let next = tokio::time::timeout(start_timeout * 3, events.next()).await;
    assert!(
        next.is_err(),
        "the job was ended or reported while its repository was still opening: {next:?}"
    );
    tokio::time::timeout(Duration::from_secs(30), control.stop())
        .await
        .expect("stopping the worker hung");
}
