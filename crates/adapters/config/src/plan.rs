//! One plan on its own, as the Plans page sends it and shows it (decision
//! 96): a plan's table in the file's keys. It is read by the loader's own
//! code, so a plan the page saves is a plan the file loads, refused at the
//! same key for the same problem, and written as the file writes it.

use app::ports::ConfigError;
use domain::planning::Plan;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The plan `table` holds in the file's keys, read as the loader reads one
/// of the file's `[[plans]]`.
pub fn read_plan(table: &impl Serialize) -> Result<Plan, ConfigError> {
    let _ = table;
    todo!()
}

/// `plan` as the file writes it, in the file's keys, each default left out,
/// as a `T`: the table the Plans page shows.
pub fn plan_as<T: DeserializeOwned>(plan: &Plan) -> Result<T, ConfigError> {
    let _ = plan;
    todo!()
}
