use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use domain::fleet::{
    Ack, AgentEnvelope, AgentRecord, Approval, ApprovalRefused, Binding, BindingKind, Challenge,
    Connect, EpochSeconds, FIRST_SEQUENCE, HANDSHAKES, Holder, Hostname, HostnameKey, HubAddress,
    HubEnvelope, Ingest, KEY_BYTES, KeyFile, ListedAgent, MESSAGE_LIFE, MachineZone, Nonce, Pair,
    PairAnswer, PairRefusal, PairRequest, Paired, PairingToken, PayloadDigest, Proof, ProofRefusal,
    ProofRequest, PublicKey, Refused, ReportSeq, SeedFile, SignedMessage, SigningSeed, TokenId,
    TokenRecord, TokenTls, TokenUse, UNUSED_TOKEN_LIMIT, Welcome, ack_kind, hello_kind,
    list_agents, report_kind, token_life, welcome_kind,
};
use domain::{AgentId, SignedDuration, TimeZoneDatabase};

use super::wire::{ack_payload, read_hello, read_report, welcome_payload};
use crate::access::Access;
use crate::ports::{
    Added, AgentRegistry, Clock, Identity, IdentityError, Random, RandomUnavailable, ReportInbox,
    Signing, SigningError, StoreError,
};

mod dialog;
mod logged;

pub use dialog::{AgentsListed, ApprovalAsk, MadeToken, TokenAsk, TokenAskRefused};

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
    /// Where the hub keeps its agents' reports, when it takes them: with
    /// none, it closes each connection after `Welcome`.
    inbox: Option<Arc<dyn ReportInbox>>,
    /// The agents whose oldest reports the hub let go of past the cap since
    /// it started: it warns of each once.
    warned_full: Mutex<HashSet<AgentId>>,
    /// The agents that sent a report far behind the last the hub took of
    /// them since it started: it warns of each once (#1387).
    warned_behind: Mutex<HashSet<AgentId>>,
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

/// What the hub's keys came to at its start.
#[derive(Debug)]
pub enum HubStart {
    /// The keys are read from the data folder, or made on a first start.
    Keys(HubKeys),
    /// A key file is gone while agents are registered. No new key is made,
    /// which would lock every agent out; the hub runs, and every agent's
    /// session is refused, until the file is restored (§16, decision 120,
    /// "The hub key").
    Unserved(KeyFile),
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
            inbox: None,
            warned_full: Mutex::default(),
            warned_behind: Mutex::default(),
        }
    }

    /// This pairing with sign-in by the admin password on, which a token
    /// asked for on the web is then confirmed by ([`Self::make_token_asked`]).
    #[must_use]
    pub fn with_access(mut self, access: Arc<Access>) -> Self {
        self.access = Some(access);
        self
    }

    /// This pairing taking an approved agent's reports into `inbox` after
    /// `Welcome`, on the same connection (§7.2).
    #[must_use]
    pub fn with_inbox(mut self, inbox: Arc<dyn ReportInbox>) -> Self {
        self.inbox = Some(inbox);
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

    /// The hub's start: its keys made on a first start and read after
    /// that, so a key file that can't be read is found here, and not by
    /// the first agent or token that asks (§16, decision 120, "The key's
    /// life"). A key file gone while agents are registered isn't a failed
    /// start: the hub runs without a key, makes none, and refuses every
    /// agent's session ([`HubStart::Unserved`], "The hub key").
    pub async fn start(&self) -> Result<HubStart, HubPairingError> {
        match self.keys().await {
            Ok(keys) => Ok(HubStart::Keys(keys)),
            Err(HubPairingError::KeyMissing { file }) => Ok(HubStart::Unserved(file)),
            Err(error) => Err(error),
        }
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
        self.approve_now(agent.agent_id, agent.key).await
    }

    /// Approves the agent `agent_id` registered with `agent_key`, now, and
    /// logs the approval, whichever way the person approved it (#1342).
    async fn approve_now(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, HubPairingError> {
        let approved = self
            .registry
            .approve(agent_id, agent_key, self.clock.now())
            .await?;
        if let Ok(agent) = &approved {
            log::info!("{}", logged::approved(agent));
        }
        Ok(approved)
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
            zone: pair.zone.as_ref(),
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
            zone: known(pair.zone.as_ref()),
            counter: pair.counter,
            kind: handshake.kind,
            hub_key: keys.key,
        };
        match self.registry.pair(&request, self.clock.now()).await? {
            Ok(pairing) => {
                log::info!("{}", logged::paired(&request, &pairing));
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
                    hostname: Some(pairing.record().hostname.clone()),
                    approval: Some(Approval::of(pairing.record().status)),
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
            Ok(proved) => {
                // Once per approval, not per connection: an agent that
                // reconnects all day logs nothing more. The answer says so
                // from the record the proof's step decided on, not the one
                // read above, which an approval may have changed since
                // (#1342).
                if proved.first_since_approval {
                    log::info!("{}", logged::connected(&proved.agent));
                }
                Ok(ProofAnswer::Proved(proved.agent))
            }
            Err(ProofRefusal::ProofRefused) => refused(),
        }
    }

    /// The hub's signed `Welcome` to `hello`, the first message of the
    /// proved `agent`, when it holds: signed by the agent's key on this
    /// connection, sequence number 1, kind `Hello`. The machine zone the
    /// `Hello` reports is kept over the one the agent held, pending or
    /// approved, so a machine that moved tells its hub at its next
    /// connection.
    pub async fn welcome(
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
        let zone = known(read.zone.as_ref());
        if zone != agent.zone {
            self.registry
                .record_zone(agent.agent_id, zone.as_ref())
                .await?;
        }
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

    /// The connection's state after the `Welcome` to `agent`, when the hub
    /// takes its reports: none for a pending agent, whose reports wait in
    /// its outbox, nor on a hub that keeps no reports, which closes after
    /// the `Welcome` (§7.2).
    pub(super) fn syncing(&self, handshake: HubHandshake, agent: &AgentRecord) -> Option<Syncing> {
        let approved = Approval::of(agent.status) == Approval::Approved;
        (approved && self.inbox.is_some()).then(|| {
            let next = FIRST_SEQUENCE.saturating_add(1);
            Syncing {
                handshake,
                agent_id: agent.agent_id,
                agent_key: agent.key,
                next_received: next,
                next_sent: next,
            }
        })
    }

    /// The hub's answer to `message`, the agent's next after its `Hello`,
    /// when it holds: signed by the agent's key on this connection with the
    /// next sequence number. A report is kept unless the hub took it
    /// before, and acknowledged either way; a message of another kind is
    /// counted and acted on in no way.
    pub(super) async fn take_message(
        &self,
        keys: &HubKeys,
        syncing: &mut Syncing,
        message: &AgentEnvelope,
    ) -> Result<Result<Option<HubEnvelope>, MessageRefused>, HubPairingError> {
        let handshake = &syncing.handshake;
        let signed = SignedMessage::AgentMessage {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            sequence: message.seq,
            kind: &message.kind,
            payload: PayloadDigest::of(&message.payload),
        };
        if !self
            .signing
            .verify(&syncing.agent_key, &signed.to_bytes(), &message.signature)
        {
            return Ok(Err(MessageRefused::Unsigned));
        }
        if message.seq != syncing.next_received {
            return Ok(Err(MessageRefused::OutOfSequence));
        }
        syncing.next_received = syncing.next_received.saturating_add(1);
        if message.kind != report_kind() {
            return Ok(Ok(None));
        }
        let (Some(inbox), Ok(entry)) = (&self.inbox, read_report(&message.payload)) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        let now = self.clock.now();
        let taken = inbox.ingest(syncing.agent_id, &entry, now).await?;
        if taken.dropped > 0 {
            self.warn_full(inbox.as_ref(), syncing.agent_id, taken.dropped)
                .await;
        }
        if taken.ingest == Ingest::Behind {
            self.warn_behind(syncing.agent_id, entry.seq);
        }
        let payload = ack_payload(Ack {
            epoch: entry.epoch,
            seq: entry.seq,
        });
        let kind = ack_kind();
        let sequence = syncing.next_sent;
        let expires_at = EpochSeconds::from_seconds(
            EpochSeconds::from_timestamp(now)
                .unwrap_or(EpochSeconds::from_seconds(0))
                .seconds()
                .saturating_add(MESSAGE_LIFE),
        );
        let answer = SignedMessage::HubMessage {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            sequence,
            expires_at,
            kind: &kind,
            payload: PayloadDigest::of(&payload),
        };
        let signature = self.signing.sign(&keys.seed, &answer.to_bytes())?;
        syncing.next_sent = sequence.saturating_add(1);
        Ok(Ok(Some(HubEnvelope {
            kind,
            seq: sequence,
            expires_at,
            payload,
            signature,
        })))
    }

    /// Warns, once per agent while the hub runs, that it lets go of the
    /// agent's oldest reports past the cap, with how many it let go of so
    /// far (`just_now` when the count doesn't read); the ones after are
    /// counted ([`ReportInbox::dropped`]) and not logged.
    async fn warn_full(&self, inbox: &dyn ReportInbox, agent: AgentId, just_now: u64) {
        let first = self
            .warned_full
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(agent);
        if first {
            let dropped = inbox.dropped(agent).await.unwrap_or(just_now);
            log::warn!("{}", logged::reports_dropped(agent, dropped));
        }
    }

    /// Warns, once per agent while the hub runs, that `agent` sent a report
    /// at `seq` far behind the last the hub took of its epoch; the ones
    /// after are acknowledged as held and not logged (#1387).
    fn warn_behind(&self, agent: AgentId, seq: ReportSeq) {
        let first = self
            .warned_behind
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(agent);
        if first {
            log::warn!("{}", logged::reports_behind(agent, seq));
        }
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

/// `zone` as the hub's copy of the zone database spells it; none for a zone
/// it doesn't know, which the hub holds as unknown rather than guess.
fn known(zone: Option<&MachineZone>) -> Option<MachineZone> {
    zone.and_then(|zone| zone.known_in(&TimeZoneDatabase::bundled()))
}

/// What the hub holds of a connection that carries an approved agent's
/// signed messages after its `Welcome`.
#[derive(Debug)]
pub(super) struct Syncing {
    handshake: HubHandshake,
    agent_id: AgentId,
    agent_key: PublicKey,
    /// The sequence number the agent's next message must carry.
    next_received: u64,
    /// The sequence number of the hub's next message.
    next_sent: u64,
}
