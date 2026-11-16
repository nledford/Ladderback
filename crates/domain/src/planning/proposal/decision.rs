//! What a person decides about a proposal, and the limits on making one
//! (decision 89, points 4 to 6 and 14): the summary Ladderback writes of
//! its changes, the warnings it sends back, what an approval is bound to,
//! a rejection's reasons, and how often a proposer may propose.

use jiff::{SignedDuration, Timestamp};

use super::{ConfigChange, ConfigRevision, Proposal, ProposalId, sha256_hex};
use crate::PlanId;
use crate::planning::{AgentConfig, Plan};

/// The most proposals one proposer makes in [`RATE_WINDOW`].
pub const MAX_PER_HOUR: usize = 30;

/// The window [`MAX_PER_HOUR`] counts over.
pub const RATE_WINDOW: SignedDuration = SignedDuration::from_hours(1);

/// How long a proposer waits after a person rejected one of its proposals
/// before it proposes again.
pub const QUIET_AFTER_REJECTION: SignedDuration = SignedDuration::from_mins(5);

/// The longest reason a person gives for one change, in characters.
pub const MAX_REASON_CHARS: usize = 1000;

/// Characters of a proposal's short ID, which confirms an approval while
/// sign-in is off.
pub const SHORT_PROPOSAL_ID_CHARS: usize = 6;

/// What a proposer has made, as the limits on making another read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProposerRecord {
    /// Its proposals still pending, by their expiry.
    pub pending: usize,
    /// When it made each of its proposals still kept, in any order.
    pub made: Vec<Timestamp>,
    /// When a person last rejected one of its proposals.
    pub last_rejected: Option<Timestamp>,
}

/// Why a proposer may not propose now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooSoon {
    /// It has as many pending as it may.
    Pending,
    /// It made [`MAX_PER_HOUR`] in the last [`RATE_WINDOW`]; it may again
    /// from `until`.
    Hourly { until: Timestamp },
    /// A person rejected one of its proposals less than
    /// [`QUIET_AFTER_REJECTION`] ago; it may again from `until`.
    AfterRejection { until: Timestamp },
}

impl ProposerRecord {
    /// Whether the proposer may make one more at `now`, with
    /// `pending_at_most` pending at most.
    pub fn admits(&self, now: Timestamp, pending_at_most: usize) -> Result<(), TooSoon> {
        let _ = (now, pending_at_most);
        todo!()
    }
}

/// A person's reason for rejecting one change: the AI agent reads it and
/// revises that part against it. It may be empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RejectReason(String);

impl RejectReason {
    /// `text`, unless it is longer than [`MAX_REASON_CHARS`] characters.
    pub fn parse(text: String) -> Option<Self> {
        let _ = text;
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a rejection was refused. Nothing changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionRefused {
    /// It gave another number of reasons than the proposal has changes.
    Count { changes: usize },
    /// The reason for the change at `index`, from 0, is too long.
    TooLong { index: usize },
}

/// The reasons a person gives for each change of a proposal they reject,
/// one per change, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rejection(Vec<RejectReason>);

impl Rejection {
    /// One reason for each of a proposal's `changes`.
    pub fn new(changes: usize, reasons: Vec<String>) -> Result<Self, RejectionRefused> {
        let _ = (changes, reasons);
        todo!()
    }

    /// Reasons as a store kept them.
    pub fn kept(reasons: Vec<RejectReason>) -> Self {
        Self(reasons)
    }

    pub fn reasons(&self) -> &[RejectReason] {
        &self.0
    }
}

/// The diff a person was shown, by the SHA-256 of its text in lower-case
/// hex: an approval is bound to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHash(String);

impl DiffHash {
    /// The hash of the diff `text`.
    pub fn of(text: &str) -> Self {
        let _ = (text, sha256_hex);
        todo!()
    }

    /// The hash `text` names, as [`DiffHash::as_str`] gives it.
    pub fn parse(text: &str) -> Option<Self> {
        let _ = (text, ConfigRevision::parse);
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the page showed a person: the configuration file the diff was
/// rebuilt against and the diff (decision 89, point 6). An approval holds
/// only while both are still what a rebuild gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub revision: ConfigRevision,
    pub diff: DiffHash,
}

/// The short ID of `id`, which confirms its approval while sign-in is off.
pub fn short_proposal_id(id: &ProposalId) -> &str {
    let _ = id;
    todo!()
}

/// Whether `typed` confirms the approval of `id` while sign-in is off: its
/// short ID or its whole ID, in either case, the spaces around it dropped.
pub fn confirms_proposal(typed: &str, id: &ProposalId) -> bool {
    let _ = (typed, id);
    todo!()
}

/// A plan's setting, as the summary names what a change changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanSetting {
    Repository,
    Host,
    Label,
    Sources,
    Tags,
    Schedule,
    Overlap,
    Retry,
    Filter,
    Retention,
}

/// One line of the plain-words summary Ladderback writes of a proposal
/// (decision 89, point 4), from its changes, never from the AI agent's
/// words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SummaryLine {
    AddsPlan(PlanId),
    /// Changes these settings of the plan, in the order of [`PlanSetting`].
    ChangesPlan {
        plan: PlanId,
        settings: Vec<PlanSetting>,
    },
    RemovesPlan(PlanId),
}

/// What the submission checks found that doesn't refuse a proposal
/// (decision 89, point 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalWarning {
    /// The change at `index`, from 0, leaves its plan backing up the same
    /// sources as the plan `other`.
    SameSources {
        index: usize,
        plan: PlanId,
        other: PlanId,
    },
    /// The change at `index` leaves its plan with a glob that can never
    /// match, read against its sources as written.
    GlobNeverMatches { index: usize, glob: String },
}

impl Proposal {
    /// What its changes do to `config`, one line each, in order, each
    /// against what the changes before it made. A change that no longer
    /// applies ends the summary.
    pub fn summary(&self, config: &AgentConfig) -> Vec<SummaryLine> {
        let _ = config;
        todo!()
    }

    /// The warnings of `changes` against `config`, in the order of the
    /// changes.
    pub fn warnings(config: &AgentConfig, changes: &[ConfigChange]) -> Vec<ProposalWarning> {
        let _ = (config, changes);
        todo!()
    }

    /// Whether a change of it could delete snapshots, so that approving it
    /// waits for the forget preview (decision 89, point 8; I6), each change
    /// read against what the changes before it made: every removal; a
    /// change of a plan's retention, repository or host; and a new plan
    /// with a retention policy, as its ID may match an orphaned group's
    /// plan, which only a read of the repository tells. A change that no
    /// longer applies could, as far as anyone can tell.
    pub fn needs_forget_preview(&self, config: &AgentConfig) -> bool {
        let _ = config;
        todo!()
    }
}
