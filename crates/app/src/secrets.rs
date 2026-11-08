//! Resolving the secrets a repository's settings refer to, when a job starts
//! (technical design §10.2). Nothing here keeps a resolved value past the
//! returned [`ResolvedRepository`], and nothing prints one.

use std::collections::BTreeMap;
use std::sync::Arc;

use domain::secrets::{SecretError, SecretRef, SecretWarning, TemplateError};

use crate::ports::{RepositoryAccess, SecretResolver};

/// A repository as the configuration describes it: its fields may hold
/// `${VAR}` and `${file:/path}` references, which are resolved when a job
/// starts. `password` says where the password is, or is the password.
#[derive(Clone, PartialEq, Eq)]
pub struct RepositorySettings {
    pub repository: String,
    /// rustic's `repo-hot`: the hot half of a hot/cold repository.
    pub repo_hot: Option<String>,
    pub password: SecretRef,
    pub options: BTreeMap<String, String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
}

/// An option written in the config can be a secret as it stands, so what the
/// options hold is masked, and so are the passwords in the repository URLs.
impl std::fmt::Debug for RepositorySettings {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("the settings with every secret masked")
    }
}

/// A repository with every reference resolved, ready to hand to the engine.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedRepository {
    /// `id` is `None`: the caller knows whether the repository was seen.
    pub access: RepositoryAccess,
    pub repo_hot: Option<String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
    /// What the user should know about how the secrets were read, each once.
    pub warnings: Vec<SecretWarning>,
}

/// `access` masks its own secrets; the hot and cold fields are masked here.
impl std::fmt::Debug for ResolvedRepository {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("the resolved repository with every secret masked")
    }
}

/// Where in a repository's settings a secret failed to resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Repository,
    RepoHot,
    Password,
    /// An entry of `options`, by its name.
    Option(String),
    OptionHot(String),
    OptionCold(String),
}

/// Why a repository's settings couldn't be resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The field isn't a valid template: it says where it goes wrong.
    Invalid(TemplateError),
    /// A secret couldn't be read: the error names where it was looked for.
    Unresolved(SecretError),
    /// The field's references expand to more than `limit` bytes.
    ExpansionTooLong { limit: usize },
    /// The field becomes an argument of a process (rclone's repository, a
    /// command option) and holds a reference: the resolved secret would show
    /// in `ps`.
    ReferenceInProcessArguments,
}

/// The first field that couldn't be resolved, tried in the order password,
/// `repository`, `repo-hot`, `options`, `options-hot`, `options-cold`, each
/// by name. A repository that can't be resolved can't be used, so nothing
/// else is tried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveError {
    pub field: Field,
    pub cause: Cause,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ResolveError {}

/// Resolves a repository's settings: the password from its source, and every
/// `${…}` reference in the repository string and the options from the
/// environment or a file.
pub struct ResolveRepository {
    resolver: Arc<dyn SecretResolver>,
}

impl ResolveRepository {
    pub fn new(_resolver: Arc<dyn SecretResolver>) -> Self {
        todo!("a use case that reads secrets through the resolver")
    }

    /// Each distinct reference is read once, so a value can't change between
    /// two places that use it. What they resolved to is in `access.secrets`,
    /// for the engine to hide wherever it reports text.
    pub async fn resolve(
        &self,
        _settings: &RepositorySettings,
    ) -> Result<ResolvedRepository, ResolveError> {
        todo!("resolve the password and every reference in the settings")
    }
}
