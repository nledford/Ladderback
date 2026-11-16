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
use domain::access::Password;
use domain::planning::{
    AgentConfig, ConfigChange, ConfigRevision, DiffHash, MAX_ENDED, MAX_PENDING, Proposal,
    ProposalId, ProposalRefused, ProposalStatus, ProposalWarning, Proposer, Rationale, Rejection,
    RejectionRefused, Shown, SummaryLine, TooSoon, confirms_proposal,
};
use domain::secrets::Redactor;

use crate::access::{Access, PasswordRefused};
use crate::ports::{
    Admission, ChangeCodec, Clock, CodecError, ConfigDrafts, ConfigError, ConfigStore, EventBus,
    ProposalStore, Random, RandomUnavailable, SavedOver, StoreError, StoredProposal,
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
    /// The platform gave no random bytes for the proposal's ID.
    Random(RandomUnavailable),
}

/// A proposal kept, and what the submission checks warn of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub proposal: Proposal,
    pub warnings: Vec<ProposalWarning>,
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
    /// What an approval of it is bound to: the file and the unmasked diff.
    pub shown: Shown,
    /// Whether the file has changed since the proposal was drafted.
    pub changed_since_drafted: bool,
    /// Whether a change of it could delete snapshots, so that only the
    /// forget preview lets a person approve it; until then, it is rejected.
    pub needs_forget_preview: bool,
    /// How many of its proposer's kept proposals a person rejected.
    pub rejected: usize,
}

/// A person's approval of a proposal, as the page sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalAsk {
    /// What the page showed.
    pub shown: Shown,
    /// The step-up confirmation: the admin password with sign-in on, the
    /// proposal's ID with it off.
    pub confirmation: String,
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
        let _ = Proposal::warnings(&config, &proposal.changes);
        todo!()
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
        todo!()
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
        Ok(proposal)
    }

    /// The proposal `id` as the "Pending changes" page shows it, brought up
    /// to date: a page opening is a read like any other.
    pub async fn review(&self, id: &ProposalId) -> Result<Review, ProposalError> {
        let _ = id;
        todo!()
    }

    /// Approves the pending proposal `id` as `ask` says the page showed it,
    /// once step-up confirms it, and applies it.
    pub async fn approve(
        &self,
        id: &ProposalId,
        ask: ApprovalAsk,
    ) -> Result<Proposal, ProposalError> {
        let _ = (id, ask);
        todo!()
    }

    /// Rejects the pending proposal `id` for `reasons`, one per change.
    pub async fn reject(
        &self,
        id: &ProposalId,
        reasons: Vec<String>,
    ) -> Result<Proposal, ProposalError> {
        let _ = (id, reasons, Rejection::default());
        todo!()
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
        proposal.status = if changed {
            now
        } else {
            // Another request moved it on first: it is as that one left it.
            self.kept_status(&proposal.id).await?
        };
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
fn status_dto(status: ProposalStatus) -> ProposalStatusDto {
    let _ = status;
    todo!()
}

/// The diff of `before` to `after`, with every secret `config` holds as
/// text masked.
fn masked_diff(config: &AgentConfig, before: &str, after: &str) -> String {
    let _ = (config, before, after);
    todo!()
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
