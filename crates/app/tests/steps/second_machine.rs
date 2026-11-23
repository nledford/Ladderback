//! Steps for `features/fleet/one_machine_per_id.feature`: a copy of an
//! agent's data folder on a second computer, refused by the hub as a second
//! machine under one machine ID (technical design §7.2; decision 150). The
//! copy holds the first machine's pin and agent key; its computer has
//! another identifier, or none that can be read.

use std::sync::Arc;

use app::fakes::InMemoryIdentity;
use app::pairing::ConnectFailure;
use app::ports::{Clock, Identity, ReportInbox};
use app::sync::SyncFailure;
use cucumber::{given, then, when};
use domain::fleet::{MachineIdentifier, SeedFile};

use super::sync_replay::other;
use super::{Agent, Machine};

/// A copy of the first machine's data folder on a computer whose
/// identifier is `identifier`.
async fn copy_on(agent: &mut Agent, identifier: Option<MachineIdentifier>) {
    let world = &mut agent.pairing;
    let first = world.first().clone();
    let identity = Arc::new(InMemoryIdentity::default());
    let pin = first.identity.pin().await.unwrap().expect("a pin");
    identity.write_pin(&pin).await.unwrap();
    let seed = first
        .identity
        .seed(SeedFile::AgentKey)
        .await
        .unwrap()
        .expect("an agent key");
    identity.keep_seed(SeedFile::AgentKey, seed).await.unwrap();
    world.second = Some(Machine {
        identity,
        identifier,
        seed: first.seed + 500_000,
        ..first
    });
}

#[given("a copy of the agent's data folder on another computer")]
async fn copy_elsewhere(agent: &mut Agent) {
    copy_on(
        agent,
        MachineIdentifier::parse(&format!("{:032x}", 0xc0_u32)).ok(),
    )
    .await;
}

#[given("a copy of the agent's data folder on a computer whose identifier can't be read")]
async fn copy_unread(agent: &mut Agent) {
    copy_on(agent, None).await;
}

#[given(regex = r"^the copy ran (\d+) backups?$")]
async fn copy_backs_up(agent: &mut Agent, count: u32) {
    let side = other(agent);
    for _ in 0..count {
        side.backup(2, false).await;
    }
}

#[when("the copy syncs with its hub")]
async fn copy_syncs(agent: &mut Agent) {
    let machine = agent.pairing.second.take().expect("the copy");
    other(agent).sync(&machine).await;
    agent.pairing.second = Some(machine);
}

#[when("the copy connects to its hub")]
async fn copy_connects(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let machine = world.second.clone().expect("the copy");
    world.connect(&machine, None).await;
}

#[then("the copy is refused because another machine holds its machine ID")]
async fn copy_refused(agent: &mut Agent) {
    let side = other(agent);
    let Some(Err(SyncFailure::Connect(ConnectFailure::Refused(refused)))) = side.synced.last()
    else {
        panic!("{:?}", side.synced.last());
    };
    assert_eq!(refused.code.as_str(), "identity-conflict");
    assert_eq!(refused.holder, None);
}

#[then("the hub holds no report of the copy")]
async fn none_of_the_copy(agent: &mut Agent) {
    let id = agent.pairing.first().agent_id;
    let held = agent.pairing.hub().inbox.reports(id).await.unwrap();
    assert!(held.is_empty(), "{held:?}");
}

#[then(regex = r"^the copy's outbox holds (\d+) reports? the hub hasn't acknowledged$")]
async fn copy_outbox_holds(agent: &mut Agent, count: usize) {
    assert_eq!(other(agent).waiting().await, count);
}

#[then("the copy is told the hub approved it")]
fn copy_told_approved(agent: &mut Agent) {
    assert_eq!(
        agent.pairing.connected().approval,
        domain::fleet::Approval::Approved
    );
}

/// When the hub's list says it last refused a second machine under the
/// agent's ID.
async fn second_machine_at(agent: &Agent) -> Option<domain::Timestamp> {
    let world = &agent.pairing;
    let id = world.first().agent_id;
    let listed = world.hub().pairing.agents().await.unwrap();
    listed
        .into_iter()
        .find(|listed| listed.record.agent_id == id)
        .expect("the agent listed")
        .second_machine_at
}

#[then("the hub lists the agent as having refused a second machine")]
async fn listed_second(agent: &mut Agent) {
    let now = agent.pairing.hub().clock.now();
    assert_eq!(second_machine_at(agent).await, Some(now));
}

#[then("the hub lists no second machine under the agent")]
async fn listed_none(agent: &mut Agent) {
    assert_eq!(second_machine_at(agent).await, None);
}

#[when("a person at the hub forgets the computer it took for the agent")]
async fn forget_the_computer(agent: &mut Agent) {
    let world = &agent.pairing;
    let id = world.first().agent_id;
    let pairing = &world.hub().pairing;
    let listed = pairing.agents().await.unwrap();
    let held = listed
        .into_iter()
        .find(|listed| listed.record.agent_id == id)
        .expect("the agent listed")
        .record;
    pairing.forget_machine(&held).await.unwrap();
}

#[then(regex = r"^the hub holds the copy's (\d+) reports, each once, in the order they happened$")]
async fn hub_holds_the_copys(agent: &mut Agent, count: usize) {
    let id = agent.pairing.first().agent_id;
    let held = agent.pairing.hub().inbox.reports(id).await.unwrap();
    let reports: Vec<_> = held.iter().map(|entry| entry.report.clone()).collect();
    assert_eq!(reports.len(), count, "{held:?}");
    assert_eq!(reports, other(agent).recorded);
}
