//! Profile interop use cases (technical design §6.2): importing a rustic
//! profile into the configuration, and exporting one plan as a profile the
//! stock `rustic` CLI accepts.
//!
//! The adapter translates rustic's format to and from Ladderback's types;
//! what the translation means for the configuration is decided here: a plan
//! ID or repository alias the configuration already has is refused before
//! anything is saved, and the hostname rule below.
//!
//! **Hostname.** When every imported plan names the same host `H`, the
//! configuration has no plan yet and the agent has no hostname of its own,
//! the agent takes `H` and the plans name none. When the agent's hostname is
//! already `H`, the plans' hosts are cleared. In every other case the plans
//! keep the hosts they came with: moving the agent's hostname would rename
//! the snapshots of the plans it already has.

use std::path::Path;
use std::sync::Arc;

use domain::execution::plan_tags::PlanTags;
use domain::{AgentId, PlanId, RepositoryAlias};

use crate::ports::{
    ConfigError, ConfigStore, ExportRefusal, PlanExport, ProfileError, ProfileNotice,
    ProfileReader, ProfileWriter,
};

/// Imports a rustic profile: reads it through the [`ProfileReader`], checks
/// it against the configuration, and saves the repository, the plans and the
/// profile's global hooks in one save, or saves nothing.
pub struct ImportProfile {
    reader: Arc<dyn ProfileReader>,
    config: Arc<dyn ConfigStore>,
}

/// What an import added to the configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub repository: RepositoryAlias,
    /// The IDs of the plans added, in the order of the profile.
    pub plans: Vec<PlanId>,
    /// What the profile could not carry over, and what changed on the way.
    pub notices: Vec<ProfileNotice>,
}

/// Why an import saved nothing. A typed code, never a sentence (§8.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The profile could not be read, or says what Ladderback cannot honour.
    Profile(ProfileError),
    /// The configuration already holds a repository under the alias.
    AliasTaken(RepositoryAlias),
    /// The configuration, or the profile itself, already has a plan with
    /// this ID.
    PlanTaken(PlanId),
    /// The configuration could not be read or written, or the result is not
    /// a configuration it accepts.
    Config(ConfigError),
}

/// Exports one plan as a profile: finds the plan and its repository in the
/// configuration and hands them, with the agent's own hooks, hostname and
/// ID, to the [`ProfileWriter`].
pub struct ExportProfile {
    writer: Arc<dyn ProfileWriter>,
    config: Arc<dyn ConfigStore>,
    agent: AgentId,
}

/// A plan as a profile: the text, and what the writer left out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedProfile {
    pub text: String,
    pub notices: Vec<ProfileNotice>,
}

/// Why a plan was not exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The configuration has no plan with this ID.
    PlanUnknown(PlanId),
    /// The plan can't be written as a profile.
    Render(ExportRefusal),
    /// The configuration could not be read.
    Config(ConfigError),
}

#[allow(clippy::result_large_err)]
impl ImportProfile {
    pub fn new(reader: Arc<dyn ProfileReader>, config: Arc<dyn ConfigStore>) -> Self {
        Self { reader, config }
    }

    /// Imports the profile in `file` under the repository alias `repository`.
    #[allow(clippy::todo, reason = "red test: interop import")]
    pub async fn import(
        &self,
        file: &Path,
        repository: RepositoryAlias,
    ) -> Result<Imported, ImportError> {
        let _ = (&self.reader, &self.config, file, repository);
        todo!("read the profile, refuse clashes, apply the hostname rule, save once")
    }
}

#[allow(clippy::result_large_err)]
impl ExportProfile {
    pub fn new(
        writer: Arc<dyn ProfileWriter>,
        config: Arc<dyn ConfigStore>,
        agent: AgentId,
    ) -> Self {
        Self {
            writer,
            config,
            agent,
        }
    }

    /// The profile of the plan `plan`.
    #[allow(clippy::todo, reason = "red test: interop export")]
    pub async fn export(&self, plan: &PlanId) -> Result<ExportedProfile, ExportError> {
        let _ = (
            &self.writer,
            &self.config,
            PlanTags::new(plan.clone(), self.agent),
        );
        let _: Option<PlanExport> = None;
        todo!("find the plan and its repository, render the profile")
    }
}
