//! Steps for `features/fleet/pairing_dialog.feature` and
//! `features/fleet/approving_on_the_hubs_page.feature`: a pairing token
//! asked for, and a waiting agent approved, on the hub's web page, with the
//! step-up (§16, decision 120, "Who may make a token, approve or re-key";
//! decision 121, "The hub's dialog", "Waiting for approval").

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use app::access::Access;
use app::fakes::{
    FakeSigning, InMemoryAdminPassword, InMemoryApiTokenStore, InMemorySessionStore, SeededRandom,
};
use app::pairing::{ApprovalAsk, TokenAsk, TokenAskRefused};
use app::ports::{AgentRegistry, Clock, Signing};
use cucumber::{given, then, when};
use domain::SignedDuration;
use domain::access::step_up::StepUpConfirmation;
use domain::fleet::{
    AgentStatus, ApprovalRefused, Fingerprint, HubAddress, PublicKey, SigningSeed, TokenId,
    TokenTls,
};

use super::{Agent, Hub, PairingWorld};

const PASSWORD: &str = "demo-admin-pass";
const ADDRESS: &str = "https://hub.example:9443";

/// The fingerprint of the certificate the hub's built-in TLS serves.
const CERTIFICATE: Fingerprint = Fingerprint::from_digest([0x3c; 32]);

impl PairingWorld {
    /// Asks the hub for a token as its page does, from beyond loopback or
    /// not, confirmed by `confirmation`, for an address behind a certificate
    /// authority's certificate; a token made becomes the world's.
    async fn ask_for_token(&mut self, beyond_loopback: bool, confirmation: &str) {
        self.ask_for_token_of(beyond_loopback, confirmation, TokenTls::Ca)
            .await;
    }

    /// Asks as [`Self::ask_for_token`] does, for an address of `tls`'s kind.
    async fn ask_for_token_of(&mut self, beyond_loopback: bool, confirmation: &str, tls: TokenTls) {
        let ask = TokenAsk {
            address: HubAddress::parse(ADDRESS).expect("a hub address"),
            tls,
            life: None,
            peer: if beyond_loopback {
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7))
            } else {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            },
            beyond_loopback,
            confirmation: StepUpConfirmation::parse(confirmation.to_owned()).ok(),
        };
        let asked = self.hub().pairing.make_token_asked(ask).await;
        if let Ok(made) = &asked {
            self.token = Some(made.token.clone());
        }
        self.asked = Some(asked);
    }

    /// The hub key's fingerprint, as every screen shows it.
    fn fingerprint(&self) -> String {
        Fingerprint::of_key(self.hub().keys.key()).to_string()
    }

    /// The hub key's key ID, which a step-up with sign-in off asked for
    /// before the fingerprint (#1344): the first 8 hex digits of its SHA-256.
    fn former_key_id(&self) -> String {
        let digest = Fingerprint::of_key(self.hub().keys.key()).full_hex();
        digest.get(..8).expect("8 hex digits").to_uppercase()
    }

    fn ask_refusal(&self) -> &TokenAskRefused {
        match self.asked.as_ref().expect("a token asked for") {
            Err(refused) => refused,
            Ok(made) => panic!("a token was made: {:?}", made.id),
        }
    }

    /// Asks the hub to approve the agent registered as `hostname`, as its
    /// page does, under `key` or the key it registered.
    async fn ask_to_approve(
        &mut self,
        hostname: &str,
        beyond_loopback: bool,
        key: Option<PublicKey>,
        confirmation: &str,
    ) {
        let record = self.agent_named(hostname).await;
        let ask = ApprovalAsk {
            agent_id: record.agent_id,
            agent_key: key.unwrap_or(record.key),
            peer: peer(beyond_loopback),
            beyond_loopback,
            confirmation: StepUpConfirmation::parse(confirmation.to_owned()).ok(),
        };
        self.approved = Some(self.hub().pairing.approve_asked(ask).await);
    }

    async fn agent_named(&self, hostname: &str) -> domain::fleet::AgentRecord {
        self.agents()
            .await
            .into_iter()
            .find(|agent| agent.hostname.as_str() == hostname)
            .unwrap_or_else(|| panic!("an agent named {hostname}"))
    }

    fn approval(
        &self,
    ) -> &Result<Result<domain::fleet::AgentRecord, ApprovalRefused>, TokenAskRefused> {
        self.approved.as_ref().expect("an approval asked for")
    }
}

fn peer(beyond_loopback: bool) -> IpAddr {
    if beyond_loopback {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7))
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
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
    let fingerprint = world.fingerprint();
    world.ask_for_token(false, &fingerprint).await;
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
    "the person asks for a new pairing token for the hub's own certificate and confirms with the admin password"
)]
async fn asks_pinned_with_password(agent: &mut Agent) {
    agent
        .pairing
        .ask_for_token_of(false, PASSWORD, TokenTls::Pinned(CERTIFICATE))
        .await;
}

#[when(
    "the person asks for a new pairing token from another computer, confirming with the hub key's fingerprint"
)]
async fn asks_from_afar(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let fingerprint = world.fingerprint();
    world.ask_for_token(true, &fingerprint).await;
}

#[when(
    "the person asks for a new pairing token on this machine and types a fingerprint that doesn't match"
)]
async fn asks_with_wrong_fingerprint(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let wrong = if world.fingerprint() == "0000-0000-0000-0000-0000" {
        "1111-1111-1111-1111-1111"
    } else {
        "0000-0000-0000-0000-0000"
    };
    world.ask_for_token(false, wrong).await;
}

#[when(
    "the person asks for a new pairing token on this machine and types the key ID it asked for before"
)]
async fn asks_with_former_key_id(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let key_id = world.former_key_id();
    world.ask_for_token(false, &key_id).await;
}

#[when(
    "the person asks for a new pairing token on this machine and types the hub key's fingerprint"
)]
async fn asks_with_fingerprint(agent: &mut Agent) {
    let world = &mut agent.pairing;
    let fingerprint = world.fingerprint().to_lowercase();
    world.ask_for_token(false, &fingerprint).await;
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
    assert_eq!(made.left, SignedDuration::from_mins(15));
    assert_eq!(hub.clock.now().duration_until(made.expires_at), made.left);
}

#[given(expr = "the hub's registry takes {int} seconds to keep a token")]
async fn registry_takes(agent: &mut Agent, seconds: i64) {
    let hub = agent.pairing.hub();
    hub.registry
        .takes_to_keep(hub.clock.clone(), SignedDuration::from_secs(seconds));
}

#[then(expr = "the token is shown with {int} seconds of its life left")]
async fn shown_with_left(agent: &mut Agent, seconds: i64) {
    let world = &agent.pairing;
    let Some(Ok(made)) = &world.asked else {
        panic!("no token was made: {:?}", world.asked);
    };
    assert_eq!(made.left, SignedDuration::from_secs(seconds));
    assert_eq!(
        world.hub().clock.now().duration_until(made.expires_at),
        made.left,
        "the life left is counted from the hub's answer"
    );
}

#[then("the token pins the certificate of the hub's built-in TLS")]
fn pins_the_certificate(agent: &mut Agent) {
    let Some(Ok(made)) = &agent.pairing.asked else {
        panic!("no token was made: {:?}", agent.pairing.asked);
    };
    assert_eq!(made.token.tls(), TokenTls::Pinned(CERTIFICATE));
}

#[then(regex = r"^the hub lists (\d+) unused tokens?$")]
async fn unused_tokens(agent: &mut Agent, count: usize) {
    let hub = agent.pairing.hub();
    let unused = hub.registry.unused_tokens(hub.clock.now()).await;
    assert_eq!(unused.expect("the unused tokens").len(), count);
}

#[when(
    regex = r#"^the person approves "([^"]+)" (on the hub's page|from another computer) with (the key it showed|another key), confirming with (the admin password|a wrong password|the hub key's fingerprint)$"#
)]
async fn approves_on_the_page(
    agent: &mut Agent,
    hostname: String,
    from: String,
    key: String,
    confirmation: String,
) {
    let world = &mut agent.pairing;
    let key = (key == "another key").then(|| {
        FakeSigning
            .public_key(&SigningSeed::from_bytes([0x5a; 32]))
            .expect("a key")
    });
    let confirmation = match confirmation.as_str() {
        "the admin password" => PASSWORD.to_owned(),
        "a wrong password" => "not-the-password".to_owned(),
        _ => world.fingerprint(),
    };
    world
        .ask_to_approve(&hostname, from != "on the hub's page", key, &confirmation)
        .await;
}

#[given(expr = "the person approved {string} on the hub's page")]
async fn approved_before(agent: &mut Agent, hostname: String) {
    let world = &mut agent.pairing;
    let fingerprint = world.fingerprint();
    world
        .ask_to_approve(&hostname, false, None, &fingerprint)
        .await;
    assert!(
        matches!(world.approval(), Ok(Ok(_))),
        "{:?}",
        world.approved
    );
}

#[then(expr = "the hub lists {string} as approved")]
async fn listed_approved(agent: &mut Agent, hostname: String) {
    let world = &agent.pairing;
    let Ok(Ok(approved)) = world.approval() else {
        panic!("nothing was approved: {:?}", world.approved);
    };
    let listed = world.agent_named(&hostname).await;
    assert_eq!(&listed, approved);
    assert_eq!(
        listed.status,
        AgentStatus::Approved {
            at: world.hub().clock.now()
        }
    );
}

#[then(expr = "the hub lists {string} as waiting for approval")]
async fn listed_waiting(agent: &mut Agent, hostname: String) {
    let listed = agent.pairing.agent_named(&hostname).await;
    assert!(listed.is_pending(), "{listed:?}");
}

#[then(
    regex = r#"^the hub refuses the approval (as a step-up that failed|and says to set an admin password|because no such machine waits|because "[^"]+" is approved already)$"#
)]
fn approval_refused(agent: &mut Agent, why: String) {
    let expected = match why.as_str() {
        "as a step-up that failed" => Err(TokenAskRefused::StepUpFailed),
        "and says to set an admin password" => Err(TokenAskRefused::NeedsSignIn),
        "because no such machine waits" => Ok(Err(ApprovalRefused::Unknown)),
        _ => Ok(Err(ApprovalRefused::AlreadyApproved)),
    };
    assert_eq!(agent.pairing.approval(), &expected);
}
