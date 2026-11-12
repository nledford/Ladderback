//! The maintenance routes' side of the wiring (technical design §4.2, §7.3):
//! a repository's history grouped by owner, a preview of a run and the start
//! of one, from the use case of `app` to the DTOs and the typed refusals the
//! routes answer with. `app` decides what a run does; this only says what it
//! decided, and why when it refused.

use std::sync::Arc;

use api::{ErrorCode, MsgArg};
use app::maintenance::{
    Adoption, AdoptionSource, MaintenanceError, MaintenanceKind, MaintenancePreview,
    RepositoryGroups, RunMaintenance,
};
use app::ports::{ConfigStore, StoreError};
use app::secrets::ResolveError;
use domain::execution::FailureReason;
use domain::maintenance::{OtherHosts, SelectionToken, Snapshot, Tally};
use domain::{OperationId, PlanId, RepositoryAlias};
use http_adapter::{MaintenanceRefusal, MaintenanceRequest};
use platform::SystemClock;
use secrets::SystemSecrets;
use uuid::Uuid;

use super::{PlanRunner, secret_refusal};

impl PlanRunner {
    /// The maintenance use case over this runner's parts.
    fn maintenance(&self) -> RunMaintenance {
        RunMaintenance::new(
            Arc::clone(&self.engine),
            Arc::new(SystemClock),
            Arc::clone(&self.store),
            self.resolve.as_ref().clone(),
            Arc::clone(&self.retention),
            Arc::clone(&self.config),
            self.agent_id,
            platform::hostname(),
            Arc::new(engine::RusticSources),
        )
        .with_events(Arc::clone(&self.events))
    }
}

/// The repository a request names. An alias no repository could have is as
/// unknown as one the configuration happens not to hold.
fn alias_of(repository: &str) -> Result<RepositoryAlias, MaintenanceRefusal> {
    RepositoryAlias::parse(repository).map_err(|_| MaintenanceRefusal::UnknownRepository)
}

/// The run a request asks for. An adoption's destination that no plan could
/// be called is as unknown as a plan the configuration happens not to hold.
fn kind_of(request: MaintenanceRequest) -> Result<MaintenanceKind, MaintenanceRefusal> {
    Ok(match request {
        MaintenanceRequest::Forget => MaintenanceKind::Forget,
        MaintenanceRequest::Prune => MaintenanceKind::Prune,
        MaintenanceRequest::Check => MaintenanceKind::Check,
        MaintenanceRequest::Adopt { into, from } => {
            let Ok(into_plan) = PlanId::parse(&into) else {
                return Err(MaintenanceRefusal::UnknownPlan { plan: into });
            };
            MaintenanceKind::Adopt(Adoption {
                from: match from {
                    api::AdoptionFromDto::Orphaned { agent, plan } => {
                        AdoptionSource::Orphaned { agent, plan }
                    }
                    api::AdoptionFromDto::Unmanaged => AdoptionSource::Unmanaged,
                },
                into: into_plan,
            })
        }
    })
}

/// The selection a request carries, read back from the text a preview named
/// it by. Text that is no selection is a body no run can take.
fn held_of(selection: &str) -> Result<SelectionToken, MaintenanceRefusal> {
    SelectionToken::parse(selection).ok_or(MaintenanceRefusal::InvalidBody)
}

/// Starts the run `request` asks for on `repository`, held to `selection` when
/// there is one, and answers with the operation's ID. The run happens on a
/// task of its own and reports through the event bus (§4.2).
///
/// The reads come first, on the request's own future, the check of the held
/// selection among them: they store nothing and take no slot, so a client
/// that hangs up meanwhile has left nothing behind. The start writes the log,
/// and a write dropped after SQLite committed it leaves a running row that
/// holds the repository (I7) and that nothing in this process ends, so the
/// start happens on a task of its own, which the client's hanging up can't
/// drop, and the route waits for that task's answer.
pub(super) async fn start(
    runner: &PlanRunner,
    repository: String,
    request: MaintenanceRequest,
    selection: Option<String>,
) -> Result<String, MaintenanceRefusal> {
    let alias = alias_of(&repository)?;
    let kind = kind_of(request)?;
    let held = selection.as_deref().map(held_of).transpose()?;
    let use_case = runner.maintenance();
    let operation = OperationId::from_uuid(Uuid::now_v7());
    let assessed = match &held {
        Some(token) => {
            use_case
                .assess_previewed(operation, &alias, kind, token)
                .await
        }
        None => use_case.assess(operation, &alias, kind).await,
    };
    let assessment = told(runner, &alias, assessed).await?;
    log_other_hosts(&alias, assessment.other_hosts());
    let (started, wait_for_start) = futures::channel::oneshot::channel();
    let registry = Arc::clone(&runner.registry);
    let config = Arc::clone(&runner.config);
    tokio::spawn(async move {
        let prepared = match use_case.start(assessment).await {
            Ok(prepared) => prepared,
            Err(refused) => {
                let _ = started.send(Err(refusal(config.as_ref(), &alias, refused).await));
                return;
            }
        };
        // The cancel switch is parked before the route answers, not lazily
        // when the run first polls it, so a cancel that arrives the instant
        // the route answers finds it (§4.2).
        let cancel = registry.register(operation);
        let cancel = async move {
            let _ = cancel.await;
        };
        let _ = started.send(Ok(()));
        let finished = use_case.execute(prepared, cancel).await;
        registry.finish(&operation);
        super::log_ended(&finished);
    });
    // A task that ended without answering has not started anything the route
    // could name.
    match wait_for_start.await {
        Ok(answer) => answer.map(|()| operation.as_uuid().hyphenated().to_string()),
        Err(_) => Err(unavailable(ErrorCode::OperationFailed, Vec::new(), None)),
    }
}

/// What `request` would do to `repository`, told as the page reads it. A read:
/// it takes no maintenance slot and writes nothing to the log.
pub(super) async fn preview(
    runner: &PlanRunner,
    repository: String,
    request: MaintenanceRequest,
) -> Result<api::MaintenancePreviewDto, MaintenanceRefusal> {
    let alias = alias_of(&repository)?;
    let kind = kind_of(request)?;
    let operation = OperationId::from_uuid(Uuid::now_v7());
    let preview = runner.maintenance().preview(operation, &alias, &kind).await;
    let preview = told(runner, &alias, preview).await?;
    Ok(preview_dto(&alias, &preview.preview).warned(other_hosts_warnings(&preview.other_hosts)))
}

/// `repository`'s snapshots grouped by who owns them, read live like a
/// snapshot listing.
pub(super) async fn groups(
    runner: &PlanRunner,
    repository: String,
) -> Result<api::SnapshotGroupsDto, MaintenanceRefusal> {
    let alias = alias_of(&repository)?;
    let operation = OperationId::from_uuid(Uuid::now_v7());
    let groups = runner.maintenance().groups(operation, &alias).await;
    let groups = told(runner, &alias, groups).await?;
    Ok(groups_dto(&alias, &groups))
}

/// A snapshot as the page is told of it. One the wire can't carry is left
/// out of its list (and still counted there), as the snapshot listing does.
fn snapshot_dto(alias: &RepositoryAlias, snapshot: &Snapshot) -> Option<api::SnapshotDto> {
    api::SnapshotDto::new(
        snapshot.id.as_str().to_owned(),
        snapshot.time.to_string(),
        snapshot.hostname.clone(),
        snapshot.label.clone(),
        snapshot.tags.clone(),
        snapshot.paths.clone(),
    )
    .map(|dto| dto.taken_at_offset(snapshot.offset))
    .map_err(|problem| {
        log::error!(
            "a snapshot of {} the page cannot show: {problem:?}",
            alias.as_str()
        );
    })
    .ok()
}

fn plan_names(plans: &[PlanId]) -> Vec<String> {
    plans.iter().map(|plan| plan.as_str().to_owned()).collect()
}

/// A count as the wire carries it: saturated, never wrapped.
fn counted(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

fn tally_dto(tally: Tally) -> api::TallyDto {
    api::TallyDto {
        groups: counted(tally.groups),
        snapshots: counted(tally.snapshots),
    }
}

/// The groups of `groups` as the page reads them: each list shows the first
/// of its snapshots and counts them all.
fn groups_dto(alias: &RepositoryAlias, groups: &RepositoryGroups) -> api::SnapshotGroupsDto {
    let shown = |snapshot: &Snapshot| snapshot_dto(alias, snapshot);
    api::SnapshotGroupsDto {
        plans: groups
            .plans
            .iter()
            .map(|history| {
                api::PlanGroupDto::new(history.plan.as_str().to_owned(), &history.snapshots, shown)
            })
            .collect(),
        orphaned: groups
            .orphaned
            .iter()
            .map(|history| {
                api::OrphanedGroupDto::new(
                    history.group.agent.clone(),
                    history.group.plan.clone(),
                    &history.group.snapshots,
                    plan_names(&history.adopt_into),
                    shown,
                )
            })
            .collect(),
        unmanaged: groups
            .unmanaged
            .iter()
            .map(|history| {
                api::UnmanagedGroupDto::new(
                    history.group.hostname.clone(),
                    history.group.label.clone(),
                    history.group.paths.clone(),
                    &history.group.snapshots,
                    plan_names(&history.adopt_into),
                    shown,
                )
            })
            .collect(),
        warnings: other_hosts_warnings(&groups.other_hosts),
    }
}

/// What the page warns of the other hosts a repository maintained here
/// holds snapshots from (§7.3): nothing when there are none.
fn other_hosts_warnings(other_hosts: &OtherHosts) -> Vec<api::WarningDto> {
    if other_hosts.is_empty() {
        Vec::new()
    } else {
        vec![api::WarningDto::other_hosts(
            other_hosts.count(),
            other_hosts.hosts(),
        )]
    }
}

/// Says in the daemon's log, in the catalog's words, that a run of a
/// repository maintained here goes ahead beside snapshots from other hosts,
/// any of which may maintain it too (I1, §7.3).
fn log_other_hosts(alias: &RepositoryAlias, other_hosts: &OtherHosts) {
    if !other_hosts.is_empty() {
        let localizer = l10n::Localizer::new(&l10n::PosixEnv::from_process().preferences());
        log::warn!("{}", other_hosts_line(alias, other_hosts, &localizer));
    }
}

/// The line [`log_other_hosts`] writes. A host name is whatever a snapshot
/// in the repository records, so it is escaped: a control character in one
/// can't start a line of its own or forge another.
fn other_hosts_line(
    alias: &RepositoryAlias,
    other_hosts: &OtherHosts,
    localizer: &l10n::Localizer,
) -> String {
    let escaped: Vec<String> = other_hosts
        .hosts()
        .iter()
        .map(|host| host.escape_debug().to_string())
        .collect();
    let warning = api::WarningDto::other_hosts(other_hosts.count(), &escaped);
    let args: Vec<(&str, MsgArg)> = warning
        .args
        .iter()
        .map(|(name, value)| (name.as_str(), value.clone()))
        .collect();
    let message = localizer.warning(warning.code, &args, None);
    format!("repository {}: {message}", alias.as_str())
}

/// A preview as the page reads it. The selection of a forget's and an
/// adoption's is the token a start can be held to.
fn preview_dto(
    alias: &RepositoryAlias,
    preview: &MaintenancePreview,
) -> api::MaintenancePreviewDto {
    let shown = |snapshot: &Snapshot| snapshot_dto(alias, snapshot);
    match preview {
        MaintenancePreview::Forget(forget) => api::MaintenancePreviewDto::Forget {
            plans: forget
                .plans
                .iter()
                .map(|plan| {
                    api::ForgetPlanDto::new(
                        plan.plan.as_str().to_owned(),
                        counted(plan.total),
                        &plan.remove,
                        shown,
                    )
                })
                .collect(),
            left_alone: api::LeftAloneDto {
                orphaned: tally_dto(forget.left_alone.orphaned),
                unmanaged: tally_dto(forget.left_alone.unmanaged),
            },
            selection: forget.selection().as_str().to_owned(),
            warnings: Vec::new(),
        },
        MaintenancePreview::Adopt(adopt) => api::MaintenancePreviewDto::adopt(
            adopt.into.as_str().to_owned(),
            &adopt.snapshots,
            adopt.selection().as_str().to_owned(),
            shown,
        ),
        MaintenancePreview::Prune(prune) => api::MaintenancePreviewDto::Prune {
            packs_marked: prune.packs_marked,
            packs_deleted: prune.packs_deleted,
            packs_recovered: prune.packs_recovered,
            bytes_deleted: prune.bytes_deleted,
            // A `keep-delete` is never shorter than rustic's own minimum, so
            // it is never negative.
            keep_delete_hours: u64::try_from(prune.keep_delete.get().as_hours()).unwrap_or(0),
            warnings: Vec::new(),
        },
        MaintenancePreview::Check(check) => api::MaintenancePreviewDto::Check {
            read_data: check.read_data,
        },
    }
}

/// The daemon can't do it now: the code and the arguments its message
/// declares, and the platform's own words when there are some.
fn unavailable(
    code: ErrorCode,
    args: Vec<(&'static str, MsgArg)>,
    detail: Option<String>,
) -> MaintenanceRefusal {
    MaintenanceRefusal::Unavailable { code, args, detail }
}

fn text(value: impl std::fmt::Display) -> MsgArg {
    MsgArg::Text(value.to_string())
}

/// What `result` came to: an error is told as the typed refusal the route
/// answers with.
async fn told<T>(
    runner: &PlanRunner,
    repository: &RepositoryAlias,
    result: Result<T, MaintenanceError>,
) -> Result<T, MaintenanceRefusal> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(refusal(runner.config.as_ref(), repository, error).await),
    }
}

/// Why a run was refused or a read failed, as the typed refusal the route
/// answers with. A secret that couldn't be resolved is told as the repository
/// routes tell it, by the code of its cause, with what a command or the
/// platform said masked of every secret of `repository`'s settings (I4): those
/// are read from the configuration as it stands now.
async fn refusal(
    config: &dyn ConfigStore,
    repository: &RepositoryAlias,
    error: MaintenanceError,
) -> MaintenanceRefusal {
    match error {
        MaintenanceError::UnknownRepository => MaintenanceRefusal::UnknownRepository,
        MaintenanceError::NotOwner => MaintenanceRefusal::NotOwner,
        // The configuration is refused as a save of it is (#347).
        MaintenanceError::Unavailable(error) => super::config_unusable(*error, unavailable),
        MaintenanceError::Resolve(error) => secret_refused(config, repository, &error).await,
        MaintenanceError::Engine { detail } => {
            unavailable(ErrorCode::EngineUnavailable, Vec::new(), Some(detail))
        }
        MaintenanceError::Failed(reason) => failed(reason),
        MaintenanceError::TimedOut => MaintenanceRefusal::TimedOut,
        MaintenanceError::KeepsNothing { plan } => MaintenanceRefusal::KeepsNothing {
            plan: plan.as_str().to_owned(),
        },
        MaintenanceError::UnknownPlan { plan } => MaintenanceRefusal::UnknownPlan {
            plan: plan.as_str().to_owned(),
        },
        MaintenanceError::AdoptionNoMatch => MaintenanceRefusal::AdoptionNoMatch,
        MaintenanceError::AdoptionAmbiguous { plans } => MaintenanceRefusal::AdoptionAmbiguous {
            plans: plan_names(&plans),
        },
        MaintenanceError::Busy { running } => MaintenanceRefusal::Busy {
            running: running.as_uuid().hyphenated().to_string(),
        },
        // The operation log: its own words say what it couldn't do.
        MaintenanceError::Store(
            StoreError::Unavailable { detail } | StoreError::Corrupt { detail },
        ) => unavailable(ErrorCode::OperationFailed, Vec::new(), Some(detail)),
        MaintenanceError::SelectionChanged => MaintenanceRefusal::SelectionChanged,
        // The core names no sentence for it: the catalog's plainest words.
        MaintenanceError::InvalidRetention { .. } => {
            unavailable(ErrorCode::OperationFailed, Vec::new(), None)
        }
    }
}

/// A secret of `repository`'s settings that couldn't be resolved, as the
/// refusal the route answers with. A configuration that can't be read now is
/// refused as a save of it is (#347), and a repository the configuration no
/// longer holds is unknown.
async fn secret_refused(
    config: &dyn ConfigStore,
    repository: &RepositoryAlias,
    error: &ResolveError,
) -> MaintenanceRefusal {
    let loaded = match config.load().await {
        Ok(loaded) => loaded,
        Err(unusable) => return super::config_unusable(unusable, unavailable),
    };
    match loaded.repository(repository) {
        Some(found) => MaintenanceRefusal::SecretRefused(
            secret_refusal(error, &found.settings, &SystemSecrets::default()).await,
        ),
        None => MaintenanceRefusal::UnknownRepository,
    }
}

/// A read of the repository that ran and failed: the engine's own words for a
/// failure it reports, a typed code for the rest. The engine redacts what it
/// says.
fn failed(reason: FailureReason) -> MaintenanceRefusal {
    use FailureReason as F;
    match reason {
        F::EngineFailed { detail } => MaintenanceRefusal::OperationFailed { detail },
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
        F::EngineStartTimedOut => unavailable(ErrorCode::EngineStartTimedOut, Vec::new(), None),
        F::EngineExitTimedOut => unavailable(ErrorCode::EngineExitTimedOut, Vec::new(), None),
        // A read of the repository runs no hooks and creates nothing: these
        // are still failures, in the catalog's plainest words.
        F::HookFailed { .. } | F::RepositoryExists => {
            unavailable(ErrorCode::OperationFailed, Vec::new(), None)
        }
        F::RepositoryReplaced => unavailable(ErrorCode::RepositoryReplaced, Vec::new(), None),
        F::RepositoryNotFound => MaintenanceRefusal::RepositoryNotFound,
        F::SecretsTooLargeToMask { field, limit } => {
            MaintenanceRefusal::SecretRefused(super::too_much_to_mask(field, limit))
        }
        // Only an operation of the log can be ended by a stopped daemon, and a
        // read of the repository is not one; the failure has its code all the
        // same, as it has everywhere it is told.
        F::DaemonStopped => unavailable(ErrorCode::DaemonStopped, Vec::new(), None),
    }
}

#[cfg(test)]
mod tests {
    use app::fakes::InMemoryConfigStore;
    use app::maintenance::{
        AdoptPreview, CheckPreview, OrphanedHistory, PlanHistory, PrunePreview, UnmanagedHistory,
        default_keep_delete,
    };
    use app::ports::ConfigError;
    use app::secrets::{Cause, Field, ResolveError};
    use async_trait::async_trait;
    use domain::execution::Transition;
    use domain::execution::deadline::KeepDelete;
    use domain::maintenance::{
        ForgetPreview, LeftAlone, OrphanedGroup, PlanForget, UnmanagedGroup,
    };
    use domain::{SignedDuration, SnapshotId};
    use http_adapter::SecretRefusal;

    use super::*;

    fn alias() -> RepositoryAlias {
        RepositoryAlias::parse("demo").expect("an alias")
    }

    fn plan(name: &str) -> PlanId {
        PlanId::parse(name).expect("a plan ID")
    }

    fn snapshot(n: u32) -> Snapshot {
        Snapshot {
            id: SnapshotId::parse(&format!("{n:064x}")).expect("a snapshot ID"),
            time: "2026-11-09T10:15:00Z".parse().expect("a time"),
            offset: domain::Offset::from_seconds(-6 * 3600).expect("an offset"),
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["demo".to_owned()],
            paths: vec!["/home/demo".to_owned()],
        }
    }

    /// Snapshot `n` as the page is told of it.
    fn shown(n: u32) -> api::SnapshotDto {
        snapshot_dto(&alias(), &snapshot(n)).expect("a snapshot the page can show")
    }

    #[test]
    fn a_snapshot_is_told_with_every_field_the_page_shows() {
        assert_eq!(
            snapshot_dto(&alias(), &snapshot(1)),
            Some(api::SnapshotDto {
                id: format!("{:064x}", 1),
                time: "2026-11-09T10:15:00Z".to_owned(),
                offset: Some(api::ClockOffset::new(
                    domain::Offset::from_seconds(-6 * 3600).expect("an offset")
                )),
                hostname: "demo-host".to_owned(),
                label: "home".to_owned(),
                tags: vec!["demo".to_owned()],
                paths: vec!["/home/demo".to_owned()],
                paths_truncated: false,
                tags_truncated: false,
            })
        );
    }

    #[test]
    fn a_host_name_in_the_log_is_escaped_so_it_cannot_forge_a_line() {
        let settings = domain::planning::RepositorySettings {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: domain::secrets::SecretRef::Env("DEMO_PASSWORD".to_owned()),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        };
        let config = domain::planning::AgentConfig::new(
            domain::planning::AgentSettings::default(),
            domain::planning::Hooks::default(),
            std::collections::BTreeMap::from([(
                alias(),
                domain::planning::RepositoryConfig {
                    id: None,
                    settings,
                    maintenance: domain::planning::Maintenance::Local,
                    group_by: Default::default(),
                    hooks: Default::default(),
                },
            )]),
            Vec::new(),
        )
        .expect("a valid configuration");
        let forged = Snapshot {
            hostname: "demo-nas\nrepository demo: all is well".to_owned(),
            ..snapshot(1)
        };
        let other_hosts = OtherHosts::of(
            &config,
            &alias(),
            &domain::AgentId::from_uuid(Uuid::from_u128(1)),
            "demo-laptop",
            forged.time,
            [&forged],
        );
        let line = other_hosts_line(&alias(), &other_hosts, &l10n::Localizer::new(&[]));
        assert!(line.starts_with("repository demo: "), "{line}");
        assert!(!line.contains('\n'), "{line}");
        assert!(
            line.contains(r"demo-nas\nrepository demo: all is well"),
            "{line}"
        );
    }

    #[test]
    fn the_groups_are_told_by_owner_with_where_each_could_be_adopted() {
        let groups = RepositoryGroups {
            plans: vec![
                PlanHistory {
                    plan: plan("home"),
                    snapshots: vec![snapshot(2), snapshot(1)],
                },
                PlanHistory {
                    plan: plan("mirror"),
                    snapshots: Vec::new(),
                },
            ],
            orphaned: vec![OrphanedHistory {
                group: OrphanedGroup {
                    agent: Some("agent-2".to_owned()),
                    plan: None,
                    snapshots: vec![snapshot(3)],
                },
                adopt_into: vec![plan("home"), plan("mirror")],
            }],
            unmanaged: vec![UnmanagedHistory {
                group: UnmanagedGroup {
                    hostname: "demo-host".to_owned(),
                    label: "docs".to_owned(),
                    paths: vec!["/home/demo/docs".to_owned()],
                    snapshots: vec![snapshot(4)],
                },
                adopt_into: vec![plan("home")],
            }],
            other_hosts: Default::default(),
        };
        assert_eq!(
            groups_dto(&alias(), &groups),
            api::SnapshotGroupsDto {
                plans: vec![
                    api::PlanGroupDto {
                        plan: "home".to_owned(),
                        count: 2,
                        snapshots: vec![shown(2), shown(1)],
                    },
                    api::PlanGroupDto {
                        plan: "mirror".to_owned(),
                        count: 0,
                        snapshots: Vec::new(),
                    },
                ],
                orphaned: vec![api::OrphanedGroupDto {
                    agent: Some("agent-2".to_owned()),
                    plan: None,
                    count: 1,
                    snapshots: vec![shown(3)],
                    adopt_into: vec!["home".to_owned(), "mirror".to_owned()],
                }],
                unmanaged: vec![api::UnmanagedGroupDto {
                    hostname: "demo-host".to_owned(),
                    label: "docs".to_owned(),
                    paths: vec!["/home/demo/docs".to_owned()],
                    count: 1,
                    snapshots: vec![shown(4)],
                    adopt_into: vec!["home".to_owned()],
                }],
                warnings: Vec::new(),
            }
        );
    }

    #[test]
    fn a_group_counts_every_snapshot_and_lists_the_first_of_them() {
        let held = api::MAX_GROUP_SNAPSHOTS + 1;
        let groups = RepositoryGroups {
            plans: vec![PlanHistory {
                plan: plan("home"),
                snapshots: (0..u32::try_from(held).expect("a small number"))
                    .map(snapshot)
                    .collect(),
            }],
            ..RepositoryGroups::default()
        };
        let told = groups_dto(&alias(), &groups);
        let group = told.plans.first().expect("the plan's group");
        assert_eq!(group.count, 501);
        assert_eq!(group.snapshots.len(), api::MAX_GROUP_SNAPSHOTS);
        assert_eq!(group.snapshots.first(), Some(&shown(0)));
    }

    #[test]
    fn a_snapshot_the_page_cannot_show_is_left_out_and_still_counted() {
        let mut cannot = snapshot(1);
        cannot.tags = (0..=api::MAX_SNAPSHOT_TAGS)
            .map(|n| format!("tag-{n}"))
            .collect();
        assert_eq!(snapshot_dto(&alias(), &cannot), None);
        let groups = RepositoryGroups {
            plans: vec![PlanHistory {
                plan: plan("home"),
                snapshots: vec![cannot, snapshot(2)],
            }],
            ..RepositoryGroups::default()
        };
        let told = groups_dto(&alias(), &groups);
        let group = told.plans.first().expect("the plan's group");
        assert_eq!(group.count, 2);
        assert_eq!(group.snapshots, [shown(2)]);
    }

    #[test]
    fn a_forget_preview_is_told_with_its_selection_and_what_it_leaves_alone() {
        let forget = ForgetPreview {
            plans: vec![PlanForget {
                plan: plan("home"),
                total: 3,
                remove: vec![snapshot(2), snapshot(1)],
            }],
            left_alone: LeftAlone {
                orphaned: Tally {
                    groups: 1,
                    snapshots: 4,
                },
                unmanaged: Tally {
                    groups: 2,
                    snapshots: 5,
                },
            },
        };
        let selection = SelectionToken::of(&[snapshot(2).id, snapshot(1).id]);
        assert_eq!(forget.selection(), selection);
        assert_eq!(
            preview_dto(&alias(), &MaintenancePreview::Forget(forget)),
            api::MaintenancePreviewDto::Forget {
                plans: vec![api::ForgetPlanDto {
                    plan: "home".to_owned(),
                    total: 3,
                    remove: 2,
                    listed: vec![shown(2), shown(1)],
                }],
                left_alone: api::LeftAloneDto {
                    orphaned: api::TallyDto {
                        groups: 1,
                        snapshots: 4,
                    },
                    unmanaged: api::TallyDto {
                        groups: 2,
                        snapshots: 5,
                    },
                },
                selection: selection.as_str().to_owned(),
                warnings: Vec::new(),
            }
        );
    }

    #[test]
    fn an_adoption_preview_is_told_with_its_selection() {
        let adopt = AdoptPreview {
            into: plan("home"),
            snapshots: vec![snapshot(2), snapshot(1)],
        };
        let selection =
            SelectionToken::of_adoption(&plan("home"), &[snapshot(2).id, snapshot(1).id]);
        assert_eq!(adopt.selection(), selection);
        assert_eq!(
            preview_dto(&alias(), &MaintenancePreview::Adopt(adopt)),
            api::MaintenancePreviewDto::Adopt {
                into: "home".to_owned(),
                count: 2,
                listed: vec![shown(2), shown(1)],
                selection: selection.as_str().to_owned(),
                warnings: Vec::new(),
            }
        );
    }

    #[test]
    fn a_prune_preview_says_how_long_a_pack_stays_marked_in_hours() {
        let prune = |keep_delete| {
            MaintenancePreview::Prune(PrunePreview {
                packs_marked: 3,
                packs_deleted: 2,
                packs_recovered: 1,
                bytes_deleted: 4096,
                keep_delete,
            })
        };
        for (keep_delete, hours) in [
            (default_keep_delete(), 144),
            (
                KeepDelete::new(SignedDuration::from_hours(25)).expect("a long enough time"),
                25,
            ),
        ] {
            assert_eq!(
                preview_dto(&alias(), &prune(keep_delete)),
                api::MaintenancePreviewDto::Prune {
                    packs_marked: 3,
                    packs_deleted: 2,
                    packs_recovered: 1,
                    bytes_deleted: 4096,
                    keep_delete_hours: hours,
                    warnings: Vec::new(),
                }
            );
        }
    }

    #[test]
    fn a_check_preview_says_whether_it_reads_data() {
        for read_data in [false, true] {
            assert_eq!(
                preview_dto(
                    &alias(),
                    &MaintenancePreview::Check(CheckPreview { read_data })
                ),
                api::MaintenancePreviewDto::Check { read_data }
            );
        }
    }

    #[test]
    fn a_request_becomes_the_run_it_asks_for() {
        assert_eq!(
            kind_of(MaintenanceRequest::Forget),
            Ok(MaintenanceKind::Forget)
        );
        assert_eq!(
            kind_of(MaintenanceRequest::Prune),
            Ok(MaintenanceKind::Prune)
        );
        assert_eq!(
            kind_of(MaintenanceRequest::Check),
            Ok(MaintenanceKind::Check)
        );
        assert_eq!(
            kind_of(MaintenanceRequest::Adopt {
                into: "home".to_owned(),
                from: api::AdoptionFromDto::Orphaned {
                    agent: "agent-2".to_owned(),
                    plan: "old".to_owned(),
                },
            }),
            Ok(MaintenanceKind::Adopt(Adoption {
                from: AdoptionSource::Orphaned {
                    agent: "agent-2".to_owned(),
                    plan: "old".to_owned(),
                },
                into: plan("home"),
            }))
        );
        assert_eq!(
            kind_of(MaintenanceRequest::Adopt {
                into: "home".to_owned(),
                from: api::AdoptionFromDto::Unmanaged,
            }),
            Ok(MaintenanceKind::Adopt(Adoption {
                from: AdoptionSource::Unmanaged,
                into: plan("home"),
            }))
        );
    }

    #[test]
    fn an_adoption_into_a_plan_no_plan_could_be_called_is_unknown() {
        assert_eq!(
            kind_of(MaintenanceRequest::Adopt {
                into: "Not A Plan".to_owned(),
                from: api::AdoptionFromDto::Unmanaged,
            }),
            Err(MaintenanceRefusal::UnknownPlan {
                plan: "Not A Plan".to_owned()
            })
        );
    }

    #[test]
    fn a_repository_no_alias_could_name_is_unknown() {
        assert_eq!(alias_of("demo"), Ok(alias()));
        assert_eq!(
            alias_of("Not An Alias"),
            Err(MaintenanceRefusal::UnknownRepository)
        );
    }

    #[test]
    fn only_the_text_of_a_selection_is_a_selection() {
        let held = held_of("0123456789abcdef").expect("a selection");
        assert_eq!(held.as_str(), "0123456789abcdef");
        for text in ["", "short", "0123456789ABCDEF", "0123456789abcdeg"] {
            assert_eq!(
                held_of(text),
                Err(MaintenanceRefusal::InvalidBody),
                "{text:?}"
            );
        }
    }

    fn running() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(3))
    }

    fn unavailable_with(
        code: ErrorCode,
        args: Vec<(&'static str, MsgArg)>,
        detail: Option<&str>,
    ) -> MaintenanceRefusal {
        MaintenanceRefusal::Unavailable {
            code,
            args,
            detail: detail.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn a_repository_this_agent_does_not_maintain_is_told_as_the_route_answers_it() {
        assert_eq!(
            refusal(
                &InMemoryConfigStore::new(),
                &alias(),
                MaintenanceError::NotOwner
            )
            .await,
            MaintenanceRefusal::NotOwner
        );
    }

    #[tokio::test]
    async fn every_refusal_of_the_use_case_is_told_as_the_route_answers_it() {
        let cases = [
            (
                MaintenanceError::UnknownRepository,
                MaintenanceRefusal::UnknownRepository,
            ),
            (
                MaintenanceError::Engine {
                    detail: "no worker program".to_owned(),
                },
                unavailable_with(
                    ErrorCode::EngineUnavailable,
                    Vec::new(),
                    Some("no worker program"),
                ),
            ),
            (MaintenanceError::TimedOut, MaintenanceRefusal::TimedOut),
            (
                MaintenanceError::KeepsNothing { plan: plan("home") },
                MaintenanceRefusal::KeepsNothing {
                    plan: "home".to_owned(),
                },
            ),
            (
                MaintenanceError::UnknownPlan {
                    plan: plan("mirror"),
                },
                MaintenanceRefusal::UnknownPlan {
                    plan: "mirror".to_owned(),
                },
            ),
            (
                MaintenanceError::AdoptionNoMatch,
                MaintenanceRefusal::AdoptionNoMatch,
            ),
            (
                MaintenanceError::AdoptionAmbiguous {
                    plans: vec![plan("home"), plan("mirror")],
                },
                MaintenanceRefusal::AdoptionAmbiguous {
                    plans: vec!["home".to_owned(), "mirror".to_owned()],
                },
            ),
            (
                MaintenanceError::Busy { running: running() },
                MaintenanceRefusal::Busy {
                    running: "00000000-0000-0000-0000-000000000003".to_owned(),
                },
            ),
            (
                MaintenanceError::Store(StoreError::Unavailable {
                    detail: "disk full".to_owned(),
                }),
                unavailable_with(ErrorCode::OperationFailed, Vec::new(), Some("disk full")),
            ),
            (
                MaintenanceError::Store(StoreError::Corrupt {
                    detail: "a row of no kind".to_owned(),
                }),
                unavailable_with(
                    ErrorCode::OperationFailed,
                    Vec::new(),
                    Some("a row of no kind"),
                ),
            ),
            (
                MaintenanceError::SelectionChanged,
                MaintenanceRefusal::SelectionChanged,
            ),
            (
                MaintenanceError::InvalidRetention { plan: plan("home") },
                unavailable_with(ErrorCode::OperationFailed, Vec::new(), None),
            ),
        ];
        let config = InMemoryConfigStore::new();
        for (error, expected) in cases {
            let said = format!("{error:?}");
            assert_eq!(refusal(&config, &alias(), error).await, expected, "{said}");
        }
    }

    /// A configuration that a read, a preview or a run can't use is refused as
    /// a save of it is (#347): a mistake names its file, its line and its
    /// setting.
    #[tokio::test]
    async fn a_configuration_that_cannot_be_used_is_told_as_a_save_of_it_is() {
        for (error, code, args, detail) in
            super::super::repository_service_tests::unusable_configurations()
        {
            assert_eq!(
                refusal(
                    &InMemoryConfigStore::new(),
                    &alias(),
                    MaintenanceError::Unavailable(Box::new(error.clone()))
                )
                .await,
                unavailable_with(code, args, detail),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_read_that_ran_and_failed_is_told_in_the_engines_words_or_by_its_code() {
        let cases = [
            (
                FailureReason::EngineFailed {
                    detail: "wrong password or no repository at the location".to_owned(),
                },
                MaintenanceRefusal::OperationFailed {
                    detail: "wrong password or no repository at the location".to_owned(),
                },
            ),
            (
                FailureReason::EngineUnavailable {
                    detail: "no worker program".to_owned(),
                },
                unavailable_with(
                    ErrorCode::EngineUnavailable,
                    Vec::new(),
                    Some("no worker program"),
                ),
            ),
            (
                FailureReason::JobRejected {
                    detail: "a job it refuses".to_owned(),
                },
                unavailable_with(
                    ErrorCode::EngineUnavailable,
                    Vec::new(),
                    Some("a job it refuses"),
                ),
            ),
            (
                FailureReason::EngineExited { exit_code: Some(9) },
                unavailable_with(
                    ErrorCode::EngineExited,
                    vec![("exit_code", MsgArg::Text("9".to_owned()))],
                    None,
                ),
            ),
            (
                FailureReason::EngineExited { exit_code: None },
                unavailable_with(ErrorCode::EngineExited, Vec::new(), None),
            ),
            (
                FailureReason::ProtocolViolated {
                    transition: Transition::Start,
                },
                unavailable_with(
                    ErrorCode::ProtocolViolated,
                    vec![("transition", MsgArg::Text("Start".to_owned()))],
                    None,
                ),
            ),
            (
                FailureReason::DeadlinePassed,
                unavailable_with(ErrorCode::DeadlinePassed, Vec::new(), None),
            ),
            (
                FailureReason::EngineStartTimedOut,
                unavailable_with(ErrorCode::EngineStartTimedOut, Vec::new(), None),
            ),
            (
                FailureReason::EngineExitTimedOut,
                unavailable_with(ErrorCode::EngineExitTimedOut, Vec::new(), None),
            ),
            (
                FailureReason::HookFailed { exit_code: Some(1) },
                unavailable_with(ErrorCode::OperationFailed, Vec::new(), None),
            ),
            (
                FailureReason::RepositoryExists,
                unavailable_with(ErrorCode::OperationFailed, Vec::new(), None),
            ),
            (
                FailureReason::RepositoryReplaced,
                unavailable_with(ErrorCode::RepositoryReplaced, Vec::new(), None),
            ),
            (
                FailureReason::RepositoryNotFound,
                MaintenanceRefusal::RepositoryNotFound,
            ),
            (
                FailureReason::SecretsTooLargeToMask {
                    field: Some("HTTPS_PROXY".to_owned()),
                    limit: 262_144,
                },
                MaintenanceRefusal::SecretRefused(SecretRefusal {
                    code: ErrorCode::SecretsTooLargeToMask,
                    args: vec![
                        ("limit", MsgArg::Bytes(262_144)),
                        ("field", text("HTTPS_PROXY")),
                    ],
                    detail: None,
                }),
            ),
            (
                FailureReason::DaemonStopped,
                unavailable_with(ErrorCode::DaemonStopped, Vec::new(), None),
            ),
        ];
        let config = InMemoryConfigStore::new();
        for (reason, expected) in cases {
            let said = format!("{reason:?}");
            assert_eq!(
                refusal(&config, &alias(), MaintenanceError::Failed(reason)).await,
                expected,
                "{said}"
            );
        }
    }

    /// A configuration that holds the repository "demo", with the password
    /// `demo-password` written into it.
    async fn config_holding_demo() -> InMemoryConfigStore {
        let settings = domain::planning::RepositorySettings {
            repository: "/srv/demo/repo".to_owned(),
            repo_hot: None,
            password: domain::secrets::SecretRef::Inline(domain::secrets::SecretValue::new(
                "demo-password",
            )),
            options: Default::default(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        };
        let config = domain::planning::AgentConfig::new(
            domain::planning::AgentSettings::default(),
            domain::planning::Hooks::default(),
            std::collections::BTreeMap::from([(
                alias(),
                domain::planning::RepositoryConfig {
                    id: None,
                    settings,
                    maintenance: domain::planning::Maintenance::None,
                    group_by: Default::default(),
                    hooks: Default::default(),
                },
            )]),
            Vec::new(),
        )
        .expect("a valid configuration");
        let store = InMemoryConfigStore::new();
        store.save(&config).await.expect("the configuration saves");
        store
    }

    /// A secret that failed to resolve is told by the code of its cause, as
    /// the snapshot listing tells it, with the words a command said masked of
    /// the secrets of the repository the request named (I4).
    #[tokio::test]
    async fn a_secret_that_failed_to_resolve_is_told_with_the_secrets_of_its_repository_masked() {
        let config = config_holding_demo().await;
        let error = MaintenanceError::Resolve(ResolveError {
            field: Field::Password,
            cause: Cause::Unresolved(domain::secrets::SecretError {
                source: domain::secrets::SourceName::Command {
                    program: "pass".to_owned(),
                },
                problem: domain::secrets::Problem::CommandFailed {
                    status: Some(1),
                    detail: "wrote demo-password to stderr".to_owned(),
                },
            }),
        });
        assert_eq!(
            refusal(&config, &alias(), error).await,
            MaintenanceRefusal::SecretRefused(SecretRefusal {
                code: ErrorCode::SecretCommandFailed,
                args: vec![("field", text("password")), ("source", text("pass"))],
                detail: Some(format!("wrote {} to stderr", domain::secrets::REDACTED)),
            })
        );
    }

    /// A secret whose repository the configuration cannot name now is told as
    /// the configuration is: unknown when the repository was removed since,
    /// and as a save of it is refused when the file cannot be read (#347).
    #[tokio::test]
    async fn a_secret_is_told_by_the_configuration_when_it_cannot_name_the_repository() {
        let error = || {
            MaintenanceError::Resolve(ResolveError {
                field: Field::Password,
                cause: Cause::ReferenceInProcessArguments,
            })
        };
        assert_eq!(
            refusal(&InMemoryConfigStore::new(), &alias(), error()).await,
            MaintenanceRefusal::UnknownRepository
        );
        assert_eq!(
            refusal(&Unreadable, &alias(), error()).await,
            unavailable_with(
                ErrorCode::ConfigUnavailable,
                vec![("file", text("config.toml"))],
                Some("permission denied")
            )
        );
    }

    /// A configuration store that can't be read.
    struct Unreadable;

    #[async_trait]
    impl ConfigStore for Unreadable {
        async fn load(&self) -> Result<domain::planning::AgentConfig, ConfigError> {
            Err(ConfigError::Unavailable {
                file: "config.toml".to_owned(),
                detail: "permission denied".to_owned(),
            })
        }

        async fn save(&self, _config: &domain::planning::AgentConfig) -> Result<(), ConfigError> {
            Err(ConfigError::Unavailable {
                file: "config.toml".to_owned(),
                detail: "permission denied".to_owned(),
            })
        }

        async fn record_id(
            &self,
            _alias: &domain::RepositoryAlias,
            _read: &domain::planning::RepositoryConfig,
            _id: &domain::RepositoryId,
        ) -> Result<(), app::ports::RecordError> {
            Err(app::ports::RecordError::Store(ConfigError::Unavailable {
                file: "config.toml".to_owned(),
                detail: "permission denied".to_owned(),
            }))
        }
    }
}
