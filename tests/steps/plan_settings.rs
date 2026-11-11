//! Steps for `features/execution/plan_settings.feature`: the plan's source
//! filter and host and the agent's hostname, written to the configuration of
//! the real daemon, and the backup's snapshot read back with `rustic_core`,
//! never through Ladderback.

use std::fs;

use cucumber::{given, then};
use domain::planning::Hostname;

use super::Agent;
use super::backup_steps::{back_up_without_ladderback, names_in_newest, newest_snapshot};
use super::events_steps::save_daemon_config;

fn plan_folder(agent: &Agent) -> std::path::PathBuf {
    agent.backup.source.clone().expect("a plan's folder")
}

fn hostname(text: &str) -> Hostname {
    Hostname::parse(text).expect("a hostname")
}

#[given(expr = "the plan's folder holds a {string} folder")]
fn holds_a_folder(agent: &mut Agent, name: String) {
    let folder = plan_folder(agent).join(name).join("pkg");
    fs::create_dir_all(&folder).expect("the folder");
    fs::write(folder.join("index.js"), "demo\n").expect("a file in the folder");
}

/// A `.git` folder makes the plan's folder a git repository, which is where
/// rustic applies a `.gitignore` unless told `no-require-git`.
#[given(expr = "the plan's folder is a git repository whose {string} lists {string}")]
fn a_git_repository(agent: &mut Agent, ignore_file: String, listed: String) {
    let folder = plan_folder(agent);
    fs::create_dir_all(folder.join(".git")).expect("the .git folder");
    fs::write(folder.join(ignore_file), format!("{listed}\n")).expect("the ignore file");
    fs::write(folder.join(listed), "ignored\n").expect("the ignored file");
}

#[given(expr = "the plan's source filter has the iglob {string} and follows \".gitignore\" files")]
async fn a_source_filter(agent: &mut Agent, iglob: String) {
    agent.backup.filter.iglobs = vec![iglob];
    agent.backup.filter.git_ignore = true;
    save_daemon_config(agent, None).await;
}

#[given(expr = "the agent's hostname is {string}")]
async fn the_agents_hostname(agent: &mut Agent, name: String) {
    agent.backup.agent_hostname = Some(hostname(&name));
    save_daemon_config(agent, None).await;
}

#[given(expr = "the plan's host is {string}")]
async fn the_plans_host(agent: &mut Agent, name: String) {
    agent.backup.plan_host = Some(hostname(&name));
    save_daemon_config(agent, None).await;
}

#[given(expr = "the folder was backed up without Ladderback under the host {string}")]
async fn backed_up_under_a_host(agent: &mut Agent, host: String) {
    back_up_without_ladderback(agent, Some(host)).await;
}

#[then(expr = "the backup's snapshot holds the plan's {int} files")]
async fn holds_the_files(agent: &mut Agent, count: usize) {
    let names = names_in_newest(agent).await;
    for n in 1..=count {
        let file = format!("file-{n}.txt");
        assert!(names.contains(&file), "{file} is missing: {names:?}");
    }
}

#[then(expr = "the backup's snapshot holds nothing named {string} or {string}")]
async fn holds_neither(agent: &mut Agent, first: String, second: String) {
    let names = names_in_newest(agent).await;
    for left_out in [first, second] {
        assert!(!names.contains(&left_out), "{left_out} is in it: {names:?}");
    }
}

#[then(expr = "the backup's snapshot records the host {string}")]
async fn records_the_host(agent: &mut Agent, host: String) {
    assert_eq!(newest_snapshot(agent).await.hostname, host);
}
