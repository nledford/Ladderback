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
    MAX_REPORTS_HELD, OutboxEpoch, ReportSeq, SEND_WINDOW, WireName,
};
use domain::{AgentId, OperationId, Timestamp, Uuid};

use crate::ports::{Appended, Outbox, OutboxEntry, ReportInbox};

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
    a_full_outbox_says_which_start_gave_way(harness).await;
}

pub async fn run_inbox(harness: &dyn ReportInboxHarness) {
    a_report_is_taken_once(harness).await;
    a_gap_is_no_reason_to_refuse(harness).await;
    a_report_far_behind_the_last_taken_is_behind(harness).await;
    each_agent_and_epoch_counts_alone(harness).await;
    the_epoch_just_taken_is_kept_however_the_clock_moved(harness).await;
    taken_reports_outlive_the_hub(harness).await;
    racing_deliveries_are_taken_once(harness).await;
    past_the_cap_the_oldest_give_way_and_are_counted(harness).await;
}

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

fn entry(epoch: OutboxEpoch, place: u64) -> OutboxEntry {
    OutboxEntry {
        epoch,
        seq: seq(place),
        report: report(u128::from(place), Happening::Succeeded),
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
    assert_eq!(
        taken_at(inbox, AGENT, &entry(*first, 1), later).await,
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
    taken(inbox.as_ref(), OTHER_AGENT, &entry(EPOCH, 1)).await;
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
        ingested(inbox.as_ref(), OTHER_AGENT, &entry(EPOCH, 2)).await,
        Some((Ingest::New, None)),
        "another agent's reports are counted apart"
    );
    assert_eq!(
        reports(inbox.as_ref(), OTHER_AGENT).await,
        [entry(EPOCH, 1), entry(EPOCH, 2)]
    );
}
