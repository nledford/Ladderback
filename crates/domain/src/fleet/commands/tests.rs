use super::*;

fn plan(id: &str) -> PlanId {
    PlanId::parse(id).unwrap()
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

const NOW: i64 = 1_800_000_000;

#[test]
fn accepts_reads_the_commands_it_knows_and_passes_over_the_rest() {
    let accepts = Accepts::from_names(["cancel-op", "restore", "run-plan"]).unwrap();
    assert_eq!(accepts, Accepts::all());
    assert!(accepts.takes(CommandKind::RunPlan));
    let only_cancel = Accepts::from_names(["cancel-op"]).unwrap();
    assert!(!only_cancel.takes(CommandKind::RunPlan));
    assert!(only_cancel.takes(CommandKind::CancelOp));
    assert_eq!(Accepts::from_names([]).unwrap(), Accepts::none());
}

#[test]
fn accepts_refuses_more_names_than_a_hello_holds() {
    let names = vec!["run-plan"; MAX_ACCEPTS];
    assert!(Accepts::from_names(names.iter().copied()).is_ok());
    let names = vec!["run-plan"; MAX_ACCEPTS + 1];
    assert_eq!(
        Accepts::from_names(names.iter().copied()),
        Err(TooManyAccepted)
    );
}

#[test]
fn each_command_has_its_wire_name() {
    for kind in CommandKind::ALL {
        assert_eq!(CommandKind::from_wire_name(kind.wire_name()), Some(kind));
    }
    assert_eq!(CommandKind::RunPlan.wire_name(), "run-plan");
    assert_eq!(CommandKind::CancelOp.wire_name(), "cancel-op");
    assert_eq!(CommandKind::from_wire_name("RunPlan"), None);
}

#[test]
fn a_command_id_is_a_uuidv7_of_the_hubs_clock() {
    let id = CommandId::v7(1_800_000_000_123, &[7; 10]);
    assert_eq!(id.as_uuid().get_version_num(), 7);
    let (seconds, nanos) = id.as_uuid().get_timestamp().unwrap().to_unix();
    assert_eq!((seconds, nanos / 1_000_000), (1_800_000_000, 123));
}

#[test]
fn a_command_is_fresh_within_fifteen_minutes_of_the_hubs_time_either_way() {
    let welcomed = EpochSeconds::from_seconds(1_800_000_000);
    let since = SignedDuration::from_mins(10);
    let hub_now = 1_800_000_000 + 600;
    let fresh = |asked: u64| command_fresh(welcomed, since, EpochSeconds::from_seconds(asked));
    assert!(fresh(hub_now));
    assert!(fresh(hub_now - 900));
    assert!(!fresh(hub_now - 901));
    assert!(fresh(hub_now + 900));
    assert!(!fresh(hub_now + 901));
}

#[test]
fn a_commands_freshness_ignores_how_the_machines_own_clock_stands() {
    // Hours on the machine's monotonic clock since the `Welcome`: a command
    // asked at the `Welcome`'s time has long expired.
    let welcomed = EpochSeconds::from_seconds(1_800_000_000);
    let asked = EpochSeconds::from_seconds(1_800_000_000);
    assert!(!command_fresh(
        welcomed,
        SignedDuration::from_hours(2),
        asked
    ));
    assert!(command_fresh(welcomed, SignedDuration::ZERO, asked));
}

#[test]
fn the_hub_starts_no_second_run_of_a_plan_within_five_minutes() {
    let runs = [HubRun {
        plan: plan("photos"),
        at: at(NOW - 299),
    }];
    assert_eq!(
        hub_run_admitted(&runs, &plan("photos"), at(NOW)),
        Err(at(NOW + 1))
    );
    assert_eq!(hub_run_admitted(&runs, &plan("documents"), at(NOW)), Ok(()));
    assert_eq!(
        hub_run_admitted(&runs, &plan("photos"), at(NOW + 1)),
        Ok(())
    );
}

#[test]
fn the_hub_starts_at_most_twelve_runs_an_hour() {
    let runs: Vec<HubRun> = (0..12)
        .map(|n| HubRun {
            plan: plan(&format!("plan-{n}")),
            at: at(NOW - 3_000 + n * 60),
        })
        .collect();
    // Until the oldest of the twelve is an hour old.
    assert_eq!(
        hub_run_admitted(&runs, &plan("other"), at(NOW)),
        Err(at(NOW + 600))
    );
    assert_eq!(
        hub_run_admitted(&runs[1..], &plan("other"), at(NOW)),
        Ok(())
    );
    assert_eq!(
        hub_run_admitted(&runs, &plan("other"), at(NOW + 600)),
        Ok(())
    );
}

#[test]
fn a_run_ahead_of_the_machines_clock_counts_by_how_far_it_lies() {
    let runs = [HubRun {
        plan: plan("photos"),
        at: at(NOW + 60),
    }];
    assert_eq!(
        hub_run_admitted(&runs, &plan("photos"), at(NOW)),
        Err(at(NOW + 360))
    );
    let far = [HubRun {
        plan: plan("photos"),
        at: at(NOW + 86_400),
    }];
    assert_eq!(hub_run_admitted(&far, &plan("photos"), at(NOW)), Ok(()));
}

/// A hub that sends a machine every command its limit lets it, for as long
/// as the machine remembers a command's ID, sends fewer than the machine
/// remembers: its asks alone never fill the machine's ledger.
#[test]
fn a_hub_at_its_limit_for_a_day_never_fills_a_machines_ledger() {
    let mut sent = Vec::new();
    let mut now = SignedDuration::ZERO;
    while now < COMMANDS_REMEMBERED_FOR {
        match hub_command_admitted(&sent, now) {
            Ok(()) => sent.push(now),
            Err(again) => {
                assert!(again > now, "{again:?} isn't after {now:?}");
                now = again;
            }
        }
    }
    let hour = SignedDuration::from_hours(1);
    assert_eq!(
        sent.iter().filter(|at| **at < hour).count(),
        HUB_COMMANDS_PER_HOUR
    );
    assert!(sent.len() < MAX_COMMANDS_REMEMBERED, "{}", sent.len());
}

/// The hub sends the next command once the oldest of the hour's leaves it.
#[test]
fn a_hub_at_its_limit_sends_again_an_hour_after_its_oldest() {
    let minute = SignedDuration::from_mins(1);
    let sent: Vec<SignedDuration> = (0..HUB_COMMANDS_PER_HOUR)
        .map(|number| minute * i32::try_from(number).unwrap())
        .collect();
    let now = SignedDuration::from_mins(50);
    assert_eq!(
        hub_command_admitted(&sent, now),
        Err(SignedDuration::from_hours(1))
    );
    assert_eq!(
        hub_command_admitted(sent.get(1..).unwrap(), now),
        Ok(())
    );
    assert_eq!(
        hub_command_admitted(&sent, SignedDuration::from_hours(1)),
        Ok(())
    );
}
