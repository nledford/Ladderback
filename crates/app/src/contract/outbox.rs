//! The `Outbox` and `ReportInbox` contracts (technical design §7.2): the
//! agent's reports, numbered in order as they enter and kept until the hub
//! acknowledges them, across restarts, a full outbox saying what it gave
//! up; and the hub's, each taken once however often it comes, its oldest
//! let go past the cap and counted.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use domain::fleet::{
    AgentReport, EpochSeconds, Happening, Ingest, MAX_EPOCHS_HELD, MAX_OUTBOX_REPORTS,
    MAX_REPORTS_HELD, OutboxEpoch, OutboxPosition, ReportSeq, SEND_WINDOW, WireName,
};
use domain::{AgentId, OperationId, Timestamp, Uuid};

use crate::ports::{Appended, LatestReports, Outbox, OutboxEntry, ReportInbox};

/// Opens an outbox: a new, empty one, or the one before opened again on
/// the same place.
#[async_trait]
pub trait OutboxHarness: Send + Sync {
    /// An outbox that holds nothing yet.
    async fn empty(&self) -> Arc<dyn Outbox>;
    /// The outbox [`empty`](Self::empty) last gave, opened again, as after
    /// a restart of the agent.
    async fn reopened(&self) -> Arc<dyn Outbox>;
}

/// Opens the hub's reports: new, or the ones before opened again.
#[async_trait]
pub trait ReportInboxHarness: Send + Sync {
    async fn empty(&self) -> Arc<dyn ReportInbox>;
    async fn reopened(&self) -> Arc<dyn ReportInbox>;
}

pub async fn run(harness: &dyn OutboxHarness) {
    nothing_waits_at_first(harness).await;
    reports_wait_in_the_order_they_entered(harness).await;
    an_ack_lets_go_of_the_reports_through_it_only(harness).await;
    a_place_is_never_given_twice(harness).await;
    an_ack_of_another_epoch_lets_go_of_nothing(harness).await;
    reports_and_their_places_outlive_the_agent(harness).await;
    racing_appends_each_get_a_place(harness).await;
    a_failure_keeps_its_code(harness).await;
    a_cancel_is_kept(harness).await;
    a_full_outbox_says_which_start_gave_way(harness).await;
    the_position_is_the_epoch_and_the_place_given_next(harness).await;
    a_new_epoch_keeps_the_places_and_the_reports_waiting(harness).await;
}

pub async fn run_inbox(harness: &dyn ReportInboxHarness) {
    a_report_is_taken_once(harness).await;
    a_cancel_is_taken(harness).await;
    a_gap_is_no_reason_to_refuse(harness).await;
    a_report_far_behind_the_last_taken_is_behind(harness).await;
    each_agent_and_epoch_counts_alone(harness).await;
    the_epoch_just_taken_is_kept_however_the_clock_moved(harness).await;
    taken_reports_outlive_the_hub(harness).await;
    racing_deliveries_are_taken_once(harness).await;
    past_the_cap_the_oldest_give_way_and_are_counted(harness).await;
    the_latest_reports_are_read_with_the_count_let_go_of(harness).await;
    an_operations_report_is_held_once_across_machines(harness).await;
    the_last_place_taken_is_read_for_each_agent_and_epoch(harness).await;
    a_report_unlike_the_one_taken_at_its_place_diverges(harness).await;
    a_diverged_mark_is_forgotten_with_its_epoch(harness).await;
    a_report_held_under_another_machine_id_is_sent_again(harness).await;
    live::the_latest_plans_are_kept_in_place_of_the_last(harness).await;
    live::the_latest_progress_is_kept_in_place_of_the_last(harness).await;
}

mod live;

/// The value of `result`, after a failed assertion if it is an error.
fn ok<T, E: fmt::Debug>(result: Result<T, E>, what: &str) -> Option<T> {
    let error = result.as_ref().err();
    assert!(error.is_none(), "{what}: {error:?}");
    result.ok()
}

fn op(n: u128) -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(n))
}

fn report(n: u128, happening: Happening) -> AgentReport {
    AgentReport {
        op_id: op(n),
        happening,
        at: EpochSeconds::from_seconds(1_800_000_000 + u64::try_from(n).unwrap_or(0)),
    }
}

/// The place `number`; every place a test names is above 0.
fn seq(number: u64) -> ReportSeq {
    ReportSeq::new(number).unwrap_or(ReportSeq::FIRST)
}

/// The place `report` was kept at, after a failed assertion if it wasn't
/// kept as an outbox with room keeps it.
async fn appended(outbox: &dyn Outbox, report: &AgentReport) -> Option<ReportSeq> {
    let appended = ok(outbox.append(report).await, "appending a report")?;
    let kept = match appended {
        Appended::Kept(seq) => Some(seq),
        Appended::KeptGivingUp { .. } | Appended::Refused => None,
    };
    assert!(kept.is_some(), "the report wasn't kept: {appended:?}");
    kept
}

async fn waiting(outbox: &dyn Outbox) -> Vec<OutboxEntry> {
    ok(
        outbox.unacknowledged(None, usize::MAX).await,
        "the reports waiting",
    )
    .unwrap_or_default()
}

fn places(entries: &[OutboxEntry]) -> Vec<u64> {
    entries.iter().map(|entry| entry.seq.get()).collect()
}

/// The epoch of the first of `entries`, after a failed assertion if there
/// is none.
fn epoch_of(entries: &[OutboxEntry]) -> Option<OutboxEpoch> {
    let epoch = entries.first().map(|entry| entry.epoch);
    assert!(epoch.is_some(), "no report waits");
    epoch
}

async fn acknowledge(outbox: &dyn Outbox, epoch: OutboxEpoch, through: u64) {
    ok(
        outbox.acknowledge(epoch, seq(through)).await,
        "acknowledging reports",
    );
}

async fn position(outbox: &dyn Outbox) -> Option<OutboxPosition> {
    ok(outbox.position().await, "the outbox's position").flatten()
}

async fn start_epoch(outbox: &dyn Outbox, from: OutboxEpoch) -> Option<OutboxEpoch> {
    ok(outbox.start_epoch(from).await, "starting a new epoch").flatten()
}

/// An outbox stands nowhere before its first report; after, at its epoch
/// and the place after the last it gave, whatever the hub acknowledged,
/// and there again once it is opened again (decision 177).
async fn the_position_is_the_epoch_and_the_place_given_next(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    assert_eq!(position(outbox.as_ref()).await, None);
    appended(outbox.as_ref(), &report(1, Happening::Started)).await;
    appended(outbox.as_ref(), &report(1, Happening::Succeeded)).await;
    let Some(epoch) = epoch_of(&waiting(outbox.as_ref()).await) else {
        return;
    };
    let at_three = Some(OutboxPosition {
        epoch,
        next: seq(3),
    });
    assert_eq!(position(outbox.as_ref()).await, at_three);
    acknowledge(outbox.as_ref(), epoch, 2).await;
    assert_eq!(position(outbox.as_ref()).await, at_three);
    drop(outbox);
    let outbox = harness.reopened().await;
    assert_eq!(position(outbox.as_ref()).await, at_three);
}

/// A new epoch replaces the one it is started from, and only that one:
/// the reports waiting wait under it at their places, the count goes on
/// where it stood, an acknowledgement of the old epoch lets go of nothing,
/// and it outlives the agent. Started again from the old epoch, as by an
/// agent that stopped before it took the hub's answer in, it changes
/// nothing. An outbox with no report yet starts none (decision 177).
async fn a_new_epoch_keeps_the_places_and_the_reports_waiting(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    let unknown = OutboxEpoch::from_uuid(Uuid::from_u128(0x0e));
    assert_eq!(start_epoch(outbox.as_ref(), unknown).await, None);
    for n in 1..=3 {
        appended(outbox.as_ref(), &report(n, Happening::Started)).await;
    }
    let Some(old) = epoch_of(&waiting(outbox.as_ref()).await) else {
        return;
    };
    acknowledge(outbox.as_ref(), old, 1).await;
    let before = waiting(outbox.as_ref()).await;
    let other = OutboxEpoch::from_uuid(Uuid::from_u128(old.as_uuid().as_u128() ^ 1));
    assert_eq!(
        start_epoch(outbox.as_ref(), other).await,
        None,
        "another epoch than the outbox's starts none"
    );
    let new = start_epoch(outbox.as_ref(), old).await;
    assert!(new.is_some_and(|new| new != old), "{new:?} after {old:?}");
    let Some(new) = new else {
        return;
    };
    let after = waiting(outbox.as_ref()).await;
    assert_eq!(places(&after), [2, 3]);
    assert!(after.iter().all(|entry| entry.epoch == new), "{after:?}");
    let said = |entries: &[OutboxEntry]| -> Vec<AgentReport> {
        entries.iter().map(|entry| entry.report.clone()).collect()
    };
    assert_eq!(said(&after), said(&before));
    assert_eq!(
        position(outbox.as_ref()).await,
        Some(OutboxPosition {
            epoch: new,
            next: seq(4)
        })
    );
    the_new_epoch_outlives_the_old(harness, outbox, old, new).await;
}

/// The rest of the test above, once `outbox` started `new` from `old` with
/// places 2 and 3 waiting.
async fn the_new_epoch_outlives_the_old(
    harness: &dyn OutboxHarness,
    outbox: Arc<dyn Outbox>,
    old: OutboxEpoch,
    new: OutboxEpoch,
) {
    acknowledge(outbox.as_ref(), old, 3).await;
    assert_eq!(places(&waiting(outbox.as_ref()).await), [2, 3]);
    assert_eq!(
        start_epoch(outbox.as_ref(), old).await,
        None,
        "the old epoch is gone"
    );
    drop(outbox);
    let outbox = harness.reopened().await;
    assert_eq!(
        appended(outbox.as_ref(), &report(4, Happening::Started)).await,
        Some(seq(4))
    );
    let reopened = waiting(outbox.as_ref()).await;
    assert_eq!(places(&reopened), [2, 3, 4]);
    assert!(
        reopened.iter().all(|entry| entry.epoch == new),
        "{reopened:?}"
    );
    acknowledge(outbox.as_ref(), new, 3).await;
    assert_eq!(places(&waiting(outbox.as_ref()).await), [4]);
}

async fn nothing_waits_at_first(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    assert!(waiting(outbox.as_ref()).await.is_empty());
}

async fn reports_wait_in_the_order_they_entered(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    let reports = [
        report(1, Happening::Started),
        report(1, Happening::Succeeded),
        report(2, Happening::Started),
    ];
    for (place, report) in (1..).zip(&reports) {
        assert_eq!(appended(outbox.as_ref(), report).await, Some(seq(place)));
    }
    let held = waiting(outbox.as_ref()).await;
    assert_eq!(places(&held), [1, 2, 3]);
    let said: Vec<AgentReport> = held.iter().map(|entry| entry.report.clone()).collect();
    assert_eq!(said, reports);
    let epoch = epoch_of(&held);
    assert!(held.iter().all(|entry| Some(entry.epoch) == epoch));
    let after = ok(
        outbox.unacknowledged(Some(seq(1)), 1).await,
        "the reports after the first",
    )
    .unwrap_or_default();
    assert_eq!(places(&after), [2], "after the first, at most one");
}

async fn an_ack_lets_go_of_the_reports_through_it_only(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    for n in 1..=4 {
        appended(outbox.as_ref(), &report(n, Happening::Started)).await;
    }
    let Some(epoch) = epoch_of(&waiting(outbox.as_ref()).await) else {
        return;
    };
    acknowledge(outbox.as_ref(), epoch, 2).await;
    assert_eq!(places(&waiting(outbox.as_ref()).await), [3, 4]);
    acknowledge(outbox.as_ref(), epoch, 1).await;
    assert_eq!(
        places(&waiting(outbox.as_ref()).await),
        [3, 4],
        "an older ack changes nothing"
    );
}

async fn a_place_is_never_given_twice(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    appended(outbox.as_ref(), &report(1, Happening::Started)).await;
    appended(outbox.as_ref(), &report(1, Happening::Succeeded)).await;
    let Some(epoch) = epoch_of(&waiting(outbox.as_ref()).await) else {
        return;
    };
    acknowledge(outbox.as_ref(), epoch, 2).await;
    let next = appended(outbox.as_ref(), &report(2, Happening::Started)).await;
    assert_eq!(next, Some(seq(3)));
}

async fn an_ack_of_another_epoch_lets_go_of_nothing(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    appended(outbox.as_ref(), &report(1, Happening::Started)).await;
    let Some(epoch) = epoch_of(&waiting(outbox.as_ref()).await) else {
        return;
    };
    let other = OutboxEpoch::from_uuid(Uuid::from_u128(epoch.as_uuid().as_u128() ^ 1));
    acknowledge(outbox.as_ref(), other, 1).await;
    assert_eq!(places(&waiting(outbox.as_ref()).await), [1]);
}

async fn reports_and_their_places_outlive_the_agent(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    appended(outbox.as_ref(), &report(1, Happening::Started)).await;
    appended(outbox.as_ref(), &report(1, Happening::Succeeded)).await;
    let before = waiting(outbox.as_ref()).await;
    let Some(epoch) = epoch_of(&before) else {
        return;
    };
    acknowledge(outbox.as_ref(), epoch, 1).await;
    drop(outbox);
    let outbox = harness.reopened().await;
    let after = waiting(outbox.as_ref()).await;
    assert_eq!(after, before.get(1..).unwrap_or_default());
    let next = appended(outbox.as_ref(), &report(2, Happening::Started)).await;
    assert_eq!(next, Some(seq(3)), "the count goes on where it stood");
}

async fn racing_appends_each_get_a_place(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    let racing = (0..8).map(|n| {
        let outbox = Arc::clone(&outbox);
        async move { appended(outbox.as_ref(), &report(n, Happening::Started)).await }
    });
    let mut got: Vec<u64> = futures::future::join_all(racing)
        .await
        .into_iter()
        .flatten()
        .map(ReportSeq::get)
        .collect();
    got.sort_unstable();
    assert_eq!(got, (1..=8).collect::<Vec<_>>());
}

async fn a_failure_keeps_its_code(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    let Some(code) = ok(WireName::parse("repository-unreachable"), "a code") else {
        return;
    };
    let failed = report(9, Happening::Failed { code });
    appended(outbox.as_ref(), &failed).await;
    let held = waiting(outbox.as_ref()).await;
    assert_eq!(held.first().map(|entry| &entry.report), Some(&failed));
}

/// A cancel waits like any other end, and reads back as a cancel.
async fn a_cancel_is_kept(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    let canceled = report(9, Happening::Canceled);
    appended(outbox.as_ref(), &canceled).await;
    let held = waiting(outbox.as_ref()).await;
    assert_eq!(held.first().map(|entry| &entry.report), Some(&canceled));
}

/// Full, the outbox gives up its oldest start for a new report and says
/// which it gave up, so the agent can log it; an end before that start
/// stays (`domain::fleet::admit`).
async fn a_full_outbox_says_which_start_gave_way(harness: &dyn OutboxHarness) {
    let outbox = harness.empty().await;
    appended(outbox.as_ref(), &report(1, Happening::Succeeded)).await;
    for n in 2..=u128::from(MAX_OUTBOX_REPORTS) {
        appended(outbox.as_ref(), &report(n, Happening::Started)).await;
    }
    let next = MAX_OUTBOX_REPORTS + 1;
    let newest = report(u128::from(next), Happening::Started);
    assert_eq!(
        ok(outbox.append(&newest).await, "appending to a full outbox"),
        Some(Appended::KeptGivingUp {
            seq: seq(next),
            gave_up: seq(2),
        })
    );
    let held = ok(outbox.unacknowledged(None, 2).await, "the oldest waiting").unwrap_or_default();
    assert_eq!(
        places(&held),
        [1, 3],
        "the end stays, the oldest start went"
    );
}

const AGENT: AgentId = AgentId::from_uuid(Uuid::from_u128(0xa1));
const OTHER_AGENT: AgentId = AgentId::from_uuid(Uuid::from_u128(0xa2));
const EPOCH: OutboxEpoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe1));
const OTHER_EPOCH: OutboxEpoch = OutboxEpoch::from_uuid(Uuid::from_u128(0xe2));

/// The machines the inbox suite takes reports from. A harness whose inbox
/// keeps the plans and progress of registered machines only registers
/// these in `empty`.
pub const MACHINES: [AgentId; 2] = [AGENT, OTHER_AGENT];

/// The report at `place` of `epoch`, of an operation of its own: the hub
/// holds an operation's report once, whichever machine sends it (#1513).
fn entry(epoch: OutboxEpoch, place: u64) -> OutboxEntry {
    let report = report(u128::from(place), Happening::Succeeded);
    OutboxEntry {
        epoch,
        seq: seq(place),
        report: AgentReport {
            op_id: op(epoch.as_uuid().as_u128() << 32 | u128::from(place)),
            ..report
        },
    }
}

fn now() -> Timestamp {
    Timestamp::constant(1_800_000_100, 0)
}

/// What the hub did with `entry`, none after a failed assertion if it
/// couldn't take it.
async fn taken(inbox: &dyn ReportInbox, agent: AgentId, entry: &OutboxEntry) -> Option<Ingest> {
    taken_at(inbox, agent, entry, now()).await
}

/// What the hub did with `entry`, taken at `at`.
async fn taken_at(
    inbox: &dyn ReportInbox,
    agent: AgentId,
    entry: &OutboxEntry,
    at: Timestamp,
) -> Option<Ingest> {
    ok(inbox.ingest(agent, entry, at).await, "taking a report").map(|taken| taken.ingest)
}

async fn reports(inbox: &dyn ReportInbox, agent: AgentId) -> Vec<OutboxEntry> {
    ok(inbox.reports(agent).await, "the reports taken").unwrap_or_default()
}

async fn latest(inbox: &dyn ReportInbox, agent: AgentId, limit: usize) -> Option<LatestReports> {
    ok(inbox.latest(agent, limit).await, "the latest reports")
}

/// The latest `limit` reports of one agent, in the order the hub took
/// them, and how many of its reports it let go of: none of an agent it
/// took nothing of, and never another agent's.
async fn the_latest_reports_are_read_with_the_count_let_go_of(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    assert_eq!(
        latest(inbox.as_ref(), AGENT, 3).await,
        Some(LatestReports {
            reports: Vec::new(),
            dropped: 0,
        })
    );
    for place in 1..=4 {
        taken(inbox.as_ref(), AGENT, &entry(EPOCH, place)).await;
    }
    taken(inbox.as_ref(), OTHER_AGENT, &entry(EPOCH, 9)).await;
    let reports = |places: &[u64]| -> Vec<AgentReport> {
        places
            .iter()
            .map(|place| entry(EPOCH, *place).report)
            .collect()
    };
    assert_eq!(
        latest(inbox.as_ref(), AGENT, 3).await,
        Some(LatestReports {
            reports: reports(&[2, 3, 4]),
            dropped: 0,
        })
    );
    assert_eq!(
        latest(inbox.as_ref(), AGENT, 10).await,
        Some(LatestReports {
            reports: reports(&[1, 2, 3, 4]),
            dropped: 0,
        })
    );
    assert_eq!(
        latest(inbox.as_ref(), OTHER_AGENT, 10).await,
        Some(LatestReports {
            reports: reports(&[9]),
            dropped: 0,
        })
    );
}

/// What the hub did with `entry`, and how many of the agent's reports it
/// had let go of in all when this one made it let go of some.
async fn ingested(
    inbox: &dyn ReportInbox,
    agent: AgentId,
    entry: &OutboxEntry,
) -> Option<(Ingest, Option<u64>)> {
    ok(inbox.ingest(agent, entry, now()).await, "taking a report")
        .map(|taken| (taken.ingest, taken.dropped))
}

async fn a_report_is_taken_once(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 1)).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 2)).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 1)).await,
        Some(Ingest::Duplicate)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 2)).await,
        Some(Ingest::Duplicate)
    );
    assert_eq!(
        reports(inbox, AGENT).await,
        [entry(EPOCH, 1), entry(EPOCH, 2)]
    );
}

/// The hub keeps a cancel as it took it.
async fn a_cancel_is_taken(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    let canceled = OutboxEntry {
        report: report(1, Happening::Canceled),
        ..entry(EPOCH, 1)
    };
    assert_eq!(taken(inbox, AGENT, &canceled).await, Some(Ingest::New));
    assert_eq!(reports(inbox, AGENT).await, [canceled]);
}

async fn a_gap_is_no_reason_to_refuse(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    taken(inbox, AGENT, &entry(EPOCH, 1)).await;
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 5)).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 3)).await,
        Some(Ingest::Duplicate),
        "behind the last taken"
    );
}

/// A report a send window or more behind the last taken of its epoch is
/// one taken before that no resend explains (#1387): said apart from a
/// duplicate, and kept no more than one.
async fn a_report_far_behind_the_last_taken_is_behind(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    let window = u64::try_from(SEND_WINDOW).unwrap_or(u64::MAX);
    taken(inbox, AGENT, &entry(EPOCH, window + 1)).await;
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 1)).await,
        Some(Ingest::Behind)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 2)).await,
        Some(Ingest::Duplicate)
    );
    assert_eq!(reports(inbox, AGENT).await, [entry(EPOCH, window + 1)]);
}

async fn each_agent_and_epoch_counts_alone(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    taken(inbox, AGENT, &entry(EPOCH, 3)).await;
    assert_eq!(
        taken(inbox, OTHER_AGENT, &entry(EPOCH, 1)).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(OTHER_EPOCH, 1)).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken(inbox, AGENT, &entry(EPOCH, 2)).await,
        Some(Ingest::Duplicate),
        "a new epoch leaves the old one's count"
    );
    assert_eq!(reports(inbox, OTHER_AGENT).await, [entry(EPOCH, 1)]);
}

/// The hub keeps the last place of an agent's [`MAX_EPOCHS_HELD`] newest
/// epochs, by when it last took one of each, and always of the epoch it
/// just took, even when its clock stepped back behind the others': a
/// resend of that report is no new one.
async fn the_epoch_just_taken_is_kept_however_the_clock_moved(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    let later = Timestamp::constant(1_800_000_200, 0);
    let epochs: Vec<OutboxEpoch> = (1..=MAX_EPOCHS_HELD + 1)
        .map(|n| OutboxEpoch::from_uuid(Uuid::from_u128(0x100 + u128::from(n))))
        .collect();
    let (Some(first), Some((newest, before))) = (epochs.first(), epochs.split_last()) else {
        return;
    };
    for epoch in before {
        taken_at(inbox, AGENT, &entry(*epoch, 1), later).await;
    }
    assert_eq!(
        taken_at(inbox, AGENT, &entry(*newest, 1), now()).await,
        Some(Ingest::New)
    );
    assert_eq!(
        taken_at(inbox, AGENT, &entry(*newest, 1), now()).await,
        Some(Ingest::Duplicate),
        "the epoch just taken is kept"
    );
    // Another report at the first epoch's place: the place is free again,
    // and the report is not one the hub holds (#1513).
    let mut at_first = entry(*first, 1);
    at_first.report.op_id = op(0xf125);
    assert_eq!(
        taken_at(inbox, AGENT, &at_first, later).await,
        Some(Ingest::New),
        "the first epoch of those taken at once gave way"
    );
}

async fn taken_reports_outlive_the_hub(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 1)).await;
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(reports(inbox.as_ref(), AGENT).await, [entry(EPOCH, 1)]);
    assert_eq!(
        taken(inbox.as_ref(), AGENT, &entry(EPOCH, 1)).await,
        Some(Ingest::Duplicate)
    );
}

async fn racing_deliveries_are_taken_once(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let racing = (0..8).map(|_| {
        let inbox = Arc::clone(&inbox);
        async move { taken(inbox.as_ref(), AGENT, &entry(EPOCH, 1)).await }
    });
    let answers = futures::future::join_all(racing).await;
    let new = answers
        .iter()
        .filter(|answer| **answer == Some(Ingest::New))
        .count();
    assert_eq!(new, 1);
    assert_eq!(reports(inbox.as_ref(), AGENT).await.len(), 1);
}

/// Past [`MAX_REPORTS_HELD`] of one agent, the hub lets go of its oldest,
/// and each report that made it says how many it let go of in all, counted
/// across restarts, for that agent alone.
async fn past_the_cap_the_oldest_give_way_and_are_counted(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    taken(inbox.as_ref(), OTHER_AGENT, &entry(OTHER_EPOCH, 1)).await;
    for place in 1..=MAX_REPORTS_HELD {
        assert_eq!(
            ingested(inbox.as_ref(), AGENT, &entry(EPOCH, place)).await,
            Some((Ingest::New, None)),
            "within the cap"
        );
    }
    for (place, in_all) in [(MAX_REPORTS_HELD + 1, 1), (MAX_REPORTS_HELD + 2, 2)] {
        assert_eq!(
            ingested(inbox.as_ref(), AGENT, &entry(EPOCH, place)).await,
            Some((Ingest::New, Some(in_all))),
            "past the cap"
        );
    }
    assert_eq!(
        ingested(inbox.as_ref(), AGENT, &entry(EPOCH, MAX_REPORTS_HELD + 2)).await,
        Some((Ingest::Duplicate, None))
    );
    let held = reports(inbox.as_ref(), AGENT).await;
    assert_eq!(u64::try_from(held.len()).ok(), Some(MAX_REPORTS_HELD));
    assert_eq!(held.first(), Some(&entry(EPOCH, 3)), "the oldest two went");
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(
        ingested(inbox.as_ref(), AGENT, &entry(EPOCH, MAX_REPORTS_HELD + 3)).await,
        Some((Ingest::New, Some(3))),
        "the count outlives the hub"
    );
    assert_eq!(
        ingested(inbox.as_ref(), OTHER_AGENT, &entry(OTHER_EPOCH, 2)).await,
        Some((Ingest::New, None)),
        "another agent's reports are counted apart"
    );
    assert_eq!(
        latest(inbox.as_ref(), AGENT, 1)
            .await
            .map(|latest| latest.dropped),
        Some(3),
        "the count let go of is read"
    );
    assert_eq!(
        reports(inbox.as_ref(), OTHER_AGENT).await,
        [entry(OTHER_EPOCH, 1), entry(OTHER_EPOCH, 2)]
    );
}

/// The hub holds a report of an operation once, whichever machine sends
/// it: one a machine sends again under a new machine ID, after the hub
/// removed the old one, is acknowledged as a duplicate and not kept a
/// second time (#1513). Another happening of that operation is new.
async fn an_operations_report_is_held_once_across_machines(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    let first = entry(EPOCH, 1);
    assert_eq!(taken(inbox, AGENT, &first).await, Some(Ingest::New));
    let again = OutboxEntry {
        epoch: OTHER_EPOCH,
        seq: seq(4),
        report: first.report.clone(),
    };
    assert_eq!(
        taken(inbox, OTHER_AGENT, &again).await,
        Some(Ingest::Duplicate),
        "the report sent again under another machine ID"
    );
    // The duplicate still raises that machine's watermark: a report of
    // another operation at an earlier place is one it already sent.
    let earlier = OutboxEntry {
        epoch: OTHER_EPOCH,
        seq: seq(3),
        report: AgentReport {
            op_id: op(0x1513),
            ..first.report.clone()
        },
    };
    assert_eq!(
        taken(inbox, OTHER_AGENT, &earlier).await,
        Some(Ingest::Duplicate),
        "the duplicate raised the other machine's watermark"
    );
    let started = OutboxEntry {
        seq: seq(5),
        report: AgentReport {
            happening: Happening::Started,
            ..first.report.clone()
        },
        ..again
    };
    assert_eq!(
        taken(inbox, OTHER_AGENT, &started).await,
        Some(Ingest::New),
        "another happening of the operation"
    );
    assert_eq!(reports(inbox, AGENT).await, [first]);
    assert_eq!(reports(inbox, OTHER_AGENT).await, [started]);
}

/// The last place taken of each agent's epoch is read as the hub took it:
/// none of an epoch or an agent it took nothing of, a duplicate and a
/// report behind it change nothing, and it outlives the hub (decision
/// 177).
async fn the_last_place_taken_is_read_for_each_agent_and_epoch(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let through = |inbox: Arc<dyn ReportInbox>, agent, epoch| async move {
        ok(
            inbox.taken_through(agent, epoch).await,
            "the last place taken",
        )
        .flatten()
    };
    assert_eq!(through(inbox.clone(), AGENT, EPOCH).await, None);
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 1)).await;
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 5)).await;
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 5)).await;
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 3)).await;
    assert_eq!(through(inbox.clone(), AGENT, EPOCH).await, Some(seq(5)));
    assert_eq!(through(inbox.clone(), AGENT, OTHER_EPOCH).await, None);
    assert_eq!(through(inbox.clone(), OTHER_AGENT, EPOCH).await, None);
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(through(inbox, AGENT, EPOCH).await, Some(seq(5)));
}

/// What the hub did with `entry`, and whether it diverged from the report
/// taken at its place.
async fn checked(
    inbox: &dyn ReportInbox,
    agent: AgentId,
    entry: &OutboxEntry,
) -> Option<(Ingest, bool)> {
    ok(inbox.ingest(agent, entry, now()).await, "taking a report")
        .map(|taken| (taken.ingest, taken.diverged))
}

/// Whether the hub marked `agent`'s `epoch` diverged.
async fn diverged(inbox: &dyn ReportInbox, agent: AgentId, epoch: OutboxEpoch) -> Option<bool> {
    ok(inbox.diverged(agent, epoch).await, "the diverged mark")
}

/// Another operation's report at `place` of `epoch` than [`entry`]'s.
fn other_at(epoch: OutboxEpoch, place: u64) -> OutboxEntry {
    let taken = entry(epoch, place);
    OutboxEntry {
        report: AgentReport {
            op_id: op(0x1387_0000 + u128::from(place)),
            ..taken.report.clone()
        },
        ..taken
    }
}

/// A report at a place the hub took is checked against the report taken
/// there (#1387, decision 177): the same one sent again is a duplicate, or
/// behind, as before. Another diverges, near or far behind the last taken:
/// it is kept nowhere, and its epoch is marked diverged, for that agent and
/// epoch only, across restarts. With no report at the place, one the hub
/// holds nowhere diverges.
async fn a_report_unlike_the_one_taken_at_its_place_diverges(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let window = u64::try_from(SEND_WINDOW).unwrap_or(u64::MAX);
    for place in [1, 2, window + 1] {
        taken(inbox.as_ref(), AGENT, &entry(EPOCH, place)).await;
    }
    assert_eq!(
        checked(inbox.as_ref(), AGENT, &entry(EPOCH, 2)).await,
        Some((Ingest::Duplicate, false)),
        "the report taken, sent again"
    );
    assert_eq!(
        checked(inbox.as_ref(), AGENT, &entry(EPOCH, 1)).await,
        Some((Ingest::Behind, false)),
        "the report taken, far behind"
    );
    assert_eq!(diverged(inbox.as_ref(), AGENT, EPOCH).await, Some(false));
    assert_eq!(
        checked(inbox.as_ref(), AGENT, &other_at(EPOCH, 2)).await,
        Some((Ingest::Duplicate, true)),
        "another report at a place taken"
    );
    assert_eq!(
        checked(inbox.as_ref(), AGENT, &other_at(EPOCH, 1)).await,
        Some((Ingest::Behind, true)),
        "another report far behind"
    );
    assert_eq!(
        checked(inbox.as_ref(), AGENT, &other_at(EPOCH, 3)).await,
        Some((Ingest::Duplicate, true)),
        "a report held nowhere, at a place taken with none"
    );
    assert_eq!(
        reports(inbox.as_ref(), AGENT).await,
        [entry(EPOCH, 1), entry(EPOCH, 2), entry(EPOCH, window + 1)]
    );
    assert_eq!(diverged(inbox.as_ref(), AGENT, EPOCH).await, Some(true));
    assert_eq!(
        diverged(inbox.as_ref(), OTHER_AGENT, EPOCH).await,
        Some(false)
    );
    drop(inbox);
    let inbox = harness.reopened().await;
    assert_eq!(diverged(inbox.as_ref(), AGENT, EPOCH).await, Some(true));
}

/// With no report at a place the hub took, a report the hub holds under
/// another machine ID is sent again (#1513), and doesn't diverge.
async fn a_report_held_under_another_machine_id_is_sent_again(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    taken(inbox.as_ref(), AGENT, &entry(EPOCH, 5)).await;
    let held = entry(OTHER_EPOCH, 1);
    taken(inbox.as_ref(), OTHER_AGENT, &held).await;
    let again = OutboxEntry {
        epoch: EPOCH,
        seq: seq(3),
        ..held
    };
    for _ in 0..2 {
        assert_eq!(
            checked(inbox.as_ref(), AGENT, &again).await,
            Some((Ingest::Duplicate, false)),
            "a report held under another machine ID"
        );
    }
    assert_eq!(diverged(inbox.as_ref(), AGENT, EPOCH).await, Some(false));
}

/// The diverged mark lives with its epoch's last place taken: once the hub
/// forgets the epoch among the agent's oldest, the mark goes with it, so it
/// never outlasts the epoch it names (decision 177).
async fn a_diverged_mark_is_forgotten_with_its_epoch(harness: &dyn ReportInboxHarness) {
    let inbox = harness.empty().await;
    let inbox = inbox.as_ref();
    taken(inbox, AGENT, &entry(EPOCH, 1)).await;
    checked(inbox, AGENT, &other_at(EPOCH, 1)).await;
    assert_eq!(diverged(inbox, AGENT, EPOCH).await, Some(true));
    let later = Timestamp::constant(1_800_000_200, 0);
    for n in 1..=MAX_EPOCHS_HELD {
        let newer = OutboxEpoch::from_uuid(Uuid::from_u128(0x200 + u128::from(n)));
        taken_at(inbox, AGENT, &entry(newer, 1), later).await;
    }
    assert_eq!(
        ok(
            inbox.taken_through(AGENT, EPOCH).await,
            "the last place taken"
        )
        .flatten(),
        None,
        "the epoch is forgotten"
    );
    assert_eq!(diverged(inbox, AGENT, EPOCH).await, Some(false));
}
