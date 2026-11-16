//! What a person decides about a proposal, and the limits on making one
//! (decision 89, points 4 to 6 and 14): the summary Ladderback writes of
//! its changes, the warnings it sends back, what an approval is bound to,
//! a rejection's reasons, and how often a proposer may propose.

use jiff::{SignedDuration, Timestamp};
use subtle::ConstantTimeEq as _;

use super::{ConfigChange, ConfigRevision, Proposal, ProposalId, sha256_hex, sha256_hex_of};
use crate::PlanId;
use crate::planning::{AgentConfig, Plan, Source};

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
        if self.pending >= pending_at_most {
            return Err(TooSoon::Pending);
        }
        if let Some(rejected) = self.last_rejected {
            let until = later(rejected, QUIET_AFTER_REJECTION);
            if now < until {
                return Err(TooSoon::AfterRejection { until });
            }
        }
        // Those made less than an hour before `now`, the oldest first.
        let mut recent: Vec<Timestamp> = self
            .made
            .iter()
            .copied()
            .filter(|made| now.duration_since(*made) < RATE_WINDOW)
            .collect();
        if recent.len() < MAX_PER_HOUR {
            return Ok(());
        }
        recent.sort_unstable();
        // One more is made once all but `MAX_PER_HOUR - 1` of them are an
        // hour old.
        let oldest_left = recent
            .get(recent.len().saturating_sub(MAX_PER_HOUR))
            .copied()
            .unwrap_or(now);
        Err(TooSoon::Hourly {
            until: later(oldest_left, RATE_WINDOW),
        })
    }
}

/// `by` after `at`, or the last instant there is.
fn later(at: Timestamp, by: SignedDuration) -> Timestamp {
    at.saturating_add(by).unwrap_or(Timestamp::MAX)
}

/// A person's reason for rejecting one change: the AI agent reads it and
/// revises that part against it. It may be empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RejectReason(String);

impl RejectReason {
    /// `text`, unless it is longer than [`MAX_REASON_CHARS`] characters.
    pub fn parse(text: String) -> Option<Self> {
        (text.chars().count() <= MAX_REASON_CHARS).then_some(Self(text))
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
        if reasons.len() != changes {
            return Err(RejectionRefused::Count { changes });
        }
        reasons
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                RejectReason::parse(text).ok_or(RejectionRefused::TooLong { index })
            })
            .collect::<Result<_, _>>()
            .map(Self)
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
        Self(sha256_hex(text.as_bytes()))
    }

    /// The hash `text` names, as [`DiffHash::as_str`] gives it.
    pub fn parse(text: &str) -> Option<Self> {
        ConfigRevision::parse(text).map(|_| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Bytes of a [`RevisionKey`].
pub const REVISION_KEY_BYTES: usize = 32;

/// The key a daemon draws once each start, from its random source, to key
/// the revisions its pages are shown ([`ShownRevision`]). Held in memory
/// only: never logged, kept or sent, so its `Debug` hides it.
#[derive(Clone)]
pub struct RevisionKey([u8; REVISION_KEY_BYTES]);

impl RevisionKey {
    pub fn new(bytes: [u8; REVISION_KEY_BYTES]) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Debug for RevisionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RevisionKey([hidden])")
    }
}

/// The configuration file a page was shown, as the page holds it: the
/// SHA-256, in lower-case hex, of the daemon's [`RevisionKey`] followed by
/// the file's [`ConfigRevision`]. Never the file's plain revision, which is
/// a hash of its bytes, inline secrets included: without the key, a guess
/// at a secret can't be checked against it (I4). A key of fixed length
/// ahead of the revision is enough for a binding compared for equality.
/// Compared in constant time.
#[derive(Clone, Debug)]
pub struct ShownRevision(String);

impl ShownRevision {
    /// The revision `revision` as a page is shown it under `key`.
    pub fn of(key: &RevisionKey, revision: &ConfigRevision) -> Self {
        Self(sha256_hex_of(&[&key.0, revision.as_str().as_bytes()]))
    }

    /// The revision `text` names, as [`ShownRevision::as_str`] gives it.
    pub fn parse(text: &str) -> Option<Self> {
        ConfigRevision::parse(text).map(|_| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for ShownRevision {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
    }
}

impl Eq for ShownRevision {}

/// What the page showed a person: the configuration file the diff was
/// rebuilt against and the diff (decision 89, point 6). An approval holds
/// only while both are still what a rebuild gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub revision: ShownRevision,
    pub diff: DiffHash,
}

/// The short ID of `id`, which confirms its approval while sign-in is off.
pub fn short_proposal_id(id: &ProposalId) -> &str {
    id.as_str()
        .get(..SHORT_PROPOSAL_ID_CHARS)
        .unwrap_or(id.as_str())
}

/// Whether `typed` confirms the approval of `id` while sign-in is off: its
/// short ID or its whole ID, in either case, the spaces around it dropped.
pub fn confirms_proposal(typed: &str, id: &ProposalId) -> bool {
    let typed = typed.trim().to_ascii_lowercase();
    typed == short_proposal_id(id) || typed == id.as_str()
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
        let mut after = config.clone();
        let mut lines = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let line = match change {
                ConfigChange::AddPlan(plan) => SummaryLine::AddsPlan(plan.id.clone()),
                ConfigChange::ChangePlan(plan) => SummaryLine::ChangesPlan {
                    plan: plan.id.clone(),
                    settings: after
                        .plan(&plan.id)
                        .map(|kept| changed_settings(kept, plan))
                        .unwrap_or_default(),
                },
                ConfigChange::RemovePlan(id) => SummaryLine::RemovesPlan(id.clone()),
            };
            match after.apply(change) {
                Ok(next) => after = next,
                Err(_) => break,
            }
            lines.push(line);
        }
        lines
    }

    /// The warnings of `changes` against `config`, in the order of the
    /// changes.
    pub fn warnings(config: &AgentConfig, changes: &[ConfigChange]) -> Vec<ProposalWarning> {
        let mut after = config.clone();
        let mut warnings = Vec::new();
        for (index, change) in changes.iter().enumerate() {
            match after.apply(change) {
                Ok(next) => after = next,
                Err(_) => break,
            }
            let (ConfigChange::AddPlan(plan) | ConfigChange::ChangePlan(plan)) = change else {
                continue;
            };
            if let Some(other) = after
                .plans()
                .iter()
                .find(|other| other.id != plan.id && same_sources(other, plan))
            {
                warnings.push(ProposalWarning::SameSources {
                    index,
                    plan: plan.id.clone(),
                    other: other.id.clone(),
                });
            }
            let globs = plan.filter.globs.iter().map(|glob| (glob, false));
            let iglobs = plan.filter.iglobs.iter().map(|glob| (glob, true));
            for (glob, any_case) in globs.chain(iglobs) {
                if never_matches(glob, &plan.sources, any_case) {
                    warnings.push(ProposalWarning::GlobNeverMatches {
                        index,
                        glob: glob.clone(),
                    });
                }
            }
        }
        warnings
    }

    /// Whether a change of it could delete snapshots, so that approving it
    /// waits for the forget preview (decision 89, point 8; I6), each change
    /// read against what the changes before it made: every removal; a
    /// change of a plan's retention, repository or host; and a new plan
    /// with a retention policy, as its ID may match an orphaned group's
    /// plan, which only a read of the repository tells. A change that no
    /// longer applies could, as far as anyone can tell.
    pub fn needs_forget_preview(&self, config: &AgentConfig) -> bool {
        let mut after = config.clone();
        for change in &self.changes {
            let deletes = match change {
                ConfigChange::AddPlan(plan) => !plan.retention.is_empty(),
                ConfigChange::ChangePlan(plan) => after.plan(&plan.id).is_none_or(|kept| {
                    kept.retention != plan.retention
                        || kept.repository != plan.repository
                        || kept.host != plan.host
                }),
                ConfigChange::RemovePlan(_) => true,
            };
            if deletes {
                return true;
            }
            match after.apply(change) {
                Ok(next) => after = next,
                Err(_) => return true,
            }
        }
        false
    }
}

/// The settings `to` changes of `from`, in the order of [`PlanSetting`].
fn changed_settings(from: &Plan, to: &Plan) -> Vec<PlanSetting> {
    [
        (PlanSetting::Repository, from.repository != to.repository),
        (PlanSetting::Host, from.host != to.host),
        (PlanSetting::Label, from.label != to.label),
        (PlanSetting::Sources, from.sources != to.sources),
        (PlanSetting::Tags, from.tags != to.tags),
        (PlanSetting::Schedule, from.schedule != to.schedule),
        (PlanSetting::Overlap, from.overlap != to.overlap),
        (PlanSetting::Retry, from.retry != to.retry),
        (PlanSetting::Filter, from.filter != to.filter),
        (PlanSetting::Retention, from.retention != to.retention),
    ]
    .into_iter()
    .filter_map(|(setting, changed)| changed.then_some(setting))
    .collect()
}

/// Whether `a` and `b` back up the same sources, in any order.
fn same_sources(a: &Plan, b: &Plan) -> bool {
    let mut a: Vec<&str> = a.sources.iter().map(Source::as_str).collect();
    let mut b: Vec<&str> = b.sources.iter().map(Source::as_str).collect();
    a.sort_unstable();
    a.dedup();
    b.sort_unstable();
    b.dedup();
    a == b
}

/// Whether the glob `glob`, read as rustic reads a plan's globs (from the
/// root, a leading `!` excluding), can never match a path under `sources`:
/// an absolute glob whose fixed folder is neither inside a source nor holds
/// one. A relative glob matches at any depth, so it always may.
fn never_matches(glob: &str, sources: &[Source], any_case: bool) -> bool {
    let pattern = glob.strip_prefix('!').unwrap_or(glob);
    if !pattern.starts_with('/') {
        return false;
    }
    let fixed_end = pattern
        .find(['*', '?', '[', '{', '\\'])
        .unwrap_or(pattern.len());
    let fixed = pattern.get(..fixed_end).unwrap_or(pattern);
    // The folder the fixed part names whole: up to its last `/` when a
    // wildcard follows, all of it otherwise.
    let folder = if fixed_end < pattern.len() {
        fixed
            .rfind('/')
            .and_then(|end| fixed.get(..end))
            .unwrap_or("")
    } else {
        fixed.trim_end_matches('/')
    };
    let fold = |text: &str| {
        if any_case {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    let folder = fold(folder);
    !sources.iter().any(|source| {
        let source = fold(source.as_str().trim_end_matches('/'));
        within(&source, &folder) || within(&folder, &source)
    })
}

/// Whether the path `inner` is the folder `outer` or inside it.
fn within(inner: &str, outer: &str) -> bool {
    outer.is_empty()
        || inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}
