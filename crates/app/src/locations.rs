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
use crate::secrets::{Cause, JobReferences, ResolveRepository};

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
                    Err(error) => {
                        source = Some(match error.cause {
                            Cause::Unresolved(error) => source_name(&error.source),
                            _ => format!("{:?}", error.field),
                        });
                        LocationOutcome::SecretUnavailable
                    }
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
