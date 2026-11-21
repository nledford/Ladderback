//! The pairing guide, the resource `ladderback://pairing` (§4.6): the steps
//! a person takes to pair a machine with its hub, where each is taken, and
//! the refusals each most often meets, by code, in the catalog's words. MCP
//! explains pairing; a person pairs and approves.

use l10n::Localizer;
use serde_json::Value;

/// The URI of the pairing guide as an MCP resource.
pub const PAIRING_RESOURCE: &str = "ladderback://pairing";

/// What `resources/list` says the pairing guide is.
pub const PAIRING_RESOURCE_DESCRIPTION: &str = "How a person pairs a machine with its hub: \
    make a pairing token on the hub, paste it into `ladderback pair` on the machine, and \
    approve the machine on the hub. Each step says where it is taken and what its common \
    refusals mean, by code. The pairing_state tool says whether this machine is paired.";

/// The guide, said in `localizer`'s locale.
pub fn guide(localizer: &Localizer) -> Value {
    let _ = localizer;
    todo!("the guide")
}
