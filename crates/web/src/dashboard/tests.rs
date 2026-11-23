use api::{MachineOperationDto, MachineOutcomeDto};

use super::*;

fn machine(hostname: &str, last_seen: &str) -> HubMachineDto {
    HubMachineDto {
        agent_id: format!("agent-{hostname}"),
        hostname: hostname.to_owned(),
        last_seen: last_seen.to_owned(),
        zone: None,
        reports_dropped: 0,
        operations: Vec::new(),
    }
}

fn zone(name: &str) -> IanaZone {
    IanaZone::try_from(name.to_owned()).unwrap()
}

const AT: &str = "2026-11-21T08:00:00Z";
const AT_MS: i64 = 1_795_248_000_000;

/// An approval and a report read the machines again; a machine that waits
/// isn't on the dashboard, and nothing else is the dashboard's.
#[test]
fn an_approval_and_a_report_read_the_machines_again() {
    let agent = || "agent-demo-laptop".to_owned();
    assert!(reads_again(&UiEvent::AgentApproved {
        agent: agent(),
        hostname: "demo-laptop".to_owned(),
        at: AT.to_owned(),
    }));
    assert!(reads_again(&UiEvent::AgentReported {
        agent: agent(),
        operation: "op-1".to_owned(),
        outcome: MachineOutcomeDto::Succeeded,
    }));
    for other in [
        UiEvent::AgentPending {
            agent: agent(),
            hostname: "demo-laptop".to_owned(),
            at: AT.to_owned(),
        },
        UiEvent::AgentOnline {
            agent: agent(),
            at: AT.to_owned(),
        },
        UiEvent::ProposalChanged {
            proposal: "p1".to_owned(),
            status: api::ProposalStatusDto::Pending,
        },
    ] {
        assert!(!reads_again(&other), "{other:?}");
    }
}

/// Each machine that came, went or reads otherwise is one change, however
/// much of it changed; machines that read the same are none.
#[test]
fn each_machine_that_came_went_or_changed_is_one_change() {
    let held = vec![machine("demo-a", AT), machine("demo-b", AT)];
    assert_eq!(changes_since(&held, &held), 0);
    let mut later = held.clone();
    if let Some(changed) = later.get_mut(0) {
        changed.last_seen = "2026-11-21T09:00:00Z".to_owned();
        changed.reports_dropped = 2;
        changed.operations.push(MachineOperationDto {
            operation: "op-1".to_owned(),
            started_at: Some(AT.to_owned()),
            ended_at: None,
            outcome: MachineOutcomeDto::Running,
            code: None,
        });
    }
    assert_eq!(changes_since(&held, &later), 1);
    later.remove(1);
    assert_eq!(changes_since(&held, &later), 2, "one went");
    later.push(machine("demo-c", AT));
    assert_eq!(changes_since(&held, &later), 3, "one came");
}

/// A paused dashboard shows the copy and counts what waits; the resume
/// shows the latest and says how many machines changed.
#[test]
fn a_paused_dashboard_holds_its_machines_and_the_resume_counts_them() {
    let fleet = Fleet::new();
    fleet.machines.set(vec![machine("demo-a", AT)]);
    fleet.state.set(State::Ready);
    assert!(!fleet.paused());
    fleet.pause();
    assert!(fleet.paused());
    assert_eq!(fleet.pending_untracked(), 0);
    fleet.machines.set(vec![
        machine("demo-a", AT),
        machine("demo-b", AT),
        machine("demo-c", AT),
    ]);
    assert_eq!(fleet.shown().get_untracked().len(), 1);
    assert_eq!(fleet.pending(), 2);
    assert_eq!(fleet.pending_untracked(), 2);
    assert_eq!(fleet.resume(), resumed(2));
    assert_eq!(fleet.shown().get_untracked().len(), 3);
    assert_eq!(fleet.pending(), 0);
    assert_eq!(fleet.pending_untracked(), 0);
    assert!(!fleet.paused());
}

/// Each outcome reads as the operation's own status of that name, in its
/// state's badge.
#[test]
fn each_outcome_reads_as_its_status_badge() {
    assert_eq!(
        outcome_badge(MachineOutcomeDto::Running),
        (BadgeState::Running, msg::UI_STATUS_RUNNING)
    );
    assert_eq!(
        outcome_badge(MachineOutcomeDto::Succeeded),
        (BadgeState::Succeeded, msg::UI_STATUS_SUCCEEDED)
    );
    assert_eq!(
        outcome_badge(MachineOutcomeDto::Failed),
        (BadgeState::Failed, msg::UI_STATUS_FAILED)
    );
}

/// A machine's time reads in the viewer's zone alone when the zones match
/// or the machine gave none, and in both, the machine's named, when they
/// differ: one message with both times (#115).
#[test]
fn a_machines_time_reads_beside_the_viewers_only_when_their_zones_differ() {
    let viewer = zone("America/New_York");
    let one = |zone: Option<IanaZone>| Words {
        id: msg::UI_INSTANT,
        args: vec![(
            "when",
            MsgArg::Timestamp {
                utc_ms: AT_MS,
                zone,
            },
        )],
    };
    assert_eq!(
        instant(AT, Some(&viewer), Some("America/New_York")),
        Some(one(Some(viewer.clone())))
    );
    assert_eq!(
        instant(AT, Some(&viewer), None),
        Some(one(Some(viewer.clone())))
    );
    assert_eq!(
        instant(AT, Some(&viewer), Some("America/Chicago")),
        Some(Words {
            id: msg::UI_DASHBOARD_TIME_ELSEWHERE,
            args: vec![
                (
                    "time",
                    MsgArg::Timestamp {
                        utc_ms: AT_MS,
                        zone: Some(viewer.clone()),
                    },
                ),
                (
                    "there",
                    MsgArg::Timestamp {
                        utc_ms: AT_MS,
                        zone: Some(zone("America/Chicago")),
                    },
                ),
                ("zone", MsgArg::Text("America/Chicago".to_owned())),
            ],
        })
    );
    assert_eq!(
        instant(AT, None, Some("America/Chicago")),
        Some(Words {
            id: msg::UI_DASHBOARD_TIME_ELSEWHERE,
            args: vec![
                (
                    "time",
                    MsgArg::Timestamp {
                        utc_ms: AT_MS,
                        zone: None,
                    },
                ),
                (
                    "there",
                    MsgArg::Timestamp {
                        utc_ms: AT_MS,
                        zone: Some(zone("America/Chicago")),
                    },
                ),
                ("zone", MsgArg::Text("America/Chicago".to_owned())),
            ],
        }),
        "a viewer without a zone reads UTC, which isn't the machine's"
    );
    assert_eq!(
        instant(AT, Some(&viewer), Some("../not-a-zone")),
        Some(one(Some(viewer.clone()))),
        "a zone that can't be a zone is none"
    );
    assert_eq!(instant("yesterday", Some(&viewer), None), None);
}
