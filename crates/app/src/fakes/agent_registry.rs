use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::fleet::{
    AgentRecord, AgentStatus, ApprovalRefused, BindingKind, Fingerprint, HostnameKey, MachineCheck,
    MachineFingerprint, MachineZone, PairRefusal, PairRequest, Pairing, PairingView, ProofRefusal,
    ProofRequest, ProofView, Proved, PublicKey, RemovalRefused, SecondMachine, TokenId,
    TokenRecord, TokenUse, Tombstone, admits_unused_token, decide_approval, decide_pairing,
    decide_proof, decide_removal, voids_tokens_for_new_key,
};
use domain::{AgentId, SignedDuration, Timestamp};

use super::FakeClock;
use crate::ports::{Added, AgentRegistry, StoreError};

/// The hub's registry in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Debug, Default)]
pub struct InMemoryAgentRegistry {
    kept: Mutex<Kept>,
    keeping: Mutex<Option<Keeping>>,
    approving: Mutex<Option<Timestamp>>,
}

/// How long keeping a token takes: `clock` moves on by `by` as one is kept,
/// as a real store's write takes time.
struct Keeping {
    clock: Arc<FakeClock>,
    by: SignedDuration,
}

impl fmt::Debug for Keeping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keeping").field("by", &self.by).finish()
    }
}

#[derive(Debug, Default)]
struct Kept {
    tokens: Vec<TokenRecord>,
    agents: Vec<AgentRecord>,
    tls_key: Option<Fingerprint>,
    /// Per agent ID, the machine fingerprint held and when a second
    /// computer was last refused.
    machines: HashMap<AgentId, (Option<MachineFingerprint>, Option<Timestamp>)>,
    tombstones: Vec<Tombstone>,
}

impl InMemoryAgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The registry holds `agent`, as if it had paired; nothing else
    /// changes.
    pub fn hold(&self, agent: AgentRecord) {
        self.kept().agents.push(agent);
    }

    /// Keeping a token takes `by` from now on: `clock`, the hub's, moves on
    /// by that much as each one is kept.
    pub fn takes_to_keep(&self, clock: Arc<FakeClock>, by: SignedDuration) {
        *self.keeping.lock().unwrap_or_else(PoisonError::into_inner) = Some(Keeping { clock, by });
    }

    /// A person approves the agent the next proof names at `at`, in that
    /// proof's step before it is decided on: as an approval that lands after
    /// the hub read the agent to check its proof and before it kept it.
    pub fn approves_before_next_proof(&self, at: Timestamp) {
        *self
            .approving
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(at);
    }
}

/// `at` cut to the millisecond, as the real store keeps it.
fn to_the_millisecond(at: Timestamp) -> Timestamp {
    Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at)
}

fn kept_agent(mut agent: AgentRecord) -> AgentRecord {
    agent.status = match agent.status {
        AgentStatus::Pending { since } => AgentStatus::Pending {
            since: to_the_millisecond(since),
        },
        AgentStatus::Approved { at } => AgentStatus::Approved {
            at: to_the_millisecond(at),
        },
    };
    agent.last_seen = to_the_millisecond(agent.last_seen);
    agent
}

impl Kept {
    fn spend(&mut self, request: &PairRequest) {
        for token in &mut self.tokens {
            if token.pairing_key == request.pairing_key {
                token.state = TokenUse::Used {
                    agent_id: request.agent_id,
                    agent_key: request.agent_key,
                };
            }
        }
    }
}

#[async_trait]
impl AgentRegistry for InMemoryAgentRegistry {
    async fn add_token(
        &self,
        token: &TokenRecord,
        at_most: usize,
        now: Timestamp,
    ) -> Result<Added, StoreError> {
        let mut kept = self.kept();
        let unused = kept.tokens.iter().filter(|t| t.is_unused_at(now)).count();
        if admits_unused_token(unused, at_most).is_err() {
            return Ok(Added::Full);
        }
        if kept
            .tokens
            .iter()
            .any(|t| t.pairing_key == token.pairing_key)
        {
            return Err(StoreError::Unavailable {
                detail: format!("pairing token {} is kept already", token.id()),
            });
        }
        let mut token = *token;
        token.made_at = to_the_millisecond(token.made_at);
        token.expires_at = to_the_millisecond(token.expires_at);
        kept.tokens.push(token);
        if let Some(keeping) = &*self.keeping.lock().unwrap_or_else(PoisonError::into_inner) {
            keeping.clock.advance(keeping.by);
        }
        Ok(Added::Kept)
    }

    async fn unused_tokens(&self, now: Timestamp) -> Result<Vec<TokenRecord>, StoreError> {
        let kept = self.kept();
        let mut unused: Vec<_> = kept
            .tokens
            .iter()
            .filter(|t| t.is_unused_at(now))
            .copied()
            .collect();
        unused.sort_by_key(|t| t.made_at);
        Ok(unused)
    }

    async fn revoke_token(&self, id: TokenId, now: Timestamp) -> Result<bool, StoreError> {
        let mut kept = self.kept();
        let mut revoked = false;
        for token in &mut kept.tokens {
            if token.id() == id && token.is_unused_at(now) {
                token.state = TokenUse::Revoked;
                revoked = true;
            }
        }
        Ok(revoked)
    }

    async fn void_tokens_for_new_key(&self) -> Result<bool, StoreError> {
        let mut kept = self.kept();
        if !voids_tokens_for_new_key(kept.agents.len()) {
            return Ok(false);
        }
        for token in &mut kept.tokens {
            if token.state == TokenUse::Unused {
                token.state = TokenUse::Revoked;
            }
        }
        Ok(true)
    }

    async fn tls_key(&self) -> Result<Option<Fingerprint>, StoreError> {
        Ok(self.kept().tls_key)
    }

    async fn record_tls_key(&self, fingerprint: Fingerprint) -> Result<(), StoreError> {
        self.kept().tls_key.get_or_insert(fingerprint);
        Ok(())
    }

    async fn pair(
        &self,
        request: &PairRequest,
        now: Timestamp,
    ) -> Result<Result<Pairing, PairRefusal>, StoreError> {
        let now = to_the_millisecond(now);
        let mut kept = self.kept();
        let key = HostnameKey::of(&request.hostname);
        let decided = {
            let view = PairingView {
                token: kept
                    .tokens
                    .iter()
                    .find(|t| t.pairing_key == request.pairing_key),
                by_id: kept.agents.iter().find(|a| a.agent_id == request.agent_id),
                by_hostname: kept.agents.iter().find(|a| a.hostname_key() == key),
                pending: kept.agents.iter().filter(|a| a.is_pending()).count(),
                tombstone: kept
                    .tombstones
                    .iter()
                    .find(|t| t.agent_id == request.agent_id),
                key_tombstone: kept.tombstones.iter().find(|t| t.key == request.agent_key),
            };
            decide_pairing(&view, request, now)
        };
        match &decided {
            Ok(Pairing::New(agent)) => {
                kept.agents.push(kept_agent(agent.clone()));
                kept.spend(request);
            }
            Ok(Pairing::Again(agent)) => {
                for kept_one in &mut kept.agents {
                    if kept_one.agent_id == agent.agent_id {
                        *kept_one = kept_agent(agent.clone());
                    }
                }
                kept.spend(request);
            }
            Ok(Pairing::Repeated(_)) | Err(_) => {}
        }
        Ok(decided)
    }

    async fn agents(&self) -> Result<Vec<AgentRecord>, StoreError> {
        Ok(self.kept().agents.clone())
    }

    async fn agent(&self, agent_id: AgentId) -> Result<Option<AgentRecord>, StoreError> {
        Ok(self
            .kept()
            .agents
            .iter()
            .find(|agent| agent.agent_id == agent_id)
            .cloned())
    }

    async fn take_proof(
        &self,
        request: &ProofRequest,
        now: Timestamp,
    ) -> Result<Result<Proved, ProofRefusal>, StoreError> {
        let now = to_the_millisecond(now);
        let mut kept = self.kept();
        let approving = self
            .approving
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(at) = approving {
            for kept_one in &mut kept.agents {
                if kept_one.agent_id == request.agent_id {
                    kept_one.status = AgentStatus::Approved {
                        at: to_the_millisecond(at),
                    };
                }
            }
        }
        let kinds: Vec<BindingKind> = kept
            .tokens
            .iter()
            .filter(|token| {
                token.state
                    == TokenUse::Used {
                        agent_id: request.agent_id,
                        agent_key: request.agent_key,
                    }
            })
            .map(|token| token.kind)
            .collect();
        let decided = decide_proof(
            &ProofView {
                record: kept.agents.iter().find(|a| a.agent_id == request.agent_id),
                kinds: &kinds,
            },
            request,
            now,
        );
        if let Ok(proved) = &decided {
            for kept_one in &mut kept.agents {
                if kept_one.agent_id == proved.agent.agent_id {
                    *kept_one = kept_agent(proved.agent.clone());
                }
            }
        }
        Ok(decided)
    }

    async fn record_zone(
        &self,
        agent_id: AgentId,
        zone: Option<&MachineZone>,
    ) -> Result<(), StoreError> {
        if let Some(agent) = self
            .kept()
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        {
            agent.zone = zone.cloned();
        }
        Ok(())
    }

    async fn record_seen(&self, agent_id: AgentId, now: Timestamp) -> Result<(), StoreError> {
        if let Some(agent) = self
            .kept()
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        {
            agent.last_seen = now;
        }
        Ok(())
    }

    async fn check_machine(
        &self,
        agent_id: AgentId,
        presented: Option<&MachineFingerprint>,
        now: Timestamp,
    ) -> Result<Option<MachineCheck>, StoreError> {
        let mut kept = self.kept();
        if !kept.agents.iter().any(|agent| agent.agent_id == agent_id) {
            return Ok(None);
        }
        let (held, refused) = kept.machines.entry(agent_id).or_default();
        let check = MachineCheck::of(held.as_ref(), presented);
        match check {
            MachineCheck::Claimed => *held = presented.copied(),
            MachineCheck::Second => *refused = Some(now),
            MachineCheck::Same | MachineCheck::Untold => {}
        }
        Ok(Some(check))
    }

    async fn second_machines(&self) -> Result<Vec<SecondMachine>, StoreError> {
        let kept = self.kept();
        Ok(kept
            .agents
            .iter()
            .filter_map(|agent| {
                let (_, refused) = kept.machines.get(&agent.agent_id)?;
                refused.map(|at| SecondMachine {
                    agent_id: agent.agent_id,
                    at,
                })
            })
            .collect())
    }

    async fn forget_machine(&self, agent_id: AgentId) -> Result<bool, StoreError> {
        let mut kept = self.kept();
        kept.machines.remove(&agent_id);
        Ok(kept.agents.iter().any(|agent| agent.agent_id == agent_id))
    }

    async fn approve(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
        now: Timestamp,
    ) -> Result<Result<AgentRecord, ApprovalRefused>, StoreError> {
        let now = to_the_millisecond(now);
        let mut kept = self.kept();
        let decided = decide_approval(
            kept.agents.iter().find(|a| a.agent_id == agent_id),
            &agent_key,
            now,
        );
        if let Ok(agent) = &decided {
            for kept_one in &mut kept.agents {
                if kept_one.agent_id == agent.agent_id {
                    *kept_one = kept_agent(agent.clone());
                }
            }
        }
        Ok(decided)
    }

    async fn remove(
        &self,
        agent_id: AgentId,
        agent_key: PublicKey,
        now: Timestamp,
    ) -> Result<Result<Tombstone, RemovalRefused>, StoreError> {
        let now = to_the_millisecond(now);
        let mut kept = self.kept();
        let decided = decide_removal(
            kept.agents.iter().find(|a| a.agent_id == agent_id),
            &agent_key,
            now,
        );
        if let Ok(tombstone) = &decided {
            kept.agents.retain(|a| a.agent_id != agent_id);
            kept.machines.remove(&agent_id);
            kept.tombstones.retain(|t| t.agent_id != agent_id);
            kept.tombstones.push(tombstone.clone());
        }
        Ok(decided)
    }

    async fn tombstone(&self, agent_id: AgentId) -> Result<Option<Tombstone>, StoreError> {
        let kept = self.kept();
        Ok(kept
            .tombstones
            .iter()
            .find(|t| t.agent_id == agent_id)
            .cloned())
    }

    async fn drop_lapsed(&self, now: Timestamp) -> Result<Vec<AgentRecord>, StoreError> {
        let mut kept = self.kept();
        let (mut lapsed, left): (Vec<_>, Vec<_>) = std::mem::take(&mut kept.agents)
            .into_iter()
            .partition(|a| a.lapsed_at(now));
        kept.agents = left;
        for agent in &lapsed {
            kept.machines.remove(&agent.agent_id);
        }
        lapsed.sort_by_key(|a| match a.status {
            AgentStatus::Pending { since } | AgentStatus::Approved { at: since } => since,
        });
        Ok(lapsed)
    }
}
