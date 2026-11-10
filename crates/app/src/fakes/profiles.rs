//! `ProfileReader` and `ProfileWriter` fakes for the acceptance runner: a
//! reader that gives the profile a scenario scripted, and a writer that
//! records what it was asked to render. Rustic's profile format is the real
//! adapter's, so these hold none of it.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domain::RepositoryAlias;

use crate::ports::{
    ExportRefusal, ImportedProfile, PlanExport, ProfileError, ProfileNotice, ProfileReader,
    ProfileWriter, RenderedProfile,
};

/// Gives the scripted profile for any file, and remembers what it was asked.
pub struct FakeProfileReader {
    script: Mutex<Result<ImportedProfile, ProfileError>>,
    asked: Mutex<Vec<(PathBuf, RepositoryAlias)>>,
}

impl FakeProfileReader {
    pub fn new(script: Result<ImportedProfile, ProfileError>) -> Self {
        Self {
            script: Mutex::new(script),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// Replaces what the next reads give.
    pub fn script(&self, script: Result<ImportedProfile, ProfileError>) {
        *lock(&self.script) = script;
    }

    /// The files read and the aliases they were read for, in order.
    pub fn asked(&self) -> Vec<(PathBuf, RepositoryAlias)> {
        lock(&self.asked).clone()
    }
}

#[async_trait]
impl ProfileReader for FakeProfileReader {
    async fn read(
        &self,
        file: &Path,
        repository: &RepositoryAlias,
    ) -> Result<ImportedProfile, ProfileError> {
        lock(&self.asked).push((file.to_path_buf(), repository.clone()));
        lock(&self.script).clone()
    }
}

/// Renders a fixed text with the scripted notices, or the scripted refusal,
/// and records every export it was handed.
#[derive(Default)]
pub struct FakeProfileWriter {
    refusal: Mutex<Option<ExportRefusal>>,
    notices: Mutex<Vec<ProfileNotice>>,
    exports: Mutex<Vec<PlanExport>>,
}

impl FakeProfileWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes every render refuse with `refusal`.
    pub fn refuse(&self, refusal: ExportRefusal) {
        *lock(&self.refusal) = Some(refusal);
    }

    /// Makes every render hand `notice` back with its text.
    pub fn notice(&self, notice: ProfileNotice) {
        lock(&self.notices).push(notice);
    }

    /// What the writer was asked to render, in order.
    pub fn exports(&self) -> Vec<PlanExport> {
        lock(&self.exports).clone()
    }
}

impl ProfileWriter for FakeProfileWriter {
    fn render(&self, export: &PlanExport) -> Result<RenderedProfile, ExportRefusal> {
        lock(&self.exports).push(export.clone());
        if let Some(refusal) = lock(&self.refusal).clone() {
            return Err(refusal);
        }
        Ok(RenderedProfile {
            text: format!("# a profile for the plan {}\n", export.plan.id.as_str()),
            notices: lock(&self.notices).clone(),
        })
    }
}

// Each value is only ever replaced whole or appended to, so a panic elsewhere
// can't leave one half-written.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
