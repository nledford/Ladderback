//! Choosing which location of a repository a job opens (technical design
//! §6.1, decisions 80 and 86). Before a job that opens a repository reached
//! at several locations, or at one location that has a rule, the locations'
//! rules are evaluated against the machine's network link, read once for
//! the choice, and every location whose rule holds (or that has none) is
//! probed, each by a short-lived worker of its own with only its own
//! secrets: at once, beside the others, when it makes the server prove who
//! it is before it is sent a credential, or sends none; else only in its
//! turn, once every earlier location has failed. The first in order that
//! answers with the repository's ID is used, however soon a later one
//! answered, and every probe still running then is stopped: a location
//! that holds another repository is refused and never used. A repository
//! of one location is never probed: its rule decides alone, and the job's
//! own open checks the ID. The job then carries that location alone, so a
//! location the job didn't use never had its secrets in the job's process
//! (I4).

use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use async_trait::async_trait;
use domain::execution::FailureReason;
use domain::execution::locations::{
    LocationOutcome, LocationTried, LocationsRecord, recorded_address,
};
use domain::planning::{
    ExposedCredentials, LOCATION_PROBE_SECONDS, Location, MAX_RECORDED_ADDRESS_BYTES, RuleVerdict,
};
use domain::secrets::{SecretValue, SourceName};
use domain::{OperationId, RepositoryAlias, RepositoryId, SignedDuration};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt, select_biased};

use crate::conditions::LinkReading;
use crate::ports::{
    Clock, Engine, EngineError, EngineEvent, EngineResult, Job, JobControl, NetworkLink,
    ProbeAnswer, RepositoryAccess, RunningJob, SavedEntry,
};
use crate::repositories::RepositoryError;
use crate::secrets::{Cause, JobReferences, ResolveError, ResolveRepository, ResolvedRepository};

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
    /// location, which is opened as it always was, with no probe, once its
    /// rule, if it has one, holds.
    pub record: Option<LocationsRecord>,
}

/// An [`Engine`] that chooses, before a job that opens a repository reached
/// at several locations, the location the job opens.
#[derive(Clone)]
pub struct ChooseLocation {
    inner: Arc<dyn Engine>,
    clock: Arc<dyn Clock>,
    link: Arc<dyn NetworkLink>,
    resolve: ResolveRepository,
    report: Arc<dyn Fn(LocationReport) + Send + Sync>,
}

/// The probes that run now, or the job after them, so a stop of the job
/// stops them too.
type Probing = Arc<Mutex<Phase>>;

impl ChooseLocation {
    /// Probes through `inner`, each probe bounded by
    /// [`LOCATION_PROBE_SECONDS`](domain::planning::LOCATION_PROBE_SECONDS)
    /// on `clock`, evaluates the locations' rules against `link`, read once
    /// per choice, resolves each location's references with `resolve` when
    /// it is about to be probed, and tells `report` what the log says.
    pub fn new(
        inner: Arc<dyn Engine>,
        clock: Arc<dyn Clock>,
        link: Arc<dyn NetworkLink>,
        resolve: ResolveRepository,
        report: Arc<dyn Fn(LocationReport) + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            clock,
            link,
            resolve,
            report,
        }
    }

    /// Chooses the location `access` opens, for operation `op`: the access
    /// itself for a repository of one location whose rule, if any, holds,
    /// else the first location whose rule holds that answers with the
    /// entry's repository ID. Fails with the reason the operation ends with
    /// when none can be used.
    pub async fn locate(
        &self,
        op: OperationId,
        access: RepositoryAccess,
    ) -> Result<Located, (FailureReason, Option<LocationsRecord>)> {
        let phase = Arc::new(Mutex::new(Phase::Probing(Vec::new())));
        self.choose(op, access, &phase).await
    }

    async fn choose(
        &self,
        op: OperationId,
        access: RepositoryAccess,
        phase: &Probing,
    ) -> Result<Located, (FailureReason, Option<LocationsRecord>)> {
        let Some(saved) = chosen(&access) else {
            return Ok(Located {
                access,
                record: None,
            });
        };
        if !saved.entry.has_several_locations() {
            // One location with a rule is tried only when its rule holds,
            // with no probe: there is no other location to choose, and the
            // job's own open refuses another repository, as it does for a
            // location with no rule (decision 80, point 2).
            let holds = match saved.entry.location_rules().first().copied().flatten() {
                Some(rule) => {
                    let mut reading = LinkReading::new(Arc::clone(&self.link));
                    reading.verdict(rule).await == RuleVerdict::Holds
                }
                None => true,
            };
            return if holds {
                Ok(Located {
                    access,
                    record: None,
                })
            } else {
                Err((FailureReason::NoLocationRuleHolds, None))
            };
        }
        // The configuration refuses further locations on an entry with no
        // recorded ID (decision 80): without one, nothing says which
        // repository a location must hold.
        let Some(expected) = saved.entry.id.clone() else {
            return Err((FailureReason::NoLocationUsable, None));
        };
        let (record, chosen) = self
            .try_each(
                op,
                &saved,
                &access.password,
                Some(&expected),
                Reach::FirstUsed,
                phase,
            )
            .await
            .map_err(|reason| (reason, None))?;
        (self.report)(LocationReport::Chosen {
            alias: saved.alias.clone(),
            record: record.clone(),
        });
        match chosen {
            Some(location) => Ok(Located {
                access: RepositoryAccess {
                    id: Some(expected),
                    entry: Some(saved),
                    ..location
                },
                record: Some(record),
            }),
            None => Err((failure(&record), Some(record))),
        }
    }

    /// A person's Test of the saved entry `saved` (decision 80): every
    /// location is probed in order, past the one a job would use, so each
    /// has an outcome of its own and none is `not-tried`. The entry's
    /// password is resolved first; a location's own references are resolved
    /// as it is about to be probed, as for a job. An entry of one location
    /// is probed too, and while it records no repository ID, whatever
    /// repository it holds counts as its own. Nothing is recorded.
    #[allow(
        clippy::result_large_err,
        reason = "a typed code for the caller to match, as `InitRepository`'s; none of its variants is large on its own"
    )]
    pub async fn test_each(
        &self,
        op: OperationId,
        saved: &SavedEntry,
    ) -> Result<LocationsRecord, RepositoryError> {
        let password = self
            .resolve
            .resolve_password(&saved.entry.settings.password)
            .await
            .map_err(RepositoryError::Resolve)?
            .access
            .password;
        let phase = Arc::new(Mutex::new(Phase::Probing(Vec::new())));
        let (record, _) = self
            .try_each(
                op,
                saved,
                &password,
                saved.entry.id.as_ref(),
                Reach::Every,
                &phase,
            )
            .await
            .map_err(RepositoryError::Failed)?;
        Ok(record)
    }

    /// Tries the locations of `saved` in order with the repository's
    /// `password`. The rules are evaluated first, against the network link
    /// read once and only when a location has one, and its Wi-Fi network's
    /// name read once and only when a rule lists names: a location whose rule
    /// isn't met, or can't be evaluated, is skipped unresolved and unprobed.
    /// Every location whose rule holds and that checks the server first, as
    /// written and as resolved, is then resolved and probed at once, beside
    /// the others (decision 86); each other location is resolved and probed
    /// in its turn, once every earlier one has its outcome. A location holds
    /// the repository when its ID is `expected` (any ID when `None`). With
    /// [`Reach::FirstUsed`] the ones after the first usable one are
    /// `not-tried`, and every probe still running then is stopped; no probe
    /// runs on once this returns. Answers every location's outcome and the
    /// first usable one's access, or the reason the whole choice fails
    /// (secrets past what the daemon masks, met in a location's turn).
    async fn try_each(
        &self,
        op: OperationId,
        saved: &SavedEntry,
        password: &SecretValue,
        expected: Option<&RepositoryId>,
        reach: Reach,
        phase: &Probing,
    ) -> Result<(LocationsRecord, Option<RepositoryAccess>), FailureReason> {
        let SavedEntry { alias, entry } = saved;
        let rules = entry.location_rules();
        let mut reading = LinkReading::new(Arc::clone(&self.link));
        let mut verdicts = Vec::with_capacity(rules.len());
        for rule in &rules {
            verdicts.push(match rule {
                Some(rule) => reading.verdict(rule).await,
                None => RuleVerdict::Holds,
            });
        }
        let early = entry.early_probes(&verdicts);
        let settings = entry.location_settings();
        let mut read = JobReferences::default();
        let mut probes = EarlyProbes::new(settings.len());
        // A location probed early is resolved now, in order, beside the
        // probes already started; what its resolution says is kept for its
        // turn, so it skips or fails the choice only if its turn comes.
        let mut resolved: Vec<Option<Result<ResolvedRepository, ResolveError>>> =
            settings.iter().map(|_| None).collect();
        for (((index, location), early), position) in
            settings.iter().enumerate().zip(early).zip(1u8..)
        {
            if !early {
                continue;
            }
            let resolution = probes
                .alongside(self.resolve.resolve_location(location, password, &mut read))
                .await;
            if let Ok(resolution) = &resolution
                && location_of(&resolution.access).checks_server_first()
            {
                let (wanted, unwanted) = oneshot::channel();
                let probe = self.probe(
                    op,
                    position,
                    resolution.access.clone(),
                    expected,
                    phase,
                    unwanted,
                );
                probes.start(index, probe.boxed(), wanted);
            }
            if let Some(slot) = resolved.get_mut(index) {
                *slot = Some(resolution);
            }
        }
        let walked = async {
            let mut tried = Vec::with_capacity(settings.len());
            let mut chosen = None;
            for (((index, location), verdict), position) in
                settings.iter().enumerate().zip(verdicts).zip(1u8..)
            {
                let address = recorded_address(
                    &Location::of(location).masked_address(),
                    MAX_RECORDED_ADDRESS_BYTES,
                );
                let mut source = None;
                let outcome = if chosen.is_some() && reach == Reach::FirstUsed {
                    LocationOutcome::NotTried
                } else if verdict == RuleVerdict::NotMet {
                    LocationOutcome::RuleNotMet
                } else if verdict == RuleVerdict::Unknown {
                    LocationOutcome::RuleUnknown
                } else {
                    let resolution = match resolved.get_mut(index).and_then(Option::take) {
                        Some(resolution) => resolution,
                        None => {
                            probes
                                .alongside(
                                    self.resolve.resolve_location(location, password, &mut read),
                                )
                                .await
                        }
                    };
                    match resolution {
                        Err(error) => {
                            let (outcome, named) = unusable(error)?;
                            source = Some(named);
                            outcome
                        }
                        Ok(resolution) => {
                            let location = resolution.access;
                            if let Some(cause) =
                                ExposedCredentials::of_location(&location_of(&location))
                            {
                                (self.report)(LocationReport::ExposedCredentials {
                                    alias: alias.clone(),
                                    position,
                                    cause,
                                });
                            }
                            let outcome = match probes.outcome(index).await {
                                Some(outcome) => outcome,
                                None => {
                                    // Kept until the probe ends: it is wanted all along.
                                    let (_wanted, unwanted) = oneshot::channel();
                                    probes
                                        .alongside(self.probe(
                                            op,
                                            position,
                                            location.clone(),
                                            expected,
                                            phase,
                                            unwanted,
                                        ))
                                        .await
                                }
                            };
                            if outcome == LocationOutcome::OtherRepository {
                                (self.report)(LocationReport::OtherRepository {
                                    alias: alias.clone(),
                                    position,
                                });
                            }
                            if outcome == LocationOutcome::Used && chosen.is_none() {
                                chosen = Some(location);
                                if reach == Reach::FirstUsed {
                                    probes.stop().await;
                                }
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
            Ok((LocationsRecord { locations: tried }, chosen))
        }
        .await;
        probes.stop().await;
        walked
    }

    /// Probes one location, resolved, at `position`: a worker of its own
    /// that opens it with no cache (it names no repository), bounded by
    /// [`LOCATION_PROBE_SECONDS`] on the clock from its start, and stopped
    /// when the bound runs out, when the job is stopped, or when `wanted`'s
    /// sender is dropped because its answer is no longer needed.
    async fn probe(
        &self,
        op: OperationId,
        position: u8,
        mut location: RepositoryAccess,
        expected: Option<&RepositoryId>,
        phase: &Probing,
        mut wanted: oneshot::Receiver<()>,
    ) -> LocationOutcome {
        location.id = None;
        location.entry = None;
        let RunningJob { events, control } = match self.inner.run(op, Job::Probe(location)).await {
            Ok(running) => running,
            Err(_) => return LocationOutcome::Silent,
        };
        let unwanted = {
            let mut current = phase.lock().unwrap_or_else(PoisonError::into_inner);
            match &mut *current {
                // Nothing is sent on `wanted`: it is still wanted while its
                // sender lives.
                Phase::Probing(running) if matches!(wanted.try_recv(), Ok(None)) => {
                    running.push((position, control));
                    None
                }
                Phase::Probing(_) | Phase::Running(_) | Phase::Stopped => Some(control),
            }
        };
        if let Some(mut control) = unwanted {
            control.stop().await;
            return LocationOutcome::Silent;
        }
        let mut answer = std::pin::pin!(answered(events).fuse());
        let mut bound = std::pin::pin!(
            self.clock
                .sleep(SignedDuration::from_secs(LOCATION_PROBE_SECONDS))
                .fuse()
        );
        // A `Receiver` counts as terminated once its sender is dropped, and
        // `select!` skips a terminated branch: wait for it inside a future of
        // its own, which ends then.
        let mut unwanted = std::pin::pin!(
            async move {
                let _ = wanted.await;
            }
            .fuse()
        );
        let found = select_biased! {
            found = answer => found,
            () = bound => None,
            () = unwanted => None,
        };
        // A stop of the job took every probe's control already.
        let left = match &mut *phase.lock().unwrap_or_else(PoisonError::into_inner) {
            Phase::Probing(running) => running
                .iter()
                .position(|(at, _)| *at == position)
                .map(|index| running.swap_remove(index).1),
            Phase::Running(_) | Phase::Stopped => None,
        };
        if let Some(mut control) = left
            && found.is_none()
        {
            control.stop().await;
        }
        match found {
            Some(ProbeAnswer::Holds(id)) if expected.is_none_or(|expected| id == *expected) => {
                LocationOutcome::Used
            }
            Some(ProbeAnswer::Holds(_)) => LocationOutcome::OtherRepository,
            Some(ProbeAnswer::Refused) => LocationOutcome::Refused,
            Some(ProbeAnswer::NotARepository) => LocationOutcome::NotARepository,
            Some(ProbeAnswer::PasswordNotAccepted) => LocationOutcome::PasswordNotAccepted,
            None => LocationOutcome::Silent,
        }
    }
}

/// The location a resolved access opens, to see what it sends where.
fn location_of(access: &RepositoryAccess) -> Location {
    Location {
        when: None,
        repository: access.repository.clone(),
        repo_hot: access.repo_hot.clone(),
        options: access.options.clone(),
        options_hot: access.options_hot.clone(),
        options_cold: access.options_cold.clone(),
    }
}

/// A probe started before its location's turn (decision 86): running, with
/// the sender whose drop says its answer is no longer wanted, or ended.
enum Early<'a> {
    Running {
        probe: BoxFuture<'a, LocationOutcome>,
        wanted: Option<oneshot::Sender<()>>,
    },
    Ended(LocationOutcome),
}

/// A choice's early probes, by location index, each polled whatever the
/// choice waits on, so they run beside it and beside each other.
struct EarlyProbes<'a>(Vec<Option<Early<'a>>>);

impl<'a> EarlyProbes<'a> {
    fn new(count: usize) -> Self {
        Self((0..count).map(|_| None).collect())
    }

    /// Starts `probe` as the early probe of the location at `index`.
    fn start(
        &mut self,
        index: usize,
        probe: BoxFuture<'a, LocationOutcome>,
        wanted: oneshot::Sender<()>,
    ) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = Some(Early::Running {
                probe,
                wanted: Some(wanted),
            });
        }
    }

    /// Polls every probe still running, keeping the outcome of each that
    /// ends.
    fn poll_running(&mut self, cx: &mut Context<'_>) {
        for slot in &mut self.0 {
            if let Some(Early::Running { probe, .. }) = slot
                && let Poll::Ready(outcome) = probe.poll_unpin(cx)
            {
                *slot = Some(Early::Ended(outcome));
            }
        }
    }

    /// Runs `work` to its end while the early probes run on.
    async fn alongside<T>(&mut self, work: impl Future<Output = T>) -> T {
        let mut work = std::pin::pin!(work);
        futures::future::poll_fn(|cx| {
            self.poll_running(cx);
            work.as_mut().poll(cx)
        })
        .await
    }

    /// The outcome of the early probe of the location at `index` once it
    /// ends, or `None` when none was started.
    async fn outcome(&mut self, index: usize) -> Option<LocationOutcome> {
        futures::future::poll_fn(|cx| {
            self.poll_running(cx);
            match self.0.get(index) {
                Some(Some(Early::Ended(outcome))) => Poll::Ready(Some(*outcome)),
                Some(Some(Early::Running { .. })) => Poll::Pending,
                Some(None) | None => Poll::Ready(None),
            }
        })
        .await
    }

    /// Stops every probe still running and waits until each has ended, its
    /// worker stopped.
    async fn stop(&mut self) {
        for slot in &mut self.0 {
            if let Some(Early::Running { wanted, .. }) = slot {
                wanted.take();
            }
        }
        futures::future::poll_fn(|cx| {
            self.poll_running(cx);
            if self
                .0
                .iter()
                .any(|slot| matches!(slot, Some(Early::Running { .. })))
            {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
    }
}

/// The entry of `access` when a location is chosen for it: when it lists
/// several locations, or one that has a rule.
fn chosen(access: &RepositoryAccess) -> Option<SavedEntry> {
    access
        .entry
        .clone()
        .filter(|saved| saved.entry.chooses_location())
}

/// The positions, from 1, of the locations of `job`'s repository that have
/// a rule: a location the choice probed there had its rule hold, so its skip
/// is a warning of the operation (decision 80, point 8).
pub(crate) fn ruled_positions(job: &Job) -> Vec<u8> {
    job.access()
        .and_then(|access| access.entry.as_ref())
        .map(|saved| saved.entry.ruled_positions())
        .unwrap_or_default()
}

/// How far a run of the locations goes: a job's choice stops at the first
/// usable one, a person's Test probes them all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    FirstUsed,
    Every,
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

/// Why a choice that used no location fails: no rule held when every
/// location was skipped by its rule; else a password error when every
/// location that answered refused the repository's password, the likelier
/// cause; else no location was usable.
fn failure(record: &LocationsRecord) -> FailureReason {
    let by_rule = |outcome: LocationOutcome| {
        matches!(
            outcome,
            LocationOutcome::RuleNotMet | LocationOutcome::RuleUnknown
        )
    };
    if !record.locations.is_empty() && record.locations.iter().all(|tried| by_rule(tried.outcome)) {
        return FailureReason::NoLocationRuleHolds;
    }
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

/// What a choice is doing, so a stop reaches every probe that runs and
/// nothing starts after it.
enum Phase {
    /// The probes that run now, by their location's position: none before
    /// the first starts and after the last ends.
    Probing(Vec<(u8, Box<dyn JobControl>)>),
    /// The job that follows the choice runs.
    Running(Box<dyn JobControl>),
    Stopped,
}

/// Stops a located job: every probe while they run, the job once it does,
/// and nothing is started after.
struct Handover(Probing);

#[async_trait]
impl JobControl for Handover {
    async fn stop(&mut self) {
        let going = std::mem::replace(
            &mut *self.0.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Stopped,
        );
        match going {
            Phase::Probing(running) => {
                for (_, mut control) in running {
                    control.stop().await;
                }
            }
            Phase::Running(mut control) => control.stop().await,
            Phase::Stopped => {}
        }
    }
}

#[async_trait]
impl Engine for ChooseLocation {
    async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
        let Some(access) = job
            .access()
            .filter(|access| chosen(access).is_some())
            .cloned()
        else {
            return self.inner.run(op, job).await;
        };
        let phase: Probing = Arc::new(Mutex::new(Phase::Probing(Vec::new())));
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
    use crate::fakes::{FakeClock, FakeNetworkLink, FakeSecrets};

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
        let access = nas(&resolve, first).await;
        let probed = Arc::new(Mutex::new(Vec::new()));
        let choose = ChooseLocation::new(
            Arc::new(Holds(Arc::clone(&probed))),
            Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
            Arc::new(FakeNetworkLink::new()),
            resolve,
            Arc::new(|_| {}),
        );
        // Every probe answers at once, so the choice ends in one poll: a
        // choice that waits fails here rather than hanging the test run.
        let located = choose
            .locate(OperationId::from_uuid(Uuid::from_u128(1)), access)
            .now_or_never()
            .expect("the choice ends at once");
        let probed = probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (located, probed)
    }

    /// A job's access to the entry "nas", which records the repository's ID
    /// and whose locations are `first`, then `/srv/public`.
    async fn nas(resolve: &ResolveRepository, first: RepositorySettings) -> RepositoryAccess {
        let entry = RepositoryConfig {
            settings: first,
            locations: Locations {
                first_when: None,
                further: vec![Location {
                    when: None,
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
        resolve
            .resolve_saved(&alias, &entry)
            .await
            .expect("the password resolves")
            .access
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

    /// A person's Test of the entry `nas`, whose locations are `/srv/lan`
    /// and the `further` ones, recording `id`: every outcome, and the
    /// addresses probed.
    async fn test_each(
        further: &[&str],
        id: Option<&str>,
        password: bool,
    ) -> (Result<Vec<LocationOutcome>, RepositoryError>, Vec<String>) {
        let fake = FakeSecrets::default();
        if password {
            fake.set_env("NAS_PASSWORD", "demo-password");
        }
        let saved = SavedEntry {
            alias: RepositoryAlias::parse("nas").expect("an alias"),
            entry: RepositoryConfig {
                settings: first("/srv/lan", &[]),
                locations: Locations {
                    first_when: None,
                    further: further
                        .iter()
                        .map(|repository| Location {
                            repository: (*repository).to_owned(),
                            ..Location::default()
                        })
                        .collect(),
                },
                id: id.map(|id| RepositoryId::parse(id).expect("an ID")),
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        };
        let probed = Arc::new(Mutex::new(Vec::new()));
        let tested = ChooseLocation::new(
            Arc::new(Holds(Arc::clone(&probed))),
            Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
            Arc::new(FakeNetworkLink::new()),
            ResolveRepository::new(Arc::new(fake)),
            Arc::new(|_| {}),
        )
        .test_each(OperationId::from_uuid(Uuid::from_u128(1)), &saved)
        .now_or_never()
        .expect("the test ends at once")
        .map(|record| record.locations.iter().map(|tried| tried.outcome).collect());
        let probed = probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (tested, probed)
    }

    #[tokio::test]
    async fn a_test_probes_every_location_past_the_one_a_job_would_use() {
        let (tested, probed) = test_each(&["/srv/public"], Some(ID), true).await;
        assert_eq!(
            tested,
            Ok(vec![LocationOutcome::Used, LocationOutcome::Used])
        );
        assert_eq!(
            probed,
            vec!["/srv/lan".to_owned(), "/srv/public".to_owned()]
        );
    }

    #[tokio::test]
    async fn a_test_takes_the_repository_an_entry_that_records_no_id_finds_for_its_own() {
        let (tested, _) = test_each(&[], None, true).await;
        assert_eq!(tested, Ok(vec![LocationOutcome::Used]));
        let other = "6f74686572207265706f7369746f7279206f662074686520656e7472792e2e2e";
        let (tested, _) = test_each(&[], Some(other), true).await;
        assert_eq!(tested, Ok(vec![LocationOutcome::OtherRepository]));
    }

    #[tokio::test]
    async fn a_test_whose_password_doesnt_resolve_probes_nothing() {
        let (tested, probed) = test_each(&["/srv/public"], Some(ID), false).await;
        assert!(
            matches!(tested, Err(RepositoryError::Resolve(_))),
            "{tested:?}"
        );
        assert_eq!(probed, Vec::<String>::new());
    }

    /// Locations whose probe never answers until its worker is stopped, at
    /// the addresses `silent`; every other probe finds the repository at
    /// once. Every job is kept, and how many probe workers still run.
    #[derive(Clone, Default)]
    struct Sites {
        silent: Vec<String>,
        jobs: Arc<Mutex<Vec<Job>>>,
        running: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Sites {
        fn silent_at(address: &str) -> Self {
            Self {
                silent: vec![address.to_owned()],
                ..Self::default()
            }
        }

        fn running(&self) -> usize {
            self.running.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn jobs(&self) -> Vec<Job> {
            self.jobs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    /// A probe's worker that runs until it is stopped: its events end then.
    struct Worker {
        running: Arc<std::sync::atomic::AtomicUsize>,
        end: Option<futures::channel::oneshot::Sender<()>>,
    }

    #[async_trait]
    impl JobControl for Worker {
        async fn stop(&mut self) {
            if self.end.take().is_some() {
                self.running
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    #[async_trait]
    impl Engine for Sites {
        async fn run(&self, op: OperationId, job: Job) -> Result<RunningJob, EngineError> {
            self.jobs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job.clone());
            let Job::Probe(access) = &job else {
                return Ok(RunningJob {
                    events: futures::stream::empty().boxed(),
                    control: Box::new(Gone),
                });
            };
            if self.silent.contains(&access.repository) {
                let (end, ended) = futures::channel::oneshot::channel::<()>();
                self.running
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return Ok(RunningJob {
                    events: futures::stream::once(ended)
                        .filter_map(|_| futures::future::ready(None::<EngineEvent>))
                        .boxed(),
                    control: Box::new(Worker {
                        running: Arc::clone(&self.running),
                        end: Some(end),
                    }),
                });
            }
            Holds::default().run(op, job).await
        }
    }

    /// The choice in front of `sites`, on a clock that moves only when the
    /// test moves it, and a job's access to "nas" (`/srv/lan`, then
    /// `/srv/public`).
    async fn choosing(sites: &Sites) -> (ChooseLocation, RepositoryAccess, Arc<FakeClock>) {
        let secrets = FakeSecrets::default();
        secrets.set_env("NAS_PASSWORD", "demo-password");
        let resolve = ResolveRepository::new(Arc::new(secrets));
        let access = nas(&resolve, first("/srv/lan", &[])).await;
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        let choose = ChooseLocation::new(
            Arc::new(sites.clone()),
            Arc::<FakeClock>::clone(&clock),
            Arc::new(FakeNetworkLink::new()),
            resolve,
            Arc::new(|_| {}),
        );
        (choose, access, clock)
    }

    fn op() -> OperationId {
        OperationId::from_uuid(Uuid::from_u128(1))
    }

    /// A location that doesn't answer is given up at the probe's bound,
    /// 8 seconds of monotonic time and not before, its worker is stopped,
    /// and the next location is probed (decision 80).
    #[tokio::test]
    async fn a_location_that_doesnt_answer_is_given_up_at_the_probes_bound() {
        let sites = Sites::silent_at("/srv/lan");
        let (choose, access, clock) = choosing(&sites).await;
        let mut located = choose.locate(op(), access).boxed();
        assert!(futures::poll!(&mut located).is_pending());
        let bound = SignedDuration::from_secs(LOCATION_PROBE_SECONDS);
        clock.advance(bound - SignedDuration::from_nanos(1));
        assert!(
            futures::poll!(&mut located).is_pending(),
            "still waiting on the first location just before the bound"
        );
        assert_eq!(sites.running(), 1);
        clock.advance(SignedDuration::from_nanos(1));
        let futures::task::Poll::Ready(located) = futures::poll!(&mut located) else {
            panic!("the choice still waits at the bound");
        };
        let record = located
            .ok()
            .and_then(|located| located.record)
            .expect("a location used");
        let outcomes: Vec<LocationOutcome> =
            record.locations.iter().map(|tried| tried.outcome).collect();
        assert_eq!(
            outcomes,
            vec![LocationOutcome::Silent, LocationOutcome::Used]
        );
        assert_eq!(
            sites.running(),
            0,
            "the silent location's worker is stopped"
        );
    }

    /// A person's Test waits for every location's probe, those started
    /// early too: a later one still running when an earlier one is used is
    /// waited for to its own bound, not stopped (decision 86).
    #[tokio::test]
    async fn a_test_waits_for_a_later_early_probe_after_an_earlier_location_is_used() {
        let sites = Sites::silent_at("/srv/public");
        let (choose, _access, clock) = choosing(&sites).await;
        let saved = SavedEntry {
            alias: RepositoryAlias::parse("nas").expect("an alias"),
            entry: RepositoryConfig {
                settings: first("/srv/lan", &[]),
                locations: Locations {
                    first_when: None,
                    further: vec![Location {
                        repository: "/srv/public".to_owned(),
                        ..Location::default()
                    }],
                },
                id: Some(RepositoryId::parse(ID).expect("an ID")),
                maintenance: Maintenance::None,
                group_by: GroupBy::default(),
                hooks: Hooks::default(),
            },
        };
        let mut tested = choose.test_each(op(), &saved).boxed();
        assert!(
            futures::poll!(&mut tested).is_pending(),
            "the second location's probe is waited for"
        );
        clock.advance(SignedDuration::from_secs(LOCATION_PROBE_SECONDS));
        let futures::task::Poll::Ready(tested) = futures::poll!(&mut tested) else {
            panic!("the test still waits at the second location's bound");
        };
        let outcomes: Vec<LocationOutcome> = tested
            .expect("a record of the test")
            .locations
            .iter()
            .map(|tried| tried.outcome)
            .collect();
        assert_eq!(
            outcomes,
            vec![LocationOutcome::Used, LocationOutcome::Silent]
        );
        assert_eq!(sites.running(), 0);
    }

    /// A job cancelled while its choice waits on a location's probe ends at
    /// once, without the clock moving on, with no event and no job run, and
    /// leaves no probe worker running: the cancel reaches the probe.
    #[tokio::test]
    async fn a_job_cancelled_while_its_choice_probes_ends_and_leaves_no_probe_running() {
        let sites = Sites::silent_at("/srv/lan");
        let (choose, access, _clock) = choosing(&sites).await;
        let RunningJob {
            mut events,
            mut control,
        } = choose
            .run(op(), Job::Ls(access))
            .await
            .expect("the job starts");
        assert!(futures::poll!(events.next()).is_pending());
        assert_eq!(sites.running(), 1, "the first location's probe runs");
        control.stop().await;
        assert_eq!(sites.running(), 0, "the cancel stops the probe");
        assert!(
            matches!(events.next().now_or_never(), Some(None)),
            "the job ends at once, with nothing to say"
        );
        assert_eq!(sites.running(), 0);
        assert!(
            sites.jobs().iter().all(|job| matches!(job, Job::Probe(_))),
            "no job opens the repository after a cancel"
        );
    }

    /// The addresses `sites` was asked to probe, in the order asked.
    fn probed(sites: &Sites) -> Vec<String> {
        sites
            .jobs()
            .into_iter()
            .filter_map(|job| match job {
                Job::Probe(access) => Some(access.repository),
                _ => None,
            })
            .collect()
    }

    /// Every location that checks the server first is probed from the start
    /// of the choice, so a later one runs while an earlier one is waited for
    /// (decision 86); the earlier one still decides, at its own bound.
    #[tokio::test]
    async fn a_later_location_that_checks_the_server_first_is_probed_while_an_earlier_one_is_waited_for()
     {
        let sites = Sites::silent_at("/srv/lan");
        let (choose, access, clock) = choosing(&sites).await;
        let mut located = choose.locate(op(), access).boxed();
        assert!(futures::poll!(&mut located).is_pending());
        assert_eq!(
            probed(&sites),
            vec!["/srv/lan".to_owned(), "/srv/public".to_owned()],
            "both locations are probed at once"
        );
        clock.advance(SignedDuration::from_secs(LOCATION_PROBE_SECONDS));
        let futures::task::Poll::Ready(located) = futures::poll!(&mut located) else {
            panic!("the choice still waits at the first location's bound");
        };
        let record = located
            .ok()
            .and_then(|located| located.record)
            .expect("a location used");
        let outcomes: Vec<LocationOutcome> =
            record.locations.iter().map(|tried| tried.outcome).collect();
        assert_eq!(
            outcomes,
            vec![LocationOutcome::Silent, LocationOutcome::Used]
        );
        assert_eq!(sites.running(), 0);
        assert_eq!(probed(&sites).len(), 2, "no location is probed twice");
    }

    /// A probe started early that is still running when an earlier location
    /// is used is stopped before the choice ends, and its location is
    /// recorded as not tried (decision 86).
    #[tokio::test]
    async fn a_probe_still_running_when_an_earlier_location_is_used_is_stopped() {
        let sites = Sites::silent_at("/srv/public");
        let (choose, access, _clock) = choosing(&sites).await;
        let located = choose
            .locate(op(), access)
            .now_or_never()
            .expect("the first location answers at once, so the choice ends");
        let record = located
            .ok()
            .and_then(|located| located.record)
            .expect("a location used");
        let outcomes: Vec<LocationOutcome> =
            record.locations.iter().map(|tried| tried.outcome).collect();
        assert_eq!(
            outcomes,
            vec![LocationOutcome::Used, LocationOutcome::NotTried]
        );
        assert_eq!(
            probed(&sites),
            vec!["/srv/lan".to_owned(), "/srv/public".to_owned()],
            "the second location was probed early"
        );
        assert_eq!(sites.running(), 0, "its probe was stopped");
    }

    /// A job cancelled while its choice waits on several probes at once
    /// stops every one of them.
    #[tokio::test]
    async fn a_job_cancelled_while_several_probes_run_stops_every_one() {
        let sites = Sites {
            silent: vec!["/srv/lan".to_owned(), "/srv/public".to_owned()],
            ..Sites::default()
        };
        let (choose, access, _clock) = choosing(&sites).await;
        let RunningJob {
            mut events,
            mut control,
        } = choose
            .run(op(), Job::Ls(access))
            .await
            .expect("the job starts");
        assert!(futures::poll!(events.next()).is_pending());
        assert_eq!(sites.running(), 2, "both probes run");
        control.stop().await;
        assert_eq!(sites.running(), 0, "the cancel stops both");
        assert!(
            matches!(events.next().now_or_never(), Some(None)),
            "the job ends at once, with nothing to say"
        );
        assert!(
            sites.jobs().iter().all(|job| matches!(job, Job::Probe(_))),
            "no job opens the repository after a cancel"
        );
    }

    /// A choice that fails in a location's turn, here on secrets past what
    /// the daemon masks, stops every probe still running before it returns:
    /// a later location's early probe doesn't outlive it. The first location
    /// is skipped at once, its reference unresolved, so the third's probe
    /// still runs when the second's turn fails.
    #[tokio::test]
    async fn a_choice_that_fails_in_a_locations_turn_stops_every_early_probe() {
        let sites = Sites::silent_at("/srv/far");
        let secrets = FakeSecrets::default();
        secrets.set_env("NAS_PASSWORD", "demo-password");
        let mut options = BTreeMap::new();
        for n in 1..=5 {
            secrets.set_env(&format!("DEMO_LARGE_{n}"), &n.to_string().repeat(60_000));
            options.insert(format!("demo-{n}"), format!("${{DEMO_LARGE_{n}}}"));
        }
        let resolve = ResolveRepository::new(Arc::new(secrets));
        let mut access = nas(&resolve, first("${DEMO_LAN}", &[])).await;
        let saved = access.entry.as_mut().expect("the entry");
        saved.entry.locations.further = vec![
            Location {
                repository: "rest:http://backup.example.net/r".to_owned(),
                options,
                ..Location::default()
            },
            Location {
                repository: "/srv/far".to_owned(),
                ..Location::default()
            },
        ];
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        let choose = ChooseLocation::new(
            Arc::new(sites.clone()),
            Arc::<FakeClock>::clone(&clock),
            Arc::new(FakeNetworkLink::new()),
            resolve,
            Arc::new(|_| {}),
        );
        let located = choose
            .locate(op(), access)
            .now_or_never()
            .expect("the second location's turn comes at once and fails the choice");
        assert_eq!(
            probed(&sites),
            vec!["/srv/far".to_owned()],
            "the local path was probed early, and nothing in its turn"
        );
        assert!(
            matches!(
                located,
                Err((FailureReason::SecretsTooLargeToMask { .. }, _))
            ),
            "the second location's secrets fail the choice"
        );
        assert_eq!(sites.running(), 0, "the third location's probe is stopped");
    }

    /// A location that checks the server first as written but not as its
    /// references resolve, here an `https` location whose certificate check
    /// a reference turns off, waits for its turn; a later local path is
    /// still probed at once.
    #[tokio::test]
    async fn a_location_whose_resolved_settings_send_credentials_unchecked_waits_for_its_turn() {
        let sites = Sites::silent_at("/srv/lan");
        let secrets = FakeSecrets::default();
        secrets.set_env("NAS_PASSWORD", "demo-password");
        secrets.set_env("UNCHECKED", "true");
        let resolve = ResolveRepository::new(Arc::new(secrets));
        let mut access = nas(&resolve, first("/srv/lan", &[])).await;
        let saved = access.entry.as_mut().expect("the entry");
        saved.entry.locations.further = vec![
            Location {
                repository: "rest:https://demo:pw@backup.example.net/r".to_owned(),
                options: BTreeMap::from([(
                    "http-insecure-tls".to_owned(),
                    "${UNCHECKED}".to_owned(),
                )]),
                ..Location::default()
            },
            Location {
                repository: "/srv/far".to_owned(),
                ..Location::default()
            },
        ];
        let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
        let choose = ChooseLocation::new(
            Arc::new(sites.clone()),
            Arc::<FakeClock>::clone(&clock),
            Arc::new(FakeNetworkLink::new()),
            resolve,
            Arc::new(|_| {}),
        );
        let mut located = choose.locate(op(), access).boxed();
        assert!(futures::poll!(&mut located).is_pending());
        assert_eq!(
            probed(&sites),
            vec!["/srv/lan".to_owned(), "/srv/far".to_owned()],
            "the unchecked location isn't probed before its turn"
        );
        clock.advance(SignedDuration::from_secs(LOCATION_PROBE_SECONDS));
        let futures::task::Poll::Ready(located) = futures::poll!(&mut located) else {
            panic!("the choice still waits");
        };
        assert_eq!(
            probed(&sites),
            vec![
                "/srv/lan".to_owned(),
                "/srv/far".to_owned(),
                "rest:https://demo:pw@backup.example.net/r".to_owned()
            ],
            "it is probed in its turn"
        );
        let used = located
            .ok()
            .and_then(|located| located.record)
            .and_then(|record| record.used());
        assert_eq!(used, Some(2));
    }

    /// The job that follows the choice opens the location chosen, alone,
    /// and carries the entry it was made from, as every job made from one
    /// does (§6.1, decision 57).
    #[tokio::test]
    async fn the_job_after_the_choice_opens_the_location_chosen_and_carries_its_entry() {
        let sites = Sites::default();
        let (choose, access, _clock) = choosing(&sites).await;
        let entry = access.entry.clone();
        let RunningJob { events, .. } = choose
            .run(op(), Job::Ls(access))
            .now_or_never()
            .expect("the choice ends at once")
            .expect("the job starts");
        drop(
            events
                .collect::<Vec<_>>()
                .now_or_never()
                .expect("the job's events end at once"),
        );
        let opened: Vec<RepositoryAccess> = sites
            .jobs()
            .into_iter()
            .filter_map(|job| match job {
                Job::Ls(access) => Some(access),
                _ => None,
            })
            .collect();
        let [opened] = opened.as_slice() else {
            panic!("one job opens the repository: {opened:?}");
        };
        assert_eq!(opened.repository, "/srv/lan");
        assert_eq!(opened.id, Some(RepositoryId::parse(ID).expect("an ID")));
        assert!(entry.is_some());
        assert_eq!(opened.entry, entry);
    }
}
