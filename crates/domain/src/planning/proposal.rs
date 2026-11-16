//! A proposal (§3.1, §16 decision 89): configuration changes an AI agent, or
//! another API client, submits for a person to confirm. It changes nothing
//! until a person approves the whole of it; until then it is pending, for
//! 48 hours at most. The rules here are pure: a proposal is checked against
//! the configuration by the rules [`AgentConfig`] keeps, and nothing else,
//! so making one resolves no secret, runs no command and contacts no
//! location.

use std::fmt;

use jiff::{SignedDuration, Timestamp};

use super::{AgentConfig, ChangeRefused, Plan};
use crate::PlanId;
use crate::access::TOKEN_BYTES;
use crate::access::api_token::{ApiTokenId, ApiTokenLabel};

/// How long a proposal waits for a person: 48 hours after it was made, a
/// fixed limit.
pub const PROPOSAL_LIFETIME: SignedDuration = SignedDuration::from_hours(48);

/// The most changes one proposal holds.
pub const MAX_CHANGES: usize = 50;

/// The longest rationale, in characters.
pub const MAX_RATIONALE_CHARS: usize = 4000;

/// The most proposals one proposer has pending at once: one API token, or,
/// with sign-in off, every client together.
pub const MAX_PENDING: usize = 20;

/// The most ended proposals kept as history; the oldest goes first.
pub const MAX_ENDED: usize = 200;

/// One typed edit of an agent's configuration (§3.1). Each is applied by
/// [`AgentConfig::apply`], which keeps the configuration's rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigChange {
    /// Adds the plan after the configuration's last one.
    AddPlan(Plan),
    /// Puts the plan in place of the plan with its ID.
    ChangePlan(Plan),
    /// Removes the plan with this ID. Its snapshots stay in the repository.
    RemovePlan(PlanId),
}

impl AgentConfig {
    /// The configuration with `change` made, by the rule of its kind:
    /// [`AgentConfig::add_plan`], [`AgentConfig::change_plan`] or
    /// [`AgentConfig::remove_plan`].
    pub fn apply(&self, change: &ConfigChange) -> Result<Self, ChangeRefused> {
        let _ = change;
        todo!()
    }
}

/// Characters of a [`ProposalId`].
pub const PROPOSAL_ID_CHARS: usize = 12;

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A proposal's ID: what it is read and withdrawn by. Not a secret.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProposalId(String);

impl ProposalId {
    /// The ID random `bytes` make: their first 60 bits, five to a
    /// character, in lower-case base32.
    pub fn from_bytes(bytes: &[u8; TOKEN_BYTES]) -> Self {
        let _ = (bytes, BASE32);
        todo!()
    }

    /// Exactly [`PROPOSAL_ID_CHARS`] characters of lower-case base32.
    pub fn parse(text: &str) -> Option<Self> {
        let _ = text;
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProposalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who made a proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proposer {
    /// A program with this API token, while sign-in is on.
    Token {
        id: ApiTokenId,
        label: ApiTokenLabel,
    },
    /// A client with no token: a browser session, or any client while
    /// sign-in is off.
    NoToken,
}

/// The configuration a proposal was drafted against: a hash of the file's
/// text, which the store computes. Opaque here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigRevision(String);

impl ConfigRevision {
    pub fn new(hash: String) -> Self {
        Self(hash)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the AI agent proposes the changes, in its own words: shown apart
/// from the summary Ladderback writes, labelled as the AI agent's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rationale(String);

impl Rationale {
    /// `text`, unless it is longer than [`MAX_RATIONALE_CHARS`] characters.
    pub fn parse(text: String) -> Result<Self, ProposalRefused> {
        let _ = text;
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a proposal is in its life (decision 89, point 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProposalStatus {
    /// Waiting for a person.
    Pending,
    /// Approved, and being applied.
    Applying,
    Applied,
    /// Its apply stopped at a step that failed.
    Failed,
    Rejected,
    Withdrawn,
    /// Nobody approved it within [`PROPOSAL_LIFETIME`].
    Expired,
    /// A change of it no longer applies to the configuration.
    Stale,
}

impl ProposalStatus {
    /// Every status, in the order of the lifecycle.
    pub const ALL: [Self; 8] = [
        Self::Pending,
        Self::Applying,
        Self::Applied,
        Self::Failed,
        Self::Rejected,
        Self::Withdrawn,
        Self::Expired,
        Self::Stale,
    ];

    /// Its name on the wire and in the store.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
            Self::Withdrawn => "withdrawn",
            Self::Expired => "expired",
            Self::Stale => "stale",
        }
    }

    /// The status named `text`.
    pub fn parse(text: &str) -> Option<Self> {
        let _ = text;
        todo!()
    }

    /// Whether a proposal may go from this status to `next`: from pending
    /// to applying, rejected, withdrawn, expired or stale; from applying to
    /// applied, failed, or back to pending when a hand edit stops the
    /// apply; from failed to applying again on a retry. Nothing else.
    pub fn can_become(self, next: Self) -> bool {
        let _ = next;
        todo!()
    }

    /// Whether the proposal has ended: it will never be applied unless a
    /// person retries a failed one.
    pub fn has_ended(self) -> bool {
        todo!()
    }
}

/// Why a proposal was refused when it was made. Nothing was kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalRefused {
    /// It holds no change.
    NoChanges,
    /// It holds more than [`MAX_CHANGES`] changes.
    TooManyChanges,
    /// Its rationale is longer than [`MAX_RATIONALE_CHARS`] characters.
    RationaleTooLong,
    /// The change at `index`, from 0, doesn't apply to the configuration as
    /// the changes before it leave it.
    Change {
        index: usize,
        refused: ChangeRefused,
    },
    /// The change at `index`, from 0, gives a plan hooks: a new plan any, a
    /// changed plan others than it has.
    Hooks { index: usize },
}

/// Configuration changes submitted for a person to confirm (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: ProposalId,
    pub proposer: Proposer,
    /// The configuration it was drafted against.
    pub base: ConfigRevision,
    pub changes: Vec<ConfigChange>,
    pub rationale: Rationale,
    pub status: ProposalStatus,
    pub created_at: Timestamp,
    /// [`PROPOSAL_LIFETIME`] after `created_at`.
    pub expires_at: Timestamp,
}

impl Proposal {
    /// The configuration `changes` make of `config`, each applied in order
    /// to what the ones before it made: the submission check, by the
    /// configuration's rules and nothing else. A proposal holds one change
    /// at least and [`MAX_CHANGES`] at most.
    pub fn check(
        config: &AgentConfig,
        changes: &[ConfigChange],
    ) -> Result<AgentConfig, ProposalRefused> {
        let _ = (config, changes);
        todo!()
    }

    /// A pending proposal made at `now`, expiring [`PROPOSAL_LIFETIME`]
    /// later, once its changes pass [`Proposal::check`] against `config`.
    pub fn draft(
        id: ProposalId,
        proposer: Proposer,
        base: ConfigRevision,
        changes: Vec<ConfigChange>,
        rationale: Rationale,
        config: &AgentConfig,
        now: Timestamp,
    ) -> Result<Self, ProposalRefused> {
        let _ = (id, proposer, base, changes, rationale, config, now);
        todo!()
    }

    /// What the proposal's status is at `now`, against the configuration
    /// `config` as it is: a pending one is expired from its expiry on, and
    /// otherwise stale when a change of it no longer applies. Any other
    /// status stays as it is.
    pub fn status_at(&self, now: Timestamp, config: &AgentConfig) -> ProposalStatus {
        let _ = (now, config);
        todo!()
    }
}

#[cfg(test)]
mod tests;
