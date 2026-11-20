//! The catalog the API asks of the daemon (technical design §4.2): the
//! configuration's plans and repositories, one repository's snapshots and a
//! folder of one of them, as the DTOs the pages show and the typed refusals
//! the routes answer with.

use std::collections::HashMap;
use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::browse::{BrowseError, BrowseSnapshot};
use app::conditions::NameWatch;
use app::ports::{
    ConfigError, ConfigStore, Engine, EngineError, NormalizeSources, OperationStore, ScheduleStore,
    SnapshotSummary, config_unusable,
};
use app::reads::ReadTurns;
use app::repositories::CreateOffers;
use app::secrets::ResolveRepository;
use app::snapshots::{ListSnapshots, SnapshotError};
use app::status::{ReadStatus, StatusError};
use async_trait::async_trait;
use domain::RepositoryAlias;
use domain::browse::FolderPath;
use domain::execution::FailureReason;
use domain::planning::{
    InterfaceKind, Link, Location, LocationRule, MixedLinkNote, NameFilter, Plan, RepositoryConfig,
    RepositorySettings, RuleNote,
};
use domain::secrets::SecretRef;
use domain::{OperationId, PlanId, SnapshotId};
use http_adapter::{Catalog, CatalogError, FolderError, SecretRefusal, SnapshotsError};
use platform::SystemClock;
use secrets::SystemSecrets;
use uuid::Uuid;

use super::{
    exposed_args, failure_redactor, inline_secret_warning, inline_secret_warnings,
    log_secret_warning, maintenance, never_matching_glob_warnings, redacted_location,
    secret_refusal, summary, text, too_much_to_mask,
};

/// The catalog the API asks of the daemon (§4.2): the configuration's plans,
/// with the scheduler's planned runs, and repositories, and one repository's
/// snapshots, listed live through the engine (§9: a read stores nothing and
/// takes no slot). Which repositories a row offers to create is read from
/// the operation log (decision 53).
pub(super) struct ConfigCatalog {
    pub(super) config: Arc<dyn ConfigStore>,
    pub(super) engine: Arc<dyn Engine>,
    pub(super) operations: Arc<dyn OperationStore>,
    pub(super) schedules: Arc<dyn ScheduleStore>,
    /// Whether this machine reads the Wi-Fi network's name, for the notes
    /// on rules that name networks (decision 80).
    pub(super) link: Arc<NameWatch>,
    /// A plan's sources as the backup walks them, which its globs are read
    /// against (#510).
    pub(super) sources: Arc<dyn NormalizeSources>,
    /// The turns to read a folder of a snapshot, which every browse shares
    /// (decision 102).
    pub(super) reads: Arc<ReadTurns>,
    /// The turns to list a repository's snapshots, which every listing
    /// shares (#710).
    pub(super) listings: Arc<ReadTurns>,
}

#[async_trait]
impl Catalog for ConfigCatalog {
    async fn status(
        &self,
        full_disk_access: api::FullDiskAccessStateDto,
    ) -> Result<api::StatusDto, CatalogError> {
        let listed = self.plans().await?;
        let read = ReadStatus::new(
            Arc::clone(&self.config),
            Arc::clone(&self.operations),
            Arc::new(SystemClock),
            domain::TimeZone::system(),
        );
        let status = read.read().await.map_err(|error| match error {
            StatusError::Config(error) => catalog_refusal(*error),
            // The log's own words say what it couldn't do, as the history
            // tells it.
            StatusError::Store(
                app::ports::StoreError::Unavailable { detail }
                | app::ports::StoreError::Corrupt { detail },
            ) => CatalogError::Unavailable {
                code: ErrorCode::OperationFailed,
                args: Vec::new(),
                detail: Some(detail),
            },
        })?;
        let mut standings: HashMap<String, app::status::PlanStanding> = status
            .plans
            .into_iter()
            .map(|standing| (standing.plan.as_str().to_owned(), standing))
            .collect();
        // A plan the configuration gained or lost between the two readings
        // is left out until the next one.
        let plans = listed
            .plans
            .into_iter()
            .filter_map(|plan| {
                let standing = standings.remove(&plan.id)?;
                Some(api::PlanStatusDto {
                    plan,
                    last: standing.last.as_ref().and_then(summary),
                    last_success: standing.last_success.map(|success| api::LastSuccessDto {
                        operation: success.operation.id.as_uuid().hyphenated().to_string(),
                        finished_at: success
                            .operation
                            .finished_at
                            .unwrap_or(success.operation.queued_at)
                            .to_string(),
                        ago_seconds: u64::try_from(success.ago.as_secs()).unwrap_or(0),
                    }),
                    overdue: standing.overdue,
                })
            })
            .collect();
        Ok(api::StatusDto {
            plans,
            running: status.queue.iter().filter_map(summary).collect(),
            unreadable_repositories: status
                .unreadable
                .iter()
                .filter_map(|unreadable| {
                    Some(api::UnreadableRepositoryDto {
                        repository: unreadable.repository.as_str().to_owned(),
                        operation: summary(&unreadable.operation)?,
                    })
                })
                .collect(),
            full_disk_access,
        })
    }

    async fn plans(&self) -> Result<api::PlansDto, CatalogError> {
        let config = self.config.load().await.map_err(catalog_refusal)?;
        // A planned run that can't be read is no reason to hide the plans:
        // each scheduled one shows that it has none planned.
        let planned: HashMap<PlanId, domain::Timestamp> = match self.schedules.planned_runs().await
        {
            Ok(runs) => runs.into_iter().map(|run| (run.plan, run.at)).collect(),
            Err(error) => {
                log::warn!(
                    "the planned runs couldn't be read ({}); the plans are listed without them",
                    error.code()
                );
                HashMap::new()
            }
        };
        let mut plans = Vec::with_capacity(config.plans().len());
        for plan in config.plans() {
            let scheduled = plan.schedule.is_some();
            plans.push(api::PlanSummaryDto {
                id: plan.id.as_str().to_owned(),
                repository: plan.repository.as_str().to_owned(),
                scheduled,
                // A run kept for a plan whose schedule was just removed is
                // the scheduler's to forget, not a run.
                next_run: planned
                    .get(&plan.id)
                    .filter(|_| scheduled)
                    .map(ToString::to_string),
                warnings: never_matching_glob_warnings(plan, self.sources.as_ref())
                    .await
                    .into_iter()
                    .map(|(code, args)| api::WarningDto::new(code, args))
                    .collect(),
            });
        }
        Ok(api::PlansDto { plans })
    }

    async fn repositories(&self) -> Result<api::RepositoriesDto, CatalogError> {
        let config = self.config.load().await.map_err(catalog_refusal)?;
        let offered = self.offers(&config).await;
        // A rule is noted on a mixed link, and a rule that names networks
        // when the name can't be read: the first listing that has a rule
        // reads the link, and the name when one is named, once, in the
        // background, as the reads may take seconds.
        let rules = || {
            config
                .repositories()
                .values()
                .flat_map(|repository| repository.location_rules().into_iter().flatten())
        };
        if rules().next().is_some() {
            let names_asked = rules().any(LocationRule::asks_name);
            let link = Arc::clone(&self.link);
            tokio::spawn(async move { link.check_once(names_asked).await });
        }
        let reads_names = self.link.reads_names();
        let last_link = self.link.last_link();
        Ok(api::RepositoriesDto {
            repositories: config
                .repositories()
                .iter()
                .map(|(alias, repository)| {
                    let plans: Vec<&Plan> = config
                        .plans()
                        .iter()
                        .filter(|plan| &plan.repository == alias)
                        .collect();
                    let mut row = listed_repository(
                        alias,
                        repository,
                        &plans,
                        reads_names,
                        offered.contains(alias),
                    );
                    row.warnings.extend(mixed_link_note(repository, last_link));
                    row
                })
                .collect(),
        })
    }

    async fn snapshots(&self, alias: String) -> Result<api::SnapshotsDto, SnapshotsError> {
        // An alias no repository could have is as unknown as one the
        // configuration happens not to have.
        let alias = RepositoryAlias::parse(&alias).map_err(|_| SnapshotsError::Unknown)?;
        let config = self.config.load().await.map_err(|error| {
            config_unusable(error, |code, args, detail| SnapshotsError::Unavailable {
                code,
                args,
                detail,
            })
        })?;
        let repository = config.repository(&alias).ok_or(SnapshotsError::Unknown)?;
        let listing = ListSnapshots::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            ResolveRepository::new(Arc::new(SystemSecrets::default()))
                .telling(Arc::new(log_secret_warning)),
            Arc::clone(&self.listings),
        )
        .list(OperationId::from_uuid(Uuid::now_v7()), &alias, repository)
        .await;
        let listed = match listing {
            Ok(listed) => listed,
            Err(error) => return Err(listing_refusal(&alias, &repository.settings, error).await),
        };
        let snapshots = listed
            .iter()
            .filter_map(|snapshot| {
                snapshot_dto(snapshot)
                    .map_err(|problem| {
                        log::error!(
                            "a snapshot of {} the page cannot show: {problem:?}",
                            alias.as_str()
                        );
                    })
                    .ok()
            })
            .collect();
        Ok(api::SnapshotsDto { snapshots })
    }

    async fn folder(
        &self,
        alias: String,
        snapshot: SnapshotId,
        folder: FolderPath,
        from: u64,
    ) -> Result<api::FolderDto, FolderError> {
        // The repository is refused as its listing is (`snapshots`).
        let unknown = || FolderError::Repository(SnapshotsError::Unknown);
        let alias = RepositoryAlias::parse(&alias).map_err(|_| unknown())?;
        let config = self.config.load().await.map_err(|error| {
            FolderError::Repository(config_unusable(error, |code, args, detail| {
                SnapshotsError::Unavailable { code, args, detail }
            }))
        })?;
        let repository = config.repository(&alias).ok_or_else(unknown)?;
        let path = folder.to_string();
        let read = BrowseSnapshot::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            ResolveRepository::new(Arc::new(SystemSecrets::default()))
                .telling(Arc::new(log_secret_warning)),
            Arc::clone(&self.reads),
        )
        .folder(
            OperationId::from_uuid(Uuid::now_v7()),
            &alias,
            repository,
            snapshot,
            folder,
            from,
        )
        .await;
        let page = match read {
            Ok(page) => page,
            Err(error) => return Err(folder_refusal(&alias, &repository.settings, error).await),
        };
        Ok(api::FolderDto {
            path,
            entries: page.entries.into_iter().map(entry_dto).collect(),
            total: page.total,
            next: page.next,
        })
    }
}

/// A listing or a folder read the engine ran and failed, told by the code of
/// its failure as a read for maintenance tells it (#637): a wrong password is
/// `password-not-accepted`, no repository at the location 404. `None` for a
/// failure in the engine's own words or an engine that can't run, which the
/// caller tells as `engine-unavailable`, its words only in the log.
fn told_failure(reason: &FailureReason) -> Option<SnapshotsError> {
    use http_adapter::MaintenanceRefusal as M;
    match maintenance::failed(reason.clone()) {
        M::Unavailable {
            code: ErrorCode::EngineUnavailable,
            ..
        }
        | M::OperationFailed { .. } => None,
        M::Unavailable { code, args, detail } => {
            Some(SnapshotsError::Unavailable { code, args, detail })
        }
        M::SecretRefused(refusal) => Some(SnapshotsError::SecretRefused(refusal)),
        M::RepositoryNotFound => Some(SnapshotsError::RepositoryNotFound),
        M::RepositoryConfigNotFound { detail } => {
            Some(SnapshotsError::RepositoryConfigNotFound { detail })
        }
        // `failed` gives none of these: they refuse a request before
        // anything runs, or time a read the listing times itself. Each is
        // named, so a refusal added later is decided here and is never told
        // as `engine-unavailable` unnoticed.
        M::UnknownRepository
        | M::NotOwner
        | M::UnknownPlan { .. }
        | M::Busy { .. }
        | M::KeepsNothing { .. }
        | M::AdoptionNoMatch
        | M::AdoptionAmbiguous { .. }
        | M::InvalidBody
        | M::SelectionChanged
        | M::TimedOut => None,
    }
}

/// An entry of a snapshot's folder as the page is told of it.
fn entry_dto(entry: domain::browse::Entry) -> api::EntryDto {
    use domain::browse::EntryKind;
    api::EntryDto {
        name: entry.name.into(),
        display: entry.display.into(),
        kind: match entry.kind {
            EntryKind::Folder => api::EntryKindDto::Folder,
            EntryKind::File => api::EntryKindDto::File,
            EntryKind::Link => api::EntryKindDto::Link,
            EntryKind::Other => api::EntryKindDto::Other,
        },
        size: entry.size,
        modified: entry.modified.map(|at| at.to_string()),
    }
}

/// A configuration the lists can't read, refused as a save of it is.
fn catalog_refusal(error: ConfigError) -> CatalogError {
    config_unusable(error, |code, args, detail| CatalogError::Unavailable {
        code,
        args,
        detail,
    })
}

/// The refusal of a read of a folder of a snapshot of `alias`, configured
/// with `settings`, that failed with `error`. A secret: its code and the
/// words a command or the platform gave, masked of every secret of the
/// repository, in the log as in the answer (I4). A read that ran out of
/// time by its own code; the rest by its code alone, its masked words in
/// the log.
pub(super) async fn folder_refusal(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    error: BrowseError,
) -> FolderError {
    use app::maintenance::MaintenanceError;
    let refused = |refusal: SecretRefusal| {
        log::warn!(
            "a folder of a snapshot of {} could not be read: {:?}{}",
            alias.as_str(),
            refusal.code,
            refusal
                .detail
                .as_deref()
                .map_or_default(|detail| format!(" ({detail})"))
        );
        FolderError::Repository(SnapshotsError::SecretRefused(refusal))
    };
    match error {
        BrowseError::SnapshotUnknown => FolderError::SnapshotUnknown,
        BrowseError::FolderUnknown => FolderError::FolderUnknown,
        BrowseError::Resolve(error) => {
            refused(secret_refusal(&error, settings, &SystemSecrets::default()).await)
        }
        BrowseError::Read(MaintenanceError::Failed(FailureReason::SecretsTooLargeToMask {
            field,
            limit,
        })) => refused(too_much_to_mask(field, limit)),
        BrowseError::Read(MaintenanceError::TimedOut) => {
            log::warn!(
                "a folder of a snapshot of {} could not be read in time",
                alias.as_str()
            );
            FolderError::Repository(SnapshotsError::TimedOut)
        }
        error => {
            if let BrowseError::Read(MaintenanceError::Failed(reason)) = &error
                && let Some(told) = told_failure(reason)
            {
                log::warn!(
                    "a folder of a snapshot of {} could not be read: {told:?}",
                    alias.as_str()
                );
                return FolderError::Repository(told);
            }
            let said = failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&format!("{error:?}"));
            log::warn!(
                "a folder of a snapshot of {} could not be read: {said}",
                alias.as_str()
            );
            FolderError::Repository(SnapshotsError::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: None,
            })
        }
    }
}

/// The refusal of a listing of the snapshots of `alias`, configured with
/// `settings`, that failed with `error`. A secret: its code and the words a
/// command or the platform gave, masked of every secret of the repository,
/// in the log as in the answer (I4). A read that ran out of time by its own
/// code; the rest by its code alone, its masked words in the log.
pub(super) async fn listing_refusal(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    error: SnapshotError,
) -> SnapshotsError {
    let refused = |refusal: SecretRefusal| {
        log::warn!(
            "the snapshots of {} could not be listed: {:?}{}",
            alias.as_str(),
            refusal.code,
            refusal
                .detail
                .as_deref()
                .map_or_default(|detail| format!(" ({detail})"))
        );
        SnapshotsError::SecretRefused(refusal)
    };
    match error {
        SnapshotError::Resolve(error) => {
            refused(secret_refusal(&error, settings, &SystemSecrets::default()).await)
        }
        SnapshotError::Engine(EngineError::SecretsTooLargeToMask { field, limit }) => {
            refused(too_much_to_mask(field, limit))
        }
        SnapshotError::TimedOut => {
            log::warn!(
                "the snapshots of {} could not be listed in time",
                alias.as_str()
            );
            SnapshotsError::TimedOut
        }
        error => {
            if let SnapshotError::Failed(reason) = &error
                && let Some(told) = told_failure(reason)
            {
                log::warn!(
                    "the snapshots of {} could not be listed: {told:?}",
                    alias.as_str()
                );
                return told;
            }
            let said = failure_redactor(settings, &SystemSecrets::default())
                .await
                .redact(&format!("{error:?}"));
            log::warn!(
                "the snapshots of {} could not be listed: {said}",
                alias.as_str()
            );
            SnapshotsError::Unavailable {
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                detail: None,
            }
        }
    }
}

impl ConfigCatalog {
    /// The repositories whose rows offer to create them (decision 53). A log
    /// that can't be read offers none: the list is still shown, and Create
    /// is never offered on a guess.
    async fn offers(
        &self,
        config: &domain::planning::AgentConfig,
    ) -> std::collections::BTreeSet<RepositoryAlias> {
        match CreateOffers::new(Arc::clone(&self.operations))
            .offered(config)
            .await
        {
            Ok(offered) => offered,
            Err(error) => {
                log::warn!(
                    "the operation log could not be read, so no repository is offered to be created: {}",
                    error.code()
                );
                std::collections::BTreeSet::new()
            }
        }
    }
}

/// A listed snapshot as the page is told of it, with what the engine cut
/// from it said.
pub(super) fn snapshot_dto(
    snapshot: &SnapshotSummary,
) -> Result<api::SnapshotDto, api::InvalidSnapshot> {
    api::SnapshotDto::new(
        snapshot.id.as_str().to_owned(),
        snapshot.time.to_string(),
        snapshot.hostname.clone(),
        snapshot.label.clone(),
        snapshot.tags.clone(),
        snapshot.paths.clone(),
    )
    .map(|dto| {
        dto.truncated(snapshot.paths_truncated, snapshot.tags_truncated)
            .taken_at_offset(snapshot.offset)
    })
}

/// The Repositories page's note on `repository` while `link`, the link the
/// daemon last read, is mixed and the entry has a rule (decision 80): the
/// kind of link each family's traffic leaves by, never an interface's name.
pub(super) fn mixed_link_note(
    repository: &RepositoryConfig,
    link: Option<Link>,
) -> Option<api::WarningDto> {
    let kind = |kind| {
        text(match kind {
            InterfaceKind::Wired => "wired",
            InterfaceKind::WiFi => "wifi",
            InterfaceKind::Other => "other",
        })
    };
    repository
        .link_note(link)
        .map(|MixedLinkNote { ipv4, ipv6 }| api::WarningDto {
            code: api::WarningCode::LocationRuleMixedLink,
            args: [
                ("ipv4".to_owned(), kind(ipv4)),
                ("ipv6".to_owned(), kind(ipv6)),
                ("locations".to_owned(), text(repository.locations_code())),
            ]
            .into(),
        })
}

/// A configured entry as the repositories list shows it: its row, with the
/// secrets each further location writes as text by its position; for an
/// entry reached at several locations, each location in order with its
/// rule; a warning for each location that sends its credentials where an
/// impostor could read them, by position, cause and whether it has a rule,
/// never the address; and the notes about what the locations' rules mean
/// for `plans`, the plans that back up to it (decision 80). The warnings
/// and notes of a repository of one location say so, for its row lists no
/// locations.
pub(super) fn listed_repository(
    alias: &RepositoryAlias,
    repository: &RepositoryConfig,
    plans: &[&Plan],
    reads_names: bool,
    create_offered: bool,
) -> api::RepositoryDto {
    let mut row = repository_row(
        alias,
        &repository.settings,
        repository.maintenance,
        create_offered,
    );
    let dto = |code, args: Vec<(&'static str, MsgArg)>| api::WarningDto {
        code,
        args: args
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    };
    // The row holds the first location's secrets already.
    row.warnings.extend(
        inline_secret_warnings(repository)
            .into_iter()
            .filter(|(position, _, _)| *position > 1)
            .map(|(_, code, args)| dto(code, args)),
    );
    row.warnings
        .extend(repository.location_warnings().into_iter().map(|warning| {
            dto(
                api::WarningCode::LocationPlainHttpCredentials,
                exposed_args(repository, warning),
            )
        }));
    let locations = || ("locations".to_owned(), text(repository.locations_code()));
    row.warnings.extend(
        repository
            .rule_notes(plans, reads_names)
            .into_iter()
            .map(|note| {
                let (code, args) = match note {
                    RuleNote::EveryLocationRuled => {
                        (api::WarningCode::LocationEveryRuled, vec![locations()])
                    }
                    RuleNote::LastRuledWithoutRetry { plan } => (
                        api::WarningCode::LocationLastRuledNoRetry,
                        vec![("plan".to_owned(), text(plan.as_str())), locations()],
                    ),
                    RuleNote::NameUnreadable { position } => (
                        api::WarningCode::LocationRuleNameUnreadable,
                        vec![("position".to_owned(), text(position)), locations()],
                    ),
                };
                api::WarningDto {
                    code,
                    args: args.into_iter().collect(),
                }
            }),
    );
    if repository.has_several_locations() {
        let first = Location {
            when: repository.locations.first_when.clone(),
            ..Location::of(&repository.settings)
        };
        row.locations = std::iter::once(first)
            .chain(repository.locations.further.iter().cloned())
            .zip(1u8..)
            .map(|(location, position)| listed_location(&location, position))
            .collect();
    }
    row
}

/// One location of an entry as the list shows it: its address masked, and
/// each option by name, with its value only when that is references alone.
fn listed_location(location: &Location, position: u8) -> api::LocationDto {
    let options = [
        ("options", &location.options),
        ("options-hot", &location.options_hot),
        ("options-cold", &location.options_cold),
    ]
    .into_iter()
    .flat_map(|(set, options)| {
        options
            .iter()
            .map(move |(name, value)| api::LocationOptionDto {
                set: set.to_owned(),
                name: name.clone(),
                reference: domain::secrets::only_references(value).then(|| value.clone()),
            })
    })
    .collect();
    api::LocationDto {
        position,
        repository: location.masked_address(),
        repo_hot: location.masked_repo_hot(),
        options,
        when: location
            .when
            .iter()
            .flat_map(LocationRule::alternatives)
            .map(|alternative| {
                let names = |filter: Option<&NameFilter>| {
                    filter.map_or_default(|filter| {
                        filter
                            .names()
                            .iter()
                            .map(|name| name.as_str().to_owned())
                            .collect()
                    })
                };
                let filter = alternative.names();
                api::LocationAlternativeDto {
                    network: alternative.kind().as_str().to_owned(),
                    only: names(filter.filter(|filter| matches!(filter, NameFilter::Only(_)))),
                    except: names(filter.filter(|filter| matches!(filter, NameFilter::Except(_)))),
                }
            })
            .collect(),
    }
}

/// One configured repository as a list shows it: its locations masked, and
/// where its password comes from, never the password (I4). `create_offered`
/// is whether its row offers to create it (decision 53).
pub(super) fn repository_row(
    alias: &RepositoryAlias,
    settings: &RepositorySettings,
    maintenance: domain::planning::Maintenance,
    create_offered: bool,
) -> api::RepositoryDto {
    api::RepositoryDto {
        alias: alias.as_str().to_owned(),
        maintained_here: domain::maintenance::Ownership::owned_here(maintenance),
        create_offered,
        repository: redacted_location(settings, &settings.repository),
        repo_hot: settings
            .repo_hot
            .as_deref()
            .map(|hot| redacted_location(settings, hot)),
        password: match &settings.password {
            SecretRef::Inline(_) => api::PasswordSourceDto::Inline,
            SecretRef::File { path, .. } => api::PasswordSourceDto::File { path: path.clone() },
            SecretRef::Keyring(entry) => api::PasswordSourceDto::Keyring {
                entry: entry.to_string(),
            },
            SecretRef::Env(name) => api::PasswordSourceDto::Env { name: name.clone() },
            // The program only: its arguments may hold the secret itself.
            SecretRef::Command(command) => api::PasswordSourceDto::Command {
                program: command.program().to_owned(),
            },
        },
        warnings: settings
            .inline_secrets()
            .iter()
            .map(|secret| {
                let (code, args) = inline_secret_warning(settings, secret, 1);
                api::WarningDto {
                    code,
                    args: args
                        .into_iter()
                        .map(|(name, value)| (name.to_owned(), value))
                        .collect(),
                }
            })
            .collect(),
        locations: Vec::new(),
    }
}
