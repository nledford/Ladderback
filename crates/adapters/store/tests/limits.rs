//! Boundaries a real database has to hold: history past the page cap, one
//! plan's history among the others', and a count beyond what a column holds.

mod support;

use app::ports::{HistoryQuery, MAX_PAGE_SIZE, OperationStore, PageSize};
use domain::execution::{Operation, OperationKind, Target, Trigger};
use serde_json::json;
use support::*;

#[tokio::test]
async fn history_pages_past_the_page_cap_with_nothing_lost_or_repeated() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    for n in 0..250_u128 {
        save(&store, &queued_backup(0x1000 + n), None).await;
    }

    // Asking for more than the cap gets the cap.
    let query = HistoryQuery {
        limit: PageSize::new(usize::MAX),
        ..HistoryQuery::default()
    };
    let first = store.list(&query).await.expect("listing works");
    assert_eq!(first.operations.len(), MAX_PAGE_SIZE);
    assert_eq!(first.next, first.operations.last().map(|record| record.id));

    let rest = HistoryQuery {
        after: first.next,
        ..query
    };
    let second = store.list(&rest).await.expect("listing works");
    assert_eq!(second.operations.len(), 250 - MAX_PAGE_SIZE);
    assert_eq!(second.next, None);

    let listed: Vec<_> = first
        .operations
        .iter()
        .chain(&second.operations)
        .map(|record| record.id)
        .collect();
    let expected: Vec<_> = (0..250_u128).rev().map(|n| id(0x1000 + n)).collect();
    assert_eq!(listed, expected, "newest first, each operation once");
}

#[tokio::test]
async fn a_plans_history_pages_through_the_other_plans_operations() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    let plans = ["demo-home", "demo-photos", "demo-docs"];
    // 120 operations, interleaved across the plans; every fifth belongs to no
    // plan at all.
    let plan_of = |n: u128| -> Option<usize> {
        (n % 5 != 4).then(|| usize::try_from(n % 3).expect("a small number"))
    };
    for n in 0..120_u128 {
        let target = match plan_of(n) {
            Some(p) => backup_target(plans[p]),
            None => Target {
                plan: None,
                repository: Some(repository()),
            },
        };
        let op = queued(0x2000 + n, OperationKind::Backup, Trigger::Schedule, target);
        save(&store, &op, None).await;
    }

    for (p, slug) in plans.iter().enumerate() {
        let expected: Vec<_> = (0..120_u128)
            .rev()
            .filter(|n| plan_of(*n) == Some(p))
            .map(|n| id(0x2000 + n))
            .collect();
        let mut listed = Vec::new();
        let mut after = None;
        let mut pages = 0;
        loop {
            let query = HistoryQuery {
                plan: Some(plan(slug)),
                after,
                limit: PageSize::new(7),
            };
            let page = store.list(&query).await.expect("listing works");
            assert!(page.operations.len() <= 7, "{slug}: a page holds at most 7");
            listed.extend(page.operations.iter().map(|record| record.id));
            pages += 1;
            assert!(pages <= 10, "{slug}: paging ends");
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        assert_eq!(
            listed, expected,
            "{slug}: its operations, newest first, only"
        );
    }
}

#[tokio::test]
async fn a_warning_count_past_what_the_column_holds_is_stored_as_the_most_it_holds() {
    let scratch = Scratch::new();
    let store = scratch.open().await;
    // An operation can't count this many warnings by recording them, so it
    // is made the way a message from another process could be.
    let op = with_warnings(started(queued_backup(0x3000)), 2);
    let mut forged = serde_json::to_value(&op).expect("an operation is JSON");
    forged["warning_count"] = json!(u64::MAX);
    let forged: Operation = serde_json::from_value(forged).expect("an operation from JSON");
    assert_eq!(forged.warning_count(), u64::MAX);

    save(&store, &forged, None).await;

    let found = store
        .get(id(0x3000))
        .await
        .expect("it reads back, not as a corrupt row")
        .expect("it is there");
    assert_eq!(found.record.warning_count, i64::MAX.unsigned_abs());
    assert_eq!(found.warnings.len(), 2);
    let page = store
        .list(&HistoryQuery::default())
        .await
        .expect("it lists too");
    assert_eq!(page.operations.len(), 1);
}
