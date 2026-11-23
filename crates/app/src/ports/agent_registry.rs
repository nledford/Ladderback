use async_trait::async_trait;
use domain::fleet::{
    AgentRecord, ApprovalRefused, Fingerprint, MachineCheck, MachineFingerprint, MachineZone,
    PairRefusal, PairRequest, Pairing, ProofRefusal, ProofRequest, Proved, PublicKey,
    RemovalRefused, SecondMachine, TokenId, TokenRecord, Tombstone,
};
use domain::{AgentId, Timestamp};

use super::{Added, StoreError};

/// The hub's registry (§7.1; §16, decision 120, "Pairing" and "Pending
/// approval"): the agents it has registered and the pairing tokens it has
/// made, kept in its database. It keeps a token's pairing key, never its
/// seed. Times are kept to the millisecond.
#[async_trait]
pub trait AgentRegistry: Send + Sync {
    /// Keeps `token` unless `at_most` tokens are unused at `now`. The count
    /// and the keeping are one step: of adds racing for the last place, one
    /// is kept.
    async fn add_token(
        &self,
        token: &TokenRecord,
        at_most: usize,
        now: Timestamp,
    ) -> Result<Added, StoreError>;

    /// The tokens unused at `now`, oldest first.
    async fn unused_tokens(&self, now: Timestamp) -> Result<Vec<TokenRecord>, StoreError>;

    /// Revokes the tokens unused at `now` whose token ID is `id`, and
    /// answers whether there was one. Two unused tokens share an ID only by
    /// a 4-byte collision; both are revoked then, which keeps the person's
    /// intent.
    async fn revoke_token(&self, id: TokenId, now: Timestamp) -> Result<bool, StoreError>;

    /// Readies the registry for a new hub key: when no agent is
    /// registered, revokes every unused token, since each carries the key
    /// being replaced, and answers `true`; while an agent is registered,
    /// changes nothing and answers `false`, and no key may be made. The
    /// check and the revoking are one step, so no agent pairs between them.
    async fn void_tokens_for_new_key(&self) -> Result<bool, StoreError>;

    /// The fingerprint of the hub's TLS key, recorded when the key was
    /// made (§16, decision 120, "The hub key"), or `None` before then.
    async fn tls_key(&self) -> Result<Option<Fingerprint>, StoreError>;

    /// Records `fingerprint` as the hub's TLS key's, unless one is
    /// recorded, which is kept: the key is made once, and of two first
    /// starts the first record stands.
    async fn record_tls_key(&self, fingerprint: Fingerprint) -> Result<(), StoreError>;

    /// Decides `request` with [`domain::fleet::decide_pairing`] on the
    /// records it holds at `now`, and applies the decision, in one step: a
    /// new agent is kept and the token spent together, so of two pairings
    /// on one token, one is paired and the other refused `token-used`. A
    /// refusal changes nothing.
    async fn pair(
        &self,
        request: &PairRequest,
        now: Timestamp,
    ) -> Result<Result<Pairing, PairRefusal>, StoreError>;

    /// Every registered agent, pending or approved, oldest first.
    async fn agents(&self) -> Result<Vec<AgentRecord>, StoreError>;

    /// The agent registered under `agent_id`, pending or approved.
    async fn agent(&self, agent_id: AgentId) -> Result<Option<AgentRecord>, StoreError>;

    /// Decides `request` with [`domain::fleet::decide_proof`] on the
    /// records it holds at `now`, the binding kinds being those of the
    /// tokens spent on the request's agent ID and key, and keeps the record
    /// it answers, in one step: whether the proof is the agent's first
    /// since its approval is read from the record that step decided on. A
    /// refusal changes nothing.
    async fn take_proof(
        &self,
        request: &ProofRequest,
        now: Timestamp,
    ) -> Result<Result<Proved, ProofRefusal>, StoreError>;

    /// Keeps `zone` as the machine zone of the agent registered under
    /// `agent_id`, none included, over the one it held: the agent's last
    /// report wins (#114). An ID it doesn't hold changes nothing.
    async fn record_zone(
        &self,
        agent_id: AgentId,
        zone: Option<&MachineZone>,
    ) -> Result<(), StoreError>;

    /// Compares `presented`, the machine fingerprint the `Hello` of the
    /// agent registered under `agent_id` carried, with the one it holds,
    /// by [`MachineCheck::of`], and keeps what the answer asks in the same
    /// step: a first fingerprint, so of two machines' first `Hello`s at
    /// once one is claimed and the other refused, and a second computer's
    /// refusal at `now`, over the one before (decision 150). `None` for an
    /// ID it doesn't hold, which changes nothing.
    async fn check_machine(
        &self,
        agent_id: AgentId,
        presented: Option<&MachineFingerprint>,
        now: Timestamp,
    ) -> Result<Option<MachineCheck>, StoreError>;

    /// Every agent under whose ID a second computer was refused, oldest
    /// agent first, with when the last one was.
    async fn second_machines(&self) -> Result<Vec<SecondMachine>, StoreError>;

    /// Forgets the machine fingerprint held under `agent_id`, and when a
    /// second computer was last refused under it, so the next fingerprint a
    /// `Hello` carries is claimed: the way back for a computer refused as a
    /// second one (decision 150). `false` for an ID it doesn't hold.
    async fn forget_machine(&self, agent_id: AgentId) -> Result<bool, StoreError>;

    /// Approves the agent registered under `agent_id` with `agent_key`, the
    /// key the person compared, at `now`, with
    /// [`domain::fleet::decide_approval`] on the record it holds, in one
    /// step: of two approvals at once, one approves and the other is
    /// refused `AlreadyApproved`. A refusal changes nothing.
    async fn approve(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
        now: Timestamp,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, StoreError>;

    /// Removes the agent registered under `agent_id` with `agent_key`, the
    /// key the person saw, pending or approved, at `now`, with
    /// [`domain::fleet::decide_removal`] on the record it holds, and keeps
    /// its tombstone in the same step: of two removals at once, one removes
    /// and the other is refused `Unknown`. A refusal changes nothing.
    async fn remove(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
        now: Timestamp,
    ) -> Result<Result<Tombstone, RemovalRefused>, StoreError>;

    /// The tombstone kept under `agent_id`, if a person removed it.
    async fn tombstone(&self, agent_id: AgentId) -> Result<Option<Tombstone>, StoreError>;

    /// Drops every pending agent that [lapsed](AgentRecord::lapsed_at) at
    /// `now`, keeping no tombstone, and answers them, oldest first. An
    /// approval racing the lapse either approves first, and the agent
    /// stays, or is refused `Unknown`.
    async fn drop_lapsed(&self, now: Timestamp) -> Result<Vec<AgentRecord>, StoreError>;
}
