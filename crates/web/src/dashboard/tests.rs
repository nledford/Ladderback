use api::{MachineOperationDto, MachineOutcomeDto};

use super::*;

fn machine(hostname: &str, last_seen: &str) -> HubMachineDto {
    HubMachineDto {
        machine_id: format!("agent-{hostname}"),
        hostname: hostname.to_owned(),
        last_seen: last_seen.to_owned(),
        online: false,
        zone: None,
        reports_dropped: 0,
        operations: Vec::new(),
        plans: None,
        progress: None,
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
    assert!(reads_again(&UiEvent::MachineApproved {
        machine: agent(),
        hostname: "demo-laptop".to_owned(),
        at: AT.to_owned(),
    }));
    assert!(reads_again(&UiEvent::MachineReported {
        machine: agent(),
        operation: "op-1".to_owned(),
        outcome: MachineOutcomeDto::Succeeded,
    }));
    assert!(
        reads_again(&UiEvent::MachineRemoved {
            machine: agent(),
            hostname: "demo-laptop".to_owned(),
            at: AT.to_owned(),
        }),
        "a removed machine leaves the dashboard"
    );
    for other in [
        UiEvent::MachinePending {
            machine: agent(),
            hostname: "demo-laptop".to_owned(),
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

/// The hub taking a machine's plans or its running operation's progress
/// reads the machines again, so both show as they come.
#[test]
fn new_plans_and_new_progress_read_the_machines_again() {
    let agent = || "agent-demo-laptop".to_owned();
    assert!(reads_again(&UiEvent::MachinePlans { machine: agent() }));
    assert!(reads_again(&UiEvent::MachineProgress {
        machine: agent(),
        progress: progress(AT),
    }));
}

/// A machine coming online or going offline reads the machines again, so
/// the page shows whether the hub holds its connection (decision 159).
#[test]
fn a_machine_coming_online_or_going_offline_reads_the_machines_again() {
    let agent = || "agent-demo-laptop".to_owned();
    assert!(reads_again(&UiEvent::MachineOnline {
        machine: agent(),
        at: AT.to_owned(),
    }));
    assert!(reads_again(&UiEvent::MachineOffline {
        machine: agent(),
        at: AT.to_owned(),
    }));
}

fn progress(at: &str) -> MachineProgressDto {
    MachineProgressDto {
        operation: "op-1".to_owned(),
        step: Some(api::OperationStep::BackingUp),
        unit: api::ProgressUnit::Bytes,
        current: 512,
        total: Some(2048),
        at: at.to_owned(),
    }
}

/// A machine's progress draws as the Operations page's bar does, its last
/// report and the start of its measure both when the hub heard it.
#[test]
fn a_machines_progress_reads_as_the_shared_bar_draws_it() {
    let heard = Some(jiff::Timestamp::from_millisecond(AT_MS).unwrap());
    assert_eq!(
        progress_reading(&progress(AT)),
        Progress {
            step: Some(api::OperationStep::BackingUp),
            unit: api::ProgressUnit::Bytes,
            current: 512,
            total: Some(2048),
            reported_at: heard,
            since: heard,
        }
    );
}

/// A time the page can't read draws the bar with no time, never a wrong one.
#[test]
fn a_progress_heard_at_an_unreadable_time_shows_no_time() {
    let reading = progress_reading(&progress("yesterday"));
    assert_eq!((reading.reported_at, reading.since), (None, None));
    assert_eq!(reading.current, 512);
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
    assert_eq!(
        outcome_badge(MachineOutcomeDto::Cancelled),
        (BadgeState::Cancelled, msg::UI_STATUS_CANCELLED)
    );
}

/// A machine that reported its plans and had none says so; one whose plans
/// haven't reached the hub yet says that instead.
#[test]
fn a_machine_with_no_plans_tells_none_reported_from_none_yet() {
    assert_eq!(no_plans(None), msg::UI_DASHBOARD_NO_PLANS);
    let none = MachinePlansDto {
        plans: Vec::new(),
        left_out: 0,
    };
    assert_eq!(no_plans(Some(&none)), msg::UI_DASHBOARD_PLANS_NONE);
}

/// A machine whose connection the hub holds says it is online now; any
/// other says when it last connected (§16, decision 159).
#[test]
fn a_machine_holding_its_connection_reads_online_now_and_any_other_when_it_last_connected() {
    assert_eq!(connection_line(true), msg::UI_DASHBOARD_ONLINE_NOW);
    assert_eq!(connection_line(false), msg::UI_DASHBOARD_LAST_CONNECTED);
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
