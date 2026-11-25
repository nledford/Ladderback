//! The hub's registry of agents and pairing tokens (§3.1, the Fleet
//! context; §7.1; §16, decision 120, "Pairing" and "Pending approval"): an
//! agent's record, a token's record, how two hostnames compare, the
//! decision a `Pair` gets, a removed agent's tombstone and a waiting
//! agent's lapse. The rules live here; the registry's port keeps the
//! records and applies a decision in the same step that read them.

use jiff::{SignedDuration, Timestamp};
use unicode_normalization::UnicodeNormalization as _;

use super::fingerprint::TokenId;
use super::handshake::BindingKind;
use super::hostname::Hostname;
use super::keys::PublicKey;
use super::signed::WireName;
use super::zone::MachineZone;
use crate::AgentId;

/// The most agents that may wait for approval at once; a pairing past it
/// is refused (`pending-limit`) and leaves the token unspent.
pub const PENDING_LIMIT: usize = 20;

/// How long an agent may wait for approval: a pending agent this old
/// lapses, and the hub drops it, frees its hostname and keeps no tombstone,
/// since nobody judged it (decision 120, "Pending approval").
pub const PENDING_LIFE: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The most tokens that may be unused at once.
pub const UNUSED_TOKEN_LIMIT: usize = 20;

/// A new token refused: `limit` tokens are unused already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnusedTokensFull;

/// Whether one more token may be kept while `unused` are unused: fewer
/// than `limit`. The registry counts and asks this in the step that keeps
/// the token, so of adds racing for the last place one is kept.
pub fn admits_unused_token(unused: usize, limit: usize) -> Result<(), UnusedTokensFull> {
    if unused < limit {
        Ok(())
    } else {
        Err(UnusedTokensFull)
    }
}

/// Whether a new hub key voids the unused tokens, given the `agents`
/// registered: only while there are none, since a registered agent pins
/// the old key and the hub keeps it.
pub fn voids_tokens_for_new_key(agents: usize) -> bool {
    agents == 0
}

/// Why hub mode may not be off: machines are paired with this hub, and with
/// hub mode off each would lose its hub (§16, decision 136, point 5). Their
/// hostnames as the hub registered them, in the order given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubModeOffRefused {
    pub paired: Vec<Hostname>,
}

/// Whether this machine may run with hub mode off, given the machines
/// `registered` with it at `now`: only while none is paired. An approved
/// machine is paired, and so is one waiting for approval that hasn't
/// lapsed; a lapsed one isn't, since the hub drops it unjudged.
pub fn decide_hub_mode_off(
    registered: &[AgentRecord],
    now: Timestamp,
) -> Result<(), HubModeOffRefused> {
    let paired: Vec<Hostname> = registered
        .iter()
        .filter(|record| !record.lapsed_at(now))
        .map(|record| record.hostname.clone())
        .collect();
    if paired.is_empty() {
        Ok(())
    } else {
        Err(HubModeOffRefused { paired })
    }
}

/// A token's life when none is asked for.
pub const TOKEN_LIFE: SignedDuration = SignedDuration::from_mins(15);

/// The shortest life a token may be given.
pub const SHORTEST_TOKEN_LIFE: SignedDuration = SignedDuration::from_mins(5);

/// The longest life a token may be given.
pub const LONGEST_TOKEN_LIFE: SignedDuration = SignedDuration::from_hours(24);

/// A life asked for outside [`SHORTEST_TOKEN_LIFE`] to
/// [`LONGEST_TOKEN_LIFE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenLifeRefused;

/// The life a token is made with: `asked`, or [`TOKEN_LIFE`] when nothing
/// was asked for.
pub fn token_life(asked: Option<SignedDuration>) -> Result<SignedDuration, TokenLifeRefused> {
    let life = asked.unwrap_or(TOKEN_LIFE);
    if (SHORTEST_TOKEN_LIFE..=LONGEST_TOKEN_LIFE).contains(&life) {
        Ok(life)
    } else {
        Err(TokenLifeRefused)
    }
}

/// What the hub compares hostnames by: `casefold(NFC(name))`, with the
/// standard library's `to_lowercase` as the case fold (decision 120,
/// "Hostnames"). Two names with one key are one name on the hub, so "café"
/// typed two ways, or "Laptop" beside "laptop", can't both pair.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostnameKey(String);

impl HostnameKey {
    pub fn of(name: &Hostname) -> Self {
        Self(name.as_str().nfc().collect::<String>().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the hub tells lookalike hostnames by (decision 120, "Lookalike
/// hostnames"): the UTS #39 skeleton of the case fold of the name's
/// skeleton, `skeleton(casefold(skeleton(NFC(name))))`, with each run of
/// spaces read as one. Two names with one skeleton look alike, as "lаptop"
/// with a Cyrillic а looks like "Laptop", though their keys differ. The
/// inner skeleton reads a capital before the case fold can change it, so a
/// capital I is an l, as it looks, not an i.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostnameSkeleton(String);

impl HostnameSkeleton {
    /// A skeleton starts with NFD, so the name's NFC form would change
    /// nothing and isn't made.
    pub fn of(name: &Hostname) -> Self {
        let folded = unicode_security::skeleton(name.as_str())
            .collect::<String>()
            .to_lowercase();
        let mut skeleton = String::with_capacity(folded.len());
        for c in unicode_security::skeleton(&folded) {
            if !c.is_whitespace() {
                skeleton.push(c);
            } else if !skeleton.ends_with(' ') {
                skeleton.push(' ');
            }
        }
        Self(skeleton)
    }
}

/// Where an agent stands with the hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    /// Paired and waiting for a person to approve it, since `since`. Nothing
    /// it says is acted on.
    Pending { since: Timestamp },
    /// Approved by a person at `at`.
    Approved { at: Timestamp },
}

/// One agent the hub has registered: the `Agent` aggregate's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub agent_id: AgentId,
    /// The agent key every proof of this agent is checked against.
    pub key: PublicKey,
    /// The hostname registered at pairing, which identifies the agent on
    /// the hub.
    pub hostname: Hostname,
    pub status: AgentStatus,
    /// The highest connection counter the hub has taken from the agent.
    pub counter: u64,
    /// When the hub last heard from the agent.
    pub last_seen: Timestamp,
    /// The machine zone the agent last reported, as the hub's database
    /// knows it; none while it is unknown.
    pub zone: Option<MachineZone>,
}

impl AgentRecord {
    pub fn hostname_key(&self) -> HostnameKey {
        HostnameKey::of(&self.hostname)
    }

    pub const fn is_pending(&self) -> bool {
        matches!(self.status, AgentStatus::Pending { .. })
    }

    /// Whether the agent is approved and the hub hasn't heard from it since
    /// the approval: its next proof is its first connection as an approved
    /// agent, which the hub's log records once (#1342). A proof or a
    /// pairing sets `last_seen` after the approval, and an approval keeps
    /// it, so this needs no state of its own.
    pub fn unseen_since_approval(&self) -> bool {
        match self.status {
            AgentStatus::Approved { at } => self.last_seen <= at,
            AgentStatus::Pending { .. } => false,
        }
    }

    /// Whether the agent waited for approval [`PENDING_LIFE`] or longer at
    /// `now`: a lapsed agent is dropped. An approved agent never lapses.
    pub fn lapsed_at(&self, now: Timestamp) -> bool {
        match self.status {
            AgentStatus::Pending { since } => now.duration_since(since) >= PENDING_LIFE,
            AgentStatus::Approved { .. } => false,
        }
    }
}

/// The agent a pending agent's hostname looks like, which the hub flags on
/// the pending row (decision 120, "Hostnames"; decision 121): its ID, its
/// hostname as registered and when it was last seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookalike {
    pub agent_id: AgentId,
    pub hostname: Hostname,
    pub last_seen: Timestamp,
}

/// A registered agent as the hub lists it: its record, and, while it is
/// pending, the agent its hostname looks like, if one does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedAgent {
    pub record: AgentRecord,
    pub looks_like: Option<Lookalike>,
    /// When the hub last refused a second computer under the agent's ID
    /// (decision 150); none while it never did.
    pub second_machine_at: Option<Timestamp>,
}

/// The hub's agents, in the order given, each pending one flagged with the
/// first other agent, pending or approved, whose hostname has its
/// [`HostnameSkeleton`]. A lookalike isn't refused at pairing, since a false
/// match would lock out an honest name; the person sees the flag and
/// compares keys before approving (decision 120, "Hostnames").
pub fn list_agents(agents: Vec<AgentRecord>) -> Vec<ListedAgent> {
    let skeletons: Vec<HostnameSkeleton> = agents
        .iter()
        .map(|agent| HostnameSkeleton::of(&agent.hostname))
        .collect();
    let looks_like = |agent: &AgentRecord, skeleton: &HostnameSkeleton| {
        agents
            .iter()
            .zip(&skeletons)
            .find(|(other, theirs)| other.agent_id != agent.agent_id && *theirs == skeleton)
            .map(|(other, _)| Lookalike {
                agent_id: other.agent_id,
                hostname: other.hostname.clone(),
                last_seen: other.last_seen,
            })
    };
    let flags: Vec<Option<Lookalike>> = agents
        .iter()
        .zip(&skeletons)
        .map(|(agent, skeleton)| {
            if agent.is_pending() {
                looks_like(agent, skeleton)
            } else {
                None
            }
        })
        .collect();
    agents
        .into_iter()
        .zip(flags)
        .map(|(record, looks_like)| ListedAgent {
            record,
            looks_like,
            second_machine_at: None,
        })
        .collect()
}

/// What the hub keeps of an agent a person removed or rejected (decision
/// 120, "Pairing"): its agent ID, the hostname it held and its last key, so
/// the hub can tell the agent, signed, that it was removed, and refuse an
/// ordinary token under its ID. The hostname is kept as a record only, not
/// held: another machine may take the name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tombstone {
    pub agent_id: AgentId,
    pub hostname: Hostname,
    pub key: PublicKey,
    /// When the person removed it.
    pub removed_at: Timestamp,
}

/// Why a person's removal or rejection of an agent changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalRefused {
    /// No agent is registered under the ID with the key the person saw: it
    /// never paired, was removed or lapsed, or holds another key now.
    Unknown,
}

/// The decision a person's removal of the agent under `agent_key` gets at
/// `now`, given `record`, the agent registered under its ID: the agent,
/// pending or approved, leaves the registry and its tombstone is kept
/// (decision 120, "Removing, re-keying and recovering an agent"; rejecting
/// a waiting agent is the same removal).
pub fn decide_removal(
    record: Option<&AgentRecord>,
    agent_key: &PublicKey,
    now: Timestamp,
) -> Result<Tombstone, RemovalRefused> {
    let record = record
        .filter(|record| record.key == *agent_key)
        .ok_or(RemovalRefused::Unknown)?;
    Ok(Tombstone {
        agent_id: record.agent_id,
        hostname: record.hostname.clone(),
        key: record.key,
        removed_at: now,
    })
}

/// What has become of a pairing token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenUse {
    Unused,
    /// Spent on the pairing of this agent ID and key.
    Used {
        agent_id: AgentId,
        agent_key: PublicKey,
    },
    /// Revoked by a person before it was used.
    Revoked,
}

/// What the hub keeps of a pairing token: its pairing key's public half,
/// never the seed, so a stolen database holds no usable token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenRecord {
    pub pairing_key: PublicKey,
    /// The hub key the token carries.
    pub hub_key: PublicKey,
    /// The binding kind of the token's address, which `Connect` must name.
    pub kind: BindingKind,
    pub made_at: Timestamp,
    pub expires_at: Timestamp,
    pub state: TokenUse,
}

impl TokenRecord {
    /// The ID the token is listed and revoked by.
    pub fn id(&self) -> TokenId {
        TokenId::of_pairing_key(&self.pairing_key)
    }

    /// Whether the token can still pair an agent at `now`.
    pub fn is_unused_at(&self, now: Timestamp) -> bool {
        self.state == TokenUse::Unused && now < self.expires_at
    }
}

/// A `Pair` whose two signatures have been checked: what it asks of the
/// registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRequest {
    pub pairing_key: PublicKey,
    pub agent_id: AgentId,
    pub agent_key: PublicKey,
    pub hostname: Hostname,
    /// The machine zone the agent reported, as the hub's database knows
    /// it; none when it is unknown.
    pub zone: Option<MachineZone>,
    pub counter: u64,
    /// The binding kind the agent named in `Connect`.
    pub kind: BindingKind,
    /// The hub's key now, which the token must carry: a token made under
    /// a key the hub has replaced is void.
    pub hub_key: PublicKey,
}

/// Why the hub refuses a pairing. Each is a code the hub signs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairRefusal {
    /// No token of this hub has the pairing key, or the agent named
    /// another binding kind than the token's address has.
    ProofRefused,
    /// The token was spent on another pairing.
    TokenUsed,
    /// The token lapsed unused.
    TokenExpired,
    /// A person revoked the token.
    TokenRevoked,
    /// The token carries another hub key than the hub's: it was made
    /// under a key the hub has since replaced, which voided it.
    HubKeyMismatch,
    /// Another agent, pending or approved, holds the hostname. The token
    /// stays unspent.
    HostnameTaken {
        holder: Hostname,
        last_seen: Timestamp,
    },
    /// The agent ID is registered with another key. A token authorizes a
    /// new agent, never taking over a registered one.
    AgentKeyMismatch,
    /// [`PENDING_LIMIT`] agents wait already. The token stays unspent.
    PendingLimit,
    /// A person removed or rejected the agent under the ID, and the hub
    /// keeps its tombstone: an ordinary token can't bring it back.
    AgentRemoved,
    /// A person removed or rejected an agent that held the request's agent
    /// key, and the hub keeps its tombstone: the key is refused under any
    /// agent ID, so a machine removed for a leaked key can't come back with
    /// it under a new ID. The token stays unspent.
    AgentKeyRemoved,
}

impl PairRefusal {
    /// The code on the wire, which the hub signs.
    pub fn code(&self) -> WireName {
        WireName::known(match self {
            Self::ProofRefused => "proof-refused",
            Self::TokenUsed => "token-used",
            Self::TokenExpired => "token-expired",
            Self::TokenRevoked => "token-revoked",
            Self::HostnameTaken { .. } => "hostname-taken",
            Self::AgentKeyMismatch => "machine-key-mismatch",
            Self::HubKeyMismatch => "hub-key-mismatch",
            Self::PendingLimit => "pending-limit",
            Self::AgentRemoved => "machine-removed",
            Self::AgentKeyRemoved => "machine-key-removed",
        })
    }
}

/// What the registry holds that bears on one pairing, read in the step
/// that applies the decision.
#[derive(Clone, Copy, Debug)]
pub struct PairingView<'a> {
    /// The token whose pairing key the request names.
    pub token: Option<&'a TokenRecord>,
    /// The agent registered under the request's agent ID.
    pub by_id: Option<&'a AgentRecord>,
    /// The agent whose hostname has the request's hostname's key.
    pub by_hostname: Option<&'a AgentRecord>,
    /// How many agents wait for approval.
    pub pending: usize,
    /// The tombstone kept under the request's agent ID.
    pub tombstone: Option<&'a Tombstone>,
    /// A tombstone kept with the request's agent key, under any agent ID.
    pub key_tombstone: Option<&'a Tombstone>,
}

/// What a pairing does to the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pairing {
    /// A new agent, kept pending, and the token spent on it.
    New(AgentRecord),
    /// The same identity paired again: its record as it now is, with its
    /// status kept, and the token spent on it.
    Again(AgentRecord),
    /// The same pairing again on the token it spent, as when `Paired` was
    /// lost: answered as before, and nothing changes. It carries the
    /// record as the hub holds it, whose hostname and status the agent is
    /// told.
    Repeated(AgentRecord),
}

impl Pairing {
    /// The agent's record after this pairing. Its hostname is the one asked
    /// for on a new agent, and the first one on an agent that pairs again
    /// (decision 120: a pending agent can't rename itself); its status is
    /// pending on a new agent, and kept on one that pairs again.
    pub const fn record(&self) -> &AgentRecord {
        match self {
            Self::New(record) | Self::Again(record) | Self::Repeated(record) => record,
        }
    }
}

/// The decision a `Pair` gets at `now` (decision 120, "Pairing").
pub fn decide_pairing(
    view: &PairingView<'_>,
    request: &PairRequest,
    now: Timestamp,
) -> Result<Pairing, PairRefusal> {
    let token = view.token.ok_or(PairRefusal::ProofRefused)?;
    if token.hub_key != request.hub_key {
        return Err(PairRefusal::HubKeyMismatch);
    }
    if token.kind != request.kind {
        return Err(PairRefusal::ProofRefused);
    }
    match token.state {
        TokenUse::Revoked => return Err(PairRefusal::TokenRevoked),
        TokenUse::Used {
            agent_id,
            agent_key,
        } => {
            let same = agent_id == request.agent_id && agent_key == request.agent_key;
            return match view.by_id {
                Some(record) if same && record.key == agent_key => {
                    Ok(Pairing::Repeated(record.clone()))
                }
                _ => Err(PairRefusal::TokenUsed),
            };
        }
        TokenUse::Unused if now >= token.expires_at => return Err(PairRefusal::TokenExpired),
        TokenUse::Unused => {}
    }
    if view.tombstone.is_some() {
        return Err(PairRefusal::AgentRemoved);
    }
    if view.key_tombstone.is_some() {
        return Err(PairRefusal::AgentKeyRemoved);
    }
    if let Some(record) = view.by_id {
        if record.key != request.agent_key {
            return Err(PairRefusal::AgentKeyMismatch);
        }
        return Ok(Pairing::Again(AgentRecord {
            counter: record.counter.max(request.counter),
            last_seen: now,
            zone: request.zone.clone(),
            ..record.clone()
        }));
    }
    if let Some(holder) = view.by_hostname {
        return Err(PairRefusal::HostnameTaken {
            holder: holder.hostname.clone(),
            last_seen: holder.last_seen,
        });
    }
    if view.pending >= PENDING_LIMIT {
        return Err(PairRefusal::PendingLimit);
    }
    Ok(Pairing::New(AgentRecord {
        agent_id: request.agent_id,
        key: request.agent_key,
        hostname: request.hostname.clone(),
        status: AgentStatus::Pending { since: now },
        counter: request.counter,
        last_seen: now,
        zone: request.zone.clone(),
    }))
}

/// What the hub asks of its registry once a proof's signature holds under
/// `agent_key`, the key the registry held for `agent_id` when the hub read
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProofRequest {
    pub agent_id: AgentId,
    pub agent_key: PublicKey,
    /// The binding kind the agent named in `Connect`.
    pub kind: BindingKind,
    pub counter: u64,
}

/// Why the hub refuses an agent's proof (decision 120, "The handshake").
/// The hub signs one code for every cause, so a refused machine learns
/// nothing of what the hub holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofRefusal {
    /// No agent under the ID, a signature that doesn't hold under the
    /// registered key, or a binding kind the agent never paired with.
    ProofRefused,
}

impl ProofRefusal {
    /// The code on the wire, which the hub signs.
    pub fn code(self) -> WireName {
        match self {
            Self::ProofRefused => WireName::known("proof-refused"),
        }
    }
}

/// What the registry holds that bears on one proof, read in the step that
/// applies the decision.
#[derive(Clone, Copy, Debug)]
pub struct ProofView<'a> {
    /// The agent registered under the request's agent ID.
    pub record: Option<&'a AgentRecord>,
    /// The binding kinds of the tokens the agent paired with under the
    /// request's agent key: the kinds it may connect with.
    pub kinds: &'a [BindingKind],
}

/// The decision a proof gets at `now` (decision 120, "The handshake"): the
/// agent's record as it now is, seen now and keeping the greater counter,
/// and whether the proof is its first since its approval, as the record in
/// `view` says before this one is kept (#1342). Telling a copy of the agent
/// by its counter is #296's.
pub fn decide_proof(
    view: &ProofView<'_>,
    request: &ProofRequest,
    now: Timestamp,
) -> Result<Proved, ProofRefusal> {
    let Some(record) = view.record else {
        return Err(ProofRefusal::ProofRefused);
    };
    if record.agent_id != request.agent_id
        || record.key != request.agent_key
        || !view.kinds.contains(&request.kind)
    {
        return Err(ProofRefusal::ProofRefused);
    }
    Ok(Proved {
        agent: AgentRecord {
            counter: record.counter.max(request.counter),
            last_seen: now,
            ..record.clone()
        },
        first_since_approval: record.unseen_since_approval(),
    })
}

/// An agent whose proof holds (decision 120, "The handshake").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proved {
    /// The agent's record as it now is: seen now, keeping the greater
    /// counter.
    pub agent: AgentRecord,
    /// Whether this is the agent's first connection since a person approved
    /// it, which the hub's log records once (#1342). It is read from the
    /// record the decision is made on, so an approval that lands while the
    /// hub checks the proof still counts.
    pub first_since_approval: bool,
}

/// Why a person's approval of an agent changed nothing (decision 120,
/// "Pending approval").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalRefused {
    /// No agent is registered under the ID with the key the person
    /// compared: it never paired, was dropped, or holds another key now.
    Unknown,
    /// The agent is approved already: an agent is approved once.
    AlreadyApproved,
}

/// The decision a person's approval of the agent under `agent_key` gets at
/// `now`, given `record`, the agent registered under its ID: a pending
/// agent becomes approved at `now`, with the rest of its record kept.
pub fn decide_approval(
    record: Option<&AgentRecord>,
    agent_key: &PublicKey,
    now: Timestamp,
) -> Result<AgentRecord, ApprovalRefused> {
    let record = record
        .filter(|record| record.key == *agent_key)
        .ok_or(ApprovalRefused::Unknown)?;
    match record.status {
        AgentStatus::Approved { .. } => Err(ApprovalRefused::AlreadyApproved),
        AgentStatus::Pending { .. } => Ok(AgentRecord {
            status: AgentStatus::Approved { at: now },
            ..record.clone()
        }),
    }
}

#[cfg(test)]
mod tests;
