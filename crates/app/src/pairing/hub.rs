use std::fmt;
use std::sync::Arc;

use domain::fleet::{
    AgentEnvelope, AgentRecord, Approval, ApprovalRefused, Binding, BindingKind, Challenge,
    Connect, EpochSeconds, FIRST_SEQUENCE, HANDSHAKES, Holder, Hostname, HostnameKey, HubAddress,
    HubEnvelope, KEY_BYTES, KeyFile, ListedAgent, Nonce, Pair, PairAnswer, PairRefusal,
    PairRequest, Paired, PairingToken, PayloadDigest, Proof, ProofRefusal, ProofRequest, PublicKey,
    Refused, SeedFile, SignedMessage, SigningSeed, TokenId, TokenRecord, TokenTls, TokenUse,
    UNUSED_TOKEN_LIMIT, Welcome, hello_kind, list_agents, token_life, welcome_kind,
};
use domain::{AgentId, SignedDuration};

use super::wire::{read_hello, welcome_payload};
use crate::access::Access;
use crate::ports::{
    Added, AgentRegistry, Clock, Identity, IdentityError, Random, RandomUnavailable, Signing,
    SigningError, StoreError,
};

mod dialog;

pub use dialog::{MadeToken, TokenAsk, TokenAskRefused};

/// The hub's side of pairing: its keys, its tokens, and its answers to an
/// agent's `Connect` and `Pair`.
pub struct HubPairing {
    registry: Arc<dyn AgentRegistry>,
    identity: Arc<dyn Identity>,
    signing: Arc<dyn Signing>,
    random: Arc<dyn Random>,
    clock: Arc<dyn Clock>,
    /// The hub's build, as `--version` names it, which `Welcome` carries.
    version: String,
    /// The sign-in by the admin password, when it is on: a token asked for
    /// on the web is confirmed by that password, entered again.
    access: Option<Arc<Access>>,
}

/// The hub's answer to an agent's proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProofAnswer {
    /// The proof holds: the agent's record as the registry now keeps it.
    Proved(AgentRecord),
    /// The proof was refused, signed.
    Refused(Refused),
}

/// Why the hub takes no message from a proved agent. Each closes the
/// connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageRefused {
    /// The signature doesn't hold under the agent's key on this connection.
    Unsigned,
    /// Not the sequence number the hub expects next.
    OutOfSequence,
    /// A kind the hub doesn't take here, or a payload that isn't the
    /// kind's.
    Unexpected,
}

/// The hub key, with its seed, and the recovery key's public half, read
/// once at the hub's start.
pub struct HubKeys {
    seed: SigningSeed,
    key: PublicKey,
    recovery_key: PublicKey,
}

impl HubKeys {
    /// The hub key every agent pins.
    pub const fn key(&self) -> &PublicKey {
        &self.key
    }

    /// The recovery key's public half, which `Paired` gives the agent.
    pub const fn recovery_key(&self) -> &PublicKey {
        &self.recovery_key
    }
}

impl fmt::Debug for HubKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HubKeys")
            .field("key", &self.key)
            .field("recovery_key", &self.recovery_key)
            .finish_non_exhaustive()
    }
}

/// What the hub keeps of one connection between its `Challenge` and the
/// agent's next message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubHandshake {
    agent_id: AgentId,
    agent_nonce: Nonce,
    hub_nonce: Nonce,
    kind: BindingKind,
    binding: Binding,
}

/// Why the hub couldn't take a step. None carries a key, a seed or a
/// token's text (I4); the transport drops the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubPairingError {
    /// A key file is gone while agents are registered: a new key would
    /// lock out every agent that pinned the old one, so none is made.
    KeyMissing {
        file: KeyFile,
    },
    /// A token's life outside 5 minutes to 24 hours was asked for.
    TokenLife,
    /// As many tokens as the limit allows are unused; one must be used,
    /// lapse or be revoked first.
    TooManyUnusedTokens,
    /// The agent named binding kind 1 on a session that has no exporter.
    BindingUnavailable,
    Identity(IdentityError),
    Registry(StoreError),
    Signing(SigningError),
    Random(RandomUnavailable),
}

impl From<IdentityError> for HubPairingError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}

impl From<StoreError> for HubPairingError {
    fn from(error: StoreError) -> Self {
        Self::Registry(error)
    }
}

impl From<SigningError> for HubPairingError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}

impl From<RandomUnavailable> for HubPairingError {
    fn from(error: RandomUnavailable) -> Self {
        Self::Random(error)
    }
}

impl HubPairing {
    pub fn new(
        registry: Arc<dyn AgentRegistry>,
        identity: Arc<dyn Identity>,
        signing: Arc<dyn Signing>,
        random: Arc<dyn Random>,
        clock: Arc<dyn Clock>,
        version: String,
    ) -> Self {
        Self {
            registry,
            identity,
            signing,
            random,
            clock,
            version,
            access: None,
        }
    }

    /// This pairing with sign-in by the admin password on, which a token
    /// asked for on the web is then confirmed by ([`Self::make_token_asked`]).
    #[must_use]
    pub fn with_access(mut self, access: Arc<Access>) -> Self {
        self.access = Some(access);
        self
    }

    /// A seed no one can guess.
    fn fresh_seed(&self) -> Result<SigningSeed, HubPairingError> {
        Ok(SigningSeed::from_bytes(self.random.token()?))
    }

    /// A new hub key's seed, kept in its file: made only while no agent is
    /// registered, and only once every unused token, each of which carries
    /// the key it replaces, is voided (§16, decision 120, "Rotation,
    /// recovery and the handover chain"). The tokens are voided first, so
    /// a start cut short between the two leaves no token of the old key
    /// usable, and the next start makes the key.
    async fn new_hub_seed(&self) -> Result<SigningSeed, HubPairingError> {
        if !self.registry.void_tokens_for_new_key().await? {
            return Err(HubPairingError::KeyMissing {
                file: KeyFile::HubKey,
            });
        }
        let seed = self.fresh_seed()?;
        Ok(self.identity.keep_seed(SeedFile::HubKey, seed).await?)
    }

    /// The recovery seed its file holds, or a new one kept in it while no
    /// agent is registered.
    async fn recovery_seed(&self) -> Result<SigningSeed, HubPairingError> {
        if let Some(seed) = self.identity.seed(SeedFile::RecoverySeed).await? {
            return Ok(seed);
        }
        if !self.registry.agents().await?.is_empty() {
            return Err(HubPairingError::KeyMissing {
                file: KeyFile::RecoveryKey,
            });
        }
        let seed = self.fresh_seed()?;
        Ok(self
            .identity
            .keep_seed(SeedFile::RecoverySeed, seed)
            .await?)
    }

    /// The hub's keys: the hub key and the recovery key, made while no
    /// agent is registered and read from the data folder after that (§16,
    /// decision 120, "The key's life"). A new hub key voids every unused
    /// token. The recovery seed is kept before its public half, so a start
    /// cut short between the two makes the same recovery key the next
    /// time; once the Recovery Kit is confirmed only the public half is
    /// left, and it is enough.
    pub async fn keys(&self) -> Result<HubKeys, HubPairingError> {
        let seed = match self.identity.seed(SeedFile::HubKey).await? {
            Some(seed) => seed,
            None => self.new_hub_seed().await?,
        };
        let key = self.signing.public_key(&seed)?;
        let recovery_key = match self.identity.recovery_key().await? {
            Some(kept) => kept,
            None => {
                let public = self.signing.public_key(&self.recovery_seed().await?)?;
                self.identity.keep_recovery_key(public).await?
            }
        };
        Ok(HubKeys {
            seed,
            key,
            recovery_key,
        })
    }

    /// A pairing token for `address`, of `tls`'s kind, living `life` or 15
    /// minutes. The hub keeps its pairing key's public half, never the
    /// seed, which only the token's text holds.
    pub async fn make_token(
        &self,
        keys: &HubKeys,
        address: HubAddress,
        tls: TokenTls,
        life: Option<SignedDuration>,
    ) -> Result<PairingToken, HubPairingError> {
        Ok(self.mint(keys, address, tls, life).await?.0)
    }

    /// A pairing token, and the record the hub keeps of it.
    async fn mint(
        &self,
        keys: &HubKeys,
        address: HubAddress,
        tls: TokenTls,
        life: Option<SignedDuration>,
    ) -> Result<(PairingToken, TokenRecord), HubPairingError> {
        let life = token_life(life).map_err(|_| HubPairingError::TokenLife)?;
        let now = self.clock.now();
        let expires_at = now
            .checked_add(life)
            .map_err(|_| HubPairingError::TokenLife)?;
        let pairing_seed = self.fresh_seed()?;
        let record = TokenRecord {
            pairing_key: self.signing.public_key(&pairing_seed)?,
            hub_key: keys.key,
            kind: BindingKind::of_token(tls),
            made_at: now,
            expires_at,
            state: TokenUse::Unused,
        };
        match self
            .registry
            .add_token(&record, UNUSED_TOKEN_LIMIT, now)
            .await?
        {
            Added::Kept => Ok((
                PairingToken::new(keys.key, tls, pairing_seed, address),
                record,
            )),
            Added::Full => Err(HubPairingError::TooManyUnusedTokens),
        }
    }

    /// Revokes the unused token `id`, and answers whether there was one.
    pub async fn revoke_token(&self, id: TokenId) -> Result<bool, HubPairingError> {
        Ok(self.registry.revoke_token(id, self.clock.now()).await?)
    }

    /// The agents waiting for a person's approval, in the registry's order.
    pub async fn pending(&self) -> Result<Vec<AgentRecord>, HubPairingError> {
        let mut agents = self.registry.agents().await?;
        agents.retain(AgentRecord::is_pending);
        Ok(agents)
    }

    /// The agent registered under `hostname`, pending or approved: the one
    /// whose hostname compares equal to it on the hub (decision 120,
    /// "Hostnames").
    pub async fn agent_named(
        &self,
        hostname: &Hostname,
    ) -> Result<Option<AgentRecord>, HubPairingError> {
        let key = HostnameKey::of(hostname);
        Ok(self
            .registry
            .agents()
            .await?
            .into_iter()
            .find(|agent| agent.hostname_key() == key))
    }

    /// A person approves `agent`, as they compared it: its agent ID and
    /// key (decision 120, "Pending approval").
    pub async fn approve(
        &self,
        agent: &AgentRecord,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, HubPairingError> {
        Ok(self
            .registry
            .approve(agent.agent_id, agent.key, self.clock.now())
            .await?)
    }

    /// The hub's answer to `connect` on a session whose `tls-exporter`
    /// value is `exporter`: a fresh nonce and its proof over the
    /// connection.
    pub fn challenge(
        &self,
        keys: &HubKeys,
        connect: &Connect,
        exporter: Option<[u8; KEY_BYTES]>,
    ) -> Result<(Challenge, HubHandshake), HubPairingError> {
        let binding = connect
            .binding_kind
            .binding(exporter)
            .ok_or(HubPairingError::BindingUnavailable)?;
        let handshake = HubHandshake {
            agent_id: connect.agent_id,
            agent_nonce: connect.agent_nonce,
            hub_nonce: Nonce::from_bytes(self.random.token()?),
            kind: connect.binding_kind,
            binding,
        };
        let proof = SignedMessage::HubProof {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding,
            handshakes: HANDSHAKES,
        };
        let challenge = Challenge {
            hub_nonce: handshake.hub_nonce,
            hub_key: keys.key,
            hub_handshakes: HANDSHAKES,
            hub_proof: self.signing.sign(&keys.seed, &proof.to_bytes())?,
        };
        Ok((challenge, handshake))
    }

    /// The hub's answer to `pair`: a signed `Paired` for a new agent, kept
    /// pending, for the same identity again, or for the same pairing
    /// again; a signed refusal for any other, which changes nothing. Both
    /// signatures are checked before the registry is asked.
    pub async fn pair(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        pair: &Pair,
    ) -> Result<PairAnswer, HubPairingError> {
        let message = SignedMessage::Pair {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_key: pair.agent_key,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            counter: pair.counter,
            hostname: &pair.hostname,
            zone: None,
        }
        .to_bytes();
        let authorized = self
            .signing
            .verify(&pair.pairing_key, &message, &pair.pairing_signature);
        let holds_its_key = self
            .signing
            .verify(&pair.agent_key, &message, &pair.agent_signature);
        if !(authorized && holds_its_key) {
            return self
                .refuse(keys, handshake, &PairRefusal::ProofRefused)
                .map(PairAnswer::Refused);
        }
        let request = PairRequest {
            pairing_key: pair.pairing_key,
            agent_id: handshake.agent_id,
            agent_key: pair.agent_key,
            hostname: pair.hostname.clone(),
            zone: None,
            counter: pair.counter,
            kind: handshake.kind,
            hub_key: keys.key,
        };
        match self.registry.pair(&request, self.clock.now()).await? {
            Ok(_) => {
                let paired = SignedMessage::Paired {
                    hub_key: keys.key,
                    agent_id: handshake.agent_id,
                    agent_key: pair.agent_key,
                    recovery_key: keys.recovery_key,
                    agent_nonce: handshake.agent_nonce,
                    hub_nonce: handshake.hub_nonce,
                    binding: handshake.binding,
                };
                Ok(PairAnswer::Paired(Paired {
                    recovery_key: keys.recovery_key,
                    signature: self.signing.sign(&keys.seed, &paired.to_bytes())?,
                }))
            }
            Err(refusal) => self
                .refuse(keys, handshake, &refusal)
                .map(PairAnswer::Refused),
        }
    }

    /// Every agent the hub has registered, oldest first, each pending one
    /// flagged with the agent its hostname looks like (decision 120,
    /// "Hostnames"), which the pending row shows (decision 121).
    pub async fn agents(&self) -> Result<Vec<ListedAgent>, HubPairingError> {
        Ok(list_agents(self.registry.agents().await?))
    }

    /// The hub's answer to a paired agent's `proof`: the agent's record,
    /// seen now with the greater counter, when the proof holds under the
    /// key it registered for a binding kind it paired with; one signed
    /// `proof-refused` for every other cause (decision 120, "The
    /// handshake").
    pub async fn prove(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        proof: &Proof,
    ) -> Result<ProofAnswer, HubPairingError> {
        let refused = || {
            self.refuse(keys, handshake, &PairRefusal::ProofRefused)
                .map(ProofAnswer::Refused)
        };
        let Some(agent) = self.registry.agent(handshake.agent_id).await? else {
            return refused();
        };
        let message = SignedMessage::AgentProof {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_key: agent.key,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            counter: proof.counter,
        };
        if !self
            .signing
            .verify(&agent.key, &message.to_bytes(), &proof.agent_proof)
        {
            return refused();
        }
        let request = ProofRequest {
            agent_id: handshake.agent_id,
            agent_key: agent.key,
            kind: handshake.kind,
            counter: proof.counter,
        };
        match self.registry.take_proof(&request, self.clock.now()).await? {
            Ok(agent) => Ok(ProofAnswer::Proved(agent)),
            Err(ProofRefusal::ProofRefused) => refused(),
        }
    }

    /// The hub's signed `Welcome` to `hello`, the first message of the
    /// proved `agent`, when it holds: signed by the agent's key on this
    /// connection, sequence number 1, kind `Hello`.
    pub fn welcome(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        agent: &AgentRecord,
        hello: &AgentEnvelope,
    ) -> Result<Result<HubEnvelope, MessageRefused>, HubPairingError> {
        let signed = SignedMessage::AgentMessage {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            sequence: hello.seq,
            kind: &hello.kind,
            payload: PayloadDigest::of(&hello.payload),
        };
        if !self
            .signing
            .verify(&agent.key, &signed.to_bytes(), &hello.signature)
        {
            return Ok(Err(MessageRefused::Unsigned));
        }
        if hello.seq != FIRST_SEQUENCE {
            return Ok(Err(MessageRefused::OutOfSequence));
        }
        if hello.kind != hello_kind() {
            return Ok(Err(MessageRefused::Unexpected));
        }
        let Ok(read) = read_hello(&hello.payload) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        let signed_at =
            EpochSeconds::from_timestamp(self.clock.now()).unwrap_or(EpochSeconds::from_seconds(0));
        let welcome = Welcome::answering(
            &read,
            Approval::of(agent.status),
            self.version.clone(),
            signed_at,
        );
        let payload = welcome_payload(&welcome);
        let kind = welcome_kind();
        let expires_at = welcome.expires_at();
        let answer = SignedMessage::HubMessage {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            sequence: FIRST_SEQUENCE,
            expires_at,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        let signature = self.signing.sign(&keys.seed, &answer.to_bytes())?;
        Ok(Ok(HubEnvelope {
            kind,
            seq: FIRST_SEQUENCE,
            expires_at,
            payload,
            signature,
        }))
    }

    /// `refusal`, signed over its code and the connection. A taken
    /// hostname names its holder beside the signed code.
    fn refuse(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        refusal: &PairRefusal,
    ) -> Result<Refused, HubPairingError> {
        let code = refusal.code();
        let signed = SignedMessage::Refusal {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            code: &code,
        };
        let signature = self.signing.sign(&keys.seed, &signed.to_bytes())?;
        let holder = match refusal {
            PairRefusal::HostnameTaken { holder, last_seen } => Some(Holder {
                hostname: holder.clone(),
                last_seen: EpochSeconds::from_timestamp(*last_seen)
                    .unwrap_or(EpochSeconds::from_seconds(0)),
            }),
            _ => None,
        };
        Ok(Refused {
            code,
            holder,
            signature,
        })
    }
}
