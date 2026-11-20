use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, HostnameKey, PairRefusal, PairRequest, Pairing,
    PairingView, ProofRefusal, ProofRequest, ProofView, TokenId, TokenRecord, TokenUse,
    admits_unused_token, decide_pairing, decide_proof, voids_tokens_for_new_key,
};
use domain::{AgentId, Timestamp};

use crate::ports::{Added, AgentRegistry, StoreError};

/// The hub's registry in memory, for use-case tests and acceptance
/// scenarios. It keeps times to the millisecond, as the real store does.
#[derive(Debug, Default)]
pub struct InMemoryAgentRegistry {
    kept: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    tokens: Vec<TokenRecord>,
    agents: Vec<AgentRecord>,
}

impl InMemoryAgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A person approves the agent under `agent_id` at `at`, as the hub's
    /// web UI will (#567); nothing else changes.
    pub fn approve(&self, agent_id: AgentId, at: Timestamp) {
        for agent in &mut self.kept().agents {
            if agent.agent_id == agent_id {
                agent.status = AgentStatus::Approved {
                    at: to_the_millisecond(at),
                };
            }
        }
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
            Ok(Pairing::Repeated) | Err(_) => {}
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
    ) -> Result<Result<AgentRecord, ProofRefusal>, StoreError> {
        let now = to_the_millisecond(now);
        let mut kept = self.kept();
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
        if let Ok(agent) = &decided {
            for kept_one in &mut kept.agents {
                if kept_one.agent_id == agent.agent_id {
                    *kept_one = kept_agent(agent.clone());
                }
            }
        }
        Ok(decided)
    }
}
