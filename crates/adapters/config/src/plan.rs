//! One plan on its own, as the Plans page sends it and shows it (decision
//! 96): a plan's table in the file's keys. It is read by the loader's own
//! code, so a plan the page saves is a plan the file loads, refused at the
//! same key for the same problem, and written as the file writes it.

use app::ports::{ConfigError, Problem};
use domain::planning::Plan;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::de::{DeTable, Deserializer};

use crate::locate::{Path, scrub};
use crate::read::{self, Reader};
use crate::schema::strict;
use crate::tables::PlanTable;
use crate::write;

/// What an error about a plan read on its own names as its file: no file
/// holds it yet.
const PLAN: &str = "plan";

/// A refusal of the plan as a whole, at no key.
fn whole(problem: Problem, detail: String) -> ConfigError {
    ConfigError::Invalid {
        file: PLAN.to_owned(),
        key: None,
        at: None,
        problem,
        detail,
    }
}

/// The plan `table` holds in the file's keys, read as the loader reads one
/// of the file's `[[plans]]`: a refusal names its key from the plan's own
/// table (`schedule.cron`), and no place, for the text is not one a person
/// wrote. A table the format can't hold (a key with no value) is refused
/// as a whole. No refusal holds a value of the table.
pub fn read_plan(table: &impl Serialize) -> Result<Plan, ConfigError> {
    let text = toml::to_string(table)
        .map_err(|error| whole(Problem::WrongType, scrub(&error.to_string())))?;
    let doc =
        DeTable::parse(&text).map_err(|error| whole(Problem::Syntax, scrub(error.message())))?;
    let reader = Reader {
        file: PLAN,
        text: &text,
        doc: &doc,
        located: false,
    };
    let table = PlanTable::deserialize(Deserializer::from(doc.clone()))
        .map_err(|error| strict(&reader, &error))?;
    read::plan(&reader, &Path::default(), table)
}

/// The table `text` holds, a plan's keys written as TOML as a `[[plans]]`
/// entry writes them, parsed as the file is, as a map of the same keys: how
/// a plan given as text, as an MCP client proposes one (§4.6), becomes the
/// table [`read_plan`] reads. Only its form is read here, not the plan.
///
/// # Errors
///
/// `text` isn't a TOML table: [`Problem::Syntax`], at no key, its detail
/// holding none of the text.
pub fn plan_table(text: &str) -> Result<serde_json::Map<String, serde_json::Value>, ConfigError> {
    todo!("{text}")
}

/// `plan` as the file writes it, in the file's keys, each default left out,
/// as a `T`: the table the Plans page shows.
pub fn plan_as<T: DeserializeOwned>(plan: &Plan) -> Result<T, ConfigError> {
    let unwritable = |detail: String| whole(Problem::Unwritable, scrub(&detail));
    toml::Value::try_from(write::plan(plan))
        .map_err(|error| unwritable(error.to_string()))?
        .try_into()
        .map_err(|error: toml::de::Error| unwritable(error.to_string()))
}
