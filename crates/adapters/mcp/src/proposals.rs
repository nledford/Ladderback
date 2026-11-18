//! The proposal tools (decision 89, §4.6): `propose_setup` submits
//! configuration changes for a person to approve in the web UI,
//! `get_proposal` reads one back and `withdraw_proposal` withdraws one still
//! pending. They are connect mode's alone: the daemon serves the "Pending
//! changes" page a person approves on, and no tool approves anything.

use std::sync::Arc;

use api::{ErrorDto, NewProposalDto, PlanDto, ProposalChangeDto, ProposalDto};
use async_trait::async_trait;
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

use crate::arguments::invalid;

/// What the proposal tools reach: the daemon's proposal routes, as the API
/// token the server sends, which sees and withdraws only the proposals it
/// made. Each failure is the API's own `{code, args}`. It has no approval,
/// which needs a person's browser session (decision 89, point 7).
#[async_trait]
pub trait Proposals: Send + Sync + 'static {
    /// Submits `proposal`, which the daemon checks and keeps as pending:
    /// the proposal kept, with what the submission checks warn of.
    async fn propose(&self, proposal: NewProposalDto) -> Result<ProposalDto, ErrorDto>;

    /// The proposal whose ID is `id`.
    async fn proposal(&self, id: String) -> Result<ProposalDto, ErrorDto>;

    /// Withdraws the pending proposal whose ID is `id`, and returns it.
    async fn withdraw(&self, id: String) -> Result<ProposalDto, ErrorDto>;
}

/// Reads a plan's table given as TOML text, in the configuration file's
/// keys, as a `[[plans]]` entry writes it, with the configuration's own
/// parser: `propose_setup` takes a plan as one string, as `preview_import`
/// takes a profile (§4.6), so each field of its schema has one type. It
/// reads the table's form only; the daemon checks the plan.
pub trait PlanTables: Send + Sync + 'static {
    /// The table `text` holds, or none when `text` isn't a TOML table.
    fn read(&self, text: &str) -> Option<PlanDto>;
}

/// What a server offering the proposal tools reaches: the daemon's routes,
/// and the reader of a proposed plan's table.
pub(crate) struct Proposing {
    pub(crate) proposals: Arc<dyn Proposals>,
    pub(crate) tables: Arc<dyn PlanTables>,
}

/// The arguments of `propose_setup`: the body `POST /api/v1/proposals`
/// takes, each change given in the form of [`ProposedChange`].
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ProposeArguments {
    /// The changes, applied in this order when a person approves them. Each
    /// is an object with its `kind` and either `plan` or `plan_id`, never
    /// both: `add-plan` adds the plan after the last one and `change-plan`
    /// puts it in place of the plan with its ID, each giving in `plan` the
    /// plan's table as TOML text: the keys of a `[[plans]]` entry in the
    /// configuration file, without the header (`id = "home"`, `repository`,
    /// `sources` and so on); `remove-plan` gives in `plan_id` the ID of the
    /// plan to remove. A plan names a repository by its alias, never a
    /// secret. A proposal can't add a hook, and a `change-plan` must keep the
    /// plan's hooks as they are. `get_proposal` gives the changes back as the
    /// daemon keeps them: a plan as an object of its keys, and a removal's
    /// ID in `plan`.
    #[schemars(schema_with = "changes")]
    pub changes: Vec<ProposedChange>,
    /// Why, in the AI agent's own words: shown apart from the summary
    /// Ladderback writes of the changes.
    #[serde(default)]
    pub rationale: String,
}

impl ProposeArguments {
    /// The body `POST /api/v1/proposals` takes, each plan's table read from
    /// its text by `tables`.
    ///
    /// # Errors
    ///
    /// `invalid-arguments` naming `changes`: a plan's text isn't a TOML
    /// table.
    pub fn proposal(self, tables: &dyn PlanTables) -> Result<NewProposalDto, ErrorDto> {
        let table = |text: &str| tables.read(text).ok_or_else(|| invalid("changes"));
        let changes = self
            .changes
            .into_iter()
            .map(|change| match change {
                ProposedChange::Add(text) => {
                    table(&text).map(|plan| ProposalChangeDto::AddPlan { plan })
                }
                ProposedChange::Change(text) => {
                    table(&text).map(|plan| ProposalChangeDto::ChangePlan { plan })
                }
                ProposedChange::Remove(plan) => Ok(ProposalChangeDto::RemovePlan { plan }),
            })
            .collect::<Result<_, _>>()?;
        Ok(NewProposalDto {
            changes,
            rationale: self.rationale,
        })
    }
}

/// One change of `propose_setup`, read from the form a call gives it in,
/// where each field has one type (§4.6): its `kind`, and a new or changed
/// plan's table as TOML text in `plan` or a removed plan's ID in `plan_id`.
/// Any other mix, both or neither included, isn't a change, and the call is
/// `invalid-arguments`.
#[derive(Debug, Deserialize)]
#[serde(try_from = "GivenChange")]
pub enum ProposedChange {
    /// Adds the plan whose table the text holds.
    Add(String),
    /// Puts the plan whose table the text holds in place of the one with
    /// its ID.
    Change(String),
    /// Removes the plan with this ID.
    Remove(String),
}

/// A change as a call gives it, before it is read as one.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GivenChange {
    kind: ChangeKind,
    plan: Option<String>,
    plan_id: Option<String>,
}

/// A change's `kind`, as the API names it.
#[derive(Debug, Deserialize)]
enum ChangeKind {
    #[serde(rename = "add-plan")]
    Add,
    #[serde(rename = "change-plan")]
    Change,
    #[serde(rename = "remove-plan")]
    Remove,
}

impl TryFrom<GivenChange> for ProposedChange {
    type Error = &'static str;

    fn try_from(given: GivenChange) -> Result<Self, Self::Error> {
        match (given.kind, given.plan, given.plan_id) {
            (ChangeKind::Add, Some(table), None) => Ok(Self::Add(table)),
            (ChangeKind::Change, Some(table), None) => Ok(Self::Change(table)),
            (ChangeKind::Remove, None, Some(id)) => Ok(Self::Remove(id)),
            _ => Err("a plan's addition or change gives `plan`, a removal `plan_id`"),
        }
    }
}

/// The schema of `changes`, in the narrower form every client takes
/// (§4.6): each change one object of named fields, each of one type.
fn changes(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["add-plan", "change-plan", "remove-plan"],
                },
                "plan": {
                    "type": "string",
                    "description": "For `add-plan` and `change-plan`: the plan's table, as TOML text.",
                },
                "plan_id": {
                    "type": "string",
                    "description": "For `remove-plan`: the ID of the plan to remove.",
                },
            },
            "required": ["kind"],
            "additionalProperties": false,
        },
    })
}

/// The arguments of `get_proposal` and `withdraw_proposal`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ProposalArguments {
    /// The proposal's ID, as `propose_setup` returned it.
    pub proposal: String,
}
