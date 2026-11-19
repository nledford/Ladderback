//! The repository routes' service (technical design §4.2): tests, connects
//! and initialises repositories, from a request's settings to the use cases
//! of `app` and back to the typed refusals the routes answer with.

use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::locations::ChooseLocation;
use app::ports::{
    Clock, ConfigError, ConfigStore, Engine, EngineError, Keyring, KeyringError, NetworkLink,
    OperationStore,
};
use app::repositories::{
    ChangeMaintainer, ConfigWrites, ConnectRepository, CreateOffers, InitRepository,
    MaintainerError, RepositoryError, SavedEntryError, TestRepository,
};
use app::secrets::{Field, ResolveRepository};
use async_trait::async_trait;
use domain::OperationId;
use domain::RepositoryAlias;
use domain::execution::FailureReason;
use domain::planning::{Maintenance, RepositoryField, RepositorySettings};
use domain::secrets::{FileRead, KeyringEntry, SecretRef, SecretValue};
use http_adapter::{Repositories, RepositoriesError, SecretRefusal};
use secrets::SystemSecrets;
use uuid::Uuid;

use super::startup::log_location;
use super::{
    config_unusable, failure_redactor, locations_tried, redacted_location, secret_refusal, text,
    too_much_to_mask, unavailable,
};

/// Tests, connects and initialises repositories for the API (§4.2): each
/// request's settings become domain values, run through the use cases of
/// `app`, and every refusal becomes a typed one. Writes to the configuration
/// go one at a time, so two connections made at the same moment are both
/// kept.
pub(super) struct RepositoryService {
    clock: Arc<dyn Clock>,
    engine: Arc<dyn Engine>,
    /// The network link a Test evaluates the locations' rules against.
    link: Arc<dyn NetworkLink>,
    resolve: ResolveRepository,
    config: Arc<dyn ConfigStore>,
    /// The operation log, which says whether a saved repository's row
    /// offers Create (decision 53).
    pub(super) operations: Arc<dyn OperationStore>,
    /// Held across a use case's read-modify-write of the configuration, and
    /// shared with the record of an opened repository's ID.
    writes: Arc<ConfigWrites>,
    /// Where a typed password is kept when the person chose the system
    /// keyring (§10.2): the OS keyring the resolver reads.
    keyring: Arc<dyn Keyring>,
}

impl RepositoryService {
    pub(super) fn new(
        clock: Arc<dyn Clock>,
        engine: Arc<dyn Engine>,
        link: Arc<dyn NetworkLink>,
        resolve: ResolveRepository,
        config: Arc<dyn ConfigStore>,
        operations: Arc<dyn OperationStore>,
        writes: Arc<ConfigWrites>,
    ) -> Self {
        Self {
            clock,
            engine,
            link,
            resolve,
            config,
            operations,
            writes,
            keyring: Arc::new(SystemSecrets::default()),
        }
    }

    /// The service keeping typed passwords in `keyring` rather than in the
    /// OS keyring, so that no test writes to the developer's own.
    #[cfg(test)]
    pub(super) fn with_keyring(self, keyring: Arc<dyn Keyring>) -> Self {
        Self { keyring, ..self }
    }
}

#[async_trait]
impl Repositories for RepositoryService {
    async fn test(
        &self,
        request: api::TestRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        let settings = settings_of(request.settings)?;
        let id = match TestRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
        )
        .test(new_operation(), &settings)
        .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn connect(
        &self,
        request: api::ConnectRepositoryDto,
    ) -> Result<api::RepositoryIdDto, RepositoriesError> {
        // The name first: a repository that can't be saved isn't tested.
        let alias =
            RepositoryAlias::parse(&request.alias).map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        let keep_in_keyring = keeps_in_keyring(request.keep_in_keyring, Some(&alias), &settings)?;
        // Asked for, this agent maintains it; otherwise nobody is asked, for
        // a repository that exists may already be maintained elsewhere (I1).
        let maintenance = if request.maintain_here {
            Maintenance::Local
        } else {
            Maintenance::None
        };
        let _writing = self.writes.lock().await;
        let connecting = ConnectRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        );
        let connecting = if keep_in_keyring {
            connecting.keeping_password_in(Arc::clone(&self.keyring))
        } else {
            connecting
        };
        let id = match connecting
            .connect(new_operation(), alias, &settings, maintenance)
            .await
        {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryIdDto {
            id: id.as_str().to_owned(),
        })
    }

    async fn init(
        &self,
        request: api::InitRepositoryDto,
    ) -> Result<api::RepositoryInitDto, RepositoriesError> {
        // The name first: a repository that was created and then can't be
        // saved is one the person has no way to find again.
        let alias = request
            .alias
            .as_deref()
            .map(RepositoryAlias::parse)
            .transpose()
            .map_err(|_| RepositoriesError::AliasInvalid)?;
        let settings = settings_of(request.settings)?;
        let keep_in_keyring = keeps_in_keyring(request.keep_in_keyring, alias.as_ref(), &settings)?;
        // Only a saving initialisation writes the configuration.
        let _writing = match alias {
            Some(_) => Some(self.writes.lock().await),
            None => None,
        };
        let initialising = InitRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        );
        let initialising = if keep_in_keyring {
            initialising.keeping_password_in(Arc::clone(&self.keyring))
        } else {
            initialising
        };
        let initialised = match initialising.init(new_operation(), &settings, alias).await {
            Ok(value) => value,
            Err(error) => return Err(refusal_of(error, &settings).await),
        };
        Ok(api::RepositoryInitDto {
            id: initialised.id.as_str().to_owned(),
            saved: initialised.saved,
        })
    }

    async fn create_saved(&self, alias: String) -> Result<api::RepositoryIdDto, RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // Create records the new repository's ID in its entry, and whether
        // the row offers it is read under the same lock, so two Creates
        // can't both find it offered.
        let _writing = self.writes.lock().await;
        let saved = match InitRepository::new(
            Arc::clone(&self.clock),
            Arc::clone(&self.engine),
            self.resolve.clone(),
            Arc::clone(&self.config),
        )
        .init_saved(
            new_operation(),
            &parsed,
            &CreateOffers::new(Arc::clone(&self.operations)),
        )
        .await
        {
            Ok(saved) => saved,
            // Before the entry is found and its row known to offer Create,
            // nothing ran: the configuration can't be read, it doesn't hold
            // the alias, or the operation log can't be read.
            Err(SavedEntryError::Config(error)) => return Err(config_refusal(error)),
            Err(SavedEntryError::Unknown(_)) => return Err(RepositoriesError::Unknown { alias }),
            // The log's own words say what it couldn't do, as maintenance
            // tells it.
            Err(SavedEntryError::Log(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            )) => {
                return Err(unavailable(
                    ErrorCode::OperationFailed,
                    Vec::new(),
                    Some(detail),
                ));
            }
        };
        match saved.created {
            Ok(id) => Ok(api::RepositoryIdDto {
                id: id.as_str().to_owned(),
            }),
            Err(error) => Err(refusal_of(error, &saved.settings).await),
        }
    }

    async fn test_saved(&self, alias: String) -> Result<api::LocationsTestDto, RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        let config = self.config.load().await.map_err(config_refusal)?;
        let Some(entry) = config.repositories().get(&parsed).cloned() else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // Probes go through the daemon's own engine, which hands a probe
        // straight to a worker; the log hears what a job's choice tells it.
        let saved = app::ports::SavedEntry {
            alias: parsed,
            entry,
        };
        let tested = ChooseLocation::new(
            Arc::clone(&self.engine),
            Arc::clone(&self.clock),
            Arc::clone(&self.link),
            self.resolve.clone(),
            Arc::new(log_location),
        )
        .test_each(new_operation(), &saved)
        .await;
        match tested {
            Ok(record) => Ok(api::LocationsTestDto {
                locations: locations_tried(&record),
            }),
            Err(error) => Err(refusal_of(error, &saved.entry.settings).await),
        }
    }

    async fn change_maintainer(
        &self,
        alias: String,
        request: api::MaintainerDto,
    ) -> Result<(), RepositoriesError> {
        // A name that is no alias is one the configuration can't hold.
        let Ok(parsed) = RepositoryAlias::parse(&alias) else {
            return Err(RepositoriesError::Unknown { alias });
        };
        // The change runs under the configuration's write lock, which a
        // maintenance start holds too while it reads again whether this agent
        // maintains the repository and takes its slot (I1).
        match ChangeMaintainer::new(
            Arc::clone(&self.config),
            Arc::clone(&self.operations),
            Arc::clone(&self.writes),
        )
        .change(&parsed, request.maintain_here)
        .await
        {
            Ok(_) => Ok(()),
            Err(MaintainerError::Unknown(_)) => Err(RepositoriesError::Unknown { alias }),
            Err(MaintainerError::Busy { running }) => Err(RepositoriesError::MaintenanceBusy {
                operation: running.as_uuid().hyphenated().to_string(),
            }),
            Err(MaintainerError::Config(error)) => Err(config_refusal(error)),
            // The log's own words say what it couldn't do, as Create tells
            // it.
            Err(MaintainerError::Log(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            )) => Err(unavailable(
                ErrorCode::OperationFailed,
                Vec::new(),
                Some(detail),
            )),
        }
    }

    /// Asks the OS keyring the page's question, and only it: never a run. A
    /// configuration that can't be read notes nothing; the page's list says
    /// why it can't be read.
    async fn notes(&self) -> api::RepositoryNotesDto {
        let notes = match self.config.load().await {
            Ok(config) => app::repositories::page_notes(&*self.keyring, &config).await,
            Err(_) => Vec::new(),
        };
        api::RepositoryNotesDto { notes }
    }
}

/// A fresh operation ID: a UUIDv7, as every operation of the daemon has.
pub(super) fn new_operation() -> OperationId {
    OperationId::from_uuid(Uuid::now_v7())
}

/// A request's settings as domain values, read as the configuration reader
/// reads the same keys: a password file is an absolute path, and a keyring
/// entry is `service/name`. A typed password and a location (hot ones too)
/// are not empty. What it can't take is an invalid body, except a setting
/// that names a path on this machine by a relative one, which is refused at
/// that setting.
pub(super) fn settings_of(
    dto: api::RepositorySettingsDto,
) -> Result<RepositorySettings, RepositoriesError> {
    let empty_password =
        matches!(&dto.password, api::PasswordDto::Inline { value } if value.is_empty());
    if empty_password || dto.repository.is_empty() || dto.repo_hot.as_deref() == Some("") {
        return Err(RepositoriesError::InvalidBody);
    }
    let password = match dto.password {
        api::PasswordDto::Inline { value } => SecretRef::Inline(SecretValue::new(value)),
        api::PasswordDto::File { path } if path.starts_with('/') => SecretRef::File {
            path,
            read: FileRead::FirstLine,
        },
        api::PasswordDto::File { path } if path.is_empty() => {
            return Err(RepositoriesError::InvalidBody);
        }
        api::PasswordDto::File { .. } => return Err(path_relative(Field::Password)),
        api::PasswordDto::Keyring { entry } => SecretRef::Keyring(
            KeyringEntry::parse(&entry).map_err(|_| RepositoriesError::InvalidBody)?,
        ),
        api::PasswordDto::Env { name } => SecretRef::Env(name),
    };
    let settings = RepositorySettings {
        repository: dto.repository,
        repo_hot: dto.repo_hot,
        password,
        options: dto.options,
        options_hot: dto.options_hot,
        options_cold: dto.options_cold,
    };
    // What the configuration would refuse when it loads next is not saved,
    // nor tested: a request can't take more than a job does. A value over
    // its cap is refused for that first, as the configuration refuses it.
    settings
        .check_sizes()
        .map_err(|_| RepositoriesError::InvalidBody)?;
    match settings.relative_path() {
        Some(field) => Err(path_relative(match field {
            RepositoryField::Repository => Field::Repository,
            RepositoryField::RepoHot => Field::RepoHot,
            // A request has no password command; the field is the password.
            RepositoryField::PasswordCommand => Field::Password,
            RepositoryField::Option(name) => Field::Option(name),
            RepositoryField::OptionHot(name) => Field::OptionHot(name),
            RepositoryField::OptionCold(name) => Field::OptionCold(name),
        })),
        None => Ok(settings),
    }
}

/// The refusal of a relative path at `field`, named as the configuration
/// names it.
fn path_relative(field: Field) -> RepositoriesError {
    RepositoriesError::PathRelative {
        field: field.name(),
    }
}

/// Why a repository use case failed, as the typed refusal the route answers
/// with. `settings` are the request's: a location the refusal names is shown
/// redacted.
async fn refusal_of(error: RepositoryError, settings: &RepositorySettings) -> RepositoriesError {
    match error {
        RepositoryError::Exists => exists(settings),
        RepositoryError::CreateNotOffered => RepositoriesError::CreateNotOffered,
        RepositoryError::AliasTaken(alias) => RepositoriesError::AliasTaken {
            alias: alias.as_str().to_owned(),
        },
        RepositoryError::AlreadyConfigured(alias) => RepositoriesError::AlreadyConfigured {
            alias: alias.as_str().to_owned(),
        },
        RepositoryError::Resolve(error) => RepositoriesError::SecretRefused(
            secret_refusal(&error, settings, &SystemSecrets::default()).await,
        ),
        RepositoryError::Engine(EngineError::Unavailable { detail }) => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        RepositoryError::Engine(EngineError::StartTimedOut) => {
            unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None)
        }
        RepositoryError::Engine(EngineError::SecretsTooLargeToMask { field, limit }) => {
            RepositoriesError::SecretRefused(too_much_to_mask(field, limit))
        }
        RepositoryError::Failed(reason) => failed_refusal(reason, settings),
        RepositoryError::Config(error) => config_refusal(error),
        RepositoryError::Keyring { entry, error } => {
            RepositoriesError::SecretRefused(keyring_refusal(&entry, error, settings).await)
        }
        RepositoryError::TimedOut => unavailable(ErrorCode::RepositoryTimedOut, Vec::new(), None),
    }
}

/// Whether a request that asks to keep its password in the system keyring
/// may: the keyring keeps a password typed for a repository saved under a
/// name (§10.2), so a request with no name, or with a password that is
/// already a reference, is one the form never sends. A password longer than
/// the resolver reads from an entry (`MAX_SECRET_BYTES`) is refused too, as
/// settings over their caps are, rather than kept where no job could read it.
fn keeps_in_keyring(
    asked: bool,
    alias: Option<&RepositoryAlias>,
    settings: &RepositorySettings,
) -> Result<bool, RepositoriesError> {
    let readable = settings
        .password
        .inline()
        .is_some_and(|typed| typed.expose().len() <= domain::secrets::MAX_SECRET_BYTES);
    if asked && (alias.is_none() || !readable) {
        return Err(RepositoriesError::InvalidBody);
    }
    Ok(asked)
}

/// A typed password the keyring couldn't keep, as the secret refusal the
/// routes answer with: the entry it was to be kept in, and what the platform
/// said, masked of every secret of the repository, the typed password first
/// (I4). A keyring that never answered is one that couldn't keep it.
async fn keyring_refusal(
    entry: &KeyringEntry,
    error: KeyringError,
    settings: &RepositorySettings,
) -> SecretRefusal {
    let (code, said) = match error {
        KeyringError::Taken => (ErrorCode::KeyringEntryTaken, None),
        KeyringError::Unavailable { detail } => (ErrorCode::KeyringUnavailable, Some(detail)),
        KeyringError::Locked { detail } => (ErrorCode::KeyringLocked, Some(detail)),
        KeyringError::Unwritable { detail } => (ErrorCode::KeyringUnwritable, Some(detail)),
        KeyringError::TimedOut { .. } => (ErrorCode::KeyringUnwritable, None),
    };
    let detail = match said.filter(|said| !said.is_empty()) {
        Some(said) => Some(
            failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&said),
        ),
        None => None,
    };
    SecretRefusal {
        code,
        args: vec![("entry", MsgArg::Text(entry.to_string()))],
        detail,
    }
}

fn exists(settings: &RepositorySettings) -> RepositoriesError {
    RepositoriesError::Exists {
        location: redacted_location(settings, &settings.repository),
    }
}

/// A job that ran and failed: the engine's own words for a failure it
/// reports, a typed code for the rest. The engine redacts what it says.
fn failed_refusal(reason: FailureReason, settings: &RepositorySettings) -> RepositoriesError {
    use FailureReason as F;
    match reason {
        F::EngineFailed { detail } => RepositoriesError::OperationFailed { detail },
        F::EngineUnavailable { detail } | F::JobRejected { detail } => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        F::EngineExited { exit_code } => unavailable(
            ErrorCode::EngineExited,
            exit_code
                .map(|code| ("exit_code", text(code)))
                .into_iter()
                .collect(),
            None,
        ),
        F::ProtocolViolated { transition } => unavailable(
            ErrorCode::ProtocolViolated,
            vec![("transition", text(format!("{transition:?}")))],
            None,
        ),
        F::DeadlinePassed => unavailable(ErrorCode::DeadlinePassed, Vec::new(), None),
        F::RestoreTargetUnwritable => {
            unavailable(ErrorCode::RestoreTargetUnwritable, Vec::new(), None)
        }
        F::EngineStartTimedOut => unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None),
        F::EngineExitTimedOut => unavailable(ErrorCode::EngineExitTimedOut, Vec::new(), None),
        // Only a backup's before-hook stops a job this way: a repository job
        // runs no hooks. It is still a failure, in the catalog's plainest words.
        F::HookFailed { .. } => unavailable(ErrorCode::OperationFailed, Vec::new(), None),
        F::RepositoryExists => exists(settings),
        F::RepositoryReplaced => unavailable(ErrorCode::RepositoryReplaced, Vec::new(), None),
        F::RepositoryNotFound => RepositoriesError::NotFound,
        F::RepositoryConfigNotFound { detail } => RepositoriesError::ConfigNotFound { detail },
        F::CredentialsRefused { half } => unavailable(
            ErrorCode::CredentialsRefused,
            half.map(|half| ("half", text(half.code())))
                .into_iter()
                .collect(),
            None,
        ),
        F::SecretsTooLargeToMask { field, limit } => {
            RepositoriesError::SecretRefused(too_much_to_mask(field, limit))
        }
        F::DaemonStopped => unavailable(ErrorCode::DaemonStopped, Vec::new(), None),
        F::DaemonShutDown => unavailable(ErrorCode::DaemonShutDown, Vec::new(), None),
        F::NoLocationUsable => unavailable(ErrorCode::NoLocationUsable, Vec::new(), None),
        F::NoLocationRuleHolds => unavailable(ErrorCode::NoLocationRuleHolds, Vec::new(), None),
        F::PasswordNotAccepted => unavailable(ErrorCode::PasswordNotAccepted, Vec::new(), None),
        // Only a run the schedule started records a refusal (§5): a
        // repository job is refused before it runs, as `secret_refusal` says.
        F::SecretRefused { field, refusal } => {
            let (code, args, detail) = app::announce::refused(&field, &refusal);
            RepositoriesError::SecretRefused(SecretRefusal { code, args, detail })
        }
    }
}

/// A configuration that couldn't be read or written, refused by a repository
/// route.
fn config_refusal(error: ConfigError) -> RepositoriesError {
    config_unusable(error, unavailable)
}
