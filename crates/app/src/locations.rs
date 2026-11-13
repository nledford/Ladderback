//! Choosing which location of a repository a job opens (technical design
//! §6.1, decision 80). Before a job that opens a repository reached at
//! several locations, every location is probed in order, each by a
//! short-lived worker of its own with only its own secrets, and the first
//! that answers with the repository's ID is used: a location that holds
//! another repository is refused and never used. The job then carries that
//! location alone, so a location the job didn't use never had its secrets in
//! the job's process (I4).

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::locations::{
    LocationOutcome, LocationTried, LocationsRecord, recorded_address,
};
use domain::planning::{
    ExposedCredentials, LOCATION_PROBE_SECONDS, Location, MAX_RECORDED_ADDRESS_BYTES,
};
use domain::secrets::SourceName;
use domain::{OperationId, RepositoryAlias, RepositoryId, SignedDuration};
use futures::{FutureExt, StreamExt, select_biased};

use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, ProbeAnswer,
    RepositoryAccess, RunningJob, SavedEntry,
};
use crate::secrets::{Cause, JobReferences, ResolveError, ResolveRepository};

/// What the choice tells the daemon's log. Nothing here is a secret or a
/// network's name: a location is named by its position, from 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocationReport {
    /// A location, as its references resolved, sends its credentials where
    /// whoever answers at its address can read them: a warning in the log
    /// for that job, never one of the operation (§10.6).
    ExposedCredentials {
        alias: RepositoryAlias,
        position: u8,
        cause: ExposedCredentials,
    },
    /// A location holds a repository whose ID isn't the entry's.
    OtherRepository {
        alias: RepositoryAlias,
        position: u8,
    },
    /// Which location a job used, and why each earlier one was skipped.
    Chosen {
        alias: RepositoryAlias,
        record: LocationsRecord,
    },
}

/// A repository access with its location chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The chosen location's access, with only its own secrets.
    pub access: RepositoryAccess,
    /// Every location as the choice saw it; `None` for a repository of one
    /// location, which is opened as it always was, with no probe.
    pub record: Option<LocationsRecord>,
}

/// An [`Engine`] that chooses, before a job that opens a repository reached
/// at several locations, the location the job opens.
#[derive(Clone)]
pub struct ChooseLocation {
    inner: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    resolve: ResolveRepository,
    report: Arc<dyn Fn(LocationReport) + Send + Sync>,
    bound: SignedDuration,
}

/// The probe that runs now, so a stop of the job stops it too.
type Probing = Arc<Mutex<Phase>>;

impl ChooseLocation {
    /// Probes through `inner`, each probe bounded by
    /// [`LOCATION_PROBE_SECONDS`](domain::planning::LOCATION_PROBE_SECONDS)
    /// on `clock`, resolves each location's references with `resolve` when
    /// it is about to be probed, and tells `report` what the log says.
    pub fn new(
        inner: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        resolve: ResolveRepository,
        report: Arc<dyn Fn(LocationReport) + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            clock,
            resolve,
            report,
            bound: SignedDuration::from_secs(LOCATION_PROBE_SECONDS),
        }
    }

    /// The same, with each probe bounded by `bound` instead.
    pub fn with_probe_bound(self, bound: SignedDuration) -> Self {
        Self { bound, ..self }
    }

    /// Chooses the location `access` opens, for operation `op`: the access
    /// itself for a repository of one location, else the first location that
    /// answers with the entry's repository ID. Fails with the reason the
    /// operation ends with when none can be used.
    pub async fn locate(
        &self,
        op: OperationId,
        access: RepositoryAccess,
    ) -> Result<Located, (FailureReason, Option<LocationsRecord>)> {
        let phase = Arc::new(Mutex::new(Phase::Idle));
        self.choose(op, access, &phase).await
    }

    async fn choose(
        &self,
        op: OperationId,
        access: RepositoryAccess,
        phase: &Probing,
    ) -> Result<Located, (FailureReason, Option<LocationsRecord>)> {
        let Some(saved) = several(&access) else {
            return Ok(Located {
                access,
                record: None,
            });
        };
        let SavedEntry { alias, entry } = saved;
        // The configuration refuses further locations on an entry with no
        // recorded ID (decision 80): without one, nothing says which
        // repository a location must hold.
        let Some(expected) = entry.id.clone() else {
            return Err((FailureReason::NoLocationUsable, None));
        };
        let mut read = JobReferences::default();
        let mut tried = Vec::with_capacity(entry.location_count());
        let mut chosen = None;
        for (settings, position) in entry.location_settings().into_iter().zip(1u8..) {
            let address = recorded_address(
                &Location::of(&settings).masked_address(),
                MAX_RECORDED_ADDRESS_BYTES,
            );
            let mut source = None;
            let outcome = if chosen.is_some() {
                LocationOutcome::NotTried
            } else {
                match self
                    .resolve
                    .resolve_location(&settings, &access.password, &mut read)
                    .await
                {
                    Err(error) => match unusable(error) {
                        Ok((outcome, named)) => {
                            source = Some(named);
                            outcome
                        }
                        Err(reason) => return Err((reason, None)),
                    },
                    Ok(resolved) => {
                        let location = resolved.access;
                        if let Some(cause) = ExposedCredentials::of_location(&Location {
                            repository: location.repository.clone(),
                            repo_hot: location.repo_hot.clone(),
                            options: location.options.clone(),
                            options_hot: location.options_hot.clone(),
                            options_cold: location.options_cold.clone(),
                        }) {
                            (self.report)(LocationReport::ExposedCredentials {
                                alias: alias.clone(),
                                position,
                                cause,
                            });
                        }
                        let outcome = self.probe(op, location.clone(), &expected, phase).await;
                        if outcome == LocationOutcome::OtherRepository {
                            (self.report)(LocationReport::OtherRepository {
                                alias: alias.clone(),
                                position,
                            });
                        }
                        if outcome == LocationOutcome::Used {
                            chosen = Some(location);
                        }
                        outcome
                    }
                }
            };
            tried.push(LocationTried {
                position,
                address,
                outcome,
                source,
            });
        }
        let record = LocationsRecord { locations: tried };
        (self.report)(LocationReport::Chosen {
            alias: alias.clone(),
            record: record.clone(),
        });
        match chosen {
            Some(location) => Ok(Located {
                access: RepositoryAccess {
                    id: Some(expected),
                    entry: Some(SavedEntry { alias, entry }),
                    ..location
                },
                record: Some(record),
            }),
            None => Err((failure(&record), Some(record))),
        }
    }

    /// Probes one location, resolved: a worker of its own that opens it with
    /// no cache (it names no repository), bounded by `self.bound`, and
    /// stopped when the bound runs out or the job is stopped.
    async fn probe(
        &self,
        op: OperationId,
        mut location: RepositoryAccess,
        expected: &RepositoryId,
        phase: &Probing,
    ) -> LocationOutcome {
        location.id = None;
        location.entry = None;
        let RunningJob { events, control } = match self.inner.run(op, Job::Probe(location)).await {
            Ok(running) => running,
            Err(_) => return LocationOutcome::Silent,
        };
        let unwanted = {
            let mut current = phase.lock().unwrap_or_else(PoisonError::into_inner);
            if matches!(*current, Phase::Stopped) {
                Some(control)
            } else {
                *current = Phase::Probing(control);
                None
            }
        };
        if let Some(mut control) = unwanted {
            control.stop().await;
            return LocationOutcome::Silent;
        }
        let mut answer = std::pin::pin!(answered(events).fuse());
        let mut bound = std::pin::pin!(self.clock.sleep(self.bound).fuse());
        let found = select_biased! {
            found = answer => found,
            () = bound => None,
        };
        let left = std::mem::replace(
            &mut *phase.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Idle,
        );
        match left {
            Phase::Probing(mut control) => {
                if found.is_none() {
                    control.stop().await;
                }
            }
            // Only a stop or the probe itself changes the phase while it runs.
            other @ (Phase::Stopped | Phase::Running(_)) => {
                *phase.lock().unwrap_or_else(PoisonError::into_inner) = other;
            }
            Phase::Idle => {}
        }
        match found {
            Some(ProbeAnswer::Holds(id)) if id == *expected => LocationOutcome::Used,
            Some(ProbeAnswer::Holds(_)) => LocationOutcome::OtherRepository,
            Some(ProbeAnswer::Refused) => LocationOutcome::Refused,
            Some(ProbeAnswer::NotARepository) => LocationOutcome::NotARepository,
            Some(ProbeAnswer::PasswordNotAccepted) => LocationOutcome::PasswordNotAccepted,
            None => LocationOutcome::Silent,
        }
    }
}

/// The entry of `access` when it lists several locations: only then is a
/// location chosen.
fn several(access: &RepositoryAccess) -> Option<SavedEntry> {
    access
        .entry
        .clone()
        .filter(|saved| saved.entry.has_several_locations())
}

/// Why a location whose settings didn't resolve is skipped, and what the
/// record names: where an unresolved secret was looked for, else the setting
/// that can't be used, never a value. Each keeps the code a repository of one
/// location is refused with, so a template error or a relative path isn't
/// taken for a secret that couldn't be read. Secrets past what the daemon
/// masks fail the choice instead, as they fail a job (§10.2).
fn unusable(error: ResolveError) -> Result<(LocationOutcome, String), FailureReason> {
    let ResolveError { field, cause } = error;
    let outcome = match cause {
        Cause::Unresolved(error) => {
            return Ok((
                LocationOutcome::SecretUnavailable,
                source_name(&error.source),
            ));
        }
        Cause::TooMuchToMask { limit } => {
            return Err(FailureReason::SecretsTooLargeToMask {
                field: Some(field.name()),
                limit,
            });
        }
        Cause::Invalid(_) => LocationOutcome::TemplateInvalid,
        Cause::RelativePath => LocationOutcome::PathRelative,
        Cause::ExpansionTooLong { .. } => LocationOutcome::ExpansionTooLong,
        Cause::ReferenceInProcessArguments => LocationOutcome::ReferenceInProcessArguments,
        Cause::TooManyCredentials { .. } => LocationOutcome::TooManyCredentials,
    };
    Ok((outcome, field.name()))
}

/// Where a secret was looked for, never its value.
fn source_name(source: &SourceName) -> String {
    match source {
        SourceName::Command { program } => format!("command {program}"),
        SourceName::Keyring { entry } => format!("keyring {entry}"),
        SourceName::File { path } => format!("file {path}"),
        SourceName::Env { name } => format!("env {name}"),
    }
}

/// Why a choice that used no location fails: a password error when every
/// location that answered refused the repository's password, the likelier
/// cause; else no location was usable.
fn failure(record: &LocationsRecord) -> FailureReason {
    let answered: Vec<LocationOutcome> = record
        .locations
        .iter()
        .map(|tried| tried.outcome)
        .filter(|outcome| {
            !matches!(
                outcome,
                LocationOutcome::SecretUnavailable
                    | LocationOutcome::TemplateInvalid
                    | LocationOutcome::PathRelative
                    | LocationOutcome::ExpansionTooLong
                    | LocationOutcome::ReferenceInProcessArguments
                    | LocationOutcome::TooManyCredentials
                    | LocationOutcome::RuleNotMet
                    | LocationOutcome::RuleUnknown
                    | LocationOutcome::NotTried
            )
        })
        .collect();
    if !answered.is_empty()
        && answered
            .iter()
            .all(|outcome| *outcome == LocationOutcome::PasswordNotAccepted)
    {
        FailureReason::PasswordNotAccepted
    } else {
        FailureReason::NoLocationUsable
    }
}

/// What a probe's worker found, or `None` when it ended with no answer.
async fn answered(
    mut events: futures::stream::BoxStream<'static, EngineEvent>,
) -> Option<ProbeAnswer> {
    while let Some(event) = events.next().await {
        match event {
            EngineEvent::Succeeded(EngineResult::Probed(answer)) => return Some(answer),
            EngineEvent::Succeeded(_) | EngineEvent::Failed(_) => return None,
            EngineEvent::Started { .. }
            | EngineEvent::Progress(_)
            | EngineEvent::Warning(_)
            | EngineEvent::Located(_) => {}
        }
    }
    None
}

/// What a choice is doing, so a stop reaches the probe that runs and
/// nothing starts after it.
enum Phase {
    Idle,
    Probing(Box<dyn JobControl>),
    /// The job that follows the choice runs.
    Running(Box<dyn JobControl>),
    Stopped,
}

/// Stops a located job: the probe while one runs, the job once it does, and
/// nothing is started after.
struct Handover(Probing);

#[async_trait]
impl JobControl for Handover {
    async fn stop(&mut self) {
        let going = std::mem::replace(
            &mut *self.0.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Stopped,
        );
        if let Phase::Probing(mut control) | Phase::Running(mut control) = going {
            control.stop().await;
        }
    }
}

#[async_trait]
impl Engine for ChooseLocation {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let Some(access) = job
            .access()
            .filter(|access| several(access).is_some())
            .cloned()
        else {
            return self.inner.run(op, job).await;
        };
        let phase: Probing = Arc::new(Mutex::new(Phase::Idle));
        let handed = Arc::clone(&phase);
        let this = self.clone();
        let mut job = job;
        let events = futures::stream::once(async move {
            let located = this.choose(op, access, &handed).await;
            if matches!(
                *handed.lock().unwrap_or_else(PoisonError::into_inner),
                Phase::Stopped
            ) {
                return futures::stream::empty().boxed();
            }
            let Located { access, record } = match located {
                Ok(located) => located,
                Err((reason, record)) => {
                    let located = record.map(EngineEvent::Located);
                    return futures::stream::iter(
                        located.into_iter().chain([EngineEvent::Failed(reason)]),
                    )
                    .boxed();
                }
            };
            if let Some(slot) = job.access_mut() {
                *slot = access;
            }
            let located = record.map(EngineEvent::Located);
            let RunningJob { events, control } = match this.inner.run(op, job).await {
                Ok(running) => running,
                Err(error) => {
                    return futures::stream::iter(
                        located
                            .into_iter()
                            .chain([EngineEvent::Failed(FailureReason::from(error))]),
                    )
                    .boxed();
                }
            };
            let unwanted = {
                let mut current = handed.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(*current, Phase::Stopped) {
                    Some(control)
                } else {
                    *current = Phase::Running(control);
                    None
                }
            };
            match unwanted {
                Some(mut control) => {
                    control.stop().await;
                    futures::stream::empty().boxed()
                }
                None => futures::stream::iter(located).chain(events).boxed(),
            }
        })
        .flatten()
        .boxed();
        Ok(RunningJob {
            events,
            control: Box::new(Handover(phase)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::planning::{
        GroupBy, Hooks, Locations, Maintenance, RepositoryConfig, RepositorySettings,
    };
    use domain::secrets::SecretRef;
    use domain::{Timestamp, Uuid};

    use super::*;
    use crate::fakes::{FakeClock, FakeSecrets};

    const ID: &str = "4c6f636174696f6e73206f6620746865207265706f7369746f72792066697273";

    /// Every probe finds the repository; the addresses probed are kept.
    #[derive(Default)]
    struct Holds(Arc<Mutex<Vec<String>>>);

    struct Gone;

    #[async_trait]
    impl JobControl for Gone {
        async fn stop(&mut self) {}
    }

    #[async_trait]
    impl Engine for Holds {
        async fn run(&self, _op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            if let Job::Probe(access) = &job {
                self.0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(access.repository.clone());
            }
            let id = RepositoryId::parse(ID).expect("an ID");
            Ok(RunningJob {
                events: futures::stream::iter([EngineEvent::Succeeded(EngineResult::Probed(
                    ProbeAnswer::Holds(id),
                ))])
                .boxed(),
                control: Box::new(Gone),
            })
        }
    }

    /// The choice for an entry whose first location is `first` and whose
    /// second is `/srv/public`, with `secrets` set as given.
    async fn choose(
        first: RepositorySettings,
        secrets: &[(&str, &str)],
    ) -> (
        Result<Located, (FailureReason, Option<LocationsRecord>)>,
        Vec<String>,
    ) {
        let fake = FakeSecrets::default();
        fake.set_env("NAS_PASSWORD", "demo-password");
        for (name, value) in secrets {
            fake.set_env(name, value);
        }
        let resolve = ResolveRepository::new(Arc::new(fake));
        let entry = RepositoryConfig {
            settings: first,
            locations: Locations {
                further: vec![Location {
                    repository: "/srv/public".to_owned(),
                    repo_hot: None,
                    options: BTreeMap::new(),
                    options_hot: BTreeMap::new(),
                    options_cold: BTreeMap::new(),
                }],
            },
            id: Some(RepositoryId::parse(ID).expect("an ID")),
            maintenance: Maintenance::None,
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        };
        let alias = RepositoryAlias::parse("nas").expect("an alias");
        let access = resolve
            .resolve_saved(&alias, &entry)
            .await
            .expect("the password resolves")
            .access;
        let probed = Arc::new(Mutex::new(Vec::new()));
        let choose = ChooseLocation::new(
            Arc::new(Holds(Arc::clone(&probed))),
            Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
            resolve,
            Arc::new(|_| {}),
        );
        let located = choose
            .locate(OperationId::from_uuid(Uuid::from_u128(1)), access)
            .await;
        let probed = probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (located, probed)
    }

    fn first(repository: &str, options: &[(&str, &str)]) -> RepositorySettings {
        RepositorySettings {
            repository: repository.to_owned(),
            repo_hot: None,
            password: SecretRef::Env("NAS_PASSWORD".to_owned()),
            options: options
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            options_hot: BTreeMap::new(),
            options_cold: BTreeMap::new(),
        }
    }

    /// The first location's outcome and source, and the position used.
    fn skipped(
        located: &Result<Located, (FailureReason, Option<LocationsRecord>)>,
    ) -> (LocationOutcome, Option<String>, Option<u8>) {
        let record = located
            .as_ref()
            .ok()
            .and_then(|located| located.record.clone())
            .expect("a record of the choice");
        let tried = record.at(1).expect("the first location").clone();
        (tried.outcome, tried.source, record.used())
    }

    #[tokio::test]
    async fn a_location_whose_path_resolves_relative_is_skipped_as_a_relative_path() {
        let (located, probed) = choose(first("${LAN_REPO}", &[]), &[("LAN_REPO", "srv/lan")]).await;
        assert_eq!(
            skipped(&located),
            (
                LocationOutcome::PathRelative,
                Some("repository".to_owned()),
                Some(2)
            )
        );
        // The relative path is never probed: rustic would read it from
        // wherever the daemon was started.
        assert_eq!(probed, vec!["/srv/public".to_owned()]);
    }

    #[tokio::test]
    async fn a_location_whose_setting_is_no_template_is_skipped_as_one() {
        let (located, probed) = choose(first("/srv/lan", &[("password", "${LAN_KEY")]), &[]).await;
        assert_eq!(
            skipped(&located),
            (
                LocationOutcome::TemplateInvalid,
                Some("options.password".to_owned()),
                Some(2)
            )
        );
        assert_eq!(probed, vec!["/srv/public".to_owned()]);
    }

    #[tokio::test]
    async fn a_reference_that_doesnt_resolve_is_skipped_as_an_unavailable_secret() {
        let (located, _) = choose(first("/srv/lan", &[("password", "${LAN_KEY}")]), &[]).await;
        assert_eq!(
            skipped(&located),
            (
                LocationOutcome::SecretUnavailable,
                Some("env LAN_KEY".to_owned()),
                Some(2)
            )
        );
    }
}
