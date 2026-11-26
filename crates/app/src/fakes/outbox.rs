use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use domain::fleet::{
    AgentReport, Ingest, MAX_EPOCHS_HELD, MAX_REPORTS_HELD, OutboxAdmission, OutboxEpoch,
    PlansReport, ReportSeq, ReportedProgress, admit, ingest,
};
use domain::{AgentId, Timestamp};

use crate::ports::{
    Appended, Ingested, LatestReports, Outbox, OutboxEntry, ReportInbox, StoreError,
};

/// The agent's outbox in memory, for use-case tests and acceptance
/// scenarios: one instance stands for one database, so a test that keeps it
/// across a rebuilt use case has what a restart keeps.
#[derive(Debug)]
pub struct InMemoryOutbox {
    epoch: OutboxEpoch,
    kept: Mutex<Held>,
}

#[derive(Debug, Default)]
struct Held {
    /// The last place given, none before the first report.
    last: Option<ReportSeq>,
    reports: BTreeMap<ReportSeq, AgentReport>,
}

impl InMemoryOutbox {
    /// An empty outbox of `epoch`.
    pub fn new(epoch: OutboxEpoch) -> Self {
        Self {
            epoch,
            kept: Mutex::default(),
        }
    }

    pub const fn epoch(&self) -> OutboxEpoch {
        self.epoch
    }
}

#[async_trait]
impl Outbox for InMemoryOutbox {
    async fn append(&self, report: &AgentReport) -> Result<Appended, StoreError> {
        let mut held = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let next = held.last.map_or(Some(ReportSeq::FIRST), ReportSeq::next);
        let oldest_start = held
            .reports
            .iter()
            .find(|(_, report)| !report.kept())
            .map(|(seq, _)| *seq);
        let count = u64::try_from(held.reports.len()).unwrap_or(u64::MAX);
        let Some(next) = next else {
            return Ok(Appended::Refused);
        };
        let gave_up = match admit(count, oldest_start, Some(next)) {
            OutboxAdmission::Append => None,
            OutboxAdmission::EvictThenAppend(seq) => {
                held.reports.remove(&seq);
                Some(seq)
            }
            OutboxAdmission::Refuse => return Ok(Appended::Refused),
        };
        held.reports.insert(next, report.clone());
        held.last = Some(next);
        Ok(match gave_up {
            None => Appended::Kept(next),
            Some(gave_up) => Appended::KeptGivingUp { seq: next, gave_up },
        })
    }

    async fn unacknowledged(
        &self,
        after: Option<ReportSeq>,
        limit: usize,
    ) -> Result<Vec<OutboxEntry>, StoreError> {
        let held = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(held
            .reports
            .iter()
            .filter(|(seq, _)| after.is_none_or(|after| **seq > after))
            .take(limit)
            .map(|(seq, report)| OutboxEntry {
                epoch: self.epoch,
                seq: *seq,
                report: report.clone(),
            })
            .collect())
    }

    async fn acknowledge(&self, epoch: OutboxEpoch, through: ReportSeq) -> Result<(), StoreError> {
        if epoch == self.epoch {
            let mut held = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            held.reports.retain(|seq, _| *seq > through);
        }
        Ok(())
    }
}

/// The hub's reports in memory, for use-case tests and acceptance
/// scenarios.
#[derive(Debug, Default)]
pub struct InMemoryReportInbox {
    agents: Mutex<HashMap<AgentId, Agent>>,
}

#[derive(Debug, Default)]
struct Agent {
    /// Each epoch's last place taken and when, in no order.
    epochs: Vec<(OutboxEpoch, ReportSeq, Timestamp)>,
    /// The reports taken, oldest first.
    reports: Vec<OutboxEntry>,
    /// How many reports were let go of past the cap.
    dropped: u64,
    /// The plans it reported last.
    plans: Option<PlansReport>,
    /// The progress it reported last.
    progress: Option<ReportedProgress>,
}

#[async_trait]
impl ReportInbox for InMemoryReportInbox {
    async fn ingest(
        &self,
        agent: AgentId,
        entry: &OutboxEntry,
        received_at: Timestamp,
    ) -> Result<Ingested, StoreError> {
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        // As the store's `sync_reports_once` index: an operation's happening
        // is held once, whichever machine ID sent it (#1513).
        let happening = std::mem::discriminant(&entry.report.happening);
        let held_already = agents.values().any(|other| {
            other.reports.iter().any(|kept| {
                kept.report.op_id == entry.report.op_id
                    && std::mem::discriminant(&kept.report.happening) == happening
            })
        });
        let held = agents.entry(agent).or_default();
        let place = held
            .epochs
            .iter()
            .position(|(epoch, _, _)| *epoch == entry.epoch);
        let through = place
            .and_then(|place| held.epochs.get(place))
            .map(|(_, seq, _)| *seq);
        let verdict = ingest(through, entry.seq);
        if verdict != Ingest::New {
            return Ok(Ingested {
                ingest: verdict,
                dropped: None,
            });
        }
        match place.and_then(|place| held.epochs.get_mut(place)) {
            Some(epoch) => *epoch = (entry.epoch, entry.seq, received_at),
            // First, so that among epochs taken at one time the newest is
            // kept, as the store keeps it.
            None => held.epochs.insert(0, (entry.epoch, entry.seq, received_at)),
        }
        let epochs = usize::try_from(MAX_EPOCHS_HELD).unwrap_or(usize::MAX);
        // The one just taken first, however the clock moved, then the
        // newest by when the hub last took one.
        held.epochs
            .sort_by_key(|(epoch, _, at)| (*epoch != entry.epoch, std::cmp::Reverse(*at)));
        held.epochs.truncate(epochs);
        if held_already {
            return Ok(Ingested {
                ingest: Ingest::Duplicate,
                dropped: None,
            });
        }
        held.reports.push(entry.clone());
        let reports = usize::try_from(MAX_REPORTS_HELD).unwrap_or(usize::MAX);
        let over = held.reports.len().saturating_sub(reports);
        held.reports.drain(..over);
        let mut dropped = None;
        if over > 0 {
            let let_go = u64::try_from(over).unwrap_or(u64::MAX);
            held.dropped = held.dropped.saturating_add(let_go);
            dropped = Some(held.dropped);
        }
        Ok(Ingested {
            ingest: Ingest::New,
            dropped,
        })
    }

    async fn reports(&self, agent: AgentId) -> Result<Vec<OutboxEntry>, StoreError> {
        let agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(agents
            .get(&agent)
            .map(|held| held.reports.clone())
            .unwrap_or_default())
    }

    async fn latest(&self, agent: AgentId, limit: usize) -> Result<LatestReports, StoreError> {
        let agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(agents.get(&agent).map_or(
            LatestReports {
                reports: Vec::new(),
                dropped: 0,
            },
            |held| LatestReports {
                reports: held
                    .reports
                    .iter()
                    .skip(held.reports.len().saturating_sub(limit))
                    .map(|entry| entry.report.clone())
                    .collect(),
                dropped: held.dropped,
            },
        ))
    }

    async fn keep_plans(&self, agent: AgentId, plans: &PlansReport) -> Result<(), StoreError> {
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        agents.entry(agent).or_default().plans = Some(plans.clone());
        Ok(())
    }

    async fn plans(&self, agent: AgentId) -> Result<Option<PlansReport>, StoreError> {
        let agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(agents.get(&agent).and_then(|held| held.plans.clone()))
    }

    async fn keep_progress(
        &self,
        agent: AgentId,
        progress: &ReportedProgress,
    ) -> Result<(), StoreError> {
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        agents.entry(agent).or_default().progress = Some(progress.clone());
        Ok(())
    }

    async fn progress(&self, agent: AgentId) -> Result<Option<ReportedProgress>, StoreError> {
        let agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(agents.get(&agent).and_then(|held| held.progress.clone()))
    }
}
