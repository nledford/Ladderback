//! Steps for `features/maintenance/other_hosts.feature`: they give the
//! scripted repository of the forget scenarios snapshots plain rustic made on
//! other machines, ask this agent to maintain it, and read the other hosts
//! the groups, a preview and a run warn of.

use cucumber::{given, then};
use domain::execution::WarningKind;
use domain::execution::plan_tags::{AGENT_TAG_PREFIX, PLAN_TAG_PREFIX};
use domain::maintenance::OtherHosts;
use domain::planning::{Hostname, KeepCount, Maintenance, Plan, Source};
use domain::{PlanId, RepositoryAlias};

use super::Agent;
use super::forget_steps::{other_agent, self_agent};

/// The other hosts a step names: `, `-separated, in order.
fn named(hosts: &str) -> Vec<String> {
    hosts.split(", ").map(str::to_owned).collect()
}

fn assert_names(found: &OtherHosts, hosts: &str) {
    assert_eq!(found.hosts(), named(hosts), "{found:?}");
}

#[given("the repository is maintained by this agent")]
async fn maintained_here(agent: &mut Agent) {
    agent.forget.maintained_by(Maintenance::Local).await;
}

#[given(expr = "this agent's hostname is {string}")]
async fn agent_hostname(agent: &mut Agent, hostname: String) {
    agent.forget.name_agent(&hostname).await;
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s) that records the host {string}")]
async fn plan_with_host(agent: &mut Agent, plan: String, keep: i64, host: String) {
    agent.forget.repository().await;
    let mut draft = Plan::new(
        PlanId::parse(&plan).expect("a plan ID"),
        RepositoryAlias::parse("demo").expect("an alias"),
        vec![Source::parse("/home/demo").expect("a source")],
    );
    draft.retention.keep_last = Some(KeepCount::new(keep).expect("a count"));
    draft.host = Some(Hostname::parse(&host).expect("a hostname"));
    agent.forget.plan_raw(draft).await;
}

#[given(expr = "the repository holds {int} snapshot(s) plain rustic made on the host {string}")]
fn made_on(agent: &mut Agent, count: u64, host: String) {
    agent.forget.snapshots_on(count, &[], "/home/demo", &host);
}

#[given(
    regex = r#"^the repository holds (\d+) snapshots? (this|another) agent's plan "([^"]+)" made on the host "([^"]+)"$"#
)]
fn made_by_agent_on(agent: &mut Agent, count: u64, whose: String, plan: String, host: String) {
    let by = if whose == "this" {
        self_agent()
    } else {
        other_agent()
    };
    let tags = [
        format!("{PLAN_TAG_PREFIX}{plan}"),
        format!("{AGENT_TAG_PREFIX}{}", by.as_uuid().hyphenated()),
    ];
    agent.forget.snapshots_on(count, &tags, "/home/demo", &host);
}

#[then(expr = "the groups warn of snapshots from the other hosts {string}")]
fn groups_warn(agent: &mut Agent, hosts: String) {
    assert_names(&agent.preview.groups().other_hosts, &hosts);
}

#[then("the groups warn of no other host")]
fn groups_warn_of_none(agent: &mut Agent) {
    let found = &agent.preview.groups().other_hosts;
    assert!(found.is_empty(), "{found:?}");
}

#[then(expr = "the preview warns of snapshots from the other hosts {string}")]
fn preview_warns(agent: &mut Agent, hosts: String) {
    assert_names(agent.preview.other_hosts(), &hosts);
}

#[then("the preview warns of no other host")]
fn preview_warns_of_none(agent: &mut Agent) {
    let found = agent.preview.other_hosts();
    assert!(found.is_empty(), "{found:?}");
}

#[then(
    regex = r#"^the operation warns that its (forget|adoption|prune) ran beside snapshots from the other hosts "([^"]+)"$"#
)]
fn operation_warns(agent: &mut Agent, run: String, hosts: String) {
    // The warning names the run as its operation kind does.
    let during = if run == "adoption" { "adopt" } else { &run };
    let warnings: Vec<_> = agent
        .operation()
        .warnings()
        .iter()
        .filter(|warning| warning.kind == WarningKind::OtherHosts)
        .collect();
    let [warning] = warnings.as_slice() else {
        panic!("one warning of other hosts: {warnings:?}");
    };
    assert_eq!(warning.during, during, "{warning:?}");
    // Counted, and named in full: no scenario has more than five.
    let detail = format!("{}: {hosts}", named(&hosts).len());
    assert_eq!(warning.detail, detail, "{warning:?}");
    assert_eq!(warning.item, None, "{warning:?}");
}
