//! An agent's reports to its hub (technical design §7.2, §4.2; §16, decision
//! 120, "Signed messages after the handshake"): what the agent's operations
//! did enters its persistent outbox as it happens, whether or not the hub
//! can be reached, and a sync session replays what the hub hasn't
//! acknowledged, in order, once the hub has approved the agent. Delivery is
//! at least once: a report leaves the outbox only when the hub's signed
//! acknowledgement covers it, and the hub takes a report it took before as
//! a duplicate. A session then tells the hub the agent's plans when they
//! changed since it last acknowledged them, and the progress of the
//! operation it runs (`live`).

use std::sync::{Arc, Mutex, PoisonError};

use api::UiEvent;
use domain::fleet::{
    AckRefused, AgentReport, Approval, EpochSeconds, Happening, Hostname, MAX_OUTBOX_REPORTS,
    MachineCheck, MachineZone, PlansReport, ReportedProgress, SEND_WINDOW, WireName, check_ack,
};
use domain::maintenance::{RepositoryReport, RepositoryReports};
use domain::planning::Maintenance;
use domain::{AgentId, OperationId, SignedDuration, Timestamp, Uuid};

use crate::leases::MaintenanceLeases;
use crate::machine::MachineStanding;
use crate::pairing::{AgentChannel, ChannelError, ConnectFailure, ConnectToHub, Connected};
use crate::ports::{Appended, Clock, ConfigError, ConfigStore, OperationStore, Outbox, StoreError};

mod live;

pub use live::{LiveProgress, PLANS_CHECK, PROGRESS_EVERY, ReadPlans};

/// How long the agent waits before it tries its hub again after the first
/// failure; each failure after doubles it, up to [`LONGEST_BACKOFF`].
pub const FIRST_BACKOFF: SignedDuration = SignedDuration::from_secs(1);
/// The longest the agent waits between tries.
pub const LONGEST_BACKOFF: SignedDuration = SignedDuration::from_secs(300);
/// How often the agent looks for reports in its outbox while it has
/// nothing to send, and how long it waits after a session that sent them.
pub const POLL: SignedDuration = SignedDuration::from_secs(2);
/// How often a machine paired with a hub asks for its grants when it has
/// no report to send: far inside a lease's life, so a lease the hub keeps
/// renewing never runs out while the hub can be reached.
pub const GRANTS_EVERY: SignedDuration = SignedDuration::from_mins(15);

/// Puts each report the hub needs into the outbox as the agent's operations
/// make it.
pub struct RecordReports {
    outbox: Arc<dyn Outbox>,
}

impl RecordReports {
    pub fn new(outbox: Arc<dyn Outbox>) -> Self {
        Self { outbox }
    }

    /// The report `event` makes: an operation's start or end; none for its
    /// progress, its warnings or anything else the hub doesn't need, nor
    /// for an event whose ID or time doesn't read.
    pub fn report_of(event: &UiEvent) -> Option<AgentReport> {
        let (operation, happening, at) = match event {
            UiEvent::OperationStarted {
                operation,
                started_at,
                ..
            } => (operation, Happening::Started, started_at),
            UiEvent::OperationSucceeded {
                operation,
                finished_at,
            } => (operation, Happening::Succeeded, finished_at),
            UiEvent::OperationFailed {
                operation,
                code,
                finished_at,
                ..
            } => (
                operation,
                Happening::Failed {
                    code: WireName::parse(&api::sync::code_name(*code)).ok()?,
                },
                finished_at,
            ),
            _ => return None,
        };
        Some(AgentReport {
            op_id: OperationId::from_uuid(Uuid::try_parse(operation).ok()?),
            happening,
            at: EpochSeconds::from_timestamp(at.parse::<Timestamp>().ok()?)?,
        })
    }

    /// Puts `event`'s report into the outbox, none for an event that makes
    /// none. A full outbox loses no report silently: the start it gives up
    /// for this one, or this one when it refuses it, is logged as a
    /// warning. Neither fails the operation that made the event.
    pub async fn record(&self, event: &UiEvent) -> Result<Option<Appended>, StoreError> {
        let Some(report) = Self::report_of(event) else {
            return Ok(None);
        };
        let appended = self.outbox.append(&report).await?;
        let operation = report.op_id.as_uuid();
        match appended {
            Appended::Kept(_) => {}
            Appended::KeptGivingUp { gave_up, .. } => log::warn!(
                "the outbox holds {MAX_OUTBOX_REPORTS} reports the hub hasn't acknowledged: \
                 it gave up its oldest start, at place {}, to keep a report of operation \
                 {operation}",
                gave_up.get(),
            ),
            Appended::Refused => log::warn!(
                "the outbox holds {MAX_OUTBOX_REPORTS} ends the hub hasn't acknowledged, or \
                 ran out of places: a report of operation {operation} wasn't kept"
            ),
        }
        Ok(Some(appended))
    }
}

/// What a sync session came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synced {
    /// The hub acknowledged every report the session sent, `sent` of them.
    Replayed { sent: u64 },
    /// The hub holds the agent pending: it sent nothing, and the reports
    /// wait in the outbox for the approval.
    Pending,
}

/// Why a sync session ended before the hub acknowledged what it sent. The
/// reports stay in the outbox for the next session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncFailure {
    Connect(ConnectFailure),
    /// The connection ended, or carried a message the agent acts on none of.
    Channel(ChannelError),
    /// The hub acknowledged what this session didn't send.
    Ack(AckRefused),
    Store(StoreError),
    /// The configuration, which names the repositories whose leases the
    /// machine asks for, couldn't be read: the leases it holds are kept
    /// as they were.
    Config(Box<ConfigError>),
}

/// The agent's side of the sync: one session at a time with the hub it
/// pinned, replaying its outbox, then telling its plans and progress.
pub struct SyncWithHub {
    /// Where the machine stands, which each session tells what the hub made
    /// of it, when the sync is given it.
    standing: Option<Arc<MachineStanding>>,
    connect: ConnectToHub,
    outbox: Arc<dyn Outbox>,
    clock: Arc<dyn Clock>,
    agent_id: AgentId,
    hostname: Hostname,
    zone: Option<MachineZone>,
    /// The machine's leases, when it asks its hub for them.
    leases: Option<Leasing>,
    plans: Option<ReadPlans>,
    progress: Option<Arc<LiveProgress>>,
    told: Mutex<Told>,
}

/// What a session needs to ask for the machine's leases: the repositories
/// it uses, from its configuration, where it keeps what it is granted, and
/// the maintenance running here, which keeps its lease from a release.
struct Leasing {
    config: Arc<dyn ConfigStore>,
    leases: Arc<MaintenanceLeases>,
    operations: Arc<dyn OperationStore>,
}

/// What the agent told its hub besides its reports, on its monotonic clock.
#[derive(Default)]
struct Told {
    /// The plans the hub last acknowledged; none since the agent started.
    plans: Option<PlansReport>,
    /// When the agent last read its plans.
    plans_read: Option<SignedDuration>,
    /// Whether the last read of its plans failed, so a streak of failures
    /// is logged once.
    plans_unreadable: bool,
    /// The progress last sent, and when.
    progress: Option<(ReportedProgress, SignedDuration)>,
}

impl SyncWithHub {
    pub fn new(
        connect: ConnectToHub,
        outbox: Arc<dyn Outbox>,
        clock: Arc<dyn Clock>,
        agent_id: AgentId,
        hostname: Hostname,
        zone: Option<MachineZone>,
    ) -> Self {
        Self {
            standing: None,
            connect,
            outbox,
            clock,
            agent_id,
            hostname,
            zone,
            leases: None,
            plans: None,
            progress: None,
            told: Mutex::new(Told::default()),
        }
    }

    /// Tells the hub the plans `plans` reads, whenever they changed.
    #[must_use]
    pub fn with_plans(mut self, plans: ReadPlans) -> Self {
        self.plans = Some(plans);
        self
    }

    /// Tells the hub the progress `progress` keeps, at most every
    /// [`PROGRESS_EVERY`].
    #[must_use]
    pub fn with_progress(mut self, progress: Arc<LiveProgress>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// This sync asking, in each session, for the leases on the
    /// repositories `config` asks to maintain here, and keeping them in
    /// `leases`: a lease the configuration no longer asks for is released
    /// first, unless `operations` has a maintenance of its repository
    /// running, then every repository whose ID is known is reported and the
    /// hub's grants taken (§7.2, §7.3, I1).
    #[must_use]
    pub fn with_leases(
        mut self,
        config: Arc<dyn ConfigStore>,
        leases: Arc<MaintenanceLeases>,
        operations: Arc<dyn OperationStore>,
    ) -> Self {
        self.leases = Some(Leasing {
            config,
            leases,
            operations,
        });
        self
    }

    /// Releases what the configuration no longer asks to maintain here,
    /// reports every repository it uses, and takes the hub's grants.
    async fn exchange_leases(
        &self,
        leasing: &Leasing,
        connected: &Connected,
        channel: &mut AgentChannel,
    ) -> Result<(), SyncFailure> {
        let config = leasing
            .config
            .load()
            .await
            .map_err(|error| SyncFailure::Config(Box::new(error)))?;
        let mut reports = Vec::new();
        let mut local = Vec::new();
        for repository in config.repositories().values() {
            let Some(id) = &repository.id else { continue };
            if repository.maintenance == Maintenance::Local {
                local.push(id.clone());
            }
            // Whether the repository holds another machine's snapshots
            // isn't known here yet (#1465): the hub's other check, another
            // machine's report of it, still refuses a shared one.
            reports.push(RepositoryReport {
                repository: id.clone(),
                request: repository.maintenance,
                holds_other_agents_snapshots: false,
            });
        }
        let held = leasing.leases.held().await.map_err(SyncFailure::Store)?;
        for (repository, epoch) in held {
            if local.contains(&repository) {
                continue;
            }
            // A release lets the hub grant the repository to another
            // machine, so none goes out while a maintenance of it runs
            // here: the hub counts the lease until it runs out instead.
            let running = leasing
                .operations
                .running_maintenance(Some(&repository))
                .await
                .map_err(SyncFailure::Store)?;
            if running.is_some() {
                continue;
            }
            // Let go first: a release the hub takes but whose answer is
            // lost must leave nothing here to maintain under.
            leasing
                .leases
                .released(&repository, epoch)
                .await
                .map_err(SyncFailure::Store)?;
            channel
                .send_release(&repository, epoch)
                .await
                .map_err(SyncFailure::Channel)?;
            let released = channel
                .receive_released()
                .await
                .map_err(SyncFailure::Channel)?;
            if released != (repository.clone(), epoch) {
                return Err(SyncFailure::Channel(ChannelError::OutOfTurn));
            }
        }
        // The machine's clock before the request leaves: the hub signs
        // after, so the lease the machine counts never outlasts the hub's.
        let asked_at = self.clock.now();
        channel
            .send_repos(&RepositoryReports::new(reports))
            .await
            .map_err(SyncFailure::Channel)?;
        let grants = channel
            .receive_grants()
            .await
            .map_err(SyncFailure::Channel)?;
        let refused = leasing
            .leases
            .take(&grants, connected.pin.recovery_number, asked_at)
            .await
            .map_err(SyncFailure::Store)?;
        for lower in refused {
            log::warn!(
                "the hub's grant of repository {} was refused (epoch-lower): recovery {}, \
                 generation {}, epoch {} lies below the highest this machine has seen, \
                 recovery {}, generation {}, epoch {}; the hub may have been rewound or \
                 restored from an older copy, and this machine maintains the repository no \
                 more until a grant at or above it",
                lower.repository.as_str(),
                lower.granted.recovery_number,
                lower.granted.generation,
                lower.granted.epoch.get(),
                lower.floor.recovery_number,
                lower.floor.generation,
                lower.floor.epoch.get(),
            );
        }
        Ok(())
    }

    /// This sync telling `standing` what its hub made of the machine in
    /// each session: a refusal as a second computer, verified as signed by
    /// the paired hub, or a connection the hub took.
    #[must_use]
    pub fn with_standing(mut self, standing: Arc<MachineStanding>) -> Self {
        self.standing = Some(standing);
        self
    }

    /// One session: connects, sends every report not yet acknowledged, in
    /// order, in windows of at most [`SEND_WINDOW`] places ahead of the
    /// hub's acknowledgements, and once the hub acknowledged the last of
    /// them, the agent's plans when they changed since the hub last
    /// acknowledged them, waiting for that acknowledgement, and then the
    /// latest progress when one is due. A pending agent sends nothing.
    pub async fn sync_once(&self) -> Result<Synced, SyncFailure> {
        let connected = self
            .connect
            .connect_channel(self.agent_id, self.hostname.clone(), self.zone.clone())
            .await;
        if let Some(standing) = &self.standing {
            match &connected {
                // The hub's `Welcome` held: it took this computer.
                Ok(_) => standing.hub_took(),
                // Only a refusal the paired hub signed reaches here: one it
                // didn't is `Unsigned`, and unlocks nothing.
                Err(ConnectFailure::Refused(refused))
                    if refused.code == MachineCheck::refusal() =>
                {
                    standing.hub_refused();
                }
                Err(_) => {}
            }
        }
        let (connected, mut channel) = connected.map_err(SyncFailure::Connect)?;
        if connected.approval == Approval::Pending {
            return Ok(Synced::Pending);
        }
        if let Some(leasing) = &self.leases {
            self.exchange_leases(leasing, &connected, &mut channel)
                .await?;
        }
        let sent = self.replay(&mut channel).await?;
        if let Some(plans) = self.plans_to_tell().await {
            channel
                .send_plans(&plans)
                .await
                .map_err(SyncFailure::Channel)?;
            channel
                .receive_plans_ack()
                .await
                .map_err(SyncFailure::Channel)?;
            self.told().plans = Some(plans);
        }
        if let Some(progress) = self.progress_to_tell() {
            channel
                .send_progress(&progress)
                .await
                .map_err(SyncFailure::Channel)?;
            self.told().progress = Some((progress, self.clock.monotonic()));
        }
        Ok(Synced::Replayed { sent })
    }

    /// Sends every report not yet acknowledged, and returns how many once
    /// the hub acknowledged the last of them.
    async fn replay(&self, channel: &mut AgentChannel) -> Result<u64, SyncFailure> {
        let mut sent = 0;
        // Each window starts after the last place acknowledged, so a session
        // goes through the outbox once even if acknowledging kept nothing.
        let mut after = None;
        loop {
            let mut window = self
                .outbox
                .unacknowledged(after, SEND_WINDOW)
                .await
                .map_err(SyncFailure::Store)?;
            // A window spans places, not reports: the hub tells a report sent
            // again from an older database's by how far behind its last it
            // lies, and the gaps a full outbox leaves would stretch it.
            if let Some(start) = window.first().map(|entry| entry.seq.get()) {
                let span = u64::try_from(SEND_WINDOW).unwrap_or(u64::MAX);
                window.retain(|entry| entry.seq.get().saturating_sub(start) < span);
            }
            let (Some(first), Some(last)) = (window.first(), window.last()) else {
                return Ok(sent);
            };
            let (epoch, through) = (first.epoch, last.seq);
            after = Some(through);
            for entry in &window {
                channel
                    .send_report(entry)
                    .await
                    .map_err(SyncFailure::Channel)?;
                sent += 1;
            }
            loop {
                let ack = channel.receive_ack().await.map_err(SyncFailure::Channel)?;
                check_ack(epoch, Some(through), ack).map_err(SyncFailure::Ack)?;
                self.outbox
                    .acknowledge(ack.epoch, ack.seq)
                    .await
                    .map_err(SyncFailure::Store)?;
                if ack.seq >= through {
                    break;
                }
            }
        }
    }

    fn told(&self) -> std::sync::MutexGuard<'_, Told> {
        self.told.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The agent's plans as they stand now, when they differ from those the
    /// hub last acknowledged; none when they don't, or don't read. A streak
    /// of failed reads is logged once.
    async fn plans_to_tell(&self) -> Option<PlansReport> {
        let read = self.plans.as_ref()?.read().await;
        let mut told = self.told();
        told.plans_read = Some(self.clock.monotonic());
        match read {
            Ok(plans) => {
                told.plans_unreadable = false;
                (told.plans.as_ref() != Some(&plans)).then_some(plans)
            }
            Err(error) => {
                if !told.plans_unreadable {
                    log::warn!("the plans for the hub couldn't be read: {error:?}");
                }
                told.plans_unreadable = true;
                None
            }
        }
    }

    /// Whether [`PLANS_CHECK`] passed since the agent last read its plans.
    fn plans_due(&self) -> bool {
        self.plans.is_some()
            && self
                .told()
                .plans_read
                .is_none_or(|read| self.clock.monotonic().saturating_sub(read) >= PLANS_CHECK)
    }

    /// The latest progress, when it isn't the one last sent and
    /// [`PROGRESS_EVERY`] passed since then.
    fn progress_to_tell(&self) -> Option<ReportedProgress> {
        let latest = self.progress.as_ref()?.latest()?;
        let now = self.clock.monotonic();
        match &self.told().progress {
            Some((sent, at)) if *sent == latest || now.saturating_sub(*at) < PROGRESS_EVERY => None,
            _ => Some(latest),
        }
    }

    /// Whether a session has anything to tell the hub besides reports: a
    /// progress due, or plans that changed once [`PLANS_CHECK`] passed.
    async fn live_due(&self) -> bool {
        self.progress_to_tell().is_some()
            || (self.plans_due() && self.plans_to_tell().await.is_some())
    }

    /// Sessions for as long as the agent runs: whenever its outbox holds a
    /// report the hub hasn't acknowledged, a progress is due, or its plans
    /// changed when it read them, every [`PLANS_CHECK`], a session tells
    /// the hub, and the agent looks again after [`POLL`]; a sync that asks
    /// for the machine's leases also holds a session at its start and once
    /// [`GRANTS_EVERY`] has passed since the last that held, on the
    /// machine's clock either way. A failed session, or one the hub holds
    /// pending, is tried again after [`backoff`]. It never touches the
    /// agent's operations, so an agent that can't reach its hub backs up
    /// on schedule all the same.
    pub async fn run(&self) {
        let mut wait = None;
        // Each streak of failures, and of unreadable outboxes, is logged
        // once, at its first.
        let mut failing = false;
        let mut unreadable = false;
        // When the last session that held started, for the leases' cadence.
        let mut leased_at: Option<Timestamp> = None;
        loop {
            let waiting = match self.outbox.unacknowledged(None, 1).await {
                Ok(waiting) => {
                    unreadable = false;
                    !waiting.is_empty()
                }
                Err(error) => {
                    if !unreadable {
                        log::warn!("the outbox couldn't be read: {error:?}");
                    }
                    unreadable = true;
                    false
                }
            };
            let started = self.clock.now();
            // A clock set back counts as time passed, so a lease is still
            // renewed.
            let leases_due = self.leases.is_some()
                && leased_at.is_none_or(|at| started.duration_since(at).abs() >= GRANTS_EVERY);
            if !waiting && !leases_due && !self.live_due().await {
                self.clock.sleep(POLL).await;
                continue;
            }
            let failure = match self.sync_once().await {
                Ok(Synced::Replayed { .. }) => {
                    wait = None;
                    failing = false;
                    leased_at = Some(started);
                    // Never straight into the next session: an outbox that
                    // kept what the hub acknowledged would otherwise have
                    // the loop replay it without pause.
                    self.clock.sleep(POLL).await;
                    continue;
                }
                Ok(Synced::Pending) => None,
                Err(failure) => Some(failure),
            };
            if !failing && let Some(failure) = &failure {
                log::warn!("the agent's reports didn't reach its hub: {failure:?}");
            }
            failing = failure.is_some();
            let next = backoff(wait);
            wait = Some(next);
            self.clock.sleep(next).await;
        }
    }
}

/// How long to wait after a failed session that followed a wait of
/// `previous`, none after a session that held: [`FIRST_BACKOFF`], doubled
/// each time up to [`LONGEST_BACKOFF`].
pub fn backoff(previous: Option<SignedDuration>) -> SignedDuration {
    previous.map_or(FIRST_BACKOFF, |previous| {
        previous
            .checked_mul(2)
            .map_or(LONGEST_BACKOFF, |doubled| doubled.min(LONGEST_BACKOFF))
    })
}

#[cfg(test)]
mod tests;
