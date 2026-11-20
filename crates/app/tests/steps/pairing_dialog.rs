//! Steps for `features/fleet/pairing_dialog.feature`: a pairing token asked
//! for on the hub's web page, with its step-up (§16, decision 120, "Who may
//! make a token, approve or re-key"; decision 121, "The hub's dialog").

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use app::access::Access;
use app::fakes::{
    FakeSigning, InMemoryAdminPassword, InMemoryApiTokenStore, InMemorySessionStore, SeededRandom,
};
use app::pairing::{TokenAsk, TokenAskRefused};
use app::ports::{AgentRegistry, Clock, Signing};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::access::step_up::StepUpConfirmation;
use domain::fleet::{HubAddress, KeyId, TokenId, TokenTls};

use super::{Agent, Hub, PairingWorld};

const PASSWORD: &str = "demo-admin-pass";
const ADDRESS: &str = "https://hub.example:9443";

impl PairingWorld {
    /// Asks the hub for a token as its page does, from beyond loopback or
    /// not, confirmed by `confirmation`; a token made becomes the world's.
    async fn ask_for_token(&mut self, beyond_loopback: bool, confirmation: &str) {
        let ask = TokenAsk {
            address: HubAddress::parse(ADDRESS).expect("a hub address"),
            life: None,
            peer: if beyond_loopback {
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7))
            } else {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            },
            beyond_loopback,
            confirmation: StepUpConfirmation::parse(confirmation.to_owned())
                .expect("a confirmation"),
        };
        let asked = self.hub().pairing.make_token_asked(ask).await;
        if let Ok(made) = &asked {
            self.token = Some(made.token.clone());
        }
        self.asked = Some(asked);
    }

    fn key_id(&self) -> String {
        KeyId::of_key(self.hub().keys.key()).to_string()
    }

    fn ask_refusal(&self) -> &TokenAskRefused {
        match self.asked.as_ref().expect("a token asked for") {
            Err(refused) => refused,
            Ok(made) => panic!("a token was made: {:?}", made.id),
        }
    }
}

#[given("a hub with sign-in on")]
async fn sign_in_on(agent: &mut Agent) {
    let clock = Arc::new(domain_clock());
    let access = Access::new(
        Arc::new(InMemoryAdminPassword::with(PASSWORD)),
        Arc::new(SeededRandom::new(3)),
        clock,
        Arc::new(InMemoryApiTokenStore::new()),
        Arc::new(InMemorySessionStore::new()),
    );
    agent.pairing.hub = Some(Hub::with_access(1, Some(Arc::new(access))).await);
}

fn domain_clock() -> app::fakes::FakeClock {
    app::fakes::FakeClock::new(domain::Timestamp::from_second(1_800_000_000).unwrap())
}

#[given("a hub with sign-in off")]
async fn sign_in_off(agent: &mut Agent) {
    agent.pairing.hub = Some(Hub::new(1).await);
}

#[given("the person has made a pairing token on this machine")]
async fn made_on_this_machine(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let key_id = world.key_id();
    world.ask_for_token(false, &key_id).await;
    assert!(matches!(world.asked, Some(Ok(_))), "{:?}", world.asked);
}

#[when("the person asks for a new pairing token and confirms with a wrong password")]
async fn asks_with_wrong_password(agent: &mut Agent) {
    agent.pairing.ask_for_token(false, "not-the-password").await;
}

#[when("the person asks for a new pairing token and confirms with the admin password")]
async fn asks_with_password(agent: &mut Agent) {
    agent.pairing.ask_for_token(false, PASSWORD).await;
}

#[when(
    "the person asks for a new pairing token from another computer, confirming with the hub's key ID"
)]
async fn asks_from_afar(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let key_id = world.key_id();
    world.ask_for_token(true, &key_id).await;
}

#[when(
    "the person asks for a new pairing token on this machine and types a key ID that doesn't match"
)]
async fn asks_with_wrong_key_id(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let wrong = if world.key_id() == "00000000" {
        "11111111"
    } else {
        "00000000"
    };
    world.ask_for_token(false, wrong).await;
}

#[when("the person asks for a new pairing token on this machine and types the hub's key ID")]
async fn asks_with_key_id(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let key_id = world.key_id().to_lowercase();
    world.ask_for_token(false, &key_id).await;
}

#[when("the person closes the dialog")]
async fn closes_dialog(agent: &mut Agent) {
    let world = &agent.pairing;
    let Some(Ok(made)) = &world.asked else {
        panic!("no token was made: {:?}", world.asked);
    };
    assert!(world.hub().pairing.revoke_token(made.id).await.unwrap());
}

#[then("the hub refuses it as a step-up that failed")]
fn refused_step_up(agent: &mut Agent) {
    assert_eq!(agent.pairing.ask_refusal(), &TokenAskRefused::StepUpFailed);
}

#[then("the hub refuses it and says to set an admin password")]
fn refused_needs_sign_in(agent: &mut Agent) {
    assert_eq!(agent.pairing.ask_refusal(), &TokenAskRefused::NeedsSignIn);
}

#[then("the hub shows a token for its own key and the address asked for")]
async fn shows_token(agent: &mut Agent) {
    let world = &agent.pairing;
    let Some(Ok(made)) = &world.asked else {
        panic!("no token was made: {:?}", world.asked);
    };
    let hub = world.hub();
    assert_eq!(made.hub_key, *hub.keys.key());
    assert_eq!(made.token.hub_key(), hub.keys.key());
    assert_eq!(made.token.address().as_str(), ADDRESS);
    let pairing_key = FakeSigning.public_key(made.token.pairing_seed()).unwrap();
    assert_eq!(made.id, TokenId::of_pairing_key(&pairing_key));
    assert_eq!(made.made_at, hub.clock.now());
    assert_eq!(
        made.made_at.duration_until(made.expires_at),
        SignedDuration::from_mins(15)
    );
}

#[then("the token trusts a certificate authority's certificate, as the hub serves none of its own yet")]
fn trusts_a_ca(agent: &mut Agent) {
    let Some(Ok(made)) = &agent.pairing.asked else {
        panic!("no token was made: {:?}", agent.pairing.asked);
    };
    assert_eq!(made.token.tls(), TokenTls::Ca);
}

#[then(regex = r"^the hub lists (\d+) unused tokens?$")]
async fn unused_tokens(agent: &mut Agent, count: usize) {
    let hub = agent.pairing.hub();
    let unused = hub.registry.unused_tokens(hub.clock.now()).await;
    assert_eq!(unused.expect("the unused tokens").len(), count);
}
