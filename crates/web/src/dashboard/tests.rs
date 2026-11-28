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
        outcome_badge(MachineOutcomeDto::Canceled),
        (BadgeState::Canceled, msg::UI_STATUS_CANCELED)
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

/// One minute in milliseconds, for times around `AT`.
const MINUTE_MS: i64 = 60_000;

/// The instant `minutes` from `AT`, in RFC 3339.
fn at(minutes: i64) -> String {
    jiff::Timestamp::from_millisecond(AT_MS + minutes * MINUTE_MS)
        .unwrap()
        .to_string()
}

/// A plan that last ran `last` (its status and minutes from `AT`) and runs
/// next `next` minutes from `AT`.
fn plan(name: &str, last: Option<(&str, i64)>, next: Option<i64>) -> MachinePlanDto {
    MachinePlanDto {
        plan: name.to_owned(),
        last_run: last.map(|(status, minutes)| MachinePlanRunDto {
            at: at(minutes),
            status: status.to_owned(),
        }),
        next_run: next.map(at),
    }
}

/// `machine` with these plans, and `left_out` more it didn't report.
fn with_plans(
    mut machine: HubMachineDto,
    plans: Vec<MachinePlanDto>,
    left_out: u64,
) -> HubMachineDto {
    machine.plans = Some(MachinePlansDto { plans, left_out });
    machine
}

fn online(mut machine: HubMachineDto) -> HubMachineDto {
    machine.online = true;
    machine
}

fn running(mut machine: HubMachineDto) -> HubMachineDto {
    machine.operations = vec![MachineOperationDto {
        operation: "op-1".to_owned(),
        started_at: Some(at(-3)),
        ended_at: None,
        outcome: MachineOutcomeDto::Running,
        code: None,
    }];
    machine
}

/// A card tells the status of the latest run that ended, when it ended, the
/// earliest next run, and how many plans the machine has, those it left
/// out among them.
#[test]
fn a_card_tells_the_last_backup_the_next_run_and_the_plans() {
    let laptop = online(with_plans(
        machine("demo-laptop", AT),
        vec![
            plan("documents", Some(("succeeded", -60)), Some(30)),
            plan("photos", Some(("succeeded-with-warnings", -20)), Some(90)),
            plan("music", None, None),
        ],
        2,
    ));
    assert_eq!(
        card_facts(&laptop, AT_MS),
        CardFacts {
            status: CardStatus::Last("succeeded-with-warnings".to_owned()),
            last_backup: Some(at(-20)),
            next_run: NextRun::At(at(30)),
            plans: Some(5),
            attention: None,
        }
    );
}

/// A machine that runs an operation reads Running, whatever its last run
/// said; one that never ran reads Never, and one that hasn't reported its
/// plans has no count of them.
#[test]
fn a_running_machine_reads_running_and_one_that_never_ran_reads_never() {
    let busy = running(online(with_plans(
        machine("demo-desktop", AT),
        vec![plan("documents", Some(("failed", -60)), Some(-10))],
        0,
    )));
    let facts = card_facts(&busy, AT_MS);
    assert_eq!(facts.status, CardStatus::Running);
    assert_eq!(
        facts.attention,
        Some(Attention::Failed {
            plan: "documents".to_owned()
        }),
        "a run going now leaves the failure before it to say"
    );
    let mut progressing = online(machine("demo-desktop", AT));
    progressing.progress = Some(progress(AT));
    assert_eq!(card_facts(&progressing, AT_MS).status, CardStatus::Running);

    let quiet = machine("demo-tablet", AT);
    assert_eq!(
        card_facts(&quiet, AT_MS),
        CardFacts {
            status: CardStatus::Never,
            last_backup: None,
            next_run: NextRun::NotPlanned,
            plans: None,
            attention: None,
        }
    );
}

/// A machine runs operations side by side, so one queued later can end
/// while one before it still runs: newest first, the one that runs can come
/// second as well as first, and either way the card reads Running, with its
/// run past due as its next run rather than one that was due.
#[test]
fn a_machine_whose_newer_operation_ended_while_an_older_one_runs_reads_running() {
    let busy = running(online(with_plans(
        machine("demo-desktop", AT),
        vec![plan("documents", Some(("succeeded", -60)), Some(-10))],
        0,
    )));
    let ended = MachineOperationDto {
        operation: "op-2".to_owned(),
        started_at: Some(at(-2)),
        ended_at: Some(at(-1)),
        outcome: MachineOutcomeDto::Succeeded,
        code: None,
    };
    for place in [0, 1] {
        let mut both = busy.clone();
        both.operations.insert(place, ended.clone());
        let facts = card_facts(&both, AT_MS);
        assert_eq!(facts.status, CardStatus::Running, "ended one at {place}");
        assert_eq!(facts.next_run, NextRun::At(at(-10)), "ended one at {place}");
        assert_eq!(facts.attention, None, "ended one at {place}");
    }
}

/// An offline machine whose planned run is more than five minutes past
/// needs attention for that first; a run five minutes past or less is
/// still on its way, since a machine reads its plans every minute.
#[test]
fn an_offline_machine_past_a_planned_run_needs_attention() {
    let away = with_plans(
        machine("demo-laptop", AT),
        vec![
            plan("documents", Some(("failed", -1500)), Some(-60)),
            plan("photos", None, Some(-120)),
        ],
        0,
    );
    let facts = card_facts(&away, AT_MS);
    assert_eq!(
        facts.attention,
        Some(Attention::Offline {
            last_seen: AT.to_owned(),
            due: at(-120),
        })
    );
    assert_eq!(facts.next_run, NextRun::WasDue(at(-120)));

    let just_due = with_plans(
        machine("demo-laptop", AT),
        vec![plan("documents", Some(("succeeded", -60)), Some(-5))],
        0,
    );
    let facts = card_facts(&just_due, AT_MS);
    assert_eq!(facts.attention, None);
    assert_eq!(facts.next_run, NextRun::At(at(-5)));

    let asleep = with_plans(
        machine("demo-laptop", AT),
        vec![plan("documents", Some(("succeeded", -60)), Some(30))],
        0,
    );
    assert_eq!(
        card_facts(&asleep, AT_MS).attention,
        None,
        "offline before its schedule is no matter"
    );
}

/// An online machine needs attention for a failed last run, then for a
/// planned run past due while nothing runs; one running is on its way.
#[test]
fn a_failed_run_then_a_run_past_due_needs_attention() {
    let failed = online(with_plans(
        machine("demo-laptop", AT),
        vec![
            plan("documents", Some(("succeeded", -60)), Some(-30)),
            plan("photos", Some(("failed", -120)), None),
        ],
        0,
    ));
    assert_eq!(
        card_facts(&failed, AT_MS).attention,
        Some(Attention::Failed {
            plan: "photos".to_owned()
        })
    );
    let late = online(with_plans(
        machine("demo-laptop", AT),
        vec![plan("documents", Some(("succeeded", -60)), Some(-30))],
        0,
    ));
    assert_eq!(
        card_facts(&late, AT_MS).attention,
        Some(Attention::Due {
            plan: "documents".to_owned(),
            due: at(-30),
        })
    );
    assert_eq!(card_facts(&running(late), AT_MS).attention, None);
}

/// A running machine's planned run past due is on its way: its card tells
/// when it was planned, not that it fell due.
#[test]
fn a_running_machines_run_past_due_reads_as_its_next_run() {
    let late = online(with_plans(
        machine("demo-laptop", AT),
        vec![plan("documents", Some(("succeeded", -60)), Some(-30))],
        0,
    ));
    assert_eq!(card_facts(&late, AT_MS).next_run, NextRun::WasDue(at(-30)));
    assert_eq!(
        card_facts(&running(late), AT_MS).next_run,
        NextRun::At(at(-30))
    );
}

/// The sentence counts the machines and those that need attention, whose
/// links follow it.
#[test]
fn the_sentence_counts_the_machines_and_those_that_need_attention() {
    let said = |count: u64, attention: u64| Words {
        id: msg::UI_DASHBOARD_SUMMARY,
        args: vec![
            ("count", MsgArg::Count(count)),
            ("attention", MsgArg::Count(attention)),
        ],
    };
    assert_eq!(summary(3, 1), said(3, 1));
    assert_eq!(summary(1, 0), said(1, 0));
}

/// Each reason reads in its own sentence, on the card without the hostname
/// its heading gives, and in the link after the sentence with it, its times
/// as the page writes them.
#[test]
fn each_reason_reads_in_its_own_sentence_on_the_card_and_in_the_link() {
    let shown = |raw: &str| format!("<{raw}>");
    let text = |value: &str| MsgArg::Text(value.to_owned());
    let offline = Attention::Offline {
        last_seen: AT.to_owned(),
        due: at(-60),
    };
    assert_eq!(
        attention_words(&offline, shown),
        Words {
            id: msg::UI_MACHINE_CARD_OFFLINE,
            args: vec![("when", text(&shown(AT))), ("due", text(&shown(&at(-60))))],
        }
    );
    assert_eq!(
        attention_link("demo-laptop", &offline, shown),
        Words {
            id: msg::UI_DASHBOARD_ATTENTION_OFFLINE,
            args: vec![
                ("hostname", text("demo-laptop")),
                ("when", text(&shown(AT))),
                ("due", text(&shown(&at(-60)))),
            ],
        }
    );
    let failed = Attention::Failed {
        plan: "photos".to_owned(),
    };
    assert_eq!(
        attention_words(&failed, shown),
        Words {
            id: msg::UI_MACHINE_CARD_FAILED,
            args: vec![("plan", text("photos"))],
        }
    );
    assert_eq!(
        attention_link("demo-laptop", &failed, shown),
        Words {
            id: msg::UI_DASHBOARD_ATTENTION_FAILED,
            args: vec![("hostname", text("demo-laptop")), ("plan", text("photos"))],
        }
    );
    let due = Attention::Due {
        plan: "photos".to_owned(),
        due: at(-30),
    };
    assert_eq!(
        attention_words(&due, shown),
        Words {
            id: msg::UI_MACHINE_CARD_DUE,
            args: vec![("plan", text("photos")), ("due", text(&shown(&at(-30))))],
        }
    );
    assert_eq!(
        attention_link("demo-laptop", &due, shown),
        Words {
            id: msg::UI_DASHBOARD_ATTENTION_DUE,
            args: vec![
                ("hostname", text("demo-laptop")),
                ("plan", text("photos")),
                ("due", text(&shown(&at(-30)))),
            ],
        }
    );
}

/// An operation of the hub's own, as its history lists one.
fn summary_of(id: &str, plan: &str, status: &str, minutes: i64) -> api::OperationSummaryDto {
    api::OperationSummaryDto {
        id: id.to_owned(),
        plan: plan.to_owned(),
        kind: "backup".to_owned(),
        status: status.to_owned(),
        queued_at: at(minutes - 1),
        started_at: (status != "queued").then(|| at(minutes)),
        finished_at: matches!(status, "succeeded" | "failed" | "cancelled")
            .then(|| at(minutes + 1)),
        warning_codes: Vec::new(),
        failure_code: (status == "failed").then_some(api::ErrorCode::DaemonUnreachable),
        failure: None,
        result: None,
        repository: None,
        locations: Vec::new(),
        entry: None,
        restored_to: None,
    }
}

fn identity(machine_id: Option<&str>) -> MachineDto {
    MachineDto {
        standing: api::MachineStandingDto::Same,
        machine_id: machine_id.map(str::to_owned),
        hostname: Some("demo-hub".to_owned()),
        zone: Some("Europe/Berlin".to_owned()),
    }
}

/// The hub's own machine reads as a paired one does: online, named as the
/// hub names itself, its plans from its status, each one's last run when
/// it ended, its operations that started, newest first and at most ten,
/// and the progress of the one that runs.
#[test]
fn the_hubs_own_machine_reads_as_a_paired_one_does() {
    let status = StatusDto {
        plans: vec![api::PlanStatusDto {
            plan: api::PlanSummaryDto {
                id: "documents".to_owned(),
                repository: "demo".to_owned(),
                scheduled: true,
                next_run: Some(at(30)),
                warnings: Vec::new(),
            },
            last: Some(summary_of("op-2", "documents", "failed", -60)),
            last_success: None,
            overdue: false,
        }],
        running: Vec::new(),
        unreadable_repositories: Vec::new(),
        full_disk_access: api::FullDiskAccessStateDto::NotApplicable,
    };
    let mut operations = OperationsState::from_history(
        [
            vec![
                summary_of("op-q", "documents", "queued", 0),
                summary_of("op-r", "documents", "running", -2),
                summary_of("op-2", "documents", "failed", -60),
            ],
            (3..15)
                .map(|n| summary_of(&format!("op-{n}"), "documents", "succeeded", -100 - n))
                .collect(),
        ]
        .concat(),
        None,
    );
    let reading = progress_reading(&progress(AT));
    if let Some(row) = operations.rows.get_mut(1) {
        row.progress = Some(reading);
    }
    let Some(own) = own_machine(&identity(Some("agent-hub")), Some(&status), &operations) else {
        panic!("the hub's own machine isn't shown");
    };
    assert_eq!(own.machine_id, "agent-hub");
    assert_eq!(own.hostname, "demo-hub");
    assert!(own.online);
    assert_eq!(own.zone.as_deref(), Some("Europe/Berlin"));
    assert_eq!(own.reports_dropped, 0);
    assert_eq!(
        own.plans,
        Some(MachinePlansDto {
            plans: vec![plan("documents", Some(("failed", -59)), Some(30))],
            left_out: 0,
        })
    );
    assert_eq!(own.operations.len(), 10, "at most ten, and none queued");
    assert_eq!(
        own.operations.first(),
        Some(&MachineOperationDto {
            operation: "op-r".to_owned(),
            started_at: Some(at(-2)),
            ended_at: None,
            outcome: MachineOutcomeDto::Running,
            code: None,
        })
    );
    assert_eq!(
        own.operations.get(1),
        Some(&MachineOperationDto {
            operation: "op-2".to_owned(),
            started_at: Some(at(-60)),
            ended_at: Some(at(-59)),
            outcome: MachineOutcomeDto::Failed,
            code: Some("daemon-unreachable".to_owned()),
        })
    );
    assert_eq!(
        own.progress
            .as_ref()
            .map(|progress| (progress.operation.as_str(), progress.current)),
        Some(("op-r", 512))
    );
    // Before its status is read, the hub hasn't told its plans; without a
    // machine ID, it has no card.
    assert_eq!(
        own_machine(&identity(Some("agent-hub")), None, &operations).and_then(|own| own.plans),
        None
    );
    assert_eq!(
        own_machine(&identity(None), Some(&status), &operations),
        None
    );
}

/// While a plan of the hub's own runs or waits in the queue, its card's
/// last run is the plan's last success, which its status tells, so the
/// card never says no backup has ended while one runs.
#[test]
fn the_hubs_own_plan_that_runs_again_reads_its_last_success() {
    let standing = |status: &str, success: Option<i64>| api::PlanStatusDto {
        plan: api::PlanSummaryDto {
            id: "documents".to_owned(),
            repository: "demo".to_owned(),
            scheduled: true,
            next_run: None,
            warnings: Vec::new(),
        },
        last: Some(summary_of("op-3", "documents", status, -2)),
        last_success: success.map(|minutes| api::LastSuccessDto {
            operation: "op-1".to_owned(),
            finished_at: at(minutes),
            ago_seconds: 0,
        }),
        overdue: false,
    };
    let read = |plan: api::PlanStatusDto| {
        let status = StatusDto {
            plans: vec![plan],
            running: Vec::new(),
            unreadable_repositories: Vec::new(),
            full_disk_access: api::FullDiskAccessStateDto::NotApplicable,
        };
        own_machine(
            &identity(Some("agent-hub")),
            Some(&status),
            &OperationsState::default(),
        )
        .and_then(|own| own.plans)
        .map(|plans| plans.plans)
    };
    assert_eq!(
        read(standing("running", Some(-1500))),
        Some(vec![plan("documents", Some(("succeeded", -1500)), None)])
    );
    assert_eq!(
        read(standing("queued", None)),
        Some(vec![plan("documents", None, None)]),
        "never succeeded: no last run yet"
    );
}

/// An operation of the hub's own that was canceled reads Canceled on its
/// card, as a paired machine's does.
#[test]
fn the_hubs_own_canceled_operation_reads_canceled() {
    let operations = OperationsState::from_history(
        vec![summary_of("op-1", "documents", "cancelled", -10)],
        None,
    );
    let own = own_machine(&identity(Some("agent-hub")), None, &operations);
    assert_eq!(
        own.map(|own| own.operations),
        Some(vec![MachineOperationDto {
            operation: "op-1".to_owned(),
            started_at: Some(at(-10)),
            ended_at: Some(at(-9)),
            outcome: MachineOutcomeDto::Canceled,
            code: None,
        }])
    );
}

/// The page reads the hub's plans' status on opening, then again for each
/// operation that comes and each status that changes, the end above all,
/// which the card's status and last backup tell; never for progress, nor
/// for a story it has read for.
#[test]
fn a_new_operation_or_a_changed_status_reads_the_hubs_status_again() {
    let mut story = OwnStory::default();
    let mut operations = OperationsState::default();
    assert!(story.moves_on(&operations), "the first read");
    assert!(!story.moves_on(&operations), "nothing changed");
    operations =
        OperationsState::from_history(vec![summary_of("op-1", "documents", "queued", 0)], None);
    assert!(story.moves_on(&operations), "an operation came");
    operations.apply(&UiEvent::OperationStarted {
        operation: "op-1".to_owned(),
        repository: None,
        started_at: at(1),
    });
    assert!(story.moves_on(&operations), "it started");
    operations.apply(&UiEvent::OperationProgress {
        operation: "op-1".to_owned(),
        step: Some(api::OperationStep::BackingUp),
        unit: api::ProgressUnit::Bytes,
        current: 512,
        total: Some(2048),
        at: Some(at(2)),
    });
    assert!(!story.moves_on(&operations), "progress changes no status");
    operations.apply(&UiEvent::OperationSucceeded {
        operation: "op-1".to_owned(),
        finished_at: at(3),
    });
    assert!(story.moves_on(&operations), "it ended");
    assert!(!story.moves_on(&operations), "the end was read for");
}

/// The hub's own card comes first, then each approved machine in the order
/// the hub lists them; until the hub's own is read, the page waits for it.
#[test]
fn the_hubs_own_card_comes_first_and_the_page_waits_for_it() {
    let fleet = Fleet::new();
    fleet
        .machines
        .set(vec![machine("demo-a", AT), machine("demo-b", AT)]);
    fleet.state.set(State::Ready);
    assert_eq!(fleet.shown_state().get_untracked(), State::Loading);
    fleet
        .own
        .set(Own::Read(Some(Box::new(machine("demo-hub", AT)))));
    assert_eq!(fleet.shown_state().get_untracked(), State::Ready);
    let names: Vec<String> = fleet
        .shown()
        .get_untracked()
        .into_iter()
        .map(|machine| machine.hostname)
        .collect();
    assert_eq!(names, ["demo-hub", "demo-a", "demo-b"]);
    assert_eq!(
        fleet.own_id().get_untracked().as_deref(),
        Some("agent-demo-hub")
    );
    fleet.own.set(Own::Read(None));
    assert_eq!(fleet.shown().get_untracked().len(), 2);
    assert_eq!(fleet.own_id().get_untracked(), None);
}

/// A pause holds the hub's own card and the time the cards are read at as
/// it holds the machines; a change to the hub's own counts as one.
#[test]
fn a_pause_holds_the_hubs_own_card_and_the_time() {
    let fleet = Fleet::new();
    fleet.state.set(State::Ready);
    fleet
        .own
        .set(Own::Read(Some(Box::new(machine("demo-hub", AT)))));
    fleet.now.set(AT_MS);
    fleet.pause();
    fleet
        .own
        .set(Own::Read(Some(Box::new(online(machine("demo-hub", AT))))));
    fleet.now.set(AT_MS + MINUTE_MS);
    assert_eq!(fleet.shown_now().get_untracked(), AT_MS);
    assert_eq!(
        fleet.shown().get_untracked().first().map(|own| own.online),
        Some(false)
    );
    assert_eq!(fleet.pending(), 1);
    assert_eq!(fleet.resume(), resumed(1));
    assert_eq!(fleet.shown_now().get_untracked(), AT_MS + MINUTE_MS);
}

/// A page paused once the hub's own machine was read stays ready; one
/// paused before it was read still waits for it.
#[test]
fn a_paused_page_stays_ready_once_the_hubs_own_was_read() {
    let fleet = Fleet::new();
    fleet.state.set(State::Ready);
    fleet
        .own
        .set(Own::Read(Some(Box::new(machine("demo-hub", AT)))));
    fleet.pause();
    assert_eq!(fleet.shown_state().get_untracked(), State::Ready);

    let waiting = Fleet::new();
    waiting.state.set(State::Ready);
    waiting.pause();
    waiting
        .own
        .set(Own::Read(Some(Box::new(machine("demo-hub", AT)))));
    assert_eq!(
        waiting.shown_state().get_untracked(),
        State::Loading,
        "the pause holds the page as it was: waiting"
    );
}
