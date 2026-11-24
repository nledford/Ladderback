use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use domain::fleet::{
    Ack, AgentEnvelope, AgentRecord, Approval, ApprovalRefused, Binding, BindingKind, Challenge,
    Connect, EpochSeconds, FIRST_SEQUENCE, HANDSHAKES, Holder, Hostname, HostnameKey, HubAddress,
    HubEnvelope, Ingest, KEY_BYTES, KeyFile, ListedAgent, MESSAGE_LIFE, MachineCheck, MachineZone,
    Nonce, Pair, PairAnswer, PairRefusal, PairRequest, Paired, PairingToken, PayloadDigest, Proof,
    ProofRefusal, ProofRequest, PublicKey, Refused, RemovalRefused, ReportSeq, SeedFile,
    SignedMessage, SigningSeed, TokenId, TokenRecord, TokenTls, TokenUse, Tombstone,
    UNUSED_TOKEN_LIMIT, Welcome, WireName, ack_kind, grants_kind, hello_kind, list_agents,
    plans_kind, progress_kind, release_kind, released_kind, report_kind, repos_kind, token_life,
    welcome_kind, withheld_kind, GENERATION,
};
use domain::maintenance::{Grant, ProvenFloor, RepoGrants, RepositoryReports};
use domain::{AgentId, SignedDuration, TimeZoneDatabase, Timestamp};

use super::wire::{
    ack_payload, entry_expiry, grants_payload, read_hello, read_release, read_report, read_repos,
    released_payload, welcome_payload, withheld_payload,
};
use crate::access::Access;
use crate::ownership::{Granting, RepositoryOwners};
use crate::ports::{
    Added, AgentRegistry, Clock, EventBus, Identity, IdentityError, Random, RandomUnavailable,
    ReportInbox, Signing, SigningError, StoreError,
};

mod dialog;
mod fleet;
mod live;
mod logged;
#[cfg(test)]
mod tests;

pub use dialog::{AgentsListed, ApprovalAsk, MadeToken, RecoveryAsk, RemovalAsk, TokenAsk, TokenAskRefused};
pub use fleet::Machine;
pub use live::{progress_dto, time_of};

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
    /// Who maintains each repository, when the hub decides it: with none,
    /// it counts an agent's `ReposSnapshot` and `ReleaseAck` and answers
    /// neither.
    owners: Option<Arc<RepositoryOwners>>,
    /// The agents whose oldest reports the hub let go of past the cap since
    /// it started: it warns of each once.
    warned_full: Mutex<HashSet<AgentId>>,
    /// The agents that sent a report far behind the last the hub took of
    /// them since it started: it warns of each once (#1387).
    warned_behind: Mutex<HashSet<AgentId>>,
    /// Where the hub tells its browsers that a machine started waiting,
    /// was approved or reported (#569).
    events: Option<Arc<dyn EventBus>>,
    /// The agents under whose ID the hub refused a second computer since
    /// it started: it warns of each once (decision 150).
    warned_second: Mutex<HashSet<AgentId>>,
    /// Whether the hub warned it is in recovery mode since it entered it.
    warned_recovering: Mutex<bool>,
}

/// The hub's answer to a proved agent's `Hello`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelloAnswer {
    /// The signed `Welcome`.
    Welcome(HubEnvelope),
    /// A signed refusal: another computer holds the agent's ID (decision
    /// 150).
    Refused(Refused),
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
            owners: None,
            warned_full: Mutex::default(),
            warned_behind: Mutex::default(),
            events: None,
            warned_second: Mutex::default(),
            warned_recovering: Mutex::default(),
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

    /// This pairing telling `events` of its machines (#569).
    #[must_use]
    pub fn with_events(mut self, events: Arc<dyn EventBus>) -> Self {
        self.events = Some(events);
        self
    }

    /// This pairing deciding, by `owners`, which agent maintains each
    /// repository its approved agents report, and answering each
    /// `ReposSnapshot` with signed `RepoGrants` and each `ReleaseAck` with
    /// `Released` (§7.2, §7.3).
    #[must_use]
    pub fn with_owners(mut self, owners: Arc<RepositoryOwners>) -> Self {
        self.owners = Some(owners);
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
        let mut agents = self.registered().await?;
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
            .registered()
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
        self.drop_lapsed().await?;
        let approved = self
            .registry
            .approve(agent_id, agent_key, self.clock.now())
            .await?;
        if let Ok(agent) = &approved {
            log::info!("{}", logged::approved(agent));
            self.tell(fleet::approved(agent, self.clock.now())).await;
        }
        Ok(approved)
    }

    /// Removes the agent `agent_id` registered with `agent_key`, now,
    /// pending (a rejection) or approved, keeps its tombstone, logs the
    /// removal and tells the browsers (decision 154).
    pub(super) async fn remove_now(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
    ) -> Result<Result<Tombstone, RemovalRefused>, HubPairingError> {
        let removed = self
            .registry
            .remove(agent_id, agent_key, self.clock.now())
            .await?;
        if let Ok(tombstone) = &removed {
            log::info!("{}", logged::removed(tombstone));
            self.tell(fleet::removed(tombstone)).await;
        }
        Ok(removed)
    }

    /// Drops every agent that waited for approval
    /// [`PENDING_LIFE`](domain::fleet::PENDING_LIFE) or
    /// longer, now, and logs each: the lapse is taken whenever the hub
    /// reads its agents, so no timer is needed (decision 154).
    async fn drop_lapsed(&self) -> Result<(), HubPairingError> {
        for agent in self.registry.drop_lapsed(self.clock.now()).await? {
            log::info!("{}", logged::lapsed(&agent));
        }
        Ok(())
    }

    /// Every agent the registry holds once the lapsed ones are dropped.
    async fn registered(&self) -> Result<Vec<AgentRecord>, HubPairingError> {
        self.drop_lapsed().await?;
        Ok(self.registry.agents().await?)
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
        self.drop_lapsed().await?;
        match self.registry.pair(&request, self.clock.now()).await? {
            Ok(pairing) => {
                log::info!("{}", logged::paired(&request, &pairing));
                if let Some(event) = fleet::waiting(&pairing, self.clock.now()) {
                    self.tell(event).await;
                }
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

    /// A person at the hub forgets the computer it took under `agent`'s ID,
    /// so the next one to connect is taken in its place: the way back for a
    /// machine refused as a second computer (decision 150). Logged.
    pub async fn forget_machine(&self, agent: &AgentRecord) -> Result<(), HubPairingError> {
        if self.registry.forget_machine(agent.agent_id).await? {
            log::info!("{}", logged::forgot_machine(agent));
        }
        Ok(())
    }

    /// Every agent the hub has registered, oldest first, each pending one
    /// flagged with the agent its hostname looks like (decision 120,
    /// "Hostnames"), which the pending row shows (decision 121), and each
    /// with when the hub last refused a second computer under its ID
    /// (decision 150).
    pub async fn agents(&self) -> Result<Vec<ListedAgent>, HubPairingError> {
        let mut listed = list_agents(self.registered().await?);
        for second in self.registry.second_machines().await? {
            if let Some(agent) = listed
                .iter_mut()
                .find(|listed| listed.record.agent_id == second.agent_id)
            {
                agent.second_machine_at = Some(second.at);
            }
        }
        Ok(listed)
    }

    /// The hub's answer to a paired agent's `proof`: the agent's record,
    /// seen now with the greater counter, when the proof holds under the
    /// key it registered for a binding kind it paired with; a signed
    /// `agent-removed` when a person removed the agent and the proof holds
    /// under the key its tombstone keeps (decision 154); one signed
    /// `proof-refused` for every other cause (decision 120, "The
    /// handshake").
    pub async fn prove(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        proof: &Proof,
    ) -> Result<ProofAnswer, HubPairingError> {
        let refused = |refusal: &PairRefusal| {
            self.refuse(keys, handshake, refusal)
                .map(ProofAnswer::Refused)
        };
        self.drop_lapsed().await?;
        let message = |agent_key: PublicKey| SignedMessage::AgentProof {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_key,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            counter: proof.counter,
        };
        let holds = |agent_key: &PublicKey| {
            self.signing.verify(
                agent_key,
                &message(*agent_key).to_bytes(),
                &proof.agent_proof,
            )
        };
        let Some(agent) = self.registry.agent(handshake.agent_id).await? else {
            return match self.registry.tombstone(handshake.agent_id).await? {
                Some(tombstone) if holds(&tombstone.key) => refused(&PairRefusal::AgentRemoved),
                _ => refused(&PairRefusal::ProofRefused),
            };
        };
        if !holds(&agent.key) {
            return refused(&PairRefusal::ProofRefused);
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
            Err(ProofRefusal::ProofRefused) => refused(&PairRefusal::ProofRefused),
        }
    }

    /// The hub's signed `Welcome` to `hello`, the first message of the
    /// proved `agent`, when it holds: signed by the agent's key on this
    /// connection, sequence number 1, kind `Hello`. The machine fingerprint
    /// it carries is checked first: another than the hub took under the ID
    /// is a second computer, refused signed and warned of once, and nothing
    /// its `Hello` says is kept (decision 150). Its proof was taken before,
    /// as any good proof under the agent's key is: the record's last-seen
    /// time and counter are the copy's. The machine zone the `Hello`
    /// reports is kept over the one the agent held, pending or approved, so
    /// a machine that moved tells its hub at its next connection.
    pub async fn welcome(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        agent: &AgentRecord,
        hello: &AgentEnvelope,
    ) -> Result<Result<HelloAnswer, MessageRefused>, HubPairingError> {
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
        let check = self
            .registry
            .check_machine(agent.agent_id, read.machine.as_ref(), self.clock.now())
            .await?;
        if check.is_some_and(|check| !check.takes()) {
            self.warn_second(agent);
            let refused = self.signed_refusal(keys, handshake, MachineCheck::refusal(), None)?;
            return Ok(Ok(HelloAnswer::Refused(refused)));
        }
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
        Ok(Ok(HelloAnswer::Welcome(HubEnvelope {
            kind,
            seq: FIRST_SEQUENCE,
            expires_at,
            payload,
            signature,
        })))
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
    /// before, and acknowledged either way; a `ReposSnapshot` is answered
    /// with the agent's grants and a `ReleaseAck` with `Released`; a
    /// snapshot of plans is kept and acknowledged; a progress is kept and
    /// not answered; a message of another kind is counted and acted on in
    /// no way.
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
        let now = self.clock.now();
        let answered = if message.kind == report_kind() {
            self.take_report(syncing.agent_id, &message.payload, now)
                .await?
        } else if message.kind == repos_kind() {
            self.grant(keys, syncing.agent_id, &message.payload).await?
        } else if message.kind == release_kind() {
            self.take_release(syncing.agent_id, &message.payload, now)
                .await?
        } else if message.kind == plans_kind() {
            self.take_plans(syncing.agent_id, &message.payload).await?
        } else if message.kind == progress_kind() {
            self.take_progress(syncing.agent_id, &message.payload)
                .await?
        } else {
            return Ok(Ok(None));
        };
        let (kind, payload) = match answered {
            Ok(Some(answer)) => answer,
            Ok(None) => return Ok(Ok(None)),
            Err(refused) => return Ok(Err(refused)),
        };
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

    /// Keeps `agent`'s report unless the hub took it before, and answers
    /// with its acknowledgement either way.
    async fn take_report(
        &self,
        agent: AgentId,
        payload: &[u8],
        now: Timestamp,
    ) -> Result<Answered, HubPairingError> {
        let (Some(inbox), Ok(entry)) = (&self.inbox, read_report(payload)) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        let taken = inbox.ingest(agent, &entry, now).await?;
        if let Some(dropped) = taken.dropped {
            self.warn_full(agent, dropped);
        }
        if taken.ingest == Ingest::New {
            self.tell(fleet::reported(agent, &entry.report)).await;
        }
        if taken.ingest == Ingest::Behind {
            self.warn_behind(agent, entry.seq);
        }
        let payload = ack_payload(Ack {
            epoch: entry.epoch,
            seq: entry.seq,
        });
        Ok(Ok(Some((ack_kind(), payload))))
    }

    /// Decides the repositories `agent`'s `ReposSnapshot` names, with the
    /// floors whose proof holds, and answers with every lease it holds,
    /// each entry signed apart as well, at the hub's time; in recovery
    /// mode, with `GrantsWithheld`. An agent no longer approved is answered
    /// with nothing and the connection ends.
    async fn grant(
        &self,
        keys: &HubKeys,
        agent: AgentId,
        payload: &[u8],
    ) -> Result<Answered, HubPairingError> {
        let Some(owners) = &self.owners else {
            return Ok(Ok(None));
        };
        let Ok(reports) = read_repos(payload) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        let proven = self.proven(keys, agent, &reports);
        match owners.grant(agent, &reports, &proven).await? {
            Granting::Leases {
                generation,
                signed_at,
                leases,
            } => {
                let grants = leases
                    .into_iter()
                    .map(|(repository, lease)| {
                        let entry = SignedMessage::LeaseEntry {
                            hub_key: keys.key,
                            agent_id: agent,
                            repository: &repository,
                            generation,
                            epoch: lease.epoch.get(),
                            expires_at: entry_expiry(lease.expires_at),
                        };
                        let signature = self.signing.sign(&keys.seed, &entry.to_bytes())?;
                        Ok(Grant {
                            repository,
                            epoch: lease.epoch,
                            expires_at: lease.expires_at,
                            signature,
                        })
                    })
                    .collect::<Result<Vec<_>, SigningError>>()?;
                let grants = RepoGrants {
                    generation,
                    signed_at,
                    grants,
                };
                Ok(Ok(Some((grants_kind(), grants_payload(&grants)))))
            }
            Granting::Withheld => {
                self.warn_recovering(agent);
                Ok(Ok(Some((withheld_kind(), withheld_payload()))))
            }
            Granting::NotApproved => Ok(Err(MessageRefused::Unexpected)),
        }
    }

    /// The floors in `reports` whose proof is a lease entry this hub key
    /// signed for `agent` at its generation (decision 137). Each other
    /// floor doesn't count, and is logged.
    fn proven(&self, keys: &HubKeys, agent: AgentId, reports: &RepositoryReports) -> Vec<ProvenFloor> {
        reports
            .entries()
            .iter()
            .filter_map(|report| {
                let floor = report.floor?;
                let entry = SignedMessage::LeaseEntry {
                    hub_key: keys.key,
                    agent_id: agent,
                    repository: &report.repository,
                    generation: GENERATION,
                    epoch: floor.epoch.get(),
                    expires_at: entry_expiry(floor.expires_at),
                };
                if self
                    .signing
                    .verify(&keys.key, &entry.to_bytes(), &floor.signature)
                {
                    Some(ProvenFloor {
                        repository: report.repository.clone(),
                        holder: agent,
                        epoch: floor.epoch,
                        expires_at: floor.expires_at,
                        held: floor.held,
                    })
                } else {
                    log::warn!(
                        "{}",
                        logged::floor_unproven(agent, &report.repository, floor.epoch.get())
                    );
                    None
                }
            })
            .collect()
    }

    /// Whether the hub is in recovery mode; a hub that keeps no owners
    /// never is.
    pub async fn recovering(&self) -> Result<bool, HubPairingError> {
        match &self.owners {
            Some(owners) => Ok(owners.recovering().await?),
            None => Ok(false),
        }
    }

    /// Ends recovery mode, once a person's step-up holds.
    async fn end_recovery(&self) -> Result<bool, HubPairingError> {
        let Some(owners) = &self.owners else {
            return Ok(false);
        };
        let ended = owners.end_recovery().await?;
        if ended {
            *self
                .warned_recovering
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = false;
            log::warn!("{}", logged::recovered());
        }
        Ok(ended)
    }

    /// Warns, once each time the hub enters recovery mode while it runs,
    /// that `agent`'s report found it there.
    fn warn_recovering(&self, agent: AgentId) {
        let mut warned = self
            .warned_recovering
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !*warned {
            *warned = true;
            log::warn!("{}", logged::recovering(agent));
        }
    }

    /// Ends the lease `agent`'s `ReleaseAck` names, and answers with
    /// `Released`. A release the hub refuses names a lease the agent no
    /// longer holds, so the agent may let go of it all the same: it is
    /// answered alike.
    async fn take_release(
        &self,
        agent: AgentId,
        payload: &[u8],
        now: Timestamp,
    ) -> Result<Answered, HubPairingError> {
        let Some(owners) = &self.owners else {
            return Ok(Ok(None));
        };
        let Ok((repository, epoch)) = read_release(payload) else {
            return Ok(Err(MessageRefused::Unexpected));
        };
        // Either way the agent holds no lease at that epoch any more.
        let _ended = owners.release(agent, &repository, epoch).await?;
        Ok(Ok(Some((
            released_kind(),
            released_payload(&repository, epoch, now),
        ))))
    }

    /// Warns, once per agent while the hub runs, that it lets go of the
    /// agent's oldest reports past the cap, with how many it let go of so
    /// far; the ones after are counted and not logged. Nothing waits
    /// between marking the agent warned and logging, so a step cut short
    /// can't leave it marked unlogged.
    fn warn_full(&self, agent: AgentId, dropped: u64) {
        let first = self
            .warned_full
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(agent);
        if first {
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

    /// Warns, once per agent while the hub runs, that it refused a second
    /// computer under `agent`'s ID; the ones after are refused unlogged.
    fn warn_second(&self, agent: &AgentRecord) {
        let first = self
            .warned_second
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(agent.agent_id);
        if first {
            log::warn!("{}", logged::second_machine(agent));
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
        let holder = match refusal {
            PairRefusal::HostnameTaken { holder, last_seen } => Some(Holder {
                hostname: holder.clone(),
                last_seen: EpochSeconds::from_timestamp(*last_seen)
                    .unwrap_or(EpochSeconds::from_seconds(0)),
            }),
            _ => None,
        };
        self.signed_refusal(keys, handshake, refusal.code(), holder)
    }

    /// The refusal `code`, signed over the code and the connection, with
    /// `holder` beside it unsigned.
    fn signed_refusal(
        &self,
        keys: &HubKeys,
        handshake: &HubHandshake,
        code: WireName,
        holder: Option<Holder>,
    ) -> Result<Refused, HubPairingError> {
        let signed = SignedMessage::Refusal {
            hub_key: keys.key,
            agent_id: handshake.agent_id,
            agent_nonce: handshake.agent_nonce,
            hub_nonce: handshake.hub_nonce,
            binding: handshake.binding,
            code: &code,
        };
        let signature = self.signing.sign(&keys.seed, &signed.to_bytes())?;
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

impl Syncing {
    /// The agent whose messages the connection carries.
    pub(super) fn agent_id(&self) -> AgentId {
        self.agent_id
    }
}

/// The kind and payload the hub answers a message with, when it answers.
type Answered = Result<Option<(WireName, Vec<u8>)>, MessageRefused>;
