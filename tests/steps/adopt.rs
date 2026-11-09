//! Steps for the `@e2e` scenario of `features/maintenance/adopt.feature`:
//! the adoption runs against the real worker and a real scratch repository,
//! and the repository is inspected with `rustic_core` itself. Red commit:
//! every new step is a `todo!()`.

use cucumber::{then, when};

use super::Agent;

/// What the e2e adopt scenario set up, and what the run did.
#[derive(Default)]
pub struct AdoptE2e;

#[when(expr = "the owner adopts the unmanaged history that matches the plan {string}")]
async fn adopt(_agent: &mut Agent, plan: String) {
    let _ = plan;
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("run the adoption through the maintenance use case with the real engine")
    }
    body();
}

#[then(expr = "every snapshot of the folder carries this agent's and the plan's tags")]
async fn tagged(_agent: &mut Agent) {
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("read the repository with rustic_core and check both tags on every snapshot")
    }
    body();
}

#[then("no snapshot made without Ladderback is left")]
async fn originals_gone(_agent: &mut Agent) {
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("every ID the rewrite replaced is gone from the repository")
    }
    body();
}
