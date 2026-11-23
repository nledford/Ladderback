use std::fmt;
use std::sync::Arc;

use domain::AgentId;
use domain::fleet::{
    Approval, Binding, BindingKind, Challenge, Connect, HANDSHAKES, Hostname, HubPin, KEY_BYTES,
    MachineZone, Nonce, Pair, PairAnswer, PairingToken, PublicKey, Refused, SeedFile,
    SignedMessage, SigningSeed,
};
use domain::maintenance::HeldLeases;

use crate::ports::{
    Attempt, Identity, IdentityError, LeaseCache, Random, RandomUnavailable, Signing, SigningError,
    StoreError,
};

/// The agent's side of pairing: it checks the hub against the token before
/// it sends anything of the token, and pins the hub once it is paired.
pub struct AgentPairing {
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
    leases: Arc<dyn LeaseCache>,
}

/// What the agent keeps of a pairing between its `Connect` and the hub's
/// `Challenge`: the token, its ID, hostname and machine zone, and its nonce.
pub struct AgentHandshake {
    token: PairingToken,
    agent_id: AgentId,
    hostname: Hostname,
    zone: Option<MachineZone>,
    agent_nonce: Nonce,
    kind: BindingKind,
}

impl fmt::Debug for AgentHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentHandshake")
            .field("agent_id", &self.agent_id)
            .field("hostname", &self.hostname)
            .field("zone", &self.zone)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// What the agent keeps between its `Pair` and the hub's answer, with the
/// connection attempt whose counter `Pair` carries.
#[derive(Debug)]
pub struct AwaitingPaired {
    handshake: AgentHandshake,
    agent_key: PublicKey,
    hub_nonce: Nonce,
    binding: Binding,
    attempt: Attempt,
}

/// A pairing the hub accepted: the pin the agent keeps, and its own key,
/// whose fingerprint it shows beside the hub's so the person can compare
/// them before approving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedHub {
    pub pin: HubPin,
    pub agent_key: PublicKey,
    /// The hostname the hub holds this machine under, when that isn't the
    /// one the pairing asked for: a pending machine keeps the hostname it
    /// first paired under. `None` when they are the same, and from a hub
    /// that doesn't say.
    pub held_as: Option<Hostname>,
    /// Whether the hub holds this machine approved already, as a machine
    /// that pairs again may be (#1341). `false` from a hub that doesn't
    /// say, so the machine says it is pending, as it did before hubs said.
    pub approved: bool,
}

/// Why the agent stopped a pairing. None carries a seed or the token's text
/// (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentPairingError {
    /// The agent pins another hub; replacing the pin needs its person to
    /// confirm, naming both keys (decision 120, "Pairing").
    PinnedToAnotherHub {
        pinned: PublicKey,
        token: PublicKey,
    },
    /// The hub that answered has another key than the token's: nothing of
    /// the token was sent.
    HubKeyMismatch,
    /// The hub's proof isn't the token's hub key's signature over this
    /// connection: nothing of the token was sent.
    HubProofInvalid,
    /// The hub speaks no handshake this build speaks.
    NoCommonHandshake,
    /// The token's address is the hub's own TLS, and this session has no
    /// exporter to bind to.
    BindingUnavailable,
    /// An answer the hub didn't sign: treated as a dropped connection, and
    /// nothing in it is acted on.
    Unsigned,
    /// The hub refused the pairing, signed.
    Refused(Refused),
    Identity(IdentityError),
    Signing(SigningError),
    Random(RandomUnavailable),
    /// The machine couldn't forget the leases and epoch floors of the hub
    /// it paired with before: it pins the new hub only once they are gone.
    Leases(StoreError),
}

/// Why the agent stops at the hub's `Challenge`, having sent nothing after
/// its `Connect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChallengeRefused {
    /// The hub that answered has another key than the one the agent
    /// expects.
    HubKeyMismatch,
    /// The hub's proof isn't that key's signature over this connection.
    HubProofInvalid,
    /// The hub speaks no handshake this build speaks.
    NoCommonHandshake,
    /// The agent names the hub's own TLS, and this session has no exporter
    /// to bind to.
    BindingUnavailable,
}

impl From<IdentityError> for AgentPairingError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<SigningError> for AgentPairingError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

impl From<RandomUnavailable> for AgentPairingError {
    fn from(error: RandomUnavailable) -> Self {
        Self::Random(error)
    }
}

impl From<StoreError> for AgentPairingError {
    fn from(error: StoreError) -> Self {
        Self::Leases(error)
    }
}

impl From<ChallengeRefused> for AgentPairingError {
    fn from(refused: ChallengeRefused) -> Self {
        match refused {
            ChallengeRefused::HubKeyMismatch => Self::HubKeyMismatch,
            ChallengeRefused::HubProofInvalid => Self::HubProofInvalid,
            ChallengeRefused::NoCommonHandshake => Self::NoCommonHandshake,
            ChallengeRefused::BindingUnavailable => Self::BindingUnavailable,
        }
    }
}

/// The binding of a connection on which `challenge` proves the hub holds
/// `hub_key`, answering the agent's `connect` on a session whose
/// `tls-exporter` value is `exporter`; otherwise why the agent sends
/// nothing more on it.
pub(super) fn check_challenge(
    signing: &dyn Signing,
    hub_key: PublicKey,
    connect: &Connect,
    challenge: &Challenge,
    exporter: Option<[u8; KEY_BYTES]>,
) -> Result<Binding, ChallengeRefused> {
    if challenge.hub_key != hub_key {
        return Err(ChallengeRefused::HubKeyMismatch);
    }
    let binding = connect
        .binding_kind
        .binding(exporter)
        .ok_or(ChallengeRefused::BindingUnavailable)?;
    let proof = SignedMessage::HubProof {
        hub_key,
        agent_id: connect.agent_id,
        agent_nonce: connect.agent_nonce,
        hub_nonce: challenge.hub_nonce,
        binding,
        handshakes: challenge.hub_handshakes,
    };
    if !signing.verify(&hub_key, &proof.to_bytes(), &challenge.hub_proof) {
        return Err(ChallengeRefused::HubProofInvalid);
    }
    if challenge.hub_handshakes.bits() & HANDSHAKES.bits() == 0 {
        return Err(ChallengeRefused::NoCommonHandshake);
    }
    Ok(binding)
}

/// Whether `refused` is signed by `hub_key` over the connection the
/// agent's `connect` opened, the hub's `hub_nonce` and `binding`.
pub(super) fn refusal_holds(
    signing: &dyn Signing,
    hub_key: PublicKey,
    connect: &Connect,
    hub_nonce: Nonce,
    binding: Binding,
    refused: &Refused,
) -> bool {
    let signed = SignedMessage::Refusal {
        hub_key,
        agent_id: connect.agent_id,
        agent_nonce: connect.agent_nonce,
        hub_nonce,
        binding,
        code: &refused.code,
    };
    signing.verify(&hub_key, &signed.to_bytes(), &refused.signature)
}

impl AgentHandshake {
    /// The `Connect` this handshake sent.
    const fn connect(&self) -> Connect {
        Connect {
            agent_id: self.agent_id,
            agent_nonce: self.agent_nonce,
            binding_kind: self.kind,
        }
    }
}

impl AgentPairing {
    pub fn new(
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
        leases: Arc<dyn LeaseCache>,
    ) -> Self {
        Self {
            identity,
            signing,
            random,
            leases,
        }
    }

    /// The agent's `Connect` for a pairing with `token` as `agent_id` under
    /// `hostname`, in the machine zone `zone` (none when its zone has no
    /// IANA name). It names the binding kind of the token's address, and
    /// carries nothing of the token.
    pub async fn connect(
        &self,
        token: PairingToken,
        agent_id: AgentId,
        hostname: Hostname,
        zone: Option<MachineZone>,
    ) -> Result<(Connect, AgentHandshake), AgentPairingError> {
        if let Some(pin) = self.identity.pin().await?
            && pin.hub_key != *token.hub_key()
        {
            return Err(AgentPairingError::PinnedToAnotherHub {
                pinned: pin.hub_key,
                token: *token.hub_key(),
            });
        }
        let handshake = AgentHandshake {
            kind: BindingKind::of_token(token.tls()),
            token,
            agent_id,
            hostname,
            zone,
            agent_nonce: Nonce::from_bytes(self.random.token()?),
        };
        let connect = handshake.connect();
        Ok((connect, handshake))
    }

    /// The agent's `Pair`, once the hub's `Challenge` proves it holds the
    /// token's hub key on this connection. Nothing signed with the token's
    /// pairing key leaves the agent before that.
    pub async fn answer_challenge(
        &self,
        handshake: AgentHandshake,
        challenge: &Challenge,
        exporter: Option<[u8; KEY_BYTES]>,
    ) -> Result<(Pair, AwaitingPaired), AgentPairingError> {
        let hub_key = *handshake.token.hub_key();
        let binding = check_challenge(
            self.signing.as_ref(),
            hub_key,
            &handshake.connect(),
            challenge,
            exporter,
        )?;
        let agent_seed = self.agent_seed().await?;
        let agent_key = self.signing.public_key(&agent_seed)?;
        let attempt = self.identity.begin_attempt().await?;
        let counter = attempt.counter().get();
        let message = SignedMessage::Pair {
            hub_key,
            agent_id: handshake.agent_id,
            agent_key,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: challenge.hub_nonce,
            binding,
            counter,
            hostname: &handshake.hostname,
            zone: handshake.zone.as_ref(),
        }
        .to_bytes();
        let pairing_seed = handshake.token.pairing_seed();
        let pair = Pair {
            pairing_key: self.signing.public_key(pairing_seed)?,
            agent_key,
            counter,
            hostname: handshake.hostname.clone(),
            zone: handshake.zone.clone(),
            pairing_signature: self.signing.sign(pairing_seed, &message)?,
            agent_signature: self.signing.sign(&agent_seed, &message)?,
        };
        let awaiting = AwaitingPaired {
            handshake,
            agent_key,
            hub_nonce: challenge.hub_nonce,
            binding,
            attempt,
        };
        Ok((pair, awaiting))
    }

    /// The hub's answer, checked against the token's hub key. A signed
    /// `Paired` pins the hub, unless the agent pins it already, whose pin
    /// is kept as it is; a signed refusal is the hub's; anything else is a
    /// dropped connection. An agent that pins no hub forgets its leases and
    /// epoch floors before it pins this one: they were another hub's, or
    /// this one's from before the agent lost its pin, whose epochs this
    /// hub's only passed (§7.3, #1481). One that pins this hub already keeps
    /// them, so they still catch a rewound copy of it (decision 137).
    pub async fn take_answer(
        &self,
        awaiting: AwaitingPaired,
        answer: &PairAnswer,
    ) -> Result<PairedHub, AgentPairingError> {
        let AwaitingPaired {
            handshake,
            agent_key,
            hub_nonce,
            binding,
            attempt,
        } = awaiting;
        let hub_key = *handshake.token.hub_key();
        let paired = match answer {
            PairAnswer::Paired(paired) => paired,
            PairAnswer::Refused(refused) => {
                return Err(
                    if refusal_holds(
                        self.signing.as_ref(),
                        hub_key,
                        &handshake.connect(),
                        hub_nonce,
                        binding,
                        refused,
                    ) {
                        AgentPairingError::Refused(refused.clone())
                    } else {
                        AgentPairingError::Unsigned
                    },
                );
            }
        };
        let signed = SignedMessage::Paired {
            hub_key,
            agent_id: handshake.agent_id,
            agent_key,
            recovery_key: paired.recovery_key,
            agent_nonce: handshake.agent_nonce,
            hub_nonce,
            binding,
        };
        if !self
            .signing
            .verify(&hub_key, &signed.to_bytes(), &paired.signature)
        {
            return Err(AgentPairingError::Unsigned);
        }
        let pin = match self.identity.pin().await? {
            // Pairing again at another address of the same kind, as
            // `ladderback pair <address>` does for a machine the hub no
            // longer knows, keeps that address beside the token's; a pin
            // with no room for it pairs all the same.
            Some(mut kept) if kept.hub_key == hub_key => {
                if kept.tls == handshake.token.tls()
                    && kept
                        .give_address(handshake.token.address().clone())
                        .unwrap_or(false)
                {
                    self.identity.write_pin(&kept).await?;
                }
                kept
            }
            Some(other) => {
                return Err(AgentPairingError::PinnedToAnotherHub {
                    pinned: other.hub_key,
                    token: hub_key,
                });
            }
            None => {
                // Forgotten first: a failure here leaves no pin, so the
                // floors never stay under the new hub's.
                self.leases.save(&HeldLeases::default()).await?;
                let pin = HubPin::new(
                    hub_key,
                    handshake.token.tls(),
                    paired.recovery_key,
                    handshake.token.address().clone(),
                );
                self.identity.write_pin(&pin).await?;
                pin
            }
        };
        drop(attempt);
        let held_as = paired
            .hostname
            .as_ref()
            .filter(|held| **held != handshake.hostname)
            .cloned();
        Ok(PairedHub {
            pin,
            agent_key,
            held_as,
            approved: paired.approval == Some(Approval::Approved),
        })
    }

    /// The agent key's seed, made the first time the agent pairs.
    async fn agent_seed(&self) -> Result<SigningSeed, AgentPairingError> {
        if let Some(seed) = self.identity.seed(SeedFile::AgentKey).await? {
            return Ok(seed);
        }
        let fresh = SigningSeed::from_bytes(self.random.token()?);
        Ok(self.identity.keep_seed(SeedFile::AgentKey, fresh).await?)
    }
}
