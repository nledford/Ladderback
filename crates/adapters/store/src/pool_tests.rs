//! What only a test inside the crate can reach: the store's own connections.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::task::{Context, Waker};
use std::time::Duration;

use app::ports::{OperationDetail, OperationRecord, OperationStore, Start};
use domain::execution::{
    Operation, OperationKind, OperationStatus, Target, Trigger, Warning, WarningKind,
};
use domain::{OperationId, RepositoryId, Timestamp, Uuid};
use tokio::time::Instant;

use super::{ACQUIRE_TIMEOUT, BUSY_TIMEOUT, MAX_CONNECTIONS, SqliteStore};

async fn scratch_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let store = SqliteStore::open(&dir.path().join("ladderback.db"))
        .await
        .expect("the scratch database opens");
    (dir, store)
}

/// A running backup that has recorded `warnings` warnings.
fn running_with_warnings(n: u128, warnings: usize) -> Operation {
    let mut op = Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(n)),
        OperationKind::Backup,
        Trigger::Manual,
        Target::default(),
        Timestamp::constant(1_800_000_000, 0),
    );
    op.start(Timestamp::constant(1_800_000_010, 0))
        .expect("a queued operation starts");
    for item in 0..warnings {
        op.record_warning(Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some(format!("/home/demo/file-{item}")),
            during: "backup".to_owned(),
            detail: "permission denied".to_owned(),
        })
        .expect("a running operation records warnings");
    }
    op
}

async fn warning_rows(store: &SqliteStore) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM op_warnings")
        .fetch_one(&store.pool)
        .await
        .expect("a count")
}

#[tokio::test]
async fn a_warning_for_an_operation_that_is_not_there_is_refused() {
    let (_dir, store) = scratch_store().await;

    let orphan = sqlx::query(
        "INSERT INTO op_warnings (op_id, item, during, code, detail)
         VALUES (?1, NULL, 'backup', 'unknown', 'detail')",
    )
    .bind("00000000-0000-0000-0000-00000000dead")
    .execute(&store.pool)
    .await;

    assert!(
        orphan.is_err(),
        "the store's connections enforce foreign keys"
    );
    assert_eq!(warning_rows(&store).await, 0);
}

#[tokio::test]
async fn deleting_an_operation_deletes_its_warnings_and_only_its_own() {
    let (_dir, store) = scratch_store().await;
    let doomed = running_with_warnings(0x10, 3);
    let kept = running_with_warnings(0x11, 2);
    for op in [&doomed, &kept] {
        let saved = store.save(op, None).await;
        assert_eq!(saved, Ok(()));
    }
    assert_eq!(warning_rows(&store).await, 5);

    sqlx::query("DELETE FROM operations WHERE id = ?1")
        .bind(doomed.id().as_uuid().to_string())
        .execute(&store.pool)
        .await
        .expect("the operation is deleted");

    assert_eq!(
        warning_rows(&store).await,
        2,
        "only the kept operation's remain"
    );
    let found = store.get(kept.id()).await.expect("reading works");
    assert_eq!(found.map(|detail| detail.warnings.len()), Some(2));
}

/// The demo repository, which one maintenance operation at a time holds.
fn demo_repository() -> RepositoryId {
    RepositoryId::parse(&"ab".repeat(32)).expect("a valid repository ID")
}

/// A queued manual operation of `kind` on the demo repository.
fn queued_on_the_repository(n: u128, kind: OperationKind) -> Operation {
    Operation::queue(
        OperationId::from_uuid(Uuid::from_u128(n)),
        kind,
        Trigger::Manual,
        Target {
            plan: None,
            repository: Some(demo_repository()),
        },
        Timestamp::constant(1_800_000_000, 0),
    )
}

fn started(mut op: Operation) -> Operation {
    op.start(Timestamp::constant(1_800_000_010, 0))
        .expect("a queued operation starts");
    op
}

/// More polls than any call of the store's needs, a moment apart.
const MOST_POLLS: usize = 200;
const BETWEEN_POLLS: Duration = Duration::from_millis(20);

/// Holds the next commit on any of the store's connections, once armed,
/// until the test lets it through. SQLite runs the commit hook inside the
/// `COMMIT`, on the connection's own thread, so the call that sent the
/// `COMMIT` can't hear its answer while the hook waits.
struct CommitGate {
    armed: Arc<AtomicBool>,
    reached: mpsc::Receiver<()>,
    open: mpsc::Sender<()>,
}

impl CommitGate {
    /// Hooks every connection the store's pool can hand out: it takes all
    /// of them at once, so the pool opens each, and gives them back.
    async fn on_every_connection_of(store: &SqliteStore) -> Self {
        let armed = Arc::new(AtomicBool::new(false));
        let (reached_by_hook, reached) = mpsc::channel();
        let (open, opened) = mpsc::channel();
        let opened = Arc::new(Mutex::new(opened));
        let mut taken = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            let mut conn = store.pool.acquire().await.expect("a connection");
            let armed = Arc::clone(&armed);
            let reached = reached_by_hook.clone();
            let opened = Arc::clone(&opened);
            conn.lock_handle()
                .await
                .expect("the connection's handle")
                .set_commit_hook(move || {
                    if armed.swap(false, Ordering::SeqCst) {
                        // A test that gave up has dropped both ends: the
                        // commit goes ahead.
                        let _ = reached.send(());
                        let _ = opened.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    }
                    true
                });
            taken.push(conn);
        }
        Self {
            armed,
            reached,
            open,
        }
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Whether a commit waits at the gate.
    fn reached(&self) -> bool {
        self.reached.try_recv().is_ok()
    }

    fn let_the_commit_through(&self) {
        self.open.send(()).expect("the commit waits at the gate");
    }
}

/// A start cut after SQLite committed it, before the call heard so, as a
/// run's caller cuts the call that would hold it up: the start is stored
/// whole, and its operation holds the repository (I7), with no one to run it
/// unless its caller never drops a start (§16, decision 42; #135).
///
/// The store's integration tests cut a start at its transaction's start and
/// after each number of polls, but where those cuts land depends on the
/// threads, and in WAL mode no lock another connection holds keeps a commit
/// back. Only a hook on the store's own connection holds the commit, so this
/// test is here, where it reaches them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_cut_after_its_commit_holds_the_repository() {
    let (_dir, store) = scratch_store().await;
    let queued = queued_on_the_repository(1, OperationKind::Prune);
    assert_eq!(store.save(&queued, None).await, Ok(()));
    let gate = CommitGate::on_every_connection_of(&store).await;

    let running = started(queued);
    let mut start = Box::pin(store.try_start(&running));
    let mut cx = Context::from_waker(Waker::noop());
    gate.arm();
    let mut polls = 0;
    while !gate.reached() {
        assert!(
            start.as_mut().poll(&mut cx).is_pending(),
            "the start ended before its commit"
        );
        polls += 1;
        assert!(
            polls < MOST_POLLS,
            "the start didn't commit within {MOST_POLLS} polls"
        );
        tokio::time::sleep(BETWEEN_POLLS).await;
    }

    // The commit runs, and the call, not polled since, can't have heard it:
    // once another connection sees the start, the cut comes after the commit.
    gate.let_the_commit_through();
    let committed = Instant::now() + Duration::from_secs(3);
    loop {
        let seen = store.get(running.id()).await.expect("the log reads");
        if seen.is_some_and(|detail| detail.record.status == OperationStatus::Running) {
            break;
        }
        assert!(Instant::now() < committed, "the start never committed");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(start);

    let rival = started(queued_on_the_repository(2, OperationKind::Check));
    let answer = tokio::time::timeout(Duration::from_secs(3), store.try_start(&rival))
        .await
        .expect("the rival's start lands without waiting for the write lock");
    assert_eq!(
        answer,
        Ok(Start::Refused {
            running: running.id()
        }),
        "the cut start holds the repository"
    );
    let kept = store.get(running.id()).await.expect("the log reads");
    assert_eq!(
        kept,
        Some(OperationDetail {
            record: OperationRecord::of(&running, None),
            warnings: Vec::new(),
        }),
        "the cut start is stored whole"
    );
}

/// A call made before `close` that is still waiting for one of the pool's
/// connections is let finish rather than refused: the store waits for the
/// calls it has begun, not only for the connections in use (#174). The pool
/// alone would refuse it once closing began. The test holds every
/// connection itself, so the save waits for one whatever the pool's size.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_lets_a_call_still_waiting_for_a_connection_finish() {
    let (dir, store) = scratch_store().await;
    let store = Arc::new(store);
    let mut taken = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        taken.push(store.pool.acquire().await.expect("a connection"));
    }
    let queued = queued_on_the_repository(1, OperationKind::Backup);
    let saving = tokio::spawn({
        let store = Arc::clone(&store);
        let queued = queued.clone();
        async move { store.save(&queued, None).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !saving.is_finished(),
        "the save ended while every connection was taken"
    );

    let mut closing = Box::pin(store.close());
    let early = tokio::time::timeout(Duration::from_millis(300), &mut closing).await;
    assert!(early.is_err(), "close returned with a save in flight");
    drop(taken);
    // The save's own bound: a connection, then the write lock, plus slack.
    let bound = ACQUIRE_TIMEOUT + BUSY_TIMEOUT + Duration::from_secs(1);
    tokio::time::timeout(bound, closing)
        .await
        .expect("close returns once the save has ended");
    let saved = tokio::time::timeout(bound, saving)
        .await
        .expect("the save ends")
        .expect("the save's task ends");
    assert_eq!(saved, Ok(()), "the save made before close");

    let reopened = SqliteStore::open(&dir.path().join("ladderback.db"))
        .await
        .expect("the database opens again");
    let kept = reopened.get(queued.id()).await.expect("the log reads");
    assert_eq!(
        kept.map(|detail| detail.record),
        Some(OperationRecord::of(&queued, None)),
        "the save made before close is stored"
    );
}
