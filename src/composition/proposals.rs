//! The proposal routes' side of the wiring (§16, decision 89): each change
//! of a proposal read by the config adapter as the file is read when it
//! loads, then proposed, listed, read or withdrawn by the use case of
//! `app`, and each refusal said as the routes answer it: at the change and
//! its setting, never with a value.

use api::{ChangeProblem, ErrorCode, PlanProblem, ProposalChangeDto, ProposalStatusDto};
use app::ports::{ConfigError, Problem};
use app::proposals::{ProposalError, Proposals};
use async_trait::async_trait;
use config_adapter::FileConfigStore;
use domain::PlanId;
use domain::planning::{
    ChangeRefused, ConfigChange, MAX_CHANGES, MAX_PENDING, MAX_RATIONALE_CHARS, Proposal,
    ProposalId, ProposalRefused, ProposalStatus, Proposer, Rationale,
};
use http_adapter::{PlansError, ProposalsError};

use super::config_unusable;
use super::plans::{problem_of, violated};

pub(super) struct ProposalService {
    /// The configuration file, for the revision a proposal is drafted
    /// against.
    file: FileConfigStore,
    proposals: Proposals,
}

impl ProposalService {
    pub(super) fn new(file: FileConfigStore, proposals: Proposals) -> Self {
        Self { file, proposals }
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
        shown(made)
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
        status: status_dto(proposal.status),
        created_at: proposal.created_at.to_string(),
        expires_at: proposal.expires_at.to_string(),
        changes,
        rationale: proposal.rationale.as_str().to_owned(),
    })
}

fn status_dto(status: ProposalStatus) -> ProposalStatusDto {
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
    use std::sync::Arc;

    use api::{ChangeProblem, PlanProblem};
    use app::ports::ConfigStore;
    use http_adapter::Proposals as _;
    use platform::SystemClock;
    use platform::random::OsRandom;
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

    fn service() -> (ProposalService, tempfile::TempDir, std::path::PathBuf) {
        let scratch = tempfile::tempdir().expect("a scratch dir");
        let file = scratch.path().join("config.toml");
        std::fs::write(&file, CONFIG).expect("the config file");
        let config: Arc<dyn ConfigStore> =
            Arc::new(FileConfigStore::new(file.clone(), Arc::new(SystemClock)));
        let proposals = Proposals::new(
            Arc::new(app::fakes::InMemoryProposalStore::new()),
            Arc::new(config_adapter::JsonChangeCodec),
            config,
            Arc::new(SystemClock),
            Arc::new(OsRandom),
        );
        let service = ProposalService::new(
            FileConfigStore::new(file.clone(), Arc::new(SystemClock)),
            proposals,
        );
        (service, scratch, file)
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
}
