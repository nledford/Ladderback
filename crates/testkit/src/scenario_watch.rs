//! A time limit on each scenario of a cucumber runner. A step that waits for
//! something that never comes (a release a fake lost, #246) would otherwise
//! hold the run until CI cancels the whole job, 45 minutes later, without
//! naming the scenario. [`ScenarioWatch`] ends the process instead, naming
//! the scenario that ran over the limit, and when the run ends it says which
//! scenario was slowest, the figure a limit is chosen from.
//!
//! When the runner's stdout isn't a terminal, as in CI, the watch also says
//! on stderr when each scenario begins and ends, one line each. A run that
//! hangs where the limit can't see it, after a scenario's `after` hook (its
//! world being dropped) or at the runner's own end, and is stopped by CI then
//! still shows in the log which scenarios ran last and whether any began and
//! never ended (#638). In a terminal it stays quiet: cucumber's own writer
//! redraws its last lines there, and a line between them would garble it.
//! The gate is stdout alone, not cucumber's `--color`, which the watch
//! doesn't see: `--color always` into a pipe mixes cucumber's redraws with
//! these lines, and `--color never` in a terminal leaves them off.
//!
//! The runner registers [`ScenarioWatch::began`] as its `before` hook and
//! [`ScenarioWatch::ended`] as its `after` hook, naming the scenario with
//! [`ScenarioWatch::describe`]. The watch is a thread of its own, so a step
//! that blocks the runner's thread can't stop it. It takes no cucumber types:
//! as a normal dependency of `testkit`, cucumber would bring licenses
//! `cargo deny` checks only for what ships (`synthez`, BlueOak-1.0.0).

use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// How often the watch looks at the running scenarios.
const TICK: Duration = Duration::from_millis(250);

/// The scenarios a runner is running, and the slowest one that ended. Cheap
/// to clone: every clone watches the same run.
#[derive(Clone)]
pub struct ScenarioWatch {
    runner: &'static str,
    /// Whether each scenario's beginning and end are said on stderr.
    announce: bool,
    scenarios: Arc<Mutex<Running>>,
}

impl ScenarioWatch {
    /// Starts watching the scenarios of `runner` (the name its messages
    /// carry): once one has run for longer than `limit`, the process prints
    /// its name, ends every process the run owns
    /// ([`crate::children::Owned`]) and exits with status 1.
    pub fn start(runner: &'static str, limit: Duration) -> Self {
        let watch = Self {
            runner,
            announce: !io::stdout().is_terminal(),
            scenarios: Arc::new(Mutex::new(Running::new(limit))),
        };
        let watching = watch.clone();
        let spawned = thread::Builder::new()
            .name(format!("{runner} scenario watch"))
            .spawn(move || watching.watch());
        if let Err(error) = spawned {
            panic!("{runner}: the scenario watch could not start: {error}");
        }
        watch
    }

    /// How a scenario is named in the watch's messages: its name, and the
    /// feature file and line it starts at, or the feature's name when the
    /// feature has no file.
    #[must_use]
    pub fn describe(scenario: &str, line: usize, path: Option<&Path>, feature: &str) -> String {
        match path {
            Some(path) => format!("\"{scenario}\" ({}:{line})", path.display()),
            None => format!("\"{scenario}\" ({feature})"),
        }
    }

    /// Notes that `scenario`, named `name` (see [`Self::describe`]), started:
    /// the runner's `before` hook.
    pub fn began<S: ?Sized>(&self, scenario: &S, name: String) {
        if self.announce {
            self.say(&format!("began {name}"));
        }
        self.running().began(key(scenario), name, Instant::now());
    }

    /// Notes that `scenario` ended, however it ended: the runner's `after`
    /// hook.
    pub fn ended<S: ?Sized>(&self, scenario: &S) {
        let ended = self.running().ended(key(scenario), Instant::now());
        if self.announce
            && let Some((name, took)) = ended
        {
            self.say(&format!("ended {name} after {took:.1?}"));
        }
    }

    /// Writes `what` on stderr as one line in one `write`, so it never splits
    /// around a line cucumber writes at the same time. A lost line costs
    /// only a clue, so a failed write is ignored rather than ending the run.
    fn say(&self, what: &str) {
        let line = format!("{}: {what}\n", self.runner);
        let _ignored = io::stderr().lock().write_all(line.as_bytes());
    }

    /// Says on stderr which scenario was slowest, when the returned guard is
    /// dropped: when the run ends, also when a failed step ends it with a
    /// panic.
    pub fn report_slowest(&self) -> ReportSlowest {
        ReportSlowest(self.clone())
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Running> {
        self.scenarios
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn watch(&self) {
        loop {
            thread::sleep(TICK);
            let overdue = self
                .running()
                .overdue(Instant::now())
                .map(|(name, ran)| (name.to_owned(), ran));
            if let Some((name, ran)) = overdue {
                let limit = self.running().limit;
                eprintln!(
                    "{}: the scenario {name} has run for {ran:.1?}, longer than the {limit:?} \
                     a scenario may take; stopping the run. A step is waiting for something \
                     that never comes.",
                    self.runner
                );
                // `exit` runs no destructor: what the scenarios started is
                // ended here, and nothing starts more before the exit.
                let _ended = crate::children::end_all();
                std::process::exit(1);
            }
        }
    }
}

/// Prints the slowest scenario when dropped (see
/// [`ScenarioWatch::report_slowest`]).
pub struct ReportSlowest(ScenarioWatch);

impl Drop for ReportSlowest {
    fn drop(&mut self) {
        let runner = self.0.runner;
        match &self.0.running().slowest {
            Some((name, took)) => {
                eprintln!("{runner}: the slowest scenario took {took:.1?}: {name}");
            }
            None => eprintln!("{runner}: no scenario ended"),
        }
    }
}

/// Cucumber gives its `before` and `after` hooks the same scenario, so its
/// address tells the scenarios running at once apart, the expanded rows of
/// one outline too.
fn key<S: ?Sized>(scenario: &S) -> usize {
    std::ptr::from_ref(scenario).cast::<()>().addr()
}

/// The bookkeeping, apart from the clock and the thread.
struct Running {
    limit: Duration,
    started: HashMap<usize, (String, Instant)>,
    slowest: Option<(String, Duration)>,
}

impl Running {
    fn new(limit: Duration) -> Self {
        Self {
            limit,
            started: HashMap::new(),
            slowest: None,
        }
    }

    fn began(&mut self, key: usize, name: String, at: Instant) {
        self.started.insert(key, (name, at));
    }

    /// Ends the scenario `key` began, keeping it if it was the slowest yet,
    /// and gives back its name and how long it took.
    fn ended(&mut self, key: usize, at: Instant) -> Option<(String, Duration)> {
        let (name, began) = self.started.remove(&key)?;
        let took = at.saturating_duration_since(began);
        if self.slowest.as_ref().is_none_or(|(_, worst)| took > *worst) {
            self.slowest = Some((name.clone(), took));
        }
        Some((name, took))
    }

    /// The scenario that has run longest, with how long, once that is longer
    /// than the limit.
    fn overdue(&self, now: Instant) -> Option<(&str, Duration)> {
        self.started
            .values()
            .map(|(name, began)| (name.as_str(), now.saturating_duration_since(*began)))
            .max_by_key(|(_, ran)| *ran)
            .filter(|(_, ran)| *ran > self.limit)
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    const LIMIT: Duration = Duration::from_secs(60);

    #[test]
    fn a_scenario_is_overdue_only_past_the_limit() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        running.began(1, "slow".to_owned(), start);
        assert_eq!(running.overdue(start + LIMIT), None);
        assert_eq!(
            running.overdue(start + LIMIT + Duration::from_millis(1)),
            Some(("slow", LIMIT + Duration::from_millis(1)))
        );
    }

    #[test]
    fn the_longest_running_scenario_is_the_one_named() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        running.began(1, "later".to_owned(), start + Duration::from_secs(5));
        running.began(2, "first".to_owned(), start);
        running.began(3, "last".to_owned(), start + Duration::from_secs(9));
        assert_eq!(
            running.overdue(start + Duration::from_secs(61)),
            Some(("first", Duration::from_secs(61)))
        );
    }

    #[test]
    fn an_ended_scenario_is_never_overdue() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        running.began(1, "quick".to_owned(), start);
        running.began(2, "other".to_owned(), start + Duration::from_secs(30));
        running.ended(1, start + Duration::from_secs(1));
        assert_eq!(running.overdue(start + Duration::from_secs(61)), None);
    }

    #[test]
    fn the_slowest_ended_scenario_is_kept() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        assert_eq!(running.slowest, None);
        running.began(1, "two seconds".to_owned(), start);
        running.began(2, "five seconds".to_owned(), start);
        running.began(3, "three seconds".to_owned(), start);
        running.ended(1, start + Duration::from_secs(2));
        running.ended(2, start + Duration::from_secs(5));
        running.ended(3, start + Duration::from_secs(3));
        assert_eq!(
            running.slowest,
            Some(("five seconds".to_owned(), Duration::from_secs(5)))
        );
    }

    #[test]
    fn a_scenario_that_never_began_ends_without_a_trace() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        assert_eq!(running.ended(7, start + Duration::from_secs(1)), None);
        assert_eq!(running.slowest, None);
        assert!(running.started.is_empty());
    }

    #[test]
    fn an_ended_scenario_gives_its_name_and_how_long_it_took() {
        let start = Instant::now();
        let mut running = Running::new(LIMIT);
        running.began(1, "slow".to_owned(), start);
        running.began(2, "quick".to_owned(), start + Duration::from_secs(1));
        assert_eq!(
            running.ended(1, start + Duration::from_secs(4)),
            Some(("slow".to_owned(), Duration::from_secs(4)))
        );
        assert_eq!(
            running.ended(2, start + Duration::from_secs(2)),
            Some(("quick".to_owned(), Duration::from_secs(1)))
        );
    }

    /// The variable that makes this test binary, started again by
    /// [`hung_run`], play the hung run.
    const HUNG_RUN_ENV: &str = "TESTKIT_SCENARIO_WATCH_HUNG_RUN";

    /// The hung run: one scenario begins and ends, then another begins and
    /// never ends.
    fn play_hung_run() {
        let watch = ScenarioWatch::start("hung runner", Duration::from_millis(100));
        let demo = Some(Path::new("features/demo.feature"));
        let quick = "A step that returns";
        watch.began(quick, ScenarioWatch::describe(quick, 1, demo, "Demo"));
        watch.ended(quick);
        let hung = "A step that waits for ever";
        watch.began(hung, ScenarioWatch::describe(hung, 3, demo, "Demo"));
        thread::sleep(Duration::from_secs(30));
    }

    /// Runs [`play_hung_run`] in this test binary started again, its stdout
    /// a pipe as in CI, as the test `test` names. Gives back its exit status,
    /// its stderr and how long it ran.
    fn hung_run(test: &str) -> (Option<i32>, String, Duration) {
        let begun = Instant::now();
        let mut hung = Command::new(std::env::current_exe().unwrap());
        hung.args(["--exact", test, "--nocapture"])
            .env(HUNG_RUN_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = spawn_gate::hold(|| hung.spawn())
            .unwrap()
            .wait_with_output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&child.stderr).into_owned();
        (child.status.code(), stderr, begun.elapsed())
    }

    #[test]
    fn a_scenario_over_the_limit_stops_the_process_and_is_named() {
        if std::env::var_os(HUNG_RUN_ENV).is_some() {
            play_hung_run();
            return;
        }
        let (status, stderr, took) = hung_run(
            "scenario_watch::tests::a_scenario_over_the_limit_stops_the_process_and_is_named",
        );
        assert_eq!(status, Some(1), "stderr: {stderr}");
        assert!(
            stderr.contains(
                "hung runner: the scenario \"A step that waits for ever\" \
                 (features/demo.feature:3) has run for"
            ),
            "stderr: {stderr}"
        );
        assert!(
            took < Duration::from_secs(20),
            "the watch stopped the run only after {took:?}"
        );
    }

    /// What a run CI stops at its own limit leaves in the log (#638): the
    /// scenario that began and never ended.
    #[test]
    fn with_stdout_no_terminal_each_scenario_is_named_as_it_begins_and_ends() {
        if std::env::var_os(HUNG_RUN_ENV).is_some() {
            play_hung_run();
            return;
        }
        let (_, stderr, _) = hung_run(
            "scenario_watch::tests::\
             with_stdout_no_terminal_each_scenario_is_named_as_it_begins_and_ends",
        );
        let said: Vec<&str> = stderr
            .lines()
            .filter(|line| {
                line.starts_with("hung runner: began ") || line.starts_with("hung runner: ended ")
            })
            .collect();
        assert_eq!(said.len(), 3, "stderr: {stderr}");
        assert_eq!(
            said[0],
            "hung runner: began \"A step that returns\" (features/demo.feature:1)"
        );
        assert!(
            said[1].starts_with(
                "hung runner: ended \"A step that returns\" (features/demo.feature:1) after "
            ),
            "stderr: {stderr}"
        );
        assert_eq!(
            said[2],
            "hung runner: began \"A step that waits for ever\" (features/demo.feature:3)"
        );
    }
}
