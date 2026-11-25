//! Steps for `features/fleet/removing_a_machine.feature`: a person rejects
//! or removes a machine on the hub's page, and a waiting machine lapses
//! (§7.1; §16, decision 120, "Pending approval", "Removing, re-keying and
//! recovering an agent").

use app::pairing::{ConnectFailure, RemovalAsk, TokenAskRefused};
use app::ports::{AgentRegistry as _, Clock as _, Signing as _};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::fleet::{AgentStatus, PublicKey, RemovalRefused, SigningSeed, Tombstone};

use super::{Agent, PairingWorld};
use app::fakes::FakeSigning;

/// What the hub answered the last removal asked for on its page.
pub(super) type Removal = Result<Result<Tombstone, RemovalRefused>, TokenAskRefused>;

impl PairingWorld {
    /// The person removes the agent named `hostname` on the hub's page,
    /// under `key` or the key its row shows.
    async fn ask_to_remove(&mut self, hostname: &str, key: Option<PublicKey>) {
        let record = self.agent_named(hostname).await;
        let ask = RemovalAsk {
            agent_id: record.agent_id,
            agent_key: key.unwrap_or(record.key),
            beyond_loopback: false,
        };
        self.removed = Some(self.hub().pairing.remove_asked(ask).await);
    }

    fn removal(&self) -> &Removal {
        self.removed.as_ref().expect("a removal asked for")
    }
}

#[when(regex = r#"^the person (?:rejects|removes) "([^"]+)" on the hub's page$"#)]
async fn removes(agent: &mut Agent, hostname: String) {
    agent.pairing.ask_to_remove(&hostname, None).await;
}

#[given(regex = r#"^the person (?:removed|rejected) "([^"]+)" on the hub's page$"#)]
async fn removed_before(agent: &mut Agent, hostname: String) {
    let world = &mut agent.pairing;
    world.ask_to_remove(&hostname, None).await;
    assert!(matches!(world.removal(), Ok(Ok(_))), "{:?}", world.removed);
}

#[when(expr = "the person removes {string} on the hub's page with another key")]
async fn removes_with_another_key(agent: &mut Agent, hostname: String) {
    let key = FakeSigning
        .public_key(&SigningSeed::from_bytes([0x5a; 32]))
        .expect("a key");
    agent.pairing.ask_to_remove(&hostname, Some(key)).await;
}

#[then("the hub refuses the removal because no such machine is registered")]
fn removal_refused(agent: &mut Agent) {
    assert_eq!(agent.pairing.removal(), &Ok(Err(RemovalRefused::Unknown)));
}

#[then(regex = r#"^the hub keeps (a|no) tombstone of "([^"]+)"$"#)]
async fn keeps_tombstone(agent: &mut Agent, kept: String, hostname: String) {
    let world = &agent.pairing;
    let first = world.first().clone();
    assert_eq!(first.hostname.as_str(), hostname);
    let tombstone = world
        .hub()
        .registry
        .tombstone(first.agent_id)
        .await
        .expect("the tombstone");
    if kept == "a" {
        let tombstone = tombstone.expect("a tombstone");
        assert_eq!(tombstone.hostname, first.hostname);
        assert_eq!(tombstone.removed_at, world.hub().clock.now());
    } else {
        assert_eq!(tombstone, None);
    }
}

#[then("the hub tells the agent it removed it")]
fn told_removed(agent: &mut Agent) {
    let failure = agent.pairing.connect_failure();
    let ConnectFailure::Refused(refused) = failure else {
        panic!("the agent wasn't told it was removed: {failure:?}");
    };
    assert_eq!(refused.code.as_str(), "machine-removed");
}

#[when("the agent pairs again with a new token")]
async fn pairs_again(agent: &mut Agent) {
    let world = &mut agent.pairing;
    world.new_token().await;
    let machine = world.first().clone();
    world.pair_and_keep(&machine).await;
}

#[when(regex = r"^(\d+) (days?|hours?)(?: and (\d+) hours)? pass(?:es)? on the hub's clock$")]
fn time_passes(agent: &mut Agent, count: i64, unit: String, hours: String) {
    let hours = if hours.is_empty() {
        0
    } else {
        hours.parse::<i64>().expect("hours")
    };
    let hours = if unit.starts_with("day") {
        count * 24 + hours
    } else {
        count + hours
    };
    agent
        .pairing
        .hub()
        .clock
        .advance(SignedDuration::from_hours(hours));
}

#[then(expr = "the hub still lists {string} as approved")]
async fn still_approved(agent: &mut Agent, hostname: String) {
    let listed = agent.pairing.agent_named(&hostname).await;
    assert!(
        matches!(listed.status, AgentStatus::Approved { .. }),
        "{listed:?}"
    );
}

#[then(expr = "the hub lists only {string}")]
async fn lists_only(agent: &mut Agent, hostname: String) {
    let listed: Vec<String> = agent
        .pairing
        .agents()
        .await
        .into_iter()
        .map(|record| record.hostname.to_string())
        .collect();
    assert_eq!(listed, [hostname]);
}
