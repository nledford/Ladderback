//! Which machines the hub's watch finds due an offline alert (decision 168).

use std::sync::Arc;

use domain::fleet::{AgentRecord, AgentStatus, Hostname, OFFLINE_ALERT_AFTER, PublicKey};
use domain::notifications::Event;
use domain::{AgentId, Timestamp, Uuid};

use crate::fakes::{FakeClock, FakeSigning, InMemoryAgentRegistry, InMemoryIdentity, SeededRandom};
use crate::pairing::HubPairing;

fn agent_id(n: u8) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(u128::from(n)))
}

fn hostname(n: u8) -> Hostname {
    Hostname::parse(&format!("laptop-{n}")).unwrap()
}

/// Machine `n`, registered with `status` and never connected.
fn machine(n: u8, status: AgentStatus) -> AgentRecord {
    let at = Timestamp::from_second(1_700_000_000).unwrap();
    AgentRecord {
        agent_id: agent_id(n),
        key: PublicKey::from_bytes([n; 32]).unwrap(),
        hostname: hostname(n),
        status,
        counter: 1,
        last_seen: at,
        zone: None,
    }
}

/// Only an approved machine is alerted: one that waits for a person to
/// approve it is never due, however long it is offline.
#[tokio::test]
async fn a_machine_waiting_for_approval_is_never_due() {
    let start = Timestamp::from_second(1_800_000_000).unwrap();
    let registry = Arc::new(InMemoryAgentRegistry::new());
    registry.hold(machine(1, AgentStatus::Pending { since: start }));
    registry.hold(machine(2, AgentStatus::Approved { at: start }));
    let clock = Arc::new(FakeClock::new(start));
    let pairing = HubPairing::new(
        registry,
        Arc::new(InMemoryIdentity::default()),
        Arc::new(FakeSigning),
        Arc::new(SeededRandom::new(1)),
        clock.clone(),
        "0.5.0-hub".to_owned(),
    );
    assert_eq!(pairing.alerts_due().await.unwrap(), vec![]);
    clock.advance(OFFLINE_ALERT_AFTER);
    assert_eq!(
        pairing.alerts_due().await.unwrap(),
        vec![(agent_id(2), Event::machine_offline(hostname(2), start))]
    );
}
