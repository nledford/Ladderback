//! The deadline of a repository job through the real worker (#98): a test,
//! connection or initialisation that reaches a backend that never answers is
//! refused when its time is up, and its worker is killed. The clock is a
//! fake one that the test moves once the worker is really stuck in the
//! backend, so the two minutes the daemon gives a job cost none here.

mod support;

use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use app::fakes::{FakeClock, FakeSecrets};
use app::repositories::{RepositoryError, TestRepository};
use app::secrets::ResolveRepository;
use domain::Timestamp;
use domain::execution::deadline::REPOSITORY_JOB_BUDGET;
use domain::planning::RepositorySettings;
use domain::secrets::{SecretRef, SecretValue};
use support::{engine, op};

/// How many connections a server has taken and how many of them have ended.
struct Connections {
    accepted: AtomicUsize,
    ended: AtomicUsize,
}

/// A server that takes every connection and never answers one, and notices
/// when a connection ends.
fn a_server_that_never_answers() -> (u16, Arc<Connections>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(Connections {
        accepted: AtomicUsize::new(0),
        ended: AtomicUsize::new(0),
    });
    let seen = Arc::clone(&connections);
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            seen.accepted.fetch_add(1, Ordering::SeqCst);
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                // Whatever the client sends is read and dropped; the
                // connection ends when the client's side does.
                let mut sink = [0u8; 1024];
                while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
                seen.ended.fetch_add(1, Ordering::SeqCst);
            });
        }
    });
    (port, connections)
}

/// Polls `until` for up to `seconds`.
async fn within(seconds: u64, until: impl Fn() -> bool) -> bool {
    for _ in 0..seconds * 20 {
        if until() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    until()
}

#[tokio::test]
async fn a_real_worker_stuck_in_a_backend_that_never_answers_is_killed_when_the_time_is_up() {
    let (port, connections) = a_server_that_never_answers();
    let clock = Arc::new(FakeClock::new(
        Timestamp::from_second(1_800_000_000).unwrap(),
    ));
    let use_case = TestRepository::new(
        clock.clone(),
        Arc::new(
            engine()
                // A proxy in the environment would take the call meant for
                // the server on loopback.
                .with_environment(std::env::vars_os().filter(|(name, _)| {
                    !name
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("proxy")
                })),
        ),
        ResolveRepository::new(Arc::new(FakeSecrets::new())),
    );
    let settings = RepositorySettings {
        repository: format!("rest:http://127.0.0.1:{port}/"),
        repo_hot: None,
        password: SecretRef::Inline(SecretValue::new("demo-password")),
        options: Default::default(),
        options_hot: Default::default(),
        options_cold: Default::default(),
    };

    // Time moves only once the worker has called the server: before that it
    // would be refused for a repository it never tried.
    let driver = async {
        assert!(
            within(30, || connections.accepted.load(Ordering::SeqCst) > 0).await,
            "the worker never called the server"
        );
        assert!(
            within(5, || clock.sleepers() > 0).await,
            "the use case never waited on the clock"
        );
        clock.advance(REPOSITORY_JOB_BUDGET);
    };
    let (answer, ()) = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(use_case.test(op(), &settings), driver)
    })
    .await
    .expect("the use case waited for ever");
    assert_eq!(answer, Err(RepositoryError::TimedOut));

    // A worker that was left running would keep its connections open: the
    // server's own backend is silent, so nothing else ends them.
    assert!(
        within(10, || {
            let ended = connections.ended.load(Ordering::SeqCst);
            ended > 0 && ended == connections.accepted.load(Ordering::SeqCst)
        })
        .await,
        "the worker's connections stayed open: {} accepted, {} ended",
        connections.accepted.load(Ordering::SeqCst),
        connections.ended.load(Ordering::SeqCst)
    );
}
