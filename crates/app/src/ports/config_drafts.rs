use async_trait::async_trait;
use domain::planning::{AgentConfig, ConfigRevision, ProposalId};

use crate::ports::ConfigError;

/// The configuration file as it is, and as a configuration would write it,
/// each with every value that holds a secret masked (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDraft {
    /// The file as it is.
    pub revision: ConfigRevision,
    /// Its text as it is, masked; empty when there is no file.
    pub before: String,
    /// Its text as the configuration would write it, as a save would,
    /// masked.
    pub after: String,
}

/// What [`ConfigDrafts::save_over`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedOver {
    /// The file was written; `kept` names the history's version of the file
    /// as it was before, when there was one.
    Saved { kept: Option<String> },
    /// The file was no longer the revision named, so nothing was written.
    ChangedMeanwhile,
}

/// Writes the configuration for a proposal's apply (decision 89, point 12):
/// shows the file a save would write, and saves only over the file as it
/// was shown, so a hand edit is never overwritten.
///
/// Contract (checked by `contract::config_drafts`):
/// - [`draft`](Self::draft) writes nothing; its `before` is the file as
///   it is, `revision` the revision of the file's bytes, and `after` the
///   text a [`save_over`](Self::save_over) of the same configuration
///   writes. In both texts each value that holds a secret of `config`
///   ([`AgentConfig::redactors`]) is masked, however the file writes it:
///   escaped, over several lines or in pieces.
/// - [`save_over`](Self::save_over) writes `config` only when the file is
///   still the revision `base` just before it is written, as the draft
///   gave it, and otherwise writes nothing; the history's version it keeps
///   names `proposal`, and a load afterwards returns `config`.
#[async_trait]
pub trait ConfigDrafts: Send + Sync {
    async fn draft(&self, config: &AgentConfig) -> Result<ConfigDraft, ConfigError>;

    async fn save_over(
        &self,
        base: &ConfigRevision,
        config: &AgentConfig,
        proposal: &ProposalId,
    ) -> Result<SavedOver, ConfigError>;
}
