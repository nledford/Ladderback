//! Steps for `features/maintenance/adopt.feature`: they drive the
//! maintenance use case against a scripted engine, which answers an
//! identify, a listing and a rewrite, and records what it was asked to
//! rewrite. Red commit: every step is a `todo!()`, so every scenario fails
//! at run time.

use cucumber::{given, then, when};

use super::Agent;

/// What the adopt scenarios set up and what the runs answered.
#[derive(Default)]
pub struct AdoptWorld {
    /// What a refused run answered, once the use case has one.
    #[allow(dead_code, reason = "red test: the run fills it next")]
    pub refused: Option<String>,
}

impl AdoptWorld {
    /// Placeholder for the red commit; the implementation fills it.
    pub fn touch(&self) {}
}

#[given(expr = "a plan {string} keeping the last {int} snapshot(s) that backs up the same sources")]
async fn plan_same_sources(agent: &mut Agent, plan: String, keep: i64) {
    let _ = (&plan, keep);
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("add a second plan whose host, label and sources match the unmanaged group")
    }
    body();
}

#[given(expr = "the repository holds {int} snapshots no plan owns that match the plan {string}")]
async fn snapshots_matching(agent: &mut Agent, count: u64, plan: String) {
    let _ = (count, plan);
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("add untagged snapshots whose host, label and paths match the plan")
    }
    body();
}

#[when(expr = "the owner adopts the other agent's {string} history into the plan {string}")]
async fn adopt_orphaned(agent: &mut Agent, from: String, into: String) {
    let _ = (&from, &into);
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("prepare and run an adoption of the orphaned group")
    }
    body();
}

#[when(expr = "the owner adopts the unmanaged history that matches the plan {string}")]
async fn adopt_unmanaged(agent: &mut Agent, into: String) {
    let _ = &into;
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("prepare and run an adoption of the unmanaged group")
    }
    body();
}

#[when(
    expr = "the owner adopts the other agent's {string} history into the plan {string} and a forget is asked while it runs"
)]
async fn adopt_then_forget(agent: &mut Agent, from: String, into: String) {
    let _ = (&from, &into);
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("hold the adoption on its engine run and ask for a forget")
    }
    body();
}

#[when(expr = "the owner adopts the unmanaged history into the plan {string}")]
async fn adopt_unmanaged_into(agent: &mut Agent, into: String) {
    let _ = &into;
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("prepare and run an adoption of the unmanaged group")
    }
    body();
}

#[then(expr = "the {int} snapshots are rewritten with the tags of the plan {string}")]
async fn rewritten(agent: &mut Agent, count: u64, plan: String) {
    let _ = (count, plan);
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("the engine was asked to rewrite exactly those snapshots with the plan's tags")
    }
    body();
}

#[then(expr = "the snapshots the other agent held are gone")]
fn other_agents_gone(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("the originals were deleted after the rewrite")
    }
    body();
}

#[then(expr = "the snapshots no plan owned are gone")]
fn unmanaged_gone(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("the originals were deleted after the rewrite")
    }
    body();
}

#[then(expr = "the adoption is refused because no plan matches that history")]
fn refused_no_match(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("a typed refusal names the missing match")
    }
    body();
}

#[then(expr = "the adoption is refused because two plans match that history")]
fn refused_ambiguous(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("a typed refusal names the ambiguity")
    }
    body();
}

#[then(expr = "the engine was never asked to rewrite anything")]
fn no_rewrite_job(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("no rewrite job reached the engine")
    }
    body();
}

#[then(expr = "the forget is refused while the adoption holds the repository")]
fn forget_refused(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("the forget answers the busy refusal")
    }
    body();
}

#[then(expr = "the log holds no operation of this adoption")]
async fn no_operation(agent: &mut Agent) {
    agent.adopt.touch();
    #[allow(clippy::todo, reason = "red test: adopt")]
    fn body() {
        todo!("a refused adoption leaves nothing in the log")
    }
    body();
}
