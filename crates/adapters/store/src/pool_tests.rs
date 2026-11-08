//! What only a test inside the crate can reach: the store's own connections.

use app::ports::OperationStore;
use domain::execution::{Operation, OperationKind, Target, Trigger, Warning, WarningKind};
use domain::{OperationId, Timestamp, Uuid};

use super::SqliteStore;

async fn scratch_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let store = SqliteStore::open(&dir.path().join("steading.db"))
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
