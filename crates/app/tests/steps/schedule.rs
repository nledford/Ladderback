//! Steps for `features/execution/schedule.feature`: the next run of a plan,
//! asked of the domain's `NextRun` as the agent asks it, when a run starts
//! and when the agent comes back, at the times the scenario gives.

use cucumber::{given, then, when};
use domain::execution::Trigger;
use domain::execution::next_run::{NextRun, RandomDraw};
use domain::planning::{CronExpression, Interval, Schedule, parse_duration};
use domain::{TimeZone, Timestamp};

use super::Agent;

#[derive(Debug)]
pub struct ScheduleWorld {
    schedule: Option<Schedule>,
    zone: TimeZone,
    draw: RandomDraw,
    planned: Option<Timestamp>,
    back_at: Option<Timestamp>,
    next: Option<NextRun>,
}

impl Default for ScheduleWorld {
    fn default() -> Self {
        Self {
            schedule: None,
            zone: TimeZone::UTC,
            draw: RandomDraw::new(1 << 63),
            planned: None,
            back_at: None,
            next: None,
        }
    }
}

impl ScheduleWorld {
    fn schedule(&self) -> &Schedule {
        self.schedule
            .as_ref()
            .expect("a step gave the plan a schedule")
    }

    fn next(&self) -> NextRun {
        self.next.expect("a step asked for the plan's next run")
    }
}

/// A time as the scenario writes it: `2026-06-10 03:00+02:00`.
fn time(text: &str) -> Timestamp {
    text.parse()
        .unwrap_or_else(|error| panic!("{text:?} is a time with its offset: {error}"))
}

fn zone(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap_or_else(|error| panic!("{name:?} is a time zone: {error}"))
}

fn interval(text: &str) -> Interval {
    let duration = parse_duration(text).expect("a length of time");
    Interval::new(duration).expect("an interval a plan may run at")
}

#[given(expr = "a plan scheduled {string} in the time zone {string}")]
fn scheduled(agent: &mut Agent, cron: String, name: String) {
    let cron = CronExpression::parse(&cron).expect("a cron expression");
    agent.schedule.schedule = Some(Schedule::new(cron));
    agent.schedule.zone = zone(&name);
}

#[given(expr = "a plan that runs every {string}")]
fn every(agent: &mut Agent, text: String) {
    agent.schedule.schedule = Some(Schedule::new(interval(&text)));
}

#[given(expr = "a plan that runs every {string} in the time zone {string}")]
fn every_in(agent: &mut Agent, text: String, name: String) {
    every(agent, text);
    agent.schedule.zone = zone(&name);
}

#[given(expr = "the plan's runs are spread over {string}")]
fn spread_over(agent: &mut Agent, text: String) {
    let jitter = parse_duration(&text).expect("a length of time");
    let schedule = agent.schedule.schedule().clone();
    agent.schedule.schedule = Some(schedule.with_jitter(jitter).expect("a jitter"));
}

#[given("the plan doesn't make up for runs it missed")]
fn no_catch_up(agent: &mut Agent) {
    let schedule = agent.schedule.schedule().clone();
    agent.schedule.schedule = Some(schedule.with_catch_up(false));
}

#[given(expr = "the random draw is the {word} there is")]
fn draw(agent: &mut Agent, which: String) {
    agent.schedule.draw = match which.as_str() {
        "lowest" => RandomDraw::LOWEST,
        "middle" => RandomDraw::new(1 << 63),
        "highest" => RandomDraw::HIGHEST,
        other => panic!("no step draws the {other} number"),
    };
}

#[given(expr = "the plan's next run was planned for {string}")]
fn planned_for(agent: &mut Agent, text: String) {
    agent.schedule.planned = Some(time(&text));
}

#[when(expr = "the plan's run starts at {string}")]
fn run_starts(agent: &mut Agent, text: String) {
    let world = &mut agent.schedule;
    let next = NextRun::after(world.schedule(), &world.zone, time(&text), world.draw);
    world.next = Some(next.expect("the schedule has a next run"));
}

#[when(expr = "the agent comes back at {string}")]
fn comes_back(agent: &mut Agent, text: String) {
    let world = &mut agent.schedule;
    let now = time(&text);
    let planned = world.planned.expect("a step planned the plan's next run");
    let next = NextRun::resume(world.schedule(), &world.zone, planned, now, world.draw);
    world.back_at = Some(now);
    world.next = Some(next.expect("the schedule has a next run"));
}

#[then(expr = "the plan's next run is at {string}")]
fn next_run_at(agent: &mut Agent, text: String) {
    assert_eq!(
        agent.schedule.next(),
        NextRun {
            at: time(&text),
            trigger: Trigger::Schedule,
        }
    );
}

#[then("the plan runs at once to catch up")]
fn runs_at_once(agent: &mut Agent) {
    let now = agent.schedule.back_at.expect("the agent came back");
    assert_eq!(
        agent.schedule.next(),
        NextRun {
            at: now,
            trigger: Trigger::CatchUp,
        }
    );
}
