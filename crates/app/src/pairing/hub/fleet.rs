//! What the hub tells its browsers of its machines, and what its dashboard
//! reads of them (technical design §4.3, §7.2; #569): that a machine
//! started waiting for approval, was approved, was removed or reported an
//! operation, and each approved machine with its latest operations.

use api::{MachineOutcomeDto, UiEvent};
use domain::fleet::{
    AgentRecord, AgentReport, Happening, OPERATIONS_SHOWN, Pairing, REPORTS_READ,
    ReportedOperation, Tombstone, latest_operations,
};
use domain::{AgentId, Timestamp};

use crate::ports::LatestReports;

use super::{HubPairing, HubPairingError};

/// One approved machine as the hub's dashboard shows it: its record, how
/// many of its reports the hub let go of past the cap, and its latest
/// operations, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub record: AgentRecord,
    pub reports_dropped: u64,
    pub operations: Vec<ReportedOperation>,
}

impl HubPairing {
    /// Tells the browsers `event`, when the hub tells them anything.
    pub(super) async fn tell(&self, event: UiEvent) {
        if let Some(events) = &self.events {
            events.publish(event).await;
        }
    }

    /// Every approved machine, oldest first, with its latest operations:
    /// none of a hub that takes no reports.
    pub async fn machines(&self) -> Result<Vec<Machine>, HubPairingError> {
        let mut machines = Vec::new();
        for record in self.registry.agents().await? {
            if record.is_pending() {
                continue;
            }
            let latest = match &self.inbox {
                Some(inbox) => inbox.latest(record.agent_id, REPORTS_READ).await?,
                None => LatestReports {
                    reports: Vec::new(),
                    dropped: 0,
                },
            };
            machines.push(Machine {
                record,
                reports_dropped: latest.dropped,
                operations: latest_operations(&latest.reports, OPERATIONS_SHOWN),
            });
        }
        Ok(machines)
    }
}

/// What the browsers hear of `pairing`: that its machine waits for
/// approval, unless it paired before with this same request.
pub(super) fn waiting(pairing: &Pairing, at: Timestamp) -> Option<UiEvent> {
    let record = pairing.record();
    let again = matches!(pairing, Pairing::Repeated(_));
    (record.is_pending() && !again).then(|| UiEvent::AgentPending {
        agent: record.agent_id.as_uuid().to_string(),
        hostname: record.hostname.to_string(),
        at: at.to_string(),
    })
}

/// What the browsers hear of `agent`, approved at `at`.
pub(super) fn approved(agent: &AgentRecord, at: Timestamp) -> UiEvent {
    UiEvent::AgentApproved {
        agent: agent.agent_id.as_uuid().to_string(),
        hostname: agent.hostname.to_string(),
        at: at.to_string(),
    }
}

/// What the browsers hear of the agent `tombstone` keeps, which a person
/// has just removed or rejected.
pub(super) fn removed(tombstone: &Tombstone) -> UiEvent {
    UiEvent::AgentRemoved {
        agent: tombstone.agent_id.as_uuid().to_string(),
        hostname: tombstone.hostname.to_string(),
        at: tombstone.removed_at.to_string(),
    }
}

/// What the browsers hear of `report`, taken from `agent`.
pub(super) fn reported(agent: AgentId, report: &AgentReport) -> UiEvent {
    UiEvent::AgentReported {
        agent: agent.as_uuid().to_string(),
        operation: report.op_id.as_uuid().to_string(),
        outcome: match report.happening {
            Happening::Started => MachineOutcomeDto::Running,
            Happening::Succeeded => MachineOutcomeDto::Succeeded,
            Happening::Failed { .. } => MachineOutcomeDto::Failed,
        },
    }
}
