//! A proposal's changes as the store keeps them (decision 89): a JSON array
//! of the changes as the wire writes them, each plan as its table in the
//! file's keys. They are read back as the file is read when it loads, so
//! text a store kept is read with the same care as a file.

use app::ports::{ChangeCodec, CodecError};
use domain::PlanId;
use domain::planning::ConfigChange;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::plan::{plan_as, read_plan};

/// Writes a proposal's changes as JSON and reads them back.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonChangeCodec;

/// One change as it is kept: its kind, and the plan's table or ID.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Kept {
    #[serde(rename = "add-plan")]
    Add { plan: Map<String, Value> },
    #[serde(rename = "change-plan")]
    Change { plan: Map<String, Value> },
    #[serde(rename = "remove-plan")]
    Remove { plan: String },
}

fn codec_error(error: impl std::fmt::Display) -> CodecError {
    CodecError(error.to_string())
}

impl ChangeCodec for JsonChangeCodec {
    fn encode(&self, changes: &[ConfigChange]) -> Result<String, CodecError> {
        let kept = changes
            .iter()
            .map(|change| {
                Ok(match change {
                    ConfigChange::AddPlan(plan) => Kept::Add {
                        plan: plan_as(plan).map_err(codec_error)?,
                    },
                    ConfigChange::ChangePlan(plan) => Kept::Change {
                        plan: plan_as(plan).map_err(codec_error)?,
                    },
                    ConfigChange::RemovePlan(id) => Kept::Remove {
                        plan: id.as_str().to_owned(),
                    },
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        serde_json::to_string(&kept).map_err(codec_error)
    }

    fn decode(&self, text: &str) -> Result<Vec<ConfigChange>, CodecError> {
        let kept: Vec<Kept> = serde_json::from_str(text).map_err(codec_error)?;
        kept.into_iter()
            .map(|change| match change {
                Kept::Add { plan } => read_plan(&plan)
                    .map(ConfigChange::AddPlan)
                    .map_err(codec_error),
                Kept::Change { plan } => read_plan(&plan)
                    .map(ConfigChange::ChangePlan)
                    .map_err(codec_error),
                Kept::Remove { plan } => PlanId::parse(&plan)
                    .map(ConfigChange::RemovePlan)
                    .map_err(|_| CodecError("a removed plan's ID is not an ID".to_owned())),
            })
            .collect()
    }
}
