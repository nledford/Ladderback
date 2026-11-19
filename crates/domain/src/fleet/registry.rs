//! The hub's registry of agents and pairing tokens (§3.1, the Fleet
//! context; §7.1; §16, decision 120, "Pairing" and "Pending approval"): an
//! agent's record, a token's record, how two hostnames compare, and the
//! decision a `Pair` gets. The rules live here; the registry's port keeps
//! the records and applies a decision in the same step that read them.

use jiff::{SignedDuration, Timestamp};

use super::fingerprint::TokenId;
use super::handshake::BindingKind;
use super::hostname::Hostname;
use super::keys::PublicKey;
use super::signed::WireName;
use crate::AgentId;

/// The most agents that may wait for approval at once; a pairing past it
/// is refused (`pending-limit`) and leaves the token unspent.
pub const PENDING_LIMIT: usize = 20;

/// The most tokens that may be unused at once.
pub const UNUSED_TOKEN_LIMIT: usize = 20;

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
pub fn token_life(_asked: Option<SignedDuration>) -> Result<SignedDuration, TokenLifeRefused> {
    todo!()
}

/// What the hub compares hostnames by: `casefold(NFC(name))`, with the
/// standard library's `to_lowercase` as the case fold (decision 120,
/// "Hostnames"). Two names with one key are one name on the hub, so "café"
/// typed two ways, or "Laptop" beside "laptop", can't both pair.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HostnameKey(String);

impl HostnameKey {
    pub fn of(_name: &Hostname) -> Self {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
}

impl AgentRecord {
    pub fn hostname_key(&self) -> HostnameKey {
        todo!()
    }

    pub fn is_pending(&self) -> bool {
        todo!()
    }
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
        todo!()
    }

    /// Whether the token can still pair an agent at `now`.
    pub fn is_unused_at(&self, _now: Timestamp) -> bool {
        todo!()
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
}

impl PairRefusal {
    /// The code on the wire, which the hub signs.
    pub fn code(&self) -> WireName {
        todo!()
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
    /// lost: answered as before, and nothing changes.
    Repeated,
}

/// The decision a `Pair` gets at `now` (decision 120, "Pairing").
pub fn decide_pairing(
    _view: &PairingView<'_>,
    _request: &PairRequest,
    _now: Timestamp,
) -> Result<Pairing, PairRefusal> {
    todo!()
}

#[cfg(test)]
mod tests;
