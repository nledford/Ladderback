//! Proposals (§16, decision 89): an AI agent, or another API client,
//! proposes configuration changes for a person to confirm, lists and reads
//! its proposals and withdraws one; a person reviews one on the "Pending
//! changes" page, then approves or rejects it. Making a proposal checks its
//! changes against the configuration by the configuration's rules and
//! nothing else: no secret is resolved, no command runs and no location is
//! contacted, and the configuration is never written. Reading proposals
//! brings each pending one up to date first: expired from its expiry on,
//! stale once a change of it no longer applies to the configuration as it
//! is. An approval is confirmed by step-up and bound to what the page
//! showed; it is applied under the configuration's write lock, and only
//! over the file as it was shown, so a hand edit is never overwritten.

use std::net::IpAddr;
use std::sync::Arc;

use api::{ProposalStatusDto, UiEvent};
use domain::access::step_up::{StepUpConfirmation, StepUpFor};
use domain::maintenance::ForgetComparison;
use domain::planning::{
    AgentConfig, ConfigChange, ConfigRevision, DiffHash, MAX_ENDED, MAX_PENDING, Proposal,
    ProposalId, ProposalRefused, ProposalStatus, ProposalWarning, Proposer, Rationale, Rejection,
    RejectionRefused, RevisionKey, Shown, ShownRevision, SummaryLine, TooSoon,
};
use domain::secrets::Redactor;
use domain::{OperationId, RepositoryAlias};

use crate::access::{Access, StepUpRefused, confirm_step_up};
use crate::maintenance::{MaintenanceError, RunMaintenance};
use crate::ports::{
    Admission, ChangeCodec, Clock, CodecError, ConfigDraft, ConfigDrafts, ConfigError, ConfigStore,
    EventBus, ProposalStore, Random, RandomUnavailable, SavedOver, StoreError, StoredProposal,
};
use crate::repositories::ConfigWrites;
use crate::schedule::ConfigChanged;

mod diff;

pub use diff::unified_diff;

/// Why a proposal was not made, read, withdrawn, approved or rejected.
/// Nothing was kept or changed, except where a variant says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalError {
    /// The submission checks refused the proposal.
    Refused(ProposalRefused),
    /// Its proposer has [`MAX_PENDING`] proposals pending already.
    Full,
    /// Its proposer may not propose again yet.
    TooSoon(TooSoon),
    /// No proposal by that ID is kept that the asker may see.
    Unknown,
    /// The proposal has ended, or is being applied: only a pending one is
    /// withdrawn, approved or rejected.
    NotPending(ProposalStatus),
    /// The reasons of a rejection don't fit its proposal.
    Rejection(RejectionRefused),
    /// The configuration is no longer what the page showed, or changed by
    /// hand during the apply, which kept the hand edit: the proposal is
    /// pending, for a person to review and approve again.
    Changed,
    /// The proposal holds a change that could delete snapshots, which is
    /// approved only with the forget preview (decision 89, point 8): it
    /// is still pending.
    NeedsForgetPreview,
    /// The step-up confirmation wasn't the admin password, or the
    /// proposal's ID with sign-in off.
    StepUpFailed,
    /// Too many wrong passwords from this address: try again in `seconds`.
    StepUpWait { seconds: u64 },
    /// The admin password couldn't be checked.
    StepUpUnavailable(String),
    /// The configuration couldn't be read or written; an apply that failed
    /// writing leaves the proposal failed. Boxed: the error is large.
    Config(Box<ConfigError>),
    /// The proposals couldn't be read or kept.
    Store(StoreError),
    /// The changes couldn't be written to be kept.
    Codec(CodecError),
    /// The platform gave no random bytes for the proposal's ID, or none
    /// for the key a page's revisions are shown under.
    Random(RandomUnavailable),
}

/// A proposal kept, and what the submission checks warn of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub proposal: Proposal,
    pub warnings: Vec<ProposalWarning>,
}

/// A proposal applied: the configuration file holds its changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub proposal: Proposal,
    /// Why the store didn't record the apply, when it didn't: the file is
    /// saved all the same, and the store shows the proposal being applied
    /// until the daemon starts again ([`Proposals::recover`]).
    pub unrecorded: Option<StoreError>,
}

/// A proposal as the "Pending changes" page shows it (decision 89, points
/// 3, 4 and 6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub proposal: Proposal,
    /// What its changes do, in Ladderback's words.
    pub summary: Vec<SummaryLine>,
    /// The diff of the file as it is to the file a pending proposal's
    /// changes make, every secret the configuration holds masked; empty
    /// for one that isn't pending.
    pub diff: String,
    /// What an approval of it is bound to: the file, keyed, and the diff
    /// as shown, masked; the file's revision binds what the mask hides.
    pub shown: Shown,
    /// Whether the file has changed since the proposal was drafted.
    pub changed_since_drafted: bool,
    /// Whether a change of it could delete snapshots, so that only the
    /// forget preview lets a person approve it; until then, it is rejected.
    pub needs_forget_preview: bool,
    /// How many of its proposer's kept proposals a person rejected.
    pub rejected: usize,
}

/// What a pending proposal would change about the snapshots retention
/// forgets in one repository it touches (decision 89, point 8; I6), or
/// why that repository couldn't be read.
#[derive(Debug, PartialEq)]
pub struct RepositoryForget {
    pub repository: RepositoryAlias,
    pub forget: Result<ForgetComparison, MaintenanceError>,
}

/// A person's approval of a proposal, as the page sends it.
#[derive(Debug)]
pub struct ApprovalAsk {
    /// What the page showed.
    pub shown: Shown,
    /// The step-up confirmation: the admin password with sign-in on, the
    /// proposal's ID with it off. Never shown (I4).
    pub confirmation: StepUpConfirmation,
    /// The browser's address, whose sign-in waits a wrong password counts
    /// against.
    pub address: IpAddr,
}

/// What [`Proposals`] works with.
pub struct ProposalPorts {
    pub store: Arc<dyn ProposalStore>,
    pub codec: Arc<dyn ChangeCodec>,
    pub config: Arc<dyn ConfigStore>,
    /// The same configuration, for an apply.
    pub drafts: Arc<dyn ConfigDrafts>,
    /// The lock every write of the configuration holds.
    pub writes: Arc<ConfigWrites>,
    /// Told of each apply, so the scheduler reads the configuration again.
    pub changed: ConfigChanged,
    /// Where each status change is announced.
    pub events: Arc<dyn EventBus>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
    /// The key the revisions a page is shown are keyed with, drawn once
    /// each daemon start, or why the platform gave none.
    pub revision_key: Result<RevisionKey, RandomUnavailable>,
}

/// Makes, reads, withdraws, approves and rejects proposals.
pub struct Proposals {
    ports: ProposalPorts,
    /// With sign-in on: checks the admin password of a step-up.
    access: Option<Arc<Access>>,
}

impl Proposals {
    pub fn new(ports: ProposalPorts) -> Self {
        Self {
            ports,
            access: None,
        }
    }

    /// With sign-in on: an approval's step-up asks for the admin password,
    /// checked by `access` in the client's sign-in waits.
    #[must_use]
    pub fn with_access(self, access: Arc<Access>) -> Self {
        Self {
            access: Some(access),
            ..self
        }
    }

    /// Keeps a pending proposal of `changes` by `proposer`, drafted against
    /// the configuration `base`, once they pass the submission checks
    /// ([`Proposal::draft`]) and the proposer's record admits one more
    /// ([`ProposerRecord::admits`](domain::planning::ProposerRecord::admits)).
    /// The oldest ended proposals beyond [`MAX_ENDED`] are forgotten. It is
    /// made at the millisecond, so it is answered as it is read back.
    pub async fn propose(
        &self,
        proposer: Proposer,
        base: ConfigRevision,
        changes: Vec<ConfigChange>,
        rationale: Rationale,
    ) -> Result<Made, ProposalError> {
        let config = self.load().await?;
        let bytes = self.ports.random.token().map_err(ProposalError::Random)?;
        let proposal = Proposal::draft(
            ProposalId::from_bytes(&bytes),
            proposer,
            base,
            changes,
            rationale,
            &config,
            to_the_millisecond(self.ports.clock.now()),
        )
        .map_err(ProposalError::Refused)?;
        let stored = self.stored(&proposal)?;
        match self
            .ports
            .store
            .add(&stored, MAX_PENDING, MAX_ENDED)
            .await
            .map_err(ProposalError::Store)?
        {
            Admission::Kept => {}
            Admission::Refused(TooSoon::Pending) => return Err(ProposalError::Full),
            Admission::Refused(too_soon) => return Err(ProposalError::TooSoon(too_soon)),
        }
        let warnings = Proposal::warnings(&config, &proposal.changes);
        self.announce(&proposal.id, ProposalStatus::Pending).await;
        Ok(Made { proposal, warnings })
    }

    /// The proposals `asker` may see, newest first, each brought up to
    /// date. A token sees the proposals it made; a client with no token
    /// sees them all.
    pub async fn list(&self, asker: &Proposer) -> Result<Vec<Proposal>, ProposalError> {
        let all = self.ports.store.all().await.map_err(ProposalError::Store)?;
        let mut config = None;
        let mut listed = Vec::new();
        for stored in all {
            if !may_see(asker, &stored.proposer) {
                continue;
            }
            match self.up_to_date(stored, &mut config).await {
                Ok(proposal) => listed.push(proposal),
                // Forgotten since the list was read: it is left out, as a
                // list read a moment later would leave it.
                Err(ProposalError::Unknown) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(listed)
    }

    /// Brings every kept proposal up to date, as a save of the
    /// configuration asks (decision 89, point 3): a pending one whose
    /// change no longer applies goes stale now, announced, rather than at
    /// its next read.
    pub async fn refresh(&self) -> Result<(), ProposalError> {
        self.list(&Proposer::NoToken).await.map(drop)
    }

    /// The proposal `id`, brought up to date, if `asker` may see it.
    pub async fn get(&self, asker: &Proposer, id: &ProposalId) -> Result<Proposal, ProposalError> {
        let stored = self
            .ports
            .store
            .get(id)
            .await
            .map_err(ProposalError::Store)?
            .filter(|stored| may_see(asker, &stored.proposer))
            .ok_or(ProposalError::Unknown)?;
        self.up_to_date(stored, &mut None).await
    }

    /// Withdraws the pending proposal `id`, if `asker` may see it, and
    /// answers it withdrawn.
    pub async fn withdraw(
        &self,
        asker: &Proposer,
        id: &ProposalId,
    ) -> Result<Proposal, ProposalError> {
        let mut proposal = self.get(asker, id).await?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ProposalError::NotPending(proposal.status));
        }
        let withdrawn = self
            .ports
            .store
            .change_status(id, ProposalStatus::Pending, ProposalStatus::Withdrawn)
            .await
            .map_err(ProposalError::Store)?;
        if !withdrawn {
            // It ended, or began to be applied, since it was read.
            let now = self.kept_status(id).await?;
            return Err(ProposalError::NotPending(now));
        }
        proposal.status = ProposalStatus::Withdrawn;
        self.announce(id, proposal.status).await;
        Ok(proposal)
    }

    /// The proposal `id` as the "Pending changes" page shows it, brought up
    /// to date: a page opening is a read like any other.
    pub async fn review(&self, id: &ProposalId) -> Result<Review, ProposalError> {
        let proposal = self.get(&Proposer::NoToken, id).await?;
        let rejected = self
            .ports
            .store
            .all()
            .await
            .map_err(ProposalError::Store)?
            .iter()
            .filter(|one| {
                one.status == ProposalStatus::Rejected
                    && same_proposer(&one.proposer, &proposal.proposer)
            })
            .count();
        let config = self.load().await?;
        let summary = proposal.summary(&config);
        // Only a pending one is approved, so only its diff is shown; an
        // ended one shows the file as it is.
        let after = if proposal.status == ProposalStatus::Pending {
            Proposal::check(&config, &proposal.changes).ok()
        } else {
            None
        };
        let draft = self.draft(after.as_ref().unwrap_or(&config)).await?;
        let diff = if after.is_some() {
            masked_diff(&config, &draft.before, &draft.after)
        } else {
            String::new()
        };
        Ok(Review {
            changed_since_drafted: draft.revision != proposal.base,
            needs_forget_preview: proposal.needs_forget_preview(&config),
            shown: Shown {
                revision: self.shown_revision(&draft.revision)?,
                diff: DiffHash::of(&diff),
            },
            proposal,
            summary,
            diff,
            rejected,
        })
    }

    /// The forget preview of the pending proposal `id` (decision 89, point
    /// 8): for each repository its changes touch, in the order they name
    /// them, what `maintenance`'s forget would remove under the
    /// configuration as it is and under the configuration its changes
    /// make, read under the operation `op`. A person's action on the page:
    /// it reads the repositories, so their secrets are resolved and their
    /// locations contacted, and it changes nothing. A repository that
    /// couldn't be read is answered with why, the others all the same.
    pub async fn forget_preview(
        &self,
        id: &ProposalId,
        op: OperationId,
        maintenance: &RunMaintenance,
    ) -> Result<Vec<RepositoryForget>, ProposalError> {
        todo!()
    }

    /// Approves the pending proposal `id` as `ask` says the page showed it,
    /// once step-up confirms it, and applies it: under the configuration's
    /// write lock, rebuilt against the file as it is, and saved only over
    /// that file. A rebuild that isn't what the page showed, or a file
    /// edited by hand meanwhile, leaves the proposal pending.
    pub async fn approve(
        &self,
        id: &ProposalId,
        ask: ApprovalAsk,
    ) -> Result<Applied, ProposalError> {
        self.step_up(id, &ask).await?;
        let _writes = self.ports.writes.lock().await;
        let mut proposal = self.get(&Proposer::NoToken, id).await?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ProposalError::NotPending(proposal.status));
        }
        let config = self.load().await?;
        // `get` found it applies; a hand edit since then is a change of
        // the file the page showed.
        let after =
            Proposal::check(&config, &proposal.changes).map_err(|_| ProposalError::Changed)?;
        if proposal.needs_forget_preview(&config) {
            return Err(ProposalError::NeedsForgetPreview);
        }
        let draft = self.draft(&after).await?;
        let rebuilt = Shown {
            revision: self.shown_revision(&draft.revision)?,
            diff: DiffHash::of(&masked_diff(&config, &draft.before, &draft.after)),
        };
        if rebuilt != ask.shown {
            return Err(ProposalError::Changed);
        }
        let now = to_the_millisecond(self.ports.clock.now());
        let approved = self
            .ports
            .store
            .approve(id, now, Some(ask.address))
            .await
            .map_err(ProposalError::Store)?;
        if !approved {
            return Err(ProposalError::NotPending(self.kept_status(id).await?));
        }
        proposal.decided_at = Some(now);
        self.announce(id, ProposalStatus::Applying).await;
        match self
            .ports
            .drafts
            .save_over(&draft.revision, &after, id)
            .await
        {
            Ok(SavedOver::Saved { kept }) => {
                self.ports.changed.tell();
                // The file holds the changes whatever the store records: the
                // answer is that it is applied, with why the store shows
                // otherwise until the daemon starts again.
                let unrecorded = self.ports.store.applied(id, kept.as_deref()).await.err();
                proposal.status = ProposalStatus::Applied;
                proposal.applied_as = kept;
                self.announce(id, proposal.status).await;
                Ok(Applied {
                    proposal,
                    unrecorded,
                })
            }
            Ok(SavedOver::ChangedMeanwhile) => {
                self.end_apply(id, ProposalStatus::Pending).await?;
                Err(ProposalError::Changed)
            }
            Err(error) => {
                self.end_apply(id, ProposalStatus::Failed).await?;
                Err(ProposalError::Config(Box::new(error)))
            }
        }
    }

    /// Ends as failed every proposal still being applied, as a daemon that
    /// stopped mid-apply left it, announcing each (decision 89, point 2):
    /// called once, as the daemon starts, before any apply. Answers the
    /// proposals it ended.
    pub async fn recover(&self) -> Result<Vec<ProposalId>, StoreError> {
        let mut ended = Vec::new();
        for kept in self.ports.store.all().await? {
            if kept.status == ProposalStatus::Applying
                && self
                    .ports
                    .store
                    .change_status(&kept.id, ProposalStatus::Applying, ProposalStatus::Failed)
                    .await?
            {
                self.announce(&kept.id, ProposalStatus::Failed).await;
                ended.push(kept.id);
            }
        }
        Ok(ended)
    }

    /// Rejects the pending proposal `id` for `reasons`, one per change.
    pub async fn reject(
        &self,
        id: &ProposalId,
        reasons: Vec<String>,
    ) -> Result<Proposal, ProposalError> {
        let mut proposal = self.get(&Proposer::NoToken, id).await?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ProposalError::NotPending(proposal.status));
        }
        let rejection =
            Rejection::new(proposal.changes.len(), reasons).map_err(ProposalError::Rejection)?;
        let now = to_the_millisecond(self.ports.clock.now());
        let rejected = self
            .ports
            .store
            .reject(id, now, rejection.reasons())
            .await
            .map_err(ProposalError::Store)?;
        if !rejected {
            return Err(ProposalError::NotPending(self.kept_status(id).await?));
        }
        proposal.status = ProposalStatus::Rejected;
        proposal.decided_at = Some(now);
        proposal.rejection = rejection;
        self.announce(id, proposal.status).await;
        Ok(proposal)
    }

    /// The step-up confirmation of `ask`: the admin password, in the
    /// address's sign-in waits, with sign-in on; the proposal's short ID
    /// otherwise.
    async fn step_up(&self, id: &ProposalId, ask: &ApprovalAsk) -> Result<(), ProposalError> {
        confirm_step_up(
            self.access.as_deref(),
            ask.address,
            &ask.confirmation,
            StepUpFor::Proposal(id),
        )
        .await
        .map_err(|refused| match refused {
            StepUpRefused::Failed => ProposalError::StepUpFailed,
            StepUpRefused::Wait { seconds } => ProposalError::StepUpWait { seconds },
            StepUpRefused::Unavailable(text) => ProposalError::StepUpUnavailable(text),
        })
    }

    /// Ends the apply of `id` with `status`, and announces it.
    async fn end_apply(
        &self,
        id: &ProposalId,
        status: ProposalStatus,
    ) -> Result<(), ProposalError> {
        self.ports
            .store
            .change_status(id, ProposalStatus::Applying, status)
            .await
            .map_err(ProposalError::Store)?;
        self.announce(id, status).await;
        Ok(())
    }

    async fn draft(&self, config: &AgentConfig) -> Result<ConfigDraft, ProposalError> {
        self.ports
            .drafts
            .draft(config)
            .await
            .map_err(|error| ProposalError::Config(Box::new(error)))
    }

    /// `revision` as a page is shown it: keyed with this daemon's key, so
    /// the file's plain revision never leaves it.
    fn shown_revision(&self, revision: &ConfigRevision) -> Result<ShownRevision, ProposalError> {
        self.ports
            .revision_key
            .as_ref()
            .map(|key| ShownRevision::of(key, revision))
            .map_err(|error| ProposalError::Random(error.clone()))
    }

    /// Tells every page that the proposal `id` now has `status`.
    async fn announce(&self, id: &ProposalId, status: ProposalStatus) {
        self.ports
            .events
            .publish(UiEvent::ProposalChanged {
                proposal: id.to_string(),
                status: status_dto(status),
            })
            .await;
    }

    async fn load(&self) -> Result<AgentConfig, ProposalError> {
        self.ports
            .config
            .load()
            .await
            .map_err(|error| ProposalError::Config(Box::new(error)))
    }

    /// The status the proposal `id` is kept with now.
    async fn kept_status(&self, id: &ProposalId) -> Result<ProposalStatus, ProposalError> {
        self.ports
            .store
            .get(id)
            .await
            .map_err(ProposalError::Store)?
            .map(|kept| kept.status)
            .ok_or(ProposalError::Unknown)
    }

    /// The proposal `stored` keeps, brought up to date: a pending one that
    /// has expired, or gone stale against the configuration, is kept so.
    /// `config` is the configuration, loaded once, at the first pending one.
    /// Changes that can't be read back, as when a rule has tightened since
    /// they were kept, no longer apply: the proposal is answered with none,
    /// and a pending one goes stale, or expires, rather than taking every
    /// read of the proposals down with it.
    async fn up_to_date(
        &self,
        stored: StoredProposal,
        config: &mut Option<AgentConfig>,
    ) -> Result<Proposal, ProposalError> {
        let changes = self.ports.codec.decode(&stored.changes).unwrap_or_default();
        let mut proposal = Proposal {
            id: stored.id,
            proposer: stored.proposer,
            base: stored.base,
            changes,
            rationale: stored.rationale,
            status: stored.status,
            created_at: stored.created_at,
            expires_at: stored.expires_at,
            decided_at: stored.decided_at,
            rejection: Rejection::kept(stored.reasons),
            applied_as: stored.applied_as,
        };
        if proposal.status != ProposalStatus::Pending {
            return Ok(proposal);
        }
        let config = match config {
            Some(config) => config,
            None => config.insert(self.load().await?),
        };
        let now = proposal.status_at(self.ports.clock.now(), config);
        if now == ProposalStatus::Pending {
            return Ok(proposal);
        }
        let changed = self
            .ports
            .store
            .change_status(&proposal.id, ProposalStatus::Pending, now)
            .await
            .map_err(ProposalError::Store)?;
        if changed {
            proposal.status = now;
            self.announce(&proposal.id, now).await;
        } else {
            // Another request moved it on first: it is as that one left it.
            proposal.status = self.kept_status(&proposal.id).await?;
        }
        Ok(proposal)
    }

    fn stored(&self, proposal: &Proposal) -> Result<StoredProposal, ProposalError> {
        Ok(StoredProposal {
            id: proposal.id.clone(),
            proposer: proposal.proposer.clone(),
            base: proposal.base.clone(),
            changes: self
                .ports
                .codec
                .encode(&proposal.changes)
                .map_err(ProposalError::Codec)?,
            rationale: proposal.rationale.clone(),
            status: proposal.status,
            created_at: proposal.created_at,
            expires_at: proposal.expires_at,
            decided_at: proposal.decided_at,
            reasons: proposal.rejection.reasons().to_vec(),
            approved_from: None,
            applied_as: proposal.applied_as.clone(),
        })
    }
}

/// The status a page is told a proposal has.
pub fn status_dto(status: ProposalStatus) -> ProposalStatusDto {
    match status {
        ProposalStatus::Pending => ProposalStatusDto::Pending,
        ProposalStatus::Applying => ProposalStatusDto::Applying,
        ProposalStatus::Applied => ProposalStatusDto::Applied,
        ProposalStatus::Failed => ProposalStatusDto::Failed,
        ProposalStatus::Rejected => ProposalStatusDto::Rejected,
        ProposalStatus::Withdrawn => ProposalStatusDto::Withdrawn,
        ProposalStatus::Expired => ProposalStatusDto::Expired,
        ProposalStatus::Stale => ProposalStatusDto::Stale,
    }
}

/// The diff of `before` to `after`, with every secret `config` holds as
/// text masked: each repository's and each of its further locations'. The
/// drafts mask each value of the file that holds one, however it is
/// written ([`ConfigDraft`]); this masks what else holds one, such as a
/// comment. A proposal changes plans only, so the secrets are the same on
/// both sides.
fn masked_diff(config: &AgentConfig, before: &str, after: &str) -> String {
    Redactor::redact_together(&config.redactors(), &unified_diff(before, after))
}

/// Whether `a` and `b` are one proposer: one token, or both no token.
fn same_proposer(a: &Proposer, b: &Proposer) -> bool {
    match (a, b) {
        (Proposer::Token { id: a, .. }, Proposer::Token { id: b, .. }) => a == b,
        (Proposer::NoToken, Proposer::NoToken) => true,
        _ => false,
    }
}

/// Whether `asker` may see a proposal `proposer` made: a token sees its
/// own, a client with no token sees every one.
fn may_see(asker: &Proposer, proposer: &Proposer) -> bool {
    match (asker, proposer) {
        (Proposer::NoToken, _) => true,
        (Proposer::Token { id: asker, .. }, Proposer::Token { id, .. }) => asker == id,
        (Proposer::Token { .. }, Proposer::NoToken) => false,
    }
}

/// `now` cut to the millisecond, as a [`ProposalStore`] keeps times.
fn to_the_millisecond(now: domain::Timestamp) -> domain::Timestamp {
    domain::Timestamp::from_millisecond(now.as_millisecond()).unwrap_or(now)
}

#[cfg(test)]
mod tests;
