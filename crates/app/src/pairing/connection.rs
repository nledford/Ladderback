use std::sync::Arc;

use domain::AgentId;
use domain::fleet::{
    AgentEnvelope, Approval, BindingKind, Connect, EpochSeconds, FIRST_SEQUENCE, Hello, Hostname,
    HubEnvelope, HubPin, HubRoute, Nonce, PROTOCOLS, PairAnswer, PayloadDigest, Proof, PublicKey,
    Refused, RouteRefused, SeedFile, SignedMessage, WelcomeRefused, hello_kind, welcome_kind,
};

use super::agent::{ChallengeRefused, check_challenge, refusal_holds};
use super::wire::{
    HubMessage, agent_envelope_frame, connect_frame, hello_payload, hub_message, proof_frame,
    read_welcome,
};
use crate::ports::{
    Clock, HubConnector, Identity, IdentityError, LinkError, Random, RandomUnavailable, Signing,
    SigningError,
};

/// A paired agent connecting to its hub (§7.1, §7.2; §16, decision 120,
/// "The handshake" and "Signed messages after the handshake"): it checks the
/// hub against its pin before it proves anything, proves itself with its
/// agent key and connection counter, says `Hello`, and takes the hub's
/// signed `Welcome`. One connection, one greeting: the link that stays open
/// is #568's.
pub struct ConnectToHub {
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
    connector: Arc<dyn HubConnector>,
    /// The agent's build, as `--version` names it, which `Hello` carries.
    version: String,
}

/// A connection both sides proved, and what the hub's `Welcome` said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    /// Whether a person has approved this agent on the hub.
    pub approval: Approval,
    /// The pin the agent holds, its generation raised to the hub's.
    pub pin: HubPin,
    /// The agent's own key, whose fingerprint the person compares with the
    /// one the hub shows.
    pub agent_key: PublicKey,
}

/// Why the agent didn't connect. None carries a seed (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectFailure {
    /// The agent holds no pin or no agent key: it never paired.
    NotPaired,
    /// The pinned address can't be connected to; nothing was sent.
    Route(RouteRefused),
    /// The connection didn't open or ended.
    Link(LinkError),
    /// The hub sent a message out of turn, or one that doesn't read: the
    /// connection is treated as dropped, and nothing in it is acted on.
    OutOfTurn,
    /// The hub's `Challenge` didn't prove the pinned hub: the agent sent
    /// no proof.
    Challenge(ChallengeRefused),
    /// An answer the hub didn't sign on this connection, or signed out of
    /// sequence: treated as a dropped connection, and nothing in it is
    /// acted on.
    Unsigned,
    /// The hub refused the agent's proof, signed.
    Refused(Refused),
    /// The hub's `Welcome` doesn't hold, and the agent acts on none of it.
    Welcome(WelcomeRefused),
    Identity(IdentityError),
    Signing(SigningError),
    Random(RandomUnavailable),
}

impl From<LinkError> for ConnectFailure {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

impl From<IdentityError> for ConnectFailure {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<SigningError> for ConnectFailure {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

impl From<RandomUnavailable> for ConnectFailure {
    fn from(error: RandomUnavailable) -> Self {
        Self::Random(error)
    }
}

impl ConnectToHub {
    pub fn new(
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
        connector: Arc<dyn HubConnector>,
        version: String,
    ) -> Self {
        Self {
            identity,
            signing,
            random,
            clock,
            connector,
            version,
        }
    }

    /// Connects this machine, as `agent_id` under `hostname`, to the hub it
    /// pinned at pairing, and says where the hub holds it.
    pub async fn connect(
        &self,
        agent_id: AgentId,
        hostname: Hostname,
    ) -> Result<Connected, ConnectFailure> {
        let (Some(pin), Some(seed)) = (
            self.identity.pin().await?,
            self.identity.seed(SeedFile::AgentKey).await?,
        ) else {
            return Err(ConnectFailure::NotPaired);
        };
        let route = HubRoute::of(&pin.address, pin.tls).map_err(ConnectFailure::Route)?;
        let agent_key = self.signing.public_key(&seed)?;
        let connect = Connect {
            agent_id,
            agent_nonce: Nonce::from_bytes(self.random.token()?),
            binding_kind: BindingKind::of_token(pin.tls),
        };
        let mut link = self.connector.connect(&route).await?;
        link.send(&connect_frame(&connect)).await?;
        let HubMessage::Challenge(challenge) = read(link.receive().await?)? else {
            return Err(ConnectFailure::OutOfTurn);
        };
        let binding = check_challenge(
            self.signing.as_ref(),
            pin.hub_key,
            &connect,
            &challenge,
            link.exporter(),
        )
        .map_err(ConnectFailure::Challenge)?;
        // The counter is held until the hub has answered the proof.
        let attempt = self.identity.begin_attempt().await?;
        let counter = attempt.counter().get();
        let proved = SignedMessage::AgentProof {
            hub_key: pin.hub_key,
            agent_id,
            agent_key,
            agent_nonce: connect.agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            counter,
        };
        let agent_proof = self.signing.sign(&seed, &proved.to_bytes())?;
        link.send(&proof_frame(&Proof {
            counter,
            agent_proof,
        }))
        .await?;
        let hello = Hello {
            agent_version: self.version.clone(),
            hostname,
            protocols: PROTOCOLS,
            clock: EpochSeconds::from_timestamp(self.clock.now())
                .unwrap_or(EpochSeconds::from_seconds(0)),
        };
        let payload = hello_payload(&hello);
        let kind = hello_kind();
        let said = SignedMessage::AgentMessage {
            hub_key: pin.hub_key,
            agent_id,
            agent_nonce: connect.agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            sequence: FIRST_SEQUENCE,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        let signature = self.signing.sign(&seed, &said.to_bytes())?;
        // A refused proof is answered before `Hello` reaches the hub, which
        // may have closed the connection: its refusal is still read.
        let sent = link
            .send(&agent_envelope_frame(&AgentEnvelope {
                kind,
                seq: FIRST_SEQUENCE,
                payload,
                signature,
            }))
            .await;
        let answer = match link.receive().await {
            Ok(frame) => read(frame)?,
            Err(error) => return Err(sent.err().unwrap_or(error).into()),
        };
        drop(attempt);
        let welcome = match answer {
            HubMessage::Signed(envelope) => envelope,
            HubMessage::Answer(PairAnswer::Refused(refused)) => {
                return Err(
                    if refusal_holds(
                        self.signing.as_ref(),
                        pin.hub_key,
                        &connect,
                        challenge.hub_nonce,
                        binding,
                        &refused,
                    ) {
                        ConnectFailure::Refused(refused)
                    } else {
                        ConnectFailure::Unsigned
                    },
                );
            }
            HubMessage::Challenge(_) | HubMessage::Answer(PairAnswer::Paired(_)) => {
                return Err(ConnectFailure::OutOfTurn);
            }
        };
        let signed = SignedMessage::HubMessage {
            hub_key: pin.hub_key,
            agent_id,
            agent_nonce: connect.agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            sequence: welcome.seq,
            expires_at: welcome.expires_at,
            kind: &welcome.kind,
            payload: PayloadDigest::of(&welcome.payload),
        };
        if welcome.seq != FIRST_SEQUENCE
            || !self
                .signing
                .verify(&pin.hub_key, &signed.to_bytes(), &welcome.signature)
        {
            return Err(ConnectFailure::Unsigned);
        }
        self.take_welcome(pin, agent_key, &welcome).await
    }

    /// The agent's pin and approval after the hub's signed `welcome`,
    /// which it acts on only once the welcome holds.
    async fn take_welcome(
        &self,
        mut pin: HubPin,
        agent_key: PublicKey,
        welcome: &HubEnvelope,
    ) -> Result<Connected, ConnectFailure> {
        if welcome.kind != welcome_kind() {
            return Err(ConnectFailure::OutOfTurn);
        }
        let read = read_welcome(&welcome.payload).map_err(|_| ConnectFailure::Unsigned)?;
        read.check(welcome.expires_at, PROTOCOLS, pin.generation)
            .map_err(ConnectFailure::Welcome)?;
        if read.generation > pin.generation {
            pin.generation = read.generation;
            self.identity.write_pin(&pin).await?;
        }
        Ok(Connected {
            approval: read.approval,
            pin,
            agent_key,
        })
    }
}

fn read(frame: api::sync::HubFrame) -> Result<HubMessage, ConnectFailure> {
    hub_message(&frame).map_err(|_| ConnectFailure::OutOfTurn)
}

#[cfg(test)]
mod tests;
