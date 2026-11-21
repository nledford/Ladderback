use std::sync::Arc;

use api::sync::{AgentFrame, HubFrame};
use domain::fleet::{BindingKind, Connect, Nonce, TokenTls};

use super::*;
use crate::fakes::InMemoryIdentity;
use crate::pairing::tests::{EXPORTER, Hub, agent, agent_id, laptop};
use crate::pairing::wire::{connect_frame, pair_frame};
use crate::ports::AgentRegistry;

fn connect_of(kind: u8) -> AgentFrame {
    connect_frame(&Connect {
        agent_id: agent_id(),
        agent_nonce: Nonce::from_bytes([3; KEY_BYTES]),
        binding_kind: BindingKind::from_byte(kind).unwrap(),
    })
}

/// The agent's `Connect` gets the hub's `Challenge`, and its `Pair` gets
/// the hub's signed answer, which ends the session: here a refusal, since
/// the `Pair` was signed for another connection's nonce.
#[tokio::test]
async fn a_connect_is_challenged_and_a_pair_answered() {
    let hub = Hub::new(1);
    let token = hub.token(TokenTls::Ca).await;
    let agent = agent(Arc::new(InMemoryIdentity::default()));
    let (connect, handshake) = agent
        .connect(token, agent_id(), laptop(), None)
        .await
        .unwrap();
    let mut session = hub.pairing.session(None).await.unwrap();
    let step = hub
        .pairing
        .step(&mut session, &connect_frame(&connect))
        .await;
    assert!(
        matches!(step, HubStep::Reply(HubFrame::Challenge { .. })),
        "{step:?}"
    );
    let keys = hub.pairing.keys().await.unwrap();
    let (elsewhere, _) = hub.pairing.challenge(&keys, &connect, None).unwrap();
    let (pair, _) = agent
        .answer_challenge(handshake, &elsewhere, None)
        .await
        .unwrap();
    let step = hub.pairing.step(&mut session, &pair_frame(&pair)).await;
    let HubStep::Last(HubFrame::Refused { code, .. }) = step else {
        panic!("{step:?}");
    };
    assert_eq!(code, "proof-refused");
    assert!(hub.registry.agents().await.unwrap().is_empty());
}

/// A `Pair` before any `Connect`, a second `Connect`, and a frame whose
/// fields don't read close the connection unanswered.
#[tokio::test]
async fn a_message_out_of_turn_closes_the_connection() {
    let hub = Hub::new(1);
    let token = hub.token(TokenTls::Ca).await;
    let agent = agent(Arc::new(InMemoryIdentity::default()));
    let (connect, handshake) = agent
        .connect(token, agent_id(), laptop(), None)
        .await
        .unwrap();
    let keys = hub.pairing.keys().await.unwrap();
    let (challenge, _) = hub.pairing.challenge(&keys, &connect, None).unwrap();
    let (pair, _) = agent
        .answer_challenge(handshake, &challenge, None)
        .await
        .unwrap();

    let mut session = hub.pairing.session(None).await.unwrap();
    let step = hub.pairing.step(&mut session, &pair_frame(&pair)).await;
    assert!(matches!(step, HubStep::Close(PROTOCOL_ERROR)), "{step:?}");

    let mut session = hub.pairing.session(None).await.unwrap();
    hub.pairing.step(&mut session, &connect_of(0)).await;
    let step = hub.pairing.step(&mut session, &connect_of(0)).await;
    assert!(matches!(step, HubStep::Close(PROTOCOL_ERROR)), "{step:?}");

    let mut session = hub.pairing.session(None).await.unwrap();
    let AgentFrame::Connect {
        agent_nonce,
        binding_kind,
        ..
    } = connect_of(0)
    else {
        panic!("a Connect frame")
    };
    let unreadable = AgentFrame::Connect {
        agent_id: "not-a-uuid".to_owned(),
        agent_nonce,
        binding_kind,
    };
    let step = hub.pairing.step(&mut session, &unreadable).await;
    assert!(matches!(step, HubStep::Close(PROTOCOL_ERROR)), "{step:?}");
    assert!(hub.registry.agents().await.unwrap().is_empty());
}

/// Binding kind 1 on a session without the hub's own TLS closes with
/// `binding-unavailable`; on one with it, the hub challenges.
#[tokio::test]
async fn the_hub_binds_only_where_its_own_tls_is() {
    let hub = Hub::new(1);
    let mut session = hub.pairing.session(None).await.unwrap();
    let step = hub.pairing.step(&mut session, &connect_of(1)).await;
    assert!(
        matches!(step, HubStep::Close(BINDING_UNAVAILABLE)),
        "{step:?}"
    );
    let mut session = hub.pairing.session(Some(EXPORTER)).await.unwrap();
    let step = hub.pairing.step(&mut session, &connect_of(1)).await;
    assert!(
        matches!(step, HubStep::Reply(HubFrame::Challenge { .. })),
        "{step:?}"
    );
}
