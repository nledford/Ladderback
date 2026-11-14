//! Resolving the secrets a repository's settings refer to, when a job starts
//! (technical design §10.2). Nothing here keeps a resolved value past the
//! returned [`ResolvedRepository`], and nothing prints one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domain::RepositoryAlias;
use domain::execution::FailureReason;
use domain::execution::refusal::SettingRefusal;
use domain::planning::{RepositoryConfig, RepositorySettings, is_relative_location};
use domain::secrets::{
    MAX_MASKED_BYTES, MAX_RCLONE_CREDENTIALS, MAX_SECRET_BYTES, MaskedSecrets, Part, Problem,
    Redactor, Reference, SecretError, SecretRef, SecretValue, SecretWarning, SourceName, Template,
    TemplateError, TooMuchToMask, has_reference, too_many_rclone_credentials,
};

use crate::ports::{RepositoryAccess, SavedEntry, SecretResolver};

/// A repository with every reference resolved, ready to hand to the engine.
/// `access` masks its own secrets, so this one's `Debug` form is derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRepository {
    /// `id` and `entry` are `None`: the caller knows whether the repository
    /// was seen, and where its settings came from. [`ResolveRepository::resolve_saved`]
    /// sets both from the entry it resolves.
    pub access: RepositoryAccess,
    /// What the user should know about how the secrets were read, each once,
    /// with every path masked of the repository's secrets (I4).
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

impl Field {
    /// The setting's name as the configuration writes it: `repository`,
    /// `options.<name>`.
    pub fn name(&self) -> String {
        match self {
            Self::Repository => "repository".to_owned(),
            Self::RepoHot => "repo-hot".to_owned(),
            Self::Password => "password".to_owned(),
            Self::Option(name) => format!("options.{name}"),
            Self::OptionHot(name) => format!("options-hot.{name}"),
            Self::OptionCold(name) => format!("options-cold.{name}"),
        }
    }
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
    /// The field is a location that, resolved, names a folder on this machine
    /// by a relative path
    /// ([`is_relative_location`](domain::planning::is_relative_location)),
    /// which rustic would read from wherever the daemon was started. Nothing
    /// of what it resolved to is kept: a reference may hold a secret (I4).
    RelativePath,
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

impl ResolveError {
    /// Why a run of the repository is refused, by its code: `Err` with the
    /// cap when the repository's secrets come to more than a job masks
    /// ([`Cause::TooMuchToMask`]), whose code is a job's too. Where a secret
    /// was looked for, and what a platform or a command said, are masked
    /// through `redactor` first (I4), and a text masked to nothing is left
    /// out: it says nothing.
    pub fn refusal(&self, redactor: &Redactor) -> Result<SettingRefusal, usize> {
        let bytes = |limit: usize| u64::try_from(limit).unwrap_or(u64::MAX);
        let refusal = match &self.cause {
            Cause::Unresolved(secret) => {
                let source = redactor.redact(&source_text(&secret.source));
                // A line cut at its cap (it ends in `…`) may end in the start
                // of a secret whoever cut it didn't know, which is masked as
                // a whole one is.
                let said = |detail: &str| {
                    Some(redactor.redact_clipped(detail, "…")).filter(|said| !said.is_empty())
                };
                match &secret.problem {
                    Problem::Missing => SettingRefusal::SecretMissing { source },
                    Problem::Unreachable { detail } => SettingRefusal::SecretUnreadable {
                        source,
                        detail: said(detail),
                    },
                    // The platform's words about the service, never about a
                    // secret it gave: none was read. Masked all the same.
                    Problem::KeyringUnavailable { detail } => {
                        SettingRefusal::SecretKeyringUnavailable {
                            source,
                            detail: said(detail),
                        }
                    }
                    Problem::NotUnicode => SettingRefusal::SecretNotText { source },
                    Problem::TooLarge { limit } => SettingRefusal::SecretTooLarge {
                        source,
                        limit: bytes(*limit),
                    },
                    Problem::CommandFailed { detail, .. } => SettingRefusal::SecretCommandFailed {
                        source,
                        detail: said(detail),
                    },
                    // Only a command is run for a time; a file or a keyring
                    // that doesn't answer is a source that can't be read.
                    Problem::TimedOut { seconds } => match secret.source {
                        SourceName::Command { .. } => SettingRefusal::SecretCommandTimedOut {
                            source,
                            seconds: *seconds,
                        },
                        SourceName::Keyring { .. }
                        | SourceName::File { .. }
                        | SourceName::Env { .. } => SettingRefusal::SecretUnreadable {
                            source,
                            detail: None,
                        },
                    },
                    // A command that succeeded while something it started
                    // kept its output open before its secret's line ended
                    // gave no secret, and nothing it printed is shown. Not a
                    // failure of the command, which exited as it should.
                    Problem::OutputCut => SettingRefusal::SecretCommandOutputOpen { source },
                }
            }
            Cause::ReferenceInProcessArguments => SettingRefusal::SecretInProcessArguments,
            Cause::ExpansionTooLong { limit } => SettingRefusal::SecretExpansionTooLong {
                limit: bytes(*limit),
            },
            Cause::TooMuchToMask { limit } => return Err(*limit),
            Cause::TooManyCredentials { .. } => SettingRefusal::LocationTooManyCredentials,
            Cause::RelativePath => SettingRefusal::RepositoryPathRelative,
            Cause::Invalid(template) => SettingRefusal::TemplateInvalid {
                at: template_error_place(template).map(bytes),
            },
        };
        Ok(refusal)
    }

    /// The failure a run the schedule started records when its start is
    /// refused for this error (§5): its [`refusal`](Self::refusal), at
    /// [`field`](Self::field).
    pub fn failure(&self, redactor: &Redactor) -> FailureReason {
        let field = self.field.name();
        match self.refusal(redactor) {
            Ok(refusal) => FailureReason::SecretRefused { field, refusal },
            Err(limit) => FailureReason::SecretsTooLargeToMask {
                field: Some(field),
                limit,
            },
        }
    }
}

/// What names a source in a refusal: the variable, the path, the keyring
/// entry or the program, never what it gave.
fn source_text(source: &SourceName) -> String {
    match source {
        SourceName::Command { program } => program.clone(),
        SourceName::Keyring { entry } => entry.clone(),
        SourceName::File { path } => path.clone(),
        SourceName::Env { name } => name.clone(),
    }
}

/// Where in a setting a template goes wrong, when the fault has a place.
fn template_error_place(error: &TemplateError) -> Option<usize> {
    match error {
        TemplateError::TooLong { .. } | TemplateError::TooManyReferences { .. } => None,
        TemplateError::Unterminated { at }
        | TemplateError::EmptyReference { at }
        | TemplateError::InvalidName { at }
        | TemplateError::EmptyPath { at }
        | TemplateError::RelativePath { at }
        | TemplateError::NestedReference { at } => Some(*at),
    }
}

/// Resolves a repository's settings: the password from its source, and every
/// `${…}` reference in the repository string and the options from the
/// environment or a file.
#[derive(Clone)]
pub struct ResolveRepository {
    resolver: Arc<dyn SecretResolver>,
    told: Option<SecretWarnings>,
}

/// Told each warning that reading a set of secrets gave, once the set is
/// read, with its path masked of those secrets (I4): the daemon says it in
/// its log, an operation's read or not (§10.2, #386).
pub type SecretWarnings = Arc<dyn Fn(&SecretWarning) + Send + Sync>;

impl ResolveRepository {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self {
            resolver,
            told: None,
        }
    }

    /// Tells `told` each warning a resolution gives.
    #[must_use]
    pub fn telling(self, told: SecretWarnings) -> Self {
        Self {
            told: Some(told),
            ..self
        }
    }

    /// `warnings` masked of the secrets `access` holds (I4), each told.
    fn warnings(
        &self,
        warnings: &[SecretWarning],
        access: &RepositoryAccess,
    ) -> Vec<SecretWarning> {
        if warnings.is_empty() {
            return Vec::new();
        }
        let redactor = access.redactor();
        let warnings: Vec<SecretWarning> = warnings
            .iter()
            .map(|warning| warning.masked(&redactor))
            .collect();
        if let Some(told) = &self.told {
            warnings.iter().for_each(|warning| told(warning));
        }
        warnings
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
        let mut run = Run::new(self.resolver.as_ref(), BTreeMap::new());
        let password = run.password(&settings.password).await?;
        self.resolve_rest(run, settings, password)
            .await
            .map(|(resolved, _)| resolved)
    }

    /// Resolves one location of a repository reached at several, when it is
    /// about to be tried (decision 80): `settings` is the location with the
    /// repository's password, which `password` already holds, so it isn't
    /// read again. A reference the job's earlier locations read is not read
    /// again either (`read` keeps them, §10.2); the access lists only the
    /// secrets this location uses, so a job carries no other location's.
    pub async fn resolve_location(
        &self,
        settings: &RepositorySettings,
        password: &SecretValue,
        read: &mut JobReferences,
    ) -> Result<ResolvedRepository, ResolveError> {
        refuse_references_in_arguments(settings)?;
        refuse_too_many_credentials(settings)?;
        let mut run = Run::new(self.resolver.as_ref(), read.values.clone());
        run.masked
            .secret(password)
            .map_err(|too_much| ResolveError {
                field: Field::Password,
                cause: too_much_to_mask(too_much),
            })?;
        let (resolved, values) = self.resolve_rest(run, settings, password.clone()).await?;
        read.values = values;
        Ok(resolved)
    }

    /// The rest of `settings`, past the password, resolved by `run`, with
    /// every reference `run` has read by the end.
    async fn resolve_rest(
        &self,
        mut run: Run<'_>,
        settings: &RepositorySettings,
        password: SecretValue,
    ) -> Result<(ResolvedRepository, BTreeMap<Reference, SecretValue>), ResolveError> {
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
        let access = RepositoryAccess {
            repository,
            repo_hot,
            password,
            options,
            options_hot,
            options_cold,
            secrets: run.secrets,
            id: None,
            entry: None,
        };
        let resolved = ResolvedRepository {
            warnings: self.warnings(&run.warnings, &access),
            access,
        };
        Ok((resolved, run.values))
    }

    /// Resolves the settings of the configuration's entry `entry`, saved
    /// under `alias`, as a job that opens a saved repository needs them: the
    /// access names the repository ID the entry records, so the engine opens
    /// that repository or none (§7.3), and the entry as it was read, so the
    /// daemon can record the ID of the repository the job opens in an entry
    /// that records none (§6.1, decision 57).
    pub async fn resolve_saved(
        &self,
        alias: &RepositoryAlias,
        entry: &RepositoryConfig,
    ) -> Result<ResolvedRepository, ResolveError> {
        let mut resolved = if entry.has_several_locations() {
            self.resolve_password(&entry.settings.password).await?
        } else {
            self.resolve(&entry.settings).await?
        };
        resolved.access.id = entry.id.clone();
        resolved.access.entry = Some(SavedEntry {
            alias: alias.clone(),
            entry: entry.clone(),
        });
        Ok(resolved)
    }
}

/// The references one job's locations have read, each once (§10.2), so a
/// probe and the job that follows it see one value. Dropped with the job.
#[derive(Default)]
pub struct JobReferences {
    values: BTreeMap<Reference, SecretValue>,
}

impl ResolveRepository {
    /// The repository's password alone, in an access that names no
    /// location: a repository reached at several has its location chosen
    /// when the job starts, and each location's references are read only
    /// then ([`Self::resolve_location`]), so nothing opens a location the
    /// choice didn't make.
    pub(crate) async fn resolve_password(
        &self,
        password: &SecretRef,
    ) -> Result<ResolvedRepository, ResolveError> {
        let mut run = Run::new(self.resolver.as_ref(), BTreeMap::new());
        let password = run.password(password).await?;
        let access = RepositoryAccess {
            repository: String::new(),
            repo_hot: None,
            password,
            options: BTreeMap::new(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
            secrets: Vec::new(),
            id: None,
            entry: None,
        };
        Ok(ResolvedRepository {
            warnings: self.warnings(&run.warnings, &access),
            access,
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

/// A location a reference decides is judged only once it is expanded: one
/// that names a folder on this machine by a relative path would be read from
/// the daemon's working directory (#247). The refusal holds nothing of the
/// expansion, which may be a secret.
fn refuse_relative_location(field: Field, expanded: &str) -> Result<(), ResolveError> {
    if is_relative_location(expanded) {
        return Err(ResolveError {
            field,
            cause: Cause::RelativePath,
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
    /// Every reference read, by this run or the job's earlier ones.
    values: BTreeMap<Reference, SecretValue>,
    /// The references this run uses, each counted and listed once.
    used: BTreeSet<Reference>,
    secrets: Vec<SecretValue>,
    warnings: Vec<SecretWarning>,
    masked: MaskedSecrets,
}

impl<'a> Run<'a> {
    fn new(resolver: &'a dyn SecretResolver, values: BTreeMap<Reference, SecretValue>) -> Self {
        Self {
            resolver,
            values,
            used: BTreeSet::new(),
            secrets: Vec::new(),
            warnings: Vec::new(),
            masked: MaskedSecrets::default(),
        }
    }

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
        refuse_relative_location(field.clone(), &expanded)?;
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
        let value = match self.values.get(reference) {
            Some(value) => value.clone(),
            None => {
                let resolved = self
                    .resolver
                    .resolve(&reference.secret_ref())
                    .await
                    .map_err(Cause::Unresolved)?;
                self.warn(resolved.warnings);
                self.values
                    .insert(reference.clone(), resolved.value.clone());
                resolved.value
            }
        };
        if self.used.insert(reference.clone()) {
            self.masked.secret(&value).map_err(too_much_to_mask)?;
            self.secrets.push(value.clone());
        }
        Ok(value)
    }

    fn warn(&mut self, warnings: Vec<SecretWarning>) {
        for warning in warnings {
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::secrets::{Problem, SecretError, SecretValue, SourceName};

    use super::*;

    const CANARY: &str = "CANARY-refused-run-secret";
    /// The start of [`CANARY`], as a line cut at its cap may end.
    const CANARY_START: &str = "CANARY-ref";

    fn unresolved(field: Field, source: SourceName, problem: Problem) -> ResolveError {
        ResolveError {
            field,
            cause: Cause::Unresolved(SecretError { source, problem }),
        }
    }

    fn env(name: &str) -> SourceName {
        SourceName::Env {
            name: name.to_owned(),
        }
    }

    fn keyring(entry: &str) -> SourceName {
        SourceName::Keyring {
            entry: entry.to_owned(),
        }
    }

    fn file(path: &str) -> SourceName {
        SourceName::File {
            path: path.to_owned(),
        }
    }

    fn command(program: &str) -> SourceName {
        SourceName::Command {
            program: program.to_owned(),
        }
    }

    fn nothing_to_mask() -> Redactor {
        Redactor::new([])
    }

    /// A secret that couldn't be read is refused by what was wrong, naming
    /// where it was looked for; only a command is run for a time, so a
    /// keyring or a file that doesn't answer is a source that can't be read.
    #[test]
    fn each_secret_that_cant_be_read_is_refused_by_what_was_wrong() {
        use SettingRefusal as R;
        let said = |text: &str| Some(text.to_owned());
        let cases = [
            (
                env("DEMO_PASSWORD"),
                Problem::Missing,
                R::SecretMissing {
                    source: "DEMO_PASSWORD".to_owned(),
                },
            ),
            (
                file("/srv/demo/password"),
                Problem::Unreachable {
                    detail: "permission denied".to_owned(),
                },
                R::SecretUnreadable {
                    source: "/srv/demo/password".to_owned(),
                    detail: said("permission denied"),
                },
            ),
            (
                keyring("ladderback/demo"),
                Problem::KeyringUnavailable {
                    detail: "no Secret Service runs".to_owned(),
                },
                R::SecretKeyringUnavailable {
                    source: "ladderback/demo".to_owned(),
                    detail: said("no Secret Service runs"),
                },
            ),
            (
                file("/srv/demo/password"),
                Problem::NotUnicode,
                R::SecretNotText {
                    source: "/srv/demo/password".to_owned(),
                },
            ),
            (
                file("/srv/demo/password"),
                Problem::TooLarge { limit: 65_536 },
                R::SecretTooLarge {
                    source: "/srv/demo/password".to_owned(),
                    limit: 65_536,
                },
            ),
            (
                command("pass"),
                Problem::CommandFailed {
                    status: Some(1),
                    detail: "pass: no such entry".to_owned(),
                },
                R::SecretCommandFailed {
                    source: "pass".to_owned(),
                    detail: said("pass: no such entry"),
                },
            ),
            (
                command("pass"),
                Problem::TimedOut { seconds: 30 },
                R::SecretCommandTimedOut {
                    source: "pass".to_owned(),
                    seconds: 30,
                },
            ),
            (
                keyring("ladderback/demo"),
                Problem::TimedOut { seconds: 30 },
                R::SecretUnreadable {
                    source: "ladderback/demo".to_owned(),
                    detail: None,
                },
            ),
            (
                file("/srv/demo/password"),
                Problem::TimedOut { seconds: 30 },
                R::SecretUnreadable {
                    source: "/srv/demo/password".to_owned(),
                    detail: None,
                },
            ),
            (
                command("pass"),
                Problem::OutputCut,
                R::SecretCommandOutputOpen {
                    source: "pass".to_owned(),
                },
            ),
        ];
        for (source, problem, refusal) in cases {
            let error = unresolved(Field::Password, source, problem);
            assert_eq!(error.refusal(&nothing_to_mask()), Ok(refusal), "{error:?}");
        }
    }

    /// Where a secret was looked for and what a platform or a command said
    /// are kept with the repository's secrets masked (I4), a line cut at its
    /// cap included, whose end may be the start of one; a detail that says
    /// nothing is left out.
    #[test]
    fn a_refusal_keeps_no_secret_of_the_repository() {
        let secret = SecretValue::new(CANARY);
        let redactor = Redactor::new([&secret]);
        let error = unresolved(
            Field::Password,
            file(&format!("/srv/{CANARY}/password")),
            Problem::Unreachable {
                detail: format!("denied: {CANARY}"),
            },
        );
        let refusal = error.refusal(&redactor);
        assert!(
            !format!("{refusal:?}").contains("CANARY"),
            "the refusal shows a secret: {refusal:?}"
        );
        assert_eq!(
            refusal,
            Ok(SettingRefusal::SecretUnreadable {
                source: "/srv/***/password".to_owned(),
                detail: Some("denied: ***".to_owned()),
            })
        );

        let clipped = unresolved(
            Field::Password,
            command("pass"),
            Problem::CommandFailed {
                status: Some(1),
                detail: format!("said {}…", CANARY_START),
            },
        );
        let refusal = clipped.refusal(&redactor);
        assert!(
            !format!("{refusal:?}").contains(CANARY_START),
            "the refusal shows the start of a secret: {refusal:?}"
        );

        let silent = unresolved(
            Field::Password,
            command("pass"),
            Problem::CommandFailed {
                status: Some(1),
                detail: String::new(),
            },
        );
        assert_eq!(
            silent.refusal(&redactor),
            Ok(SettingRefusal::SecretCommandFailed {
                source: "pass".to_owned(),
                detail: None,
            })
        );
    }

    /// A setting refused before any secret is read is refused by its own
    /// fault, and a template's by where it goes wrong, when it has a place.
    #[test]
    fn a_setting_refused_before_any_secret_is_read_is_refused_by_its_fault() {
        use SettingRefusal as R;
        let cases = [
            (
                Cause::ReferenceInProcessArguments,
                R::SecretInProcessArguments,
            ),
            (
                Cause::ExpansionTooLong { limit: 65_536 },
                R::SecretExpansionTooLong { limit: 65_536 },
            ),
            (
                Cause::TooManyCredentials { limit: 16 },
                R::LocationTooManyCredentials,
            ),
            (Cause::RelativePath, R::RepositoryPathRelative),
            (
                Cause::Invalid(TemplateError::Unterminated { at: 4 }),
                R::TemplateInvalid { at: Some(4) },
            ),
            (
                Cause::Invalid(TemplateError::EmptyReference { at: 5 }),
                R::TemplateInvalid { at: Some(5) },
            ),
            (
                Cause::Invalid(TemplateError::InvalidName { at: 6 }),
                R::TemplateInvalid { at: Some(6) },
            ),
            (
                Cause::Invalid(TemplateError::EmptyPath { at: 7 }),
                R::TemplateInvalid { at: Some(7) },
            ),
            (
                Cause::Invalid(TemplateError::RelativePath { at: 8 }),
                R::TemplateInvalid { at: Some(8) },
            ),
            (
                Cause::Invalid(TemplateError::NestedReference { at: 9 }),
                R::TemplateInvalid { at: Some(9) },
            ),
            (
                Cause::Invalid(TemplateError::TooLong { limit: 65_536 }),
                R::TemplateInvalid { at: None },
            ),
            (
                Cause::Invalid(TemplateError::TooManyReferences { limit: 64 }),
                R::TemplateInvalid { at: None },
            ),
        ];
        for (cause, refusal) in cases {
            let error = ResolveError {
                field: Field::Repository,
                cause,
            };
            assert_eq!(error.refusal(&nothing_to_mask()), Ok(refusal), "{error:?}");
        }
    }

    /// A refused run's failure names the setting as the configuration
    /// writes it; secrets that come to more than a job masks are a job's
    /// refusal, kept under its own code with the setting where they passed
    /// the cap.
    #[test]
    fn a_refused_runs_failure_names_its_setting_as_the_configuration_writes_it() {
        let missing = unresolved(
            Field::OptionHot("token".to_owned()),
            env("DEMO_TOKEN"),
            Problem::Missing,
        );
        assert_eq!(
            missing.failure(&nothing_to_mask()),
            FailureReason::SecretRefused {
                field: "options-hot.token".to_owned(),
                refusal: SettingRefusal::SecretMissing {
                    source: "DEMO_TOKEN".to_owned(),
                },
            }
        );
        let too_much = ResolveError {
            field: Field::Option("token".to_owned()),
            cause: Cause::TooMuchToMask { limit: 262_144 },
        };
        assert_eq!(too_much.refusal(&nothing_to_mask()), Err(262_144));
        assert_eq!(
            too_much.failure(&nothing_to_mask()),
            FailureReason::SecretsTooLargeToMask {
                field: Some("options.token".to_owned()),
                limit: 262_144,
            }
        );
    }

    /// A file other users can read is named by its path masked of the
    /// repository's secrets, in the resolution and to whoever is told
    /// (I4, #386): the whole repository's when it is resolved at once, the
    /// password's when it alone is.
    #[tokio::test]
    async fn a_readable_files_path_is_masked_of_the_repositorys_secrets() {
        use std::sync::Mutex;

        use domain::secrets::FileRead;

        use crate::fakes::FakeSecrets;

        let store = Arc::new(FakeSecrets::new());
        store.write_file("/home/alice/nas-password", b"bob\n", 0o644);
        let told = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&told);
        let resolve = ResolveRepository::new(store).telling(Arc::new(move |warning| {
            heard.lock().expect("the list").push(warning.clone());
        }));
        let password = SecretRef::File {
            path: "/home/alice/nas-password".to_owned(),
            read: FileRead::FirstLine,
        };
        let settings = RepositorySettings {
            repository: "/srv/backups".to_owned(),
            repo_hot: None,
            password: password.clone(),
            options: BTreeMap::from([("secret_access_key".to_owned(), "alice".to_owned())]),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        };
        let masked = |path: &str| SecretWarning::FileReadableByOthers {
            path: path.to_owned(),
            mode: 0o644,
        };

        let resolved = resolve.resolve(&settings).await.expect("it resolves");
        assert_eq!(resolved.warnings, [masked("/home/***/nas-password")]);
        assert_eq!(
            *told.lock().expect("the list"),
            [masked("/home/***/nas-password")]
        );

        // The password alone masks only itself.
        let store = Arc::new(FakeSecrets::new());
        store.write_file("/home/bob/nas-password", b"bob\n", 0o644);
        let resolved = ResolveRepository::new(store)
            .resolve_password(&SecretRef::File {
                path: "/home/bob/nas-password".to_owned(),
                read: FileRead::FirstLine,
            })
            .await
            .expect("it resolves");
        assert_eq!(resolved.warnings, [masked("/home/***/nas-password")]);
    }
}
