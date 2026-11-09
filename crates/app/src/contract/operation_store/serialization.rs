//! The operation store's contract for I7: maintenance operations on one
//! repository run one at a time. These cases run last in the suite. Each uses
//! repositories and operation IDs of its own, so none depends on what another
//! left in the log.

use domain::OperationId;
use domain::RepositoryId;
use domain::execution::OperationKind::{Backup, Check, Fake, Identify, Prune};
use domain::execution::{FailureReason, Operation, OperationKind, Target, Trigger};
use futures::future::join_all;

use super::{
    Fixtures, applied, at, failed, id, listing, read, save, started, succeeded, unreadable,
};
use crate::ports::{OperationDetail, OperationRecord, OperationStore, Start, StoreError};

pub(super) async fn run(store: &dyn OperationStore, fx: &Fixtures) {
    a_second_maintenance_operation_is_refused_and_names_the_one_running(store).await;
    maintenance_on_another_repository_is_not_held_up(store).await;
    other_operations_neither_hold_a_repository_nor_wait_for_one(store).await;
    a_repository_is_free_again_once_its_operation_has_ended(store).await;
    a_queued_maintenance_operation_holds_nothing(store).await;
    a_refused_start_leaves_the_log_as_it_was(store).await;
    the_operation_that_holds_a_repository_may_start_again(store).await;
    only_a_running_operation_can_be_started(store).await;
    save_refuses_a_second_running_maintenance_operation(store).await;
    maintenance_that_names_no_repository_is_never_refused(store, fx).await;
    starts_racing_for_a_repository_let_exactly_one_win(store).await;
}

/// The repository numbered `n`: 64 hex digits, `n`'s two digits over again.
fn numbered(n: u8) -> Option<RepositoryId> {
    let parsed = RepositoryId::parse(&format!("{n:02x}").repeat(32));
    assert!(
        parsed.is_ok(),
        "the suite's own repository IDs are valid: {parsed:?}"
    );
    parsed.ok()
}

/// A queued operation of `kind` on `repository`, or on none.
fn on(n: u128, kind: OperationKind, repository: Option<&RepositoryId>) -> Operation {
    let target = Target {
        plan: None,
        repository: repository.cloned(),
    };
    Operation::queue(id(n), kind, Trigger::Manual, target, at(0))
}

fn succeeded_with_a_warning(op: Operation) -> Operation {
    let mut op = started(op);
    applied(op.record_warning(unreadable("/home/demo/a")));
    applied(op.succeed(at(70)));
    op
}

fn cancelled(op: Operation) -> Operation {
    let mut op = started(op);
    applied(op.cancel(at(40)));
    op
}

/// What `try_start` decided. A store that can't answer fails the suite.
async fn start(store: &dyn OperationStore, op: &Operation) -> Option<Start> {
    let decided = store.try_start(op).await;
    assert!(
        decided.is_ok(),
        "starting operation {} failed: {decided:?}",
        op.id().as_uuid()
    );
    decided.ok()
}

/// What a start decides when `running` already holds the repository.
fn refused(running: OperationId) -> Option<Start> {
    Some(Start::Refused { running })
}

/// What the log holds for `op`: its record and its warnings.
fn stored(op: &Operation) -> OperationDetail {
    OperationDetail {
        record: OperationRecord::of(op, None),
        warnings: op.warnings().to_vec(),
    }
}

async fn a_second_maintenance_operation_is_refused_and_names_the_one_running(
    store: &dyn OperationStore,
) {
    let pairs = [
        (Prune, Prune),
        (Prune, Check),
        (Check, Prune),
        (Check, Check),
    ];
    for (pair, (first_kind, second_kind)) in (0_u8..).zip(pairs) {
        let Some(repository) = numbered(0x10 + pair) else {
            return;
        };
        let n = 0x2100 + 0x10 * u128::from(pair);
        let running = started(on(n, first_kind, Some(&repository)));
        let second = started(on(n + 1, second_kind, Some(&repository)));

        assert_eq!(
            start(store, &running).await,
            Some(Start::Started),
            "a {first_kind:?} starts on a free repository"
        );
        assert_eq!(
            start(store, &second).await,
            refused(running.id()),
            "a {second_kind:?} is refused while a {first_kind:?} runs"
        );
        assert_eq!(
            read(store, second.id()).await,
            None,
            "a refused start stores nothing"
        );
        assert_eq!(
            read(store, running.id()).await,
            Some(stored(&running)),
            "the operation that runs is as it was"
        );
        assert_eq!(
            start(store, &second).await,
            refused(running.id()),
            "asking again changes nothing"
        );
    }
}

async fn maintenance_on_another_repository_is_not_held_up(store: &dyn OperationStore) {
    let (Some(nas), Some(laptop)) = (numbered(0x20), numbered(0x21)) else {
        return;
    };
    let on_the_nas = started(on(0x2200, Prune, Some(&nas)));
    let on_the_laptop = started(on(0x2201, Prune, Some(&laptop)));
    let check_on_the_laptop = started(on(0x2202, Check, Some(&laptop)));

    assert_eq!(start(store, &on_the_nas).await, Some(Start::Started));
    assert_eq!(
        start(store, &on_the_laptop).await,
        Some(Start::Started),
        "another repository is another matter"
    );
    assert_eq!(
        start(store, &check_on_the_laptop).await,
        refused(on_the_laptop.id()),
        "each repository holds its own, and the refusal names that one"
    );
}

async fn other_operations_neither_hold_a_repository_nor_wait_for_one(store: &dyn OperationStore) {
    // Beside a prune, nothing else is held up.
    let Some(repository) = numbered(0x30) else {
        return;
    };
    let prune = started(on(0x2300, Prune, Some(&repository)));
    assert_eq!(start(store, &prune).await, Some(Start::Started));
    let beside = [
        (0x2301, Fake),
        (0x2302, Identify),
        (0x2303, Backup),
        (0x2304, Backup),
    ];
    for (n, kind) in beside {
        let op = started(on(n, kind, Some(&repository)));
        assert_eq!(
            start(store, &op).await,
            Some(Start::Started),
            "a {kind:?} runs beside a prune"
        );
        assert_eq!(
            read(store, op.id()).await,
            Some(stored(&op)),
            "and is stored as running"
        );
    }

    // And maintenance isn't held up by them.
    let Some(busy) = numbered(0x31) else {
        return;
    };
    let running = [
        (0x2310, Fake),
        (0x2311, Identify),
        (0x2312, Backup),
        (0x2313, Backup),
    ];
    for (n, kind) in running {
        let op = started(on(n, kind, Some(&busy)));
        assert_eq!(start(store, &op).await, Some(Start::Started));
    }
    let check = started(on(0x2314, Check, Some(&busy)));
    assert_eq!(
        start(store, &check).await,
        Some(Start::Started),
        "a check isn't held up by a backup, an identify or a diagnostic job"
    );
    let prune = started(on(0x2315, Prune, Some(&busy)));
    assert_eq!(
        start(store, &prune).await,
        refused(check.id()),
        "the refusal names the check, not one of the others"
    );
}

async fn a_repository_is_free_again_once_its_operation_has_ended(store: &dyn OperationStore) {
    type Ending = fn(Operation) -> Operation;
    let endings: [(&str, Ending); 4] = [
        ("succeeded", succeeded),
        ("succeeded with warnings", succeeded_with_a_warning),
        ("failed", |op| {
            failed(op, FailureReason::EngineExited { exit_code: Some(1) })
        }),
        ("was cancelled", cancelled),
    ];
    for (number, (how, end)) in (0_u8..).zip(endings) {
        let Some(repository) = numbered(0x40 + number) else {
            return;
        };
        let n = 0x2400 + 0x10 * u128::from(number);
        let prune = on(n, Prune, Some(&repository));
        let running = started(prune.clone());
        let check = started(on(n + 1, Check, Some(&repository)));

        assert_eq!(start(store, &running).await, Some(Start::Started));
        assert_eq!(
            start(store, &check).await,
            refused(running.id()),
            "refused while the prune runs"
        );
        save(store, &end(prune), None).await;
        assert_eq!(
            start(store, &check).await,
            Some(Start::Started),
            "the repository is free once the prune {how}"
        );
    }
}

async fn a_queued_maintenance_operation_holds_nothing(store: &dyn OperationStore) {
    let Some(repository) = numbered(0x50) else {
        return;
    };
    let waiting = on(0x2500, Prune, Some(&repository));
    save(store, &waiting, None).await;
    let check = started(on(0x2501, Check, Some(&repository)));
    assert_eq!(
        start(store, &check).await,
        Some(Start::Started),
        "a queued prune holds nothing"
    );

    // The queued prune can't start now, and stays as it was queued.
    assert_eq!(
        start(store, &started(waiting.clone())).await,
        refused(check.id())
    );
    assert_eq!(
        read(store, waiting.id()).await,
        Some(stored(&waiting)),
        "a refused operation stays queued"
    );
}

async fn a_refused_start_leaves_the_log_as_it_was(store: &dyn OperationStore) {
    let Some(repository) = numbered(0x51) else {
        return;
    };
    let mut holder = started(on(0x2600, Prune, Some(&repository)));
    applied(holder.record_warning(unreadable("/home/demo/holder")));
    assert_eq!(start(store, &holder).await, Some(Start::Started));

    let mut loser = started(on(0x2601, Check, Some(&repository)));
    for n in 0..3 {
        applied(loser.record_warning(unreadable(&format!("/home/demo/loser-{n}"))));
    }
    assert_eq!(start(store, &loser).await, refused(holder.id()));

    assert_eq!(read(store, loser.id()).await, None);
    assert_eq!(
        read(store, holder.id()).await,
        Some(stored(&holder)),
        "the holder's record and warnings are as they were"
    );
    let listed = listing(store, None, 50).await;
    assert!(
        listed.iter().all(|record| record.id != loser.id()),
        "history doesn't list a refused start"
    );
}

async fn the_operation_that_holds_a_repository_may_start_again(store: &dyn OperationStore) {
    let Some(repository) = numbered(0x52) else {
        return;
    };
    let mut holder = started(on(0x2700, Prune, Some(&repository)));
    assert_eq!(start(store, &holder).await, Some(Start::Started));

    applied(holder.record_warning(unreadable("/home/demo/a")));
    assert_eq!(
        start(store, &holder).await,
        Some(Start::Started),
        "the holder is not a second operation"
    );
    assert_eq!(
        read(store, holder.id()).await,
        Some(stored(&holder)),
        "what it stored last is what is stored"
    );

    let other = started(on(0x2701, Check, Some(&repository)));
    assert_eq!(
        start(store, &other).await,
        refused(holder.id()),
        "and it still holds the repository"
    );
}

async fn only_a_running_operation_can_be_started(store: &dyn OperationStore) {
    let Some(repository) = numbered(0x53) else {
        return;
    };
    for (number, kind) in (0_u128..).zip([Prune, Backup]) {
        let n = 0x2800 + 0x10 * number;
        let on_it = |step: u128| on(n + step, kind, Some(&repository));
        let ops = [
            ("queued", on_it(0)),
            ("succeeded", succeeded(on_it(1))),
            (
                "succeeded with warnings",
                succeeded_with_a_warning(on_it(2)),
            ),
            (
                "failed",
                failed(on_it(3), FailureReason::EngineExited { exit_code: Some(1) }),
            ),
            ("cancelled", cancelled(on_it(4))),
        ];
        for (state, op) in ops {
            let decided = store.try_start(&op).await;
            assert!(
                matches!(decided, Err(StoreError::Unavailable { .. })),
                "a {kind:?} that is {state} can't be started: {decided:?}"
            );
            assert_eq!(read(store, op.id()).await, None, "and none of it is stored");
        }
    }

    // What is stored stays as it was stored, and nothing was held meanwhile.
    let waiting = on(0x2850, Prune, Some(&repository));
    save(store, &waiting, None).await;
    let decided = store.try_start(&waiting).await;
    assert!(
        matches!(decided, Err(StoreError::Unavailable { .. })),
        "a queued prune can't be started: {decided:?}"
    );
    assert_eq!(read(store, waiting.id()).await, Some(stored(&waiting)));
    let prune = started(on(0x2851, Prune, Some(&repository)));
    assert_eq!(
        start(store, &prune).await,
        Some(Start::Started),
        "starts that were refused as not running held nothing"
    );
}

async fn save_refuses_a_second_running_maintenance_operation(store: &dyn OperationStore) {
    let (Some(repository), Some(other)) = (numbered(0x54), numbered(0x55)) else {
        return;
    };

    // Saving a running maintenance operation on a free repository is fine,
    // and holds it as a start does.
    let saved = started(on(0x2910, Prune, Some(&other)));
    save(store, &saved, None).await;
    assert_eq!(
        start(store, &started(on(0x2911, Check, Some(&other)))).await,
        refused(saved.id()),
        "a running operation that was saved holds its repository"
    );

    let holder = started(on(0x2900, Prune, Some(&repository)));
    assert_eq!(start(store, &holder).await, Some(Start::Started));

    // A second operation may be saved while it waits in the queue...
    let waiting = on(0x2901, Check, Some(&repository));
    save(store, &waiting, None).await;
    // ...and is refused when saved as running, so it stays as it was queued.
    let mut running = started(waiting.clone());
    applied(running.record_warning(unreadable("/home/demo/a")));
    let refused_save = store.save(&running, None).await;
    assert!(
        matches!(refused_save, Err(StoreError::Unavailable { .. })),
        "a second running maintenance operation isn't saved: {refused_save:?}"
    );
    assert_eq!(
        read(store, waiting.id()).await,
        Some(stored(&waiting)),
        "the queued one is as it was, warnings included"
    );

    // Nor is one that was never saved before.
    let newcomer = started(on(0x2902, Prune, Some(&repository)));
    let refused_save = store.save(&newcomer, None).await;
    assert!(
        matches!(refused_save, Err(StoreError::Unavailable { .. })),
        "a first save is refused the same way: {refused_save:?}"
    );
    assert_eq!(read(store, newcomer.id()).await, None);

    // A backup is no maintenance, and the holder saves as often as it likes.
    let backup = started(on(0x2903, Backup, Some(&repository)));
    save(store, &backup, None).await;
    assert_eq!(read(store, backup.id()).await, Some(stored(&backup)));
    let mut progressing = holder.clone();
    applied(progressing.record_warning(unreadable("/home/demo/b")));
    save(store, &progressing, None).await;
    assert_eq!(read(store, holder.id()).await, Some(stored(&progressing)));

    // Once the holder has ended, the save that was refused is accepted.
    save(
        store,
        &succeeded(on(0x2900, Prune, Some(&repository))),
        None,
    )
    .await;
    save(store, &running, None).await;
    assert_eq!(read(store, running.id()).await, Some(stored(&running)));
}

async fn maintenance_that_names_no_repository_is_never_refused(
    store: &dyn OperationStore,
    fx: &Fixtures,
) {
    let on_no_repository = |n: u128, kind: OperationKind, plan: bool| {
        let target = Target {
            plan: plan.then(|| fx.home.clone()),
            repository: None,
        };
        started(Operation::queue(
            id(n),
            kind,
            Trigger::Manual,
            target,
            at(0),
        ))
    };
    let ops = [
        on_no_repository(0x2a00, Prune, false),
        on_no_repository(0x2a01, Check, false),
        on_no_repository(0x2a02, Prune, true),
        on_no_repository(0x2a03, Check, true),
    ];
    for op in &ops {
        assert_eq!(
            start(store, op).await,
            Some(Start::Started),
            "an operation that names no repository holds none"
        );
        assert_eq!(read(store, op.id()).await, Some(stored(op)));
    }
}

async fn starts_racing_for_a_repository_let_exactly_one_win(store: &dyn OperationStore) {
    const PER_REPOSITORY: usize = 4;
    let repositories: Option<Vec<RepositoryId>> = (0x60..0x63).map(numbered).collect();
    let Some(repositories) = repositories else {
        return;
    };
    let mut racers = Vec::new();
    let mut n = 0x2b00_u128;
    for repository in &repositories {
        for kind in [Prune, Check, Prune, Check] {
            racers.push(started(on(n, kind, Some(repository))));
            n += 1;
        }
    }

    let decided = join_all(racers.iter().map(|op| store.try_start(op))).await;

    for (group, decisions) in racers
        .chunks(PER_REPOSITORY)
        .zip(decided.chunks(PER_REPOSITORY))
    {
        let winners: Vec<OperationId> = group
            .iter()
            .zip(decisions)
            .filter(|(_, decision)| **decision == Ok(Start::Started))
            .map(|(op, _)| op.id())
            .collect();
        assert_eq!(
            winners.len(),
            1,
            "exactly one of the starts racing for a repository wins: {decisions:?}"
        );
        let [winner] = winners.as_slice() else {
            return;
        };
        for (op, decision) in group.iter().zip(decisions) {
            if op.id() == *winner {
                assert_eq!(
                    read(store, op.id()).await,
                    Some(stored(op)),
                    "the winner is stored as running"
                );
            } else {
                assert_eq!(
                    decision,
                    &Ok(Start::Refused { running: *winner }),
                    "a loser is refused, naming the winner"
                );
                assert_eq!(
                    read(store, op.id()).await,
                    None,
                    "and a loser stores nothing"
                );
            }
        }
    }
}
