//! Builds a declared history (`tests/fixtures/history/*.toml`) into a new
//! repository, for screenshots and demos; `just try --demo` runs it.
//!
//! ```text
//! history-fixture <declaration> <repository> <sources> <machine-id>
//! ```
//!
//! The repository folder must not hold a repository yet. Its password is the
//! canary `demo-password`, the one the `@ui` journeys' repositories have.
//! Each plan's source folder is written under `<sources>`, and printed on
//! standard output, one line per plan: its ID, a tab, the folder.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use domain::{AgentId, Uuid};
use engine::history_fixture::{self, History, Start, Target};

/// A canary, never a real password (development.md §13.9).
const PASSWORD: &str = "demo-password";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("history-fixture: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [declaration, repository, sources, agent] = args.as_slice() else {
        return Err(
            "usage: history-fixture <declaration> <repository> <sources> <machine-id>".to_owned(),
        );
    };
    let text =
        std::fs::read_to_string(declaration).map_err(|error| format!("{declaration}: {error}"))?;
    let history: History =
        toml::from_str(&text).map_err(|error| format!("{declaration}: {error}"))?;
    let agent = Uuid::try_parse(agent.trim())
        .map_err(|error| format!("the agent ID {agent:?}: {error}"))?;
    let target = Target {
        repository: PathBuf::from(repository),
        start: Start::Init,
        password: PASSWORD.to_owned(),
        sources: PathBuf::from(sources),
        agent: AgentId::from_uuid(agent),
    };
    let started = Instant::now();
    let built = history_fixture::build(&history, &target)
        .map_err(|error| format!("{declaration}: {error}"))?;
    let failed = built
        .iter()
        .filter(|snapshot| snapshot.id.is_none())
        .count();
    eprintln!(
        "history-fixture: built {} snapshots into {repository} in {:.1} s; failed runs, which left no snapshot: {failed}",
        built.len().saturating_sub(failed),
        started.elapsed().as_secs_f64()
    );
    let mut plans: Vec<&str> = built
        .iter()
        .map(|snapshot| snapshot.plan.as_str())
        .collect();
    plans.sort_unstable();
    plans.dedup();
    for plan in plans {
        println!("{plan}\t{}", target.sources.join(plan).display());
    }
    Ok(())
}
