//! The proposal routes' side of the wiring (§16, decision 89): each change
//! of a proposal read by the config adapter as the file is read when it
//! loads, then proposed, listed, read, withdrawn or previewed by the use
//! case of `app`, and each refusal said as the routes answer it: at the
//! change and its setting, never with a value. A repository the forget
//! preview couldn't read is told as the maintenance routes tell it.

use std::sync::Arc;

use api::{ChangeProblem, ErrorCode, PlanProblem, ProposalChangeDto};
use app::maintenance::RunMaintenance;
use app::ports::{
    ConfigError, ConfigStore, EventBus, Problem, ProposalStore, Random as _, RandomUnavailable,
};
use app::proposals::{ApprovalAsk, ProposalError, ProposalPorts, Proposals, RepositoryForget};
use app::reads::ReadTurns;
use app::repositories::ConfigWrites;
use app::schedule::ConfigChanged;
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::planning::{
    ChangeRefused, ConfigChange, DiffHash, ForgetDigest, MAX_CHANGES, MAX_PENDING,
    MAX_RATIONALE_CHARS, PlanSetting, Proposal, ProposalId, ProposalRefused, ProposalWarning,
    Proposer, Rationale, RejectionRefused, RevisionKey, Shown, ShownRevision, SummaryLine, TooSoon,
    short_proposal_id,
};
use domain::{OperationId, PlanId};
use http_adapter::{Approval, PlansError, ProposalsError, SignIn};
use platform::SystemClock;
use platform::random::OsRandom;
use uuid::Uuid;

use super::plans::{problem_of, violated};
use app::ports::config_unusable;

/// What the proposals are kept and applied over, kept apart from the
/// service so the daemon builds the service again once sign-in is
/// decided, as it does the restores'.
pub(super) struct ProposalParts {
    /// The configuration file: its revision, and the drafts and saves of
    /// an apply.
    pub(super) file: Arc<FileConfigStore>,
    pub(super) store: Arc<dyn ProposalStore>,
    pub(super) config: Arc<dyn ConfigStore>,
    /// The lock every write of the configuration holds.
    pub(super) writes: Arc<ConfigWrites>,
    /// Wakes the scheduler after an apply.
    pub(super) changed: ConfigChanged,
    pub(super) events: Arc<dyn EventBus>,
    /// The key this daemon start shows the file's revisions under
    /// ([`revision_key`]): held here, so every service built over these
    /// parts shows the same file the same.
    pub(super) revision_key: Result<RevisionKey, RandomUnavailable>,
    /// The turns to list a repository's snapshots, which every listing of
    /// the daemon shares (#710), the forget previews' included.
    pub(super) listings: Arc<ReadTurns>,
}

/// A key to show the configuration's revisions under, drawn from the
/// platform's random source, once each daemon start; never kept.
pub(super) fn revision_key() -> Result<RevisionKey, RandomUnavailable> {
    OsRandom.token().map(RevisionKey::new)
}

impl ProposalParts {
    /// The proposals over these parts, with no step-up: enough to bring
    /// them up to date when a plan is saved (decision 89, point 3).
    pub(super) fn proposals(&self) -> Proposals {
        Proposals::new(ProposalPorts {
            store: Arc::clone(&self.store),
            codec: Arc::new(config_adapter::JsonChangeCodec),
            config: Arc::clone(&self.config),
            drafts: Arc::<FileConfigStore>::clone(&self.file),
            writes: Arc::clone(&self.writes),
            changed: self.changed.another(),
            events: Arc::clone(&self.events),
            clock: Arc::new(SystemClock),
            random: Arc::new(OsRandom),
            revision_key: self.revision_key.clone(),
            listings: Arc::clone(&self.listings),
        })
    }
}

pub(super) struct ProposalService {
    /// The configuration file, for the revision a proposal is drafted
    /// against.
    file: Arc<FileConfigStore>,
    /// Shared with each approval's own task, which a browser that hangs up
    /// doesn't stop.
    proposals: Arc<Proposals>,
    /// What reads the repositories a proposal's forget preview covers, as
    /// the preview and as an approval reads it again.
    maintenance: Arc<RunMaintenance>,
    /// The configuration, whose secrets a refused read masks (I4).
    config: Arc<dyn ConfigStore>,
}

impl ProposalService {
    /// The proposals over `parts`, an approval confirmed as `sign_in`
    /// says: by the admin password with sign-in on, by the proposal's
    /// short ID with it off (decision 89, point 6). A forget preview reads
    /// the repositories through `maintenance`.
    pub(super) fn new(
        parts: &ProposalParts,
        sign_in: &SignIn,
        maintenance: RunMaintenance,
    ) -> Self {
        let proposals = parts.proposals();
        let proposals = match sign_in {
            SignIn::On(access) => proposals.with_access(Arc::clone(access)),
            SignIn::Off { .. } => proposals,
        };
        Self {
            file: Arc::clone(&parts.file),
            proposals: Arc::new(proposals),
            maintenance: Arc::new(maintenance),
            config: Arc::clone(&parts.config),
        }
    }
}

#[async_trait]
impl http_adapter::Proposals for ProposalService {
    async fn propose(
        &self,
        asker: Proposer,
        proposal: api::NewProposalDto,
    ) -> Result<api::ProposalDto, ProposalsError> {
        // The revision first: the use case checks the changes against the
        // configuration it loads after, so the base is never a file newer
        // than the one the changes were checked against.
        let base = self.file.revision().await.map_err(unusable)?;
        // The count before the changes are read, so a proposal of too many
        // is refused as one, whatever a change beyond the limit holds.
        if proposal.changes.len() > MAX_CHANGES {
            return Err(refused_at(
                ProposalRefused::TooManyChanges,
                &proposal.changes,
            ));
        }
        let changes = proposal
            .changes
            .iter()
            .enumerate()
            .map(|(index, change)| read(index, change))
            .collect::<Result<Vec<_>, _>>()?;
        let rationale = Rationale::parse(proposal.rationale)
            .map_err(|refused| refused_at(refused, &proposal.changes))?;
        let made = self
            .proposals
            .propose(asker, base, changes, rationale)
            .await
            .map_err(|error| match error {
                ProposalError::Refused(refused) => refused_at(refused, &proposal.changes),
                error => refused(error, ""),
            })?;
        let mut answered = shown(made.proposal)?;
        answered.warnings = made.warnings.into_iter().map(warning).collect();
        Ok(answered)
    }

    async fn list(&self, asker: Proposer) -> Result<api::ProposalsDto, ProposalsError> {
        let listed = self
            .proposals
            .list(&asker)
            .await
            .map_err(|error| refused(error, ""))?;
        Ok(api::ProposalsDto {
            proposals: listed.into_iter().map(shown).collect::<Result<_, _>>()?,
        })
    }

    async fn proposal(
        &self,
        asker: Proposer,
        id: String,
    ) -> Result<api::ProposalDto, ProposalsError> {
        let named = named(&id)?;
        let found = self
            .proposals
            .get(&asker, &named)
            .await
            .map_err(|error| refused(error, &id))?;
        shown(found)
    }

    async fn withdraw(
        &self,
        asker: Proposer,
        id: String,
    ) -> Result<api::ProposalDto, ProposalsError> {
        let named = named(&id)?;
        let withdrawn = self
            .proposals
            .withdraw(&asker, &named)
            .await
            .map_err(|error| refused(error, &id))?;
        shown(withdrawn)
    }

    async fn review(&self, id: String) -> Result<api::ProposalReviewDto, ProposalsError> {
        let named = named(&id)?;
        let review = self
            .proposals
            .review(&named)
            .await
            .map_err(|error| refused(error, &id))?;
        Ok(api::ProposalReviewDto {
            short_id: short_proposal_id(&review.proposal.id).to_owned(),
            summary: review.summary.into_iter().map(summary_line).collect(),
            diff: review.diff,
            revision: review.shown.revision.as_str().to_owned(),
            diff_hash: review.shown.diff.as_str().to_owned(),
            changed_since_drafted: review.changed_since_drafted,
            needs_forget_preview: review.needs_forget_preview,
            rejected: u64::try_from(review.rejected).unwrap_or(u64::MAX),
            proposal: shown(review.proposal)?,
        })
    }

    async fn forget_preview(
        &self,
        id: String,
    ) -> Result<api::ProposalForgetPreviewDto, ProposalsError> {
        let named = named(&id)?;
        let operation = OperationId::from_uuid(Uuid::now_v7());
        let previewed = self
            .proposals
            .forget_preview(&named, operation, &self.maintenance)
            .await
            .map_err(|error| refused(error, &id))?;
        let mut repositories = Vec::with_capacity(previewed.repositories.len());
        for RepositoryForget { repository, forget } in previewed.repositories {
            repositories.push(match forget {
                Ok(comparison) => super::maintenance::compared_dto(&repository, &comparison),
                // Told as the maintenance routes tell it, each secret of
                // the repository's settings masked (I4).
                Err(error) => api::RepositoryForgetDto::Refused {
                    repository: repository.as_str().to_owned(),
                    error: super::maintenance::refusal(self.config.as_ref(), &repository, error)
                        .await
                        .error(repository.as_str()),
                },
            });
        }
        Ok(api::ProposalForgetPreviewDto {
            repositories,
            digest: previewed.digest.map(|digest| digest.as_str().to_owned()),
        })
    }

    async fn approve(
        &self,
        id: String,
        approval: Approval,
    ) -> Result<api::ProposalDto, ProposalsError> {
        let named = named(&id)?;
        // A revision or a hash that is none no rebuild gives: the page
        // showed something else.
        let (Some(revision), Some(diff)) = (
            ShownRevision::parse(&approval.revision),
            DiffHash::parse(&approval.diff_hash),
        ) else {
            return Err(ProposalsError::Changed);
        };
        // Nor does a forget preview read again give a digest that is none.
        let forget = match approval.forget_digest.as_deref().map(ForgetDigest::parse) {
            None => None,
            Some(Some(digest)) => Some(digest),
            Some(None) => return Err(ProposalsError::ForgetPreviewChanged),
        };
        let ask = ApprovalAsk {
            shown: Shown { revision, diff },
            forget,
            confirmation: approval.confirmation,
            address: approval.address,
        };
        let operation = OperationId::from_uuid(Uuid::now_v7());
        // In a task of its own: a browser that hangs up drops this future,
        // never an apply halfway (decision 89).
        let proposals = Arc::clone(&self.proposals);
        let maintenance = Arc::clone(&self.maintenance);
        let approved = tokio::spawn(async move {
            proposals
                .approve(&named, ask, operation, &maintenance)
                .await
        })
        .await
        .map_err(|ended| {
            let how = if ended.is_panic() {
                "panicked"
            } else {
                "was cancelled"
            };
            log::warn!("the approval of proposal {id} {how}");
            ProposalsError::Unavailable {
                code: ErrorCode::ProposalsUnavailable,
                args: Vec::new(),
                detail: None,
            }
        })?
        .map_err(|error| refused(error, &id))?;
        if let Some(error) = &approved.unrecorded {
            // Its code only: the store's own words are unchecked (I4).
            log::warn!(
                "proposal {id} is applied, but the store couldn't record it: {}; it shows as \
                 failed once the daemon starts again",
                error.code()
            );
        }
        shown(approved.proposal)
    }

    async fn reject(
        &self,
        id: String,
        reasons: Vec<String>,
    ) -> Result<api::ProposalDto, ProposalsError> {
        let named = named(&id)?;
        let rejected = self
            .proposals
            .reject(&named, reasons)
            .await
            .map_err(|error| refused(error, &id))?;
        shown(rejected)
    }
}

/// A submission check's warning as the routes answer it, its change
/// counted from 1.
fn warning(warning: ProposalWarning) -> api::ProposalWarningDto {
    let change = |index: usize| u64::try_from(index.saturating_add(1)).unwrap_or(u64::MAX);
    match warning {
        ProposalWarning::SameSources { index, plan, other } => {
            api::ProposalWarningDto::SameSources {
                change: change(index),
                plan: plan.as_str().to_owned(),
                other: other.as_str().to_owned(),
            }
        }
        ProposalWarning::GlobNeverMatches { index, glob } => {
            api::ProposalWarningDto::GlobNeverMatches {
                change: change(index),
                glob,
            }
        }
    }
}

/// A line of the summary as the page reads it.
fn summary_line(line: SummaryLine) -> api::SummaryLineDto {
    match line {
        SummaryLine::AddsPlan(plan) => api::SummaryLineDto::AddsPlan {
            plan: plan.as_str().to_owned(),
        },
        SummaryLine::ChangesPlan { plan, settings } => api::SummaryLineDto::ChangesPlan {
            plan: plan.as_str().to_owned(),
            settings: settings.into_iter().map(setting).collect(),
        },
        SummaryLine::RemovesPlan(plan) => api::SummaryLineDto::RemovesPlan {
            plan: plan.as_str().to_owned(),
        },
    }
}

fn setting(setting: PlanSetting) -> api::PlanSettingDto {
    match setting {
        PlanSetting::Repository => api::PlanSettingDto::Repository,
        PlanSetting::Host => api::PlanSettingDto::Host,
        PlanSetting::Label => api::PlanSettingDto::Label,
        PlanSetting::Sources => api::PlanSettingDto::Sources,
        PlanSetting::Tags => api::PlanSettingDto::Tags,
        PlanSetting::Schedule => api::PlanSettingDto::Schedule,
        PlanSetting::Overlap => api::PlanSettingDto::Overlap,
        PlanSetting::Retry => api::PlanSettingDto::Retry,
        PlanSetting::Filter => api::PlanSettingDto::Filter,
        PlanSetting::Retention => api::PlanSettingDto::Retention,
    }
}

/// The proposal `id` names: text that is no proposal's ID names none kept.
fn named(id: &str) -> Result<ProposalId, ProposalsError> {
    ProposalId::parse(id).ok_or_else(|| ProposalsError::Unknown {
        proposal: id.to_owned(),
    })
}

/// The change at `index`, from 0, its plan read as the loader reads one of
/// the file's. A removal naming no plan's ID names none the configuration
/// has.
fn read(index: usize, change: &ProposalChangeDto) -> Result<ConfigChange, ProposalsError> {
    let plan = |table: &api::PlanDto| {
        config_adapter::read_plan(&table.0).map_err(|error| match error {
            ConfigError::Invalid {
                key: Some(field),
                problem,
                ..
            } => match problem {
                Problem::Violation(violation) => at_change(index, change, violated(violation)),
                problem => invalid(index, field, ChangeProblem::Plan(problem_of(&problem))),
            },
            // A table the file's format can't hold, as one with a null in it.
            ConfigError::Invalid { key: None, .. } => ProposalsError::InvalidBody,
            error => unusable(error),
        })
    };
    match change {
        ProposalChangeDto::AddPlan { plan: table } => plan(table).map(ConfigChange::AddPlan),
        ProposalChangeDto::ChangePlan { plan: table } => plan(table).map(ConfigChange::ChangePlan),
        ProposalChangeDto::RemovePlan { plan } => PlanId::parse(plan)
            .map(ConfigChange::RemovePlan)
            .map_err(|_| invalid(index, "plan".to_owned(), ChangeProblem::UnknownPlan)),
    }
}

/// The change at `index`, from 0, refused at `field`: the refusal counts
/// changes from 1.
fn invalid(index: usize, field: String, problem: ChangeProblem) -> ProposalsError {
    ProposalsError::Invalid {
        change: index.saturating_add(1),
        field,
        problem,
    }
}

/// The setting of `change` that names its plan: a removal's `plan`, a
/// plan's `id`.
fn plan_field(change: Option<&ProposalChangeDto>) -> String {
    match change {
        Some(ProposalChangeDto::RemovePlan { .. }) => "plan",
        _ => "id",
    }
    .to_owned()
}

/// A plan route's refusal of a plan, as the refusal of the change at
/// `index` that holds it.
fn at_change(index: usize, change: &ProposalChangeDto, refused: PlansError) -> ProposalsError {
    match refused {
        PlansError::Invalid { field, problem } => {
            invalid(index, field, ChangeProblem::Plan(problem))
        }
        PlansError::Unknown { .. } => {
            invalid(index, plan_field(Some(change)), ChangeProblem::UnknownPlan)
        }
        PlansError::InUse { .. } => invalid(index, plan_field(Some(change)), ChangeProblem::InUse),
        PlansError::InvalidBody => ProposalsError::InvalidBody,
        PlansError::Unavailable { code, args, detail } => {
            ProposalsError::Unavailable { code, args, detail }
        }
    }
}

/// A submission check's refusal of a proposal of `changes`.
fn refused_at(refused: ProposalRefused, changes: &[ProposalChangeDto]) -> ProposalsError {
    match refused {
        ProposalRefused::NoChanges => ProposalsError::Empty,
        ProposalRefused::TooManyChanges => ProposalsError::TooLarge {
            what: "changes",
            limit: MAX_CHANGES,
        },
        ProposalRefused::RationaleTooLong => ProposalsError::TooLarge {
            what: "rationale",
            limit: MAX_RATIONALE_CHARS,
        },
        ProposalRefused::Change {
            index,
            refused: ChangeRefused::UnknownPlan(_),
        } => invalid(
            index,
            plan_field(changes.get(index)),
            ChangeProblem::UnknownPlan,
        ),
        ProposalRefused::Change {
            index,
            refused: ChangeRefused::Violation(violation),
        } => match changes.get(index) {
            Some(change) => at_change(index, change, violated(violation)),
            // The use case checks only the changes it was given.
            None => invalid(
                index,
                "id".to_owned(),
                ChangeProblem::Plan(PlanProblem::Invalid),
            ),
        },
        ProposalRefused::Hooks { index } => {
            invalid(index, "hooks".to_owned(), ChangeProblem::Hooks)
        }
    }
}

/// `proposal` as the routes answer it: each plan as its table in the file's
/// keys.
fn shown(proposal: Proposal) -> Result<api::ProposalDto, ProposalsError> {
    let changes = proposal
        .changes
        .iter()
        .map(|change| {
            Ok(match change {
                ConfigChange::AddPlan(plan) => ProposalChangeDto::AddPlan {
                    plan: config_adapter::plan_as(plan).map_err(unusable)?,
                },
                ConfigChange::ChangePlan(plan) => ProposalChangeDto::ChangePlan {
                    plan: config_adapter::plan_as(plan).map_err(unusable)?,
                },
                ConfigChange::RemovePlan(id) => ProposalChangeDto::RemovePlan {
                    plan: id.as_str().to_owned(),
                },
            })
        })
        .collect::<Result<_, ProposalsError>>()?;
    Ok(api::ProposalDto {
        id: proposal.id.as_str().to_owned(),
        proposed_by: match proposal.proposer {
            Proposer::Token { label, .. } => Some(label.as_str().to_owned()),
            Proposer::NoToken => None,
        },
        status: app::proposals::status_dto(proposal.status),
        created_at: proposal.created_at.to_string(),
        expires_at: proposal.expires_at.to_string(),
        changes,
        rationale: proposal.rationale.as_str().to_owned(),
        decided_at: proposal.decided_at.map(|at| at.to_string()),
        reasons: proposal
            .rejection
            .reasons()
            .iter()
            .map(|reason| reason.as_str().to_owned())
            .collect(),
        warnings: Vec::new(),
    })
}

/// `error` as the routes answer it, `id` the proposal asked for. A store,
/// codec or random failure is logged and answered without its detail,
/// which names files of the data folder.
fn refused(error: ProposalError, id: &str) -> ProposalsError {
    let unavailable = |what: &str, detail: String| {
        log::warn!("the proposals could not be read or kept: {what}: {detail}");
        ProposalsError::Unavailable {
            code: ErrorCode::ProposalsUnavailable,
            args: Vec::new(),
            detail: None,
        }
    };
    match error {
        ProposalError::Refused(refused) => refused_at(refused, &[]),
        ProposalError::Full => ProposalsError::Limit { limit: MAX_PENDING },
        ProposalError::Unknown => ProposalsError::Unknown {
            proposal: id.to_owned(),
        },
        ProposalError::NotPending(status) => ProposalsError::NotPending { status },
        ProposalError::Config(error) => unusable(*error),
        ProposalError::Store(error) => unavailable("the store", error.to_string()),
        ProposalError::Codec(error) => unavailable("a proposal's changes", error.0),
        ProposalError::Random(error) => unavailable("random bytes", error.0),
        ProposalError::TooSoon(TooSoon::Pending) => ProposalsError::Limit { limit: MAX_PENDING },
        ProposalError::TooSoon(TooSoon::Hourly { until }) => too_soon(false, until),
        ProposalError::TooSoon(TooSoon::AfterRejection { until }) => too_soon(true, until),
        ProposalError::Rejection(RejectionRefused::Count { changes }) => {
            ProposalsError::ReasonCount { changes }
        }
        ProposalError::Rejection(RejectionRefused::TooLong { index }) => {
            ProposalsError::ReasonTooLong {
                change: index.saturating_add(1),
            }
        }
        ProposalError::Changed => ProposalsError::Changed,
        ProposalError::NeedsForgetPreview => ProposalsError::NeedsForgetPreview,
        ProposalError::ForgetPreviewChanged => ProposalsError::ForgetPreviewChanged,
        ProposalError::StepUpFailed => ProposalsError::StepUpFailed,
        ProposalError::StepUpWait { seconds } => ProposalsError::StepUpWait { seconds },
        ProposalError::StepUpUnavailable(detail) => ProposalsError::StepUpUnavailable(detail),
    }
}

/// A proposer's wait until `until`, in whole seconds rounded up, at least
/// one: a wait that ends within this second still asks for one.
fn too_soon(after_rejection: bool, until: domain::Timestamp) -> ProposalsError {
    let left = until
        .as_millisecond()
        .saturating_sub(domain::Timestamp::now().as_millisecond());
    let seconds = u64::try_from(left.saturating_add(999) / 1000).unwrap_or(0);
    ProposalsError::TooSoon {
        after_rejection,
        seconds: seconds.max(1),
    }
}

/// A configuration that couldn't be read, as every route that reads the
/// file says it (#347).
fn unusable(error: ConfigError) -> ProposalsError {
    config_unusable(error, |code: ErrorCode, args, detail| {
        ProposalsError::Unavailable { code, args, detail }
    })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

    use api::{ChangeProblem, PlanProblem};
    use app::access::Access;
    use app::fakes::{
        FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, InMemoryProposalStore,
        InMemorySessionStore, SeededRandom,
    };
    use app::proposals::ApprovalAsk;
    use domain::access::step_up::StepUpConfirmation;
    use domain::planning::{ProposalStatus, short_proposal_id};
    use http_adapter::Proposals as _;
    use serde_json::json;

    use super::*;

    const CONFIG: &str = r#"schema = 1

[repositories.demo]
repository = "/srv/demo-repo"
password-env = "DEMO_PASSWORD"

[[plans]]
id = "home"
repository = "demo"
sources = ["/home/demo"]
"#;

    const ADMIN_PASSWORD: &str = "demo-admin-pass";

    fn service() -> (ProposalService, tempfile::TempDir, std::path::PathBuf) {
        service_with(&signed_out())
    }

    /// Sign-in off.
    fn signed_out() -> SignIn {
        SignIn::Off {
            password: Arc::new(InMemoryAdminPassword::default()),
            sessions: Arc::new(InMemorySessionStore::new()),
        }
    }

    /// Sign-in on, and the clock its waits run by.
    fn signed_in() -> (SignIn, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::new(
            domain::Timestamp::from_second(1_800_000_000).expect("a time"),
        ));
        let access = Access::new(
            Arc::new(InMemoryAdminPassword::with(ADMIN_PASSWORD)),
            Arc::new(SeededRandom::new(7)),
            clock.clone(),
            Arc::new(InMemoryApiTokenStore::new()),
            Arc::new(InMemorySessionStore::new()),
        );
        (SignIn::On(Arc::new(access)), clock)
    }

    fn service_with(sign_in: &SignIn) -> (ProposalService, tempfile::TempDir, std::path::PathBuf) {
        let (parts, scratch, file) = parts();
        (
            ProposalService::new(&parts, sign_in, maintenance(&parts)),
            scratch,
            file,
        )
    }

    /// Parts over a configuration file of `CONFIG` in a scratch folder.
    fn parts() -> (ProposalParts, tempfile::TempDir, std::path::PathBuf) {
        parts_over(CONFIG)
    }

    /// Parts over a configuration file of `config` in a scratch folder.
    fn parts_over(config: &str) -> (ProposalParts, tempfile::TempDir, std::path::PathBuf) {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let file = scratch.path().join("config.toml");
        std::fs::write(&file, config).expect("the config file");
        let file_store = Arc::new(FileConfigStore::new(file.clone(), Arc::new(SystemClock)));
        let parts = ProposalParts {
            config: file_store.clone(),
            file: file_store,
            store: Arc::new(InMemoryProposalStore::new()),
            writes: Arc::default(),
            changed: app::schedule::config_changes().0,
            events: Arc::new(app::event_bus::BroadcastEventBus::new()),
            revision_key: revision_key(),
            listings: Arc::default(),
        };
        (parts, scratch, file)
    }

    /// The maintenance use case over `parts`' configuration, each secret
    /// resolved as the daemon resolves it; its engine holds no repository.
    fn maintenance(parts: &ProposalParts) -> RunMaintenance {
        let clock: Arc<dyn app::ports::Clock> = Arc::new(SystemClock);
        RunMaintenance::new(
            Arc::new(app::fakes::SimulatedEngine::new(Arc::clone(&clock))),
            clock,
            Arc::new(app::fakes::InMemoryOperationStore::new()),
            app::secrets::ResolveRepository::new(Arc::new(secrets::SystemSecrets::default())),
            Arc::new(engine::RusticRetention),
            Arc::clone(&parts.config),
            Arc::default(),
            domain::AgentId::from_uuid(uuid::Uuid::from_u128(1)),
            "demo-host".to_owned(),
            Arc::new(engine::RusticSources),
        )
    }

    /// Approves the proposal `id` as its review shows it, confirmed by
    /// `confirmation`.
    async fn approve(
        service: &ProposalService,
        id: &ProposalId,
        confirmation: &str,
    ) -> Result<Proposal, ProposalError> {
        let review = service.proposals.review(id).await.expect("reviewed");
        service
            .proposals
            .approve(
                id,
                ApprovalAsk {
                    shown: review.shown,
                    forget: None,
                    confirmation: StepUpConfirmation::parse(confirmation.to_owned())
                        .expect("a confirmation"),
                    address: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
                },
                OperationId::from_uuid(Uuid::now_v7()),
                &service.maintenance,
            )
            .await
            .map(|applied| applied.proposal)
    }

    #[tokio::test]
    async fn with_sign_in_on_only_the_admin_password_confirms_an_approval() {
        let (sign_in, clock) = signed_in();
        let (service, _scratch, file) = service_with(&sign_in);
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "add-plan", "plan": photos()}])),
            )
            .await
            .expect("proposed");
        let id = ProposalId::parse(&made.id).expect("an ID");
        assert_eq!(
            approve(&service, &id, short_proposal_id(&id)).await,
            Err(ProposalError::StepUpFailed)
        );
        // A wrong confirmation counts in the sign-in waits: even the
        // password isn't checked until the wait is over.
        assert!(matches!(
            approve(&service, &id, ADMIN_PASSWORD).await,
            Err(ProposalError::StepUpWait { .. })
        ));
        assert_eq!(std::fs::read_to_string(&file).expect("the file"), CONFIG);
        clock.advance(domain::SignedDuration::from_mins(1));
        let approved = approve(&service, &id, ADMIN_PASSWORD)
            .await
            .expect("approved");
        assert_eq!(approved.status, ProposalStatus::Applied);
        assert_ne!(std::fs::read_to_string(&file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn with_sign_in_off_the_short_id_confirms_an_approval() {
        let (service, _scratch, file) = service();
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "add-plan", "plan": photos()}])),
            )
            .await
            .expect("proposed");
        let id = ProposalId::parse(&made.id).expect("an ID");
        assert_eq!(
            approve(&service, &id, ADMIN_PASSWORD).await,
            Err(ProposalError::StepUpFailed)
        );
        let approved = approve(&service, &id, short_proposal_id(&id))
            .await
            .expect("approved");
        assert_eq!(approved.status, ProposalStatus::Applied);
        assert_ne!(std::fs::read_to_string(&file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn a_browser_that_hangs_up_mid_approval_leaves_the_apply_to_finish() {
        let (parts, _scratch, file) = parts();
        let service = ProposalService::new(&parts, &signed_out(), maintenance(&parts));
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "add-plan", "plan": photos()}])),
            )
            .await
            .expect("proposed");
        let id = ProposalId::parse(&made.id).expect("an ID");
        let review = service.review(made.id.clone()).await.expect("reviewed");
        let approval = Approval {
            revision: review.revision,
            diff_hash: review.diff_hash,
            forget_digest: None,
            address: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            confirmation: StepUpConfirmation::parse(short_proposal_id(&id).to_owned())
                .expect("a confirmation"),
        };
        // Another write of the configuration holds its lock, so the
        // approval waits for it; the browser hangs up meanwhile.
        let writes = parts.writes.lock().await;
        let hung_up = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            service.approve(made.id.clone(), approval),
        )
        .await;
        assert!(hung_up.is_err(), "the approval waits for the lock");
        drop(writes);
        let mut status = ProposalStatus::Pending;
        for _ in 0..500 {
            status = parts
                .store
                .get(&id)
                .await
                .expect("the store reads")
                .expect("the proposal is kept")
                .status;
            if status != ProposalStatus::Pending && status != ProposalStatus::Applying {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(status, ProposalStatus::Applied);
        assert_ne!(std::fs::read_to_string(&file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn a_forget_digest_that_is_no_digest_is_refused_as_a_changed_forget_preview() {
        let (parts, _scratch, file) = parts();
        let service = ProposalService::new(&parts, &signed_out(), maintenance(&parts));
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "add-plan", "plan": photos()}])),
            )
            .await
            .expect("proposed");
        let id = ProposalId::parse(&made.id).expect("an ID");
        let review = service.review(made.id.clone()).await.expect("reviewed");
        let approval = Approval {
            revision: review.revision,
            diff_hash: review.diff_hash,
            forget_digest: Some("not a digest".to_owned()),
            address: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            confirmation: StepUpConfirmation::parse(short_proposal_id(&id).to_owned())
                .expect("a confirmation"),
        };
        assert_eq!(
            service.approve(made.id.clone(), approval).await,
            Err(ProposalsError::ForgetPreviewChanged)
        );
        let kept = parts.store.get(&id).await.expect("the store reads");
        assert_eq!(
            kept.expect("the proposal is kept").status,
            ProposalStatus::Pending
        );
        assert_eq!(std::fs::read_to_string(&file).expect("the file"), CONFIG);
    }

    /// A plan with no retention policy: adding it can't delete a snapshot,
    /// so its approval needs no forget preview.
    fn photos() -> serde_json::Value {
        json!({"id": "photos", "repository": "demo", "sources": ["/home/demo/Pictures"]})
    }

    fn proposal(changes: serde_json::Value) -> api::NewProposalDto {
        serde_json::from_value(json!({ "changes": changes, "rationale": "Nightly backups." }))
            .expect("a proposal")
    }

    fn invalid(change: usize, field: &str, problem: ChangeProblem) -> ProposalsError {
        ProposalsError::Invalid {
            change,
            field: field.to_owned(),
            problem,
        }
    }

    #[tokio::test]
    async fn a_proposal_is_answered_pending_with_its_changes_as_the_file_writes_them() {
        let (service, _scratch, file) = service();
        let plan =
            json!({"id": "photos", "repository": "demo", "sources": ["/home/demo/Pictures"]});
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([
                    {"kind": "add-plan", "plan": plan},
                    {"kind": "remove-plan", "plan": "home"},
                ])),
            )
            .await
            .expect("proposed");
        assert_eq!(made.status, api::ProposalStatusDto::Pending);
        assert_eq!(made.proposed_by, None);
        assert_eq!(made.rationale, "Nightly backups.");
        assert_eq!(
            serde_json::to_value(&made.changes).expect("JSON"),
            json!([
                {"kind": "add-plan", "plan": plan},
                {"kind": "remove-plan", "plan": "home"},
            ])
        );
        let read = service
            .proposal(Proposer::NoToken, made.id.clone())
            .await
            .expect("read back");
        assert_eq!(read, made);
        assert_eq!(std::fs::read_to_string(file).expect("the file"), CONFIG);
    }

    #[tokio::test]
    async fn a_change_is_refused_at_its_place_from_1_and_its_setting() {
        let (service, _scratch, _file) = service();
        let good = json!({"kind": "add-plan", "plan": {"id": "a", "repository": "demo", "sources": ["/a"]}});
        for (second, expected) in [
            (
                json!({"kind": "add-plan", "plan": {"id": "b", "repository": "demo", "sources": ["/b"], "schedule": {"cron": "61 * * * *"}}}),
                invalid(
                    2,
                    "schedule.cron",
                    ChangeProblem::Plan(PlanProblem::Invalid),
                ),
            ),
            (
                json!({"kind": "add-plan", "plan": {"id": "b", "repository": "nas", "sources": ["/b"]}}),
                invalid(
                    2,
                    "repository",
                    ChangeProblem::Plan(PlanProblem::UnknownRepository),
                ),
            ),
            (
                json!({"kind": "add-plan", "plan": {"id": "a", "repository": "demo", "sources": ["/b"]}}),
                invalid(2, "id", ChangeProblem::Plan(PlanProblem::Taken)),
            ),
            (
                json!({"kind": "change-plan", "plan": {"id": "b", "repository": "demo", "sources": ["/b"]}}),
                invalid(2, "id", ChangeProblem::UnknownPlan),
            ),
            (
                json!({"kind": "remove-plan", "plan": "b"}),
                invalid(2, "plan", ChangeProblem::UnknownPlan),
            ),
            (
                json!({"kind": "add-plan", "plan": {"id": "b", "repository": "demo", "sources": ["/b"], "colour": null}}),
                ProposalsError::InvalidBody,
            ),
        ] {
            let refused = service
                .propose(Proposer::NoToken, proposal(json!([good, second])))
                .await;
            assert_eq!(refused, Err(expected), "{second}");
        }
    }

    #[tokio::test]
    async fn a_plan_proposed_with_hooks_is_refused_at_its_hooks() {
        let (service, _scratch, _file) = service();
        let hooked = json!({"id": "photos", "repository": "demo", "sources": ["/p"],
            "hooks": {"run-before": ["demo-dump --all"]}});
        assert_eq!(
            service
                .propose(
                    Proposer::NoToken,
                    proposal(json!([
                        {"kind": "remove-plan", "plan": "home"},
                        {"kind": "add-plan", "plan": hooked},
                    ]))
                )
                .await,
            Err(invalid(2, "hooks", ChangeProblem::Hooks))
        );
    }

    #[tokio::test]
    async fn a_proposal_of_none_or_too_many_changes_or_words_is_refused() {
        let (service, _scratch, _file) = service();
        let many: Vec<_> = (0..=domain::planning::MAX_CHANGES)
            .map(|n| json!({"kind": "add-plan", "plan": {"id": format!("p{n}"), "repository": "demo", "sources": ["/p"]}}))
            .collect();
        assert_eq!(
            service
                .propose(Proposer::NoToken, proposal(json!([])))
                .await,
            Err(ProposalsError::Empty)
        );
        assert_eq!(
            service
                .propose(Proposer::NoToken, proposal(json!(many)))
                .await,
            Err(ProposalsError::TooLarge {
                what: "changes",
                limit: 50
            })
        );
        // The count comes first: a change beyond the limit that isn't one
        // makes it no other refusal.
        let mut wrong_beyond = many.clone();
        wrong_beyond[domain::planning::MAX_CHANGES] =
            json!({"kind": "remove-plan", "plan": "Not an ID!"});
        assert_eq!(
            service
                .propose(Proposer::NoToken, proposal(json!(wrong_beyond)))
                .await,
            Err(ProposalsError::TooLarge {
                what: "changes",
                limit: 50
            })
        );
        let mut wordy = proposal(json!([{"kind": "remove-plan", "plan": "home"}]));
        wordy.rationale = "a".repeat(domain::planning::MAX_RATIONALE_CHARS + 1);
        assert_eq!(
            service.propose(Proposer::NoToken, wordy).await,
            Err(ProposalsError::TooLarge {
                what: "rationale",
                limit: 4000
            })
        );
    }

    #[tokio::test]
    async fn an_id_that_is_none_is_unknown_and_a_withdrawn_one_is_not_pending() {
        let (service, _scratch, _file) = service();
        assert_eq!(
            service
                .proposal(Proposer::NoToken, "not an id".to_owned())
                .await,
            Err(ProposalsError::Unknown {
                proposal: "not an id".to_owned()
            })
        );
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "remove-plan", "plan": "home"}])),
            )
            .await
            .expect("proposed");
        let withdrawn = service
            .withdraw(Proposer::NoToken, made.id.clone())
            .await
            .expect("withdrawn");
        assert_eq!(withdrawn.status, api::ProposalStatusDto::Withdrawn);
        assert_eq!(
            service.withdraw(Proposer::NoToken, made.id).await,
            Err(ProposalsError::NotPending {
                status: domain::planning::ProposalStatus::Withdrawn
            })
        );
    }

    /// A repository maintained here whose password command fails, saying
    /// the repository's other secret as it does.
    const CANARY_CONFIG: &str = r#"schema = 1

[repositories.demo]
repository = "/srv/demo-repo"
password-command = "sh -c 'echo canary-inline >&2; exit 3'"
maintenance = "local"

[repositories.demo.options]
secret_access_key = "canary-inline"

[[plans]]
id = "home"
repository = "demo"
sources = ["/home/demo"]
"#;

    /// A repository the forget preview couldn't read is answered with why,
    /// as the maintenance routes tell it: what the password command said,
    /// each secret of the repository's settings masked (I4). The proposal
    /// stays pending.
    #[tokio::test]
    async fn a_forget_preview_says_why_a_repository_was_not_read_without_its_secrets() {
        let (parts, _scratch, _file) = parts_over(CANARY_CONFIG);
        let service = ProposalService::new(&parts, &signed_out(), maintenance(&parts));
        let made = service
            .propose(
                Proposer::NoToken,
                proposal(json!([{"kind": "remove-plan", "plan": "home"}])),
            )
            .await
            .expect("proposed");

        let previewed = service
            .forget_preview(made.id.clone())
            .await
            .expect("previewed");

        let [api::RepositoryForgetDto::Refused { repository, error }] =
            previewed.repositories.as_slice()
        else {
            panic!("one repository, refused: {previewed:?}");
        };
        assert_eq!(repository, "demo");
        assert_eq!(error.code, ErrorCode::SecretCommandFailed);
        let detail = error.detail.as_deref().expect("the command's words");
        assert!(detail.contains(domain::secrets::REDACTED), "{detail}");
        let wire = serde_json::to_string(&previewed).expect("it serializes");
        assert!(!wire.contains("canary-inline"), "{wire}");
        let kept = service
            .proposal(Proposer::NoToken, made.id)
            .await
            .expect("kept");
        assert_eq!(kept.status, api::ProposalStatusDto::Pending);
    }
}
