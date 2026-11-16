use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::planning::{AgentConfig, ConfigRevision, ProposalId, RepositoryConfig};
use domain::{RepositoryAlias, RepositoryId};

use crate::ports::{ConfigDraft, ConfigDrafts, ConfigError, ConfigStore, RecordError, SavedOver};

/// A configuration that lives in memory, for use-case tests and acceptance
/// scenarios. It keeps what the real store keeps, and nothing else: it never
/// fails, because a file is the only part of the real store that can.
///
/// As [`ConfigDrafts`] it writes the configuration as its debug text after a
/// comment a person can edit ([`edit_comment`](Self::edit_comment)), which
/// changes the file's revision and nothing in the configuration.
#[derive(Default)]
pub struct InMemoryConfigStore {
    saved: Mutex<Option<AgentConfig>>,
    comment: Mutex<String>,
    /// A comment a person writes just before the next [`ConfigDrafts::save_over`]
    /// writes.
    edit_before_save: Mutex<Option<String>>,
    /// The history's versions, oldest first.
    history: Mutex<Vec<String>>,
}

impl InMemoryConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    // The configuration is only ever replaced whole, so a panic elsewhere
    // can't leave it half-written.
    fn saved(&self) -> MutexGuard<'_, Option<AgentConfig>> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn comment(&self) -> MutexGuard<'_, String> {
        self.comment.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A person edits the file's comment, by hand.
    pub fn edit_comment(&self, text: &str) {
        text.clone_into(&mut self.comment());
    }

    /// The file's comment as it is.
    pub fn current_comment(&self) -> String {
        self.comment().clone()
    }

    /// A person edits the file's comment to `text` just before the next
    /// [`ConfigDrafts::save_over`] writes.
    pub fn edit_comment_before_next_save(&self, text: &str) {
        *self
            .edit_before_save
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(text.to_owned());
    }

    /// The history's versions, oldest first.
    pub fn history(&self) -> Vec<String> {
        self.history
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn text(&self, config: &AgentConfig) -> String {
        format!("# {}\n{config:#?}\n", self.comment())
    }
}

#[async_trait]
impl ConfigStore for InMemoryConfigStore {
    async fn load(&self) -> Result<AgentConfig, ConfigError> {
        Ok(self.saved().clone().unwrap_or_default())
    }

    async fn save(&self, config: &AgentConfig) -> Result<(), ConfigError> {
        *self.saved() = Some(config.clone());
        Ok(())
    }

    async fn record_id(
        &self,
        alias: &RepositoryAlias,
        read: &RepositoryConfig,
        id: &RepositoryId,
    ) -> Result<(), RecordError> {
        let mut saved = self.saved();
        let empty = AgentConfig::default();
        let current = saved.as_ref().unwrap_or(&empty);
        let recorded = current
            .record_id(alias, read, id)
            .map_err(RecordError::NotRecorded)?;
        *saved = Some(recorded);
        Ok(())
    }
}

#[async_trait]
impl ConfigDrafts for InMemoryConfigStore {
    async fn draft(&self, config: &AgentConfig) -> Result<ConfigDraft, ConfigError> {
        let current = self.saved().clone().unwrap_or_default();
        let before = self.text(&current);
        Ok(ConfigDraft {
            revision: ConfigRevision::of(before.as_bytes()),
            before,
            after: self.text(config),
        })
    }

    async fn save_over(
        &self,
        base: &ConfigRevision,
        config: &AgentConfig,
        proposal: &ProposalId,
    ) -> Result<SavedOver, ConfigError> {
        let edit = self
            .edit_before_save
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(edit) = edit {
            self.edit_comment(&edit);
        }
        let current = self.saved().clone().unwrap_or_default();
        if ConfigRevision::of(self.text(&current).as_bytes()) != *base {
            return Ok(SavedOver::ChangedMeanwhile);
        }
        let mut history = self.history.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = format!("{:06}-proposal-{proposal}", history.len().saturating_add(1));
        history.push(kept.clone());
        *self.saved() = Some(config.clone());
        Ok(SavedOver::Saved { kept: Some(kept) })
    }
}
