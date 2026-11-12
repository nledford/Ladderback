//! Resolving the secrets a repository's settings refer to, when a job starts
//! (technical design §10.2). Nothing here keeps a resolved value past the
//! returned [`ResolvedRepository`], and nothing prints one.

use std::collections::BTreeMap;
use std::sync::Arc;

use domain::planning::RepositorySettings;
use domain::secrets::{
    MAX_MASKED_BYTES, MAX_RCLONE_CREDENTIALS, MAX_SECRET_BYTES, MaskedSecrets, Part, Reference,
    SecretError, SecretRef, SecretValue, SecretWarning, Template, TemplateError, TooMuchToMask,
    has_reference, too_many_rclone_credentials,
};

use crate::ports::{RepositoryAccess, SecretResolver};

/// A repository with every reference resolved, ready to hand to the engine.
/// `access` masks its own secrets, so this one's `Debug` form is derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRepository {
    /// `id` is `None`: the caller knows whether the repository was seen.
    pub access: RepositoryAccess,
    /// What the user should know about how the secrets were read, each once.
    pub warnings: Vec<SecretWarning>,
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
    /// The field is an rclone connection string that holds more than
    /// `limit` credentials
    /// ([`too_many_rclone_credentials`](domain::secrets::too_many_rclone_credentials)):
    /// each is a secret to look for in every text a job shows, so a job
    /// takes no more.
    TooManyCredentials { limit: usize },
    /// The secrets of the repository, each counted once, come to more than
    /// `limit` bytes ([`MAX_MASKED_BYTES`](domain::secrets::MAX_MASKED_BYTES))
    /// with this field's: more than a job masks, so the job can't run (I4).
    /// Nothing after this field is read.
    TooMuchToMask { limit: usize },
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
#[derive(Clone)]
pub struct ResolveRepository {
    resolver: Arc<dyn SecretResolver>,
}

impl ResolveRepository {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self { resolver }
    }

    /// Each distinct reference is read once, so a value can't change between
    /// two places that use it. What they resolved to is in `access.secrets`,
    /// for the engine to hide wherever it reports text. The secrets a job
    /// would mask are counted as they are found, and the first field that
    /// takes them past what a redactor masks fails: nothing after it is read.
    pub async fn resolve(
        &self,
        settings: &RepositorySettings,
    ) -> Result<ResolvedRepository, ResolveError> {
        refuse_references_in_arguments(settings)?;
        refuse_too_many_credentials(settings)?;
        let mut run = Run {
            resolver: self.resolver.as_ref(),
            values: BTreeMap::new(),
            secrets: Vec::new(),
            warnings: Vec::new(),
            masked: MaskedSecrets::default(),
        };
        let password = run.password(&settings.password).await?;
        let repository = run
            .location(Field::Repository, &settings.repository)
            .await?;
        let repo_hot = match &settings.repo_hot {
            Some(text) => Some(run.location(Field::RepoHot, text).await?),
            None => None,
        };
        let options = run.expand_all(&settings.options, Field::Option).await?;
        let options_hot = run
            .expand_all(&settings.options_hot, Field::OptionHot)
            .await?;
        let options_cold = run
            .expand_all(&settings.options_cold, Field::OptionCold)
            .await?;
        Ok(ResolvedRepository {
            access: RepositoryAccess {
                repository,
                repo_hot,
                password,
                options,
                options_hot,
                options_cold,
                secrets: run.secrets,
                id: None,
            },
            warnings: run.warnings,
        })
    }
}

/// The repository strings that rustic hands to a process as an argument:
/// `rclone:…` starts rclone with the rest as its argument.
const ARGUMENT_REPOSITORY_PREFIX: &str = "rclone:";

/// The options of the pinned `rustic_backend` that are run as commands:
/// `rclone-command` starts rclone (`rclone.rs`), and the local backend runs
/// `post-create-command` and `post-delete-command` (`local.rs`).
const COMMAND_OPTIONS: [&str; 3] = [
    "rclone-command",
    "post-create-command",
    "post-delete-command",
];

/// A resolved secret in a process's arguments shows in `ps` and in
/// `/proc/<pid>/cmdline` (I4), so a reference in a field that becomes one is
/// refused, before anything is read. A plain `rclone:remote:path` is fine.
fn refuse_references_in_arguments(settings: &RepositorySettings) -> Result<(), ResolveError> {
    let refuse = |field| ResolveError {
        field,
        cause: Cause::ReferenceInProcessArguments,
    };
    let repository =
        |text: &str| text.starts_with(ARGUMENT_REPOSITORY_PREFIX) && has_reference(text);
    if repository(&settings.repository) {
        return Err(refuse(Field::Repository));
    }
    if settings.repo_hot.as_deref().is_some_and(repository) {
        return Err(refuse(Field::RepoHot));
    }
    let commands = |options: &BTreeMap<String, String>| {
        options
            .iter()
            .find(|(name, text)| COMMAND_OPTIONS.contains(&name.as_str()) && has_reference(text))
            .map(|(name, _)| name.clone())
    };
    if let Some(name) = commands(&settings.options) {
        return Err(refuse(Field::Option(name)));
    }
    if let Some(name) = commands(&settings.options_hot) {
        return Err(refuse(Field::OptionHot(name)));
    }
    if let Some(name) = commands(&settings.options_cold) {
        return Err(refuse(Field::OptionCold(name)));
    }
    Ok(())
}

/// A redactor looks for each credential written in an rclone location in
/// every text it masks, and masks a location with more than
/// [`MAX_RCLONE_CREDENTIALS`] whole, where one of them on its own, quoted in
/// an error of rclone's, would show (I4). Such a location is refused before
/// anything is read.
fn refuse_too_many_credentials(settings: &RepositorySettings) -> Result<(), ResolveError> {
    let refuse = |field| ResolveError {
        field,
        cause: Cause::TooManyCredentials {
            limit: MAX_RCLONE_CREDENTIALS,
        },
    };
    if too_many_rclone_credentials(&settings.repository) {
        return Err(refuse(Field::Repository));
    }
    if settings
        .repo_hot
        .as_deref()
        .is_some_and(too_many_rclone_credentials)
    {
        return Err(refuse(Field::RepoHot));
    }
    Ok(())
}

/// A reference can supply the `rclone:` itself (`${REPO}`), and only the
/// expansion shows it. The secret was read by then, but it goes no further.
fn refuse_supplied_prefix(
    field: Field,
    template: &str,
    expanded: &str,
) -> Result<(), ResolveError> {
    if expanded.starts_with(ARGUMENT_REPOSITORY_PREFIX) && has_reference(template) {
        return Err(ResolveError {
            field,
            cause: Cause::ReferenceInProcessArguments,
        });
    }
    Ok(())
}

/// The secrets counted are past what a redactor masks.
fn too_much_to_mask(_: TooMuchToMask) -> Cause {
    Cause::TooMuchToMask {
        limit: MAX_MASKED_BYTES,
    }
}

/// One resolution: what it has read so far, and the secrets a job would
/// mask, counted.
struct Run<'a> {
    resolver: &'a dyn SecretResolver,
    values: BTreeMap<Reference, SecretValue>,
    secrets: Vec<SecretValue>,
    warnings: Vec<SecretWarning>,
    masked: MaskedSecrets,
}

impl Run<'_> {
    async fn password(&mut self, password: &SecretRef) -> Result<SecretValue, ResolveError> {
        let fail = |cause| ResolveError {
            field: Field::Password,
            cause,
        };
        let resolved = self
            .resolver
            .resolve(password)
            .await
            .map_err(|error| fail(Cause::Unresolved(error)))?;
        self.warn(resolved.warnings);
        self.masked
            .secret(&resolved.value)
            .map_err(|too_much| fail(too_much_to_mask(too_much)))?;
        Ok(resolved.value)
    }

    /// A location with its references in, and its secrets counted.
    async fn location(&mut self, field: Field, text: &str) -> Result<String, ResolveError> {
        let expanded = self.expand(field.clone(), text).await?;
        refuse_supplied_prefix(field.clone(), text, &expanded)?;
        self.masked
            .location(&expanded)
            .map_err(|too_much| ResolveError {
                field,
                cause: too_much_to_mask(too_much),
            })?;
        Ok(expanded)
    }

    /// Options with their references in, and their secrets counted.
    async fn expand_all(
        &mut self,
        options: &BTreeMap<String, String>,
        field: fn(String) -> Field,
    ) -> Result<BTreeMap<String, String>, ResolveError> {
        let mut expanded = BTreeMap::new();
        for (name, text) in options {
            let value = self.expand(field(name.clone()), text).await?;
            self.masked
                .option(name, &value)
                .map_err(|too_much| ResolveError {
                    field: field(name.clone()),
                    cause: too_much_to_mask(too_much),
                })?;
            expanded.insert(name.clone(), value);
        }
        Ok(expanded)
    }

    /// `text` with its references replaced by what they resolve to. Never
    /// more than [`MAX_SECRET_BYTES`], so a few large files can't size an
    /// allocation.
    async fn expand(&mut self, field: Field, text: &str) -> Result<String, ResolveError> {
        let fail = |cause| ResolveError {
            field: field.clone(),
            cause,
        };
        let template = Template::parse(text).map_err(|error| fail(Cause::Invalid(error)))?;
        let mut expanded = String::new();
        for part in template.parts() {
            let resolved;
            let piece = match part {
                Part::Text(text) => text.as_str(),
                Part::Reference(reference) => {
                    resolved = self.value(reference).await.map_err(fail)?;
                    resolved.expose()
                }
            };
            if expanded.len() + piece.len() > MAX_SECRET_BYTES {
                return Err(fail(Cause::ExpansionTooLong {
                    limit: MAX_SECRET_BYTES,
                }));
            }
            expanded.push_str(piece);
        }
        Ok(expanded)
    }

    /// What `reference` resolves to, read once, and counted when first read.
    async fn value(&mut self, reference: &Reference) -> Result<SecretValue, Cause> {
        if let Some(value) = self.values.get(reference) {
            return Ok(value.clone());
        }
        let resolved = self
            .resolver
            .resolve(&reference.secret_ref())
            .await
            .map_err(Cause::Unresolved)?;
        self.warn(resolved.warnings);
        self.masked
            .secret(&resolved.value)
            .map_err(too_much_to_mask)?;
        self.secrets.push(resolved.value.clone());
        self.values
            .insert(reference.clone(), resolved.value.clone());
        Ok(resolved.value)
    }

    fn warn(&mut self, warnings: Vec<SecretWarning>) {
        for warning in warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }
}
