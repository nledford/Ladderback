//! The `CommandLedger` contract (§16, decisions 120 and 176): the IDs of
//! the commands a machine took and the backups its hub started read back
//! as kept, across a restart; an ID is remembered once, for
//! [`COMMANDS_REMEMBERED_FOR`], and at most [`MAX_COMMANDS_REMEMBERED`] at
//! once.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use domain::fleet::{COMMANDS_REMEMBERED_FOR, CommandId, HubRun, MAX_COMMANDS_REMEMBERED};
use domain::{PlanId, SignedDuration, Timestamp, Uuid};

use crate::ports::{CommandLedger, Remembered};

/// Opens a ledger: a new, empty one, or the one before opened again on the
/// same place.
#[async_trait]
pub trait CommandLedgerHarness: Send + Sync {
    /// A ledger that holds nothing yet.
    async fn empty(&self) -> Arc<dyn CommandLedger>;
    /// The ledger [`empty`](Self::empty) last gave, opened again, as after
    /// a restart of the daemon.
    async fn reopened(&self) -> Arc<dyn CommandLedger>;
}

pub async fn run(harness: &dyn CommandLedgerHarness) {
    a_command_is_new_once_and_seen_after_a_restart(harness).await;
    a_command_is_forgotten_once_remembered_for_its_time(harness).await;
    a_full_ledger_remembers_no_more_and_says_since_when(harness).await;
    the_hubs_runs_outlive_a_restart_and_go_after_an_hour(harness).await;
}

/// The value of `result`, after a failed assertion if it is an error.
fn ok<T, E: fmt::Debug>(result: Result<T, E>, what: &str) -> Option<T> {
    let error = result.as_ref().err();
    assert!(error.is_none(), "{what}: {error:?}");
    result.ok()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap_or(Timestamp::UNIX_EPOCH)
}

fn command(number: u128) -> CommandId {
    CommandId::from_uuid(Uuid::from_u128((0x0190_u128 << 112) | number))
}

fn plan(name: &str) -> Option<PlanId> {
    ok(PlanId::parse(name), "a plan ID")
}

async fn a_command_is_new_once_and_seen_after_a_restart(harness: &dyn CommandLedgerHarness) {
    let ledger = harness.empty().await;
    let first = ledger.remember(command(1), at(0)).await;
    assert_eq!(ok(first, "the first take"), Some(Remembered::New));
    let again = ledger.remember(command(1), at(1)).await;
    assert_eq!(ok(again, "the same take"), Some(Remembered::Seen));
    let other = ledger.remember(command(2), at(1)).await;
    assert_eq!(ok(other, "another command"), Some(Remembered::New));
    let reopened = harness.reopened().await;
    let replayed = reopened.remember(command(1), at(2)).await;
    assert_eq!(
        ok(replayed, "a replay after a restart"),
        Some(Remembered::Seen)
    );
}

async fn a_command_is_forgotten_once_remembered_for_its_time(harness: &dyn CommandLedgerHarness) {
    let ledger = harness.empty().await;
    let _ = ok(ledger.remember(command(1), at(0)).await, "the first take");
    let life = COMMANDS_REMEMBERED_FOR.as_secs();
    let just_before = ledger.remember(command(1), at(life - 1)).await;
    assert_eq!(
        ok(just_before, "a second before its time"),
        Some(Remembered::Seen)
    );
    let at_its_time = ledger.remember(command(1), at(life)).await;
    assert_eq!(ok(at_its_time, "at its time"), Some(Remembered::New));
}

async fn a_full_ledger_remembers_no_more_and_says_since_when(harness: &dyn CommandLedgerHarness) {
    let ledger = harness.empty().await;
    let mut number = 0;
    for _ in 0..MAX_COMMANDS_REMEMBERED {
        number += 1;
        let seconds = i64::try_from(number).unwrap_or(i64::MAX);
        let taken = ledger.remember(command(number), at(seconds)).await;
        assert_eq!(ok(taken, "a take below the limit"), Some(Remembered::New));
    }
    let past = ledger.remember(command(0xffff), at(5_000)).await;
    assert_eq!(
        ok(past, "a take past the limit"),
        Some(Remembered::Full { oldest: at(1) })
    );
    let not_kept = harness
        .reopened()
        .await
        .remember(command(0xffff), at(5_001))
        .await;
    assert_eq!(
        ok(not_kept, "the refused take, again"),
        Some(Remembered::Full { oldest: at(1) }),
        "a refused ID isn't remembered"
    );
    let life = COMMANDS_REMEMBERED_FOR.as_secs();
    let room = ledger.remember(command(0xffff), at(life + 1)).await;
    assert_eq!(
        ok(room, "a take once the oldest is forgotten"),
        Some(Remembered::New)
    );
}

async fn the_hubs_runs_outlive_a_restart_and_go_after_an_hour(harness: &dyn CommandLedgerHarness) {
    let ledger = harness.empty().await;
    let (Some(photos), Some(music)) = (plan("photos"), plan("music")) else {
        return;
    };
    assert_eq!(ok(ledger.runs().await, "no runs"), Some(Vec::new()));
    let first = HubRun {
        plan: photos.clone(),
        at: at(0),
    };
    let second = HubRun {
        plan: music,
        at: at(600),
    };
    let _ = ok(ledger.ran(&first).await, "the first run");
    let _ = ok(ledger.ran(&second).await, "the second run");
    let kept = harness.reopened().await.runs().await;
    assert_eq!(
        ok(kept, "the runs after a restart"),
        Some(vec![first, second.clone()])
    );
    let hour = SignedDuration::from_hours(1).as_secs();
    let later = HubRun {
        plan: photos,
        at: at(hour),
    };
    let _ = ok(ledger.ran(&later).await, "a run an hour after the first");
    assert_eq!(
        ok(ledger.runs().await, "the runs an hour on"),
        Some(vec![second, later]),
        "the run an hour old is forgotten"
    );
}
