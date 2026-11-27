//! What a hub asks of a machine (technical design §7.2, §10.3; §16,
//! decisions 120 and 176): run a plan's backup now, or cancel a backup it
//! runs. A machine says in its `Hello` which commands it takes; one that
//! says none, as a build before them, takes none. Each command carries an
//! ID and the hub's time when it was asked, both signed: the machine acts
//! on one asked within [`COMMAND_LIFE`] of the hub's time as it judges it,
//! once, and answers each with its result.

use std::collections::BTreeSet;

use super::{EpochSeconds, WireName};
use crate::access::SessionTag;
use crate::{OperationId, PlanId, SignedDuration, Timestamp, Uuid};

/// How far a command's time may lie from the hub's time when the machine
/// takes it, either way: an older one is a replay or a command that waited
/// too long, and one further ahead no hub of this protocol sends.
pub const COMMAND_LIFE: SignedDuration = SignedDuration::from_mins(15);

/// How long a machine remembers the ID of a command it took, on its own
/// clock: far past [`COMMAND_LIFE`], so a hub whose clock was set back
/// can't have an old command acted on again.
pub const COMMANDS_REMEMBERED_FOR: SignedDuration = SignedDuration::from_hours(24);

/// The most command IDs a machine remembers: a hub that sends more within
/// [`COMMANDS_REMEMBERED_FOR`] has the rest refused, unremembered and not
/// acted on, so a hostile hub can't fill the machine's database.
pub const MAX_COMMANDS_REMEMBERED: usize = 1_000;

/// The most commands a hub waits on the answer of from one machine at once,
/// and the most a machine holds unanswered: one more makes it drop the
/// oldest, which the hub has stopped waiting for, answering it expired.
pub const MAX_COMMANDS_IN_FLIGHT: usize = 8;

/// The most commands a hub sends one machine in an hour. A machine
/// remembers each command's ID for [`COMMANDS_REMEMBERED_FOR`], and at most
/// [`MAX_COMMANDS_REMEMBERED`] of them: a hub that sends no more than this
/// never fills that ledger, whatever its commands come to.
pub const HUB_COMMANDS_PER_HOUR: usize = 40;

/// How long a hub waits for a machine's answer to a command.
pub const COMMAND_ANSWER_WITHIN: SignedDuration = SignedDuration::from_secs(20);

/// How long after a backup of a plan the hub started a machine runs no
/// other the hub asks for of that plan.
pub const HUB_RUN_SPACING: SignedDuration = SignedDuration::from_mins(5);

/// The most backups the hub starts on a machine in an hour.
pub const HUB_RUNS_PER_HOUR: usize = 12;

/// The most names a `Hello`'s `accepts` holds: more refuses the `Hello`.
pub const MAX_ACCEPTS: usize = 16;

/// The kind of the hub's command to run a plan's backup.
pub fn run_plan_kind() -> WireName {
    WireName::known("RunPlan")
}

/// The kind of the hub's command to cancel an operation.
pub fn cancel_op_kind() -> WireName {
    WireName::known("CancelOp")
}

/// The kind of the machine's answer to a command.
pub fn command_result_kind() -> WireName {
    WireName::known("CommandResult")
}

/// A command a hub may send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandKind {
    RunPlan,
    CancelOp,
}

impl CommandKind {
    /// Every command this build takes.
    pub const ALL: [Self; 2] = [Self::RunPlan, Self::CancelOp];

    /// Its name in a `Hello`'s `accepts`.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::RunPlan => "run-plan",
            Self::CancelOp => "cancel-op",
        }
    }

    /// The command a name in `accepts` stands for; none for one this build
    /// doesn't know.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.wire_name() == name)
    }
}

/// The commands a machine takes from its hub, as its `Hello` names them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accepts(BTreeSet<CommandKind>);

/// An `accepts` with more than [`MAX_ACCEPTS`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyAccepted;

impl Accepts {
    /// None: a machine that turned its hub's commands off.
    pub fn none() -> Self {
        Self::default()
    }

    /// Every command this build takes.
    pub fn all() -> Self {
        Self(CommandKind::ALL.into_iter().collect())
    }

    /// Whether the machine takes `kind`.
    pub fn takes(&self, kind: CommandKind) -> bool {
        self.0.contains(&kind)
    }

    /// The commands it takes, in a fixed order.
    pub fn kinds(&self) -> impl Iterator<Item = CommandKind> + '_ {
        self.0.iter().copied()
    }

    /// The commands `names` stand for: a name this build doesn't know, as
    /// one a newer machine takes, is passed over, and more than
    /// [`MAX_ACCEPTS`] names are refused.
    pub fn from_names<'a>(
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, TooManyAccepted> {
        let mut kinds = BTreeSet::new();
        for (count, name) in names.into_iter().enumerate() {
            if count >= MAX_ACCEPTS {
                return Err(TooManyAccepted);
            }
            kinds.extend(CommandKind::from_wire_name(name));
        }
        Ok(Self(kinds))
    }
}

/// The ID a hub gives a command: a UUIDv7 it makes when it asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(Uuid);

impl CommandId {
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// The UUIDv7 of `unix_ms`, the hub's clock in milliseconds, and
    /// `random`, bytes from its `Random` port.
    pub fn v7(unix_ms: u64, random: &[u8; 10]) -> Self {
        Self(uuid::Builder::from_unix_timestamp_millis(unix_ms, random).into_uuid())
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

/// What a hub asks of a machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubCommand {
    /// Run a backup of `plan` now, as its machine's Back up now does.
    RunPlan { plan: PlanId },
    /// Cancel the backup `op`.
    CancelOp { op: OperationId },
}

impl HubCommand {
    pub const fn kind(&self) -> CommandKind {
        match self {
            Self::RunPlan { .. } => CommandKind::RunPlan,
            Self::CancelOp { .. } => CommandKind::CancelOp,
        }
    }
}

/// One command as the hub signs it: its ID, the hub's time when it was
/// asked, which of the hub's sessions asked, when sign-in is on, and what
/// it asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRequest {
    pub id: CommandId,
    pub requested_at: EpochSeconds,
    pub asked_by: Option<SessionTag>,
    pub command: HubCommand,
}

/// Whether a command asked at `requested_at` may be acted on now, on the
/// hub's time: the machine's connection was welcomed by a `Welcome` signed
/// at `welcomed_at`, `since` ago on the machine's monotonic clock. It may
/// lie at most [`COMMAND_LIFE`] from the hub's time either way, whatever
/// the machine's wall clock says (decision 120, "Expiry is judged on the
/// hub's time").
pub fn command_fresh(
    welcomed_at: EpochSeconds,
    since: SignedDuration,
    requested_at: EpochSeconds,
) -> bool {
    let hub_now = i128::from(welcomed_at.seconds()) + i128::from(since.as_secs());
    let apart = (hub_now - i128::from(requested_at.seconds())).abs();
    apart <= i128::from(COMMAND_LIFE.as_secs())
}

/// A backup the hub started on a machine: of which plan, and when the
/// machine took the command, on its clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubRun {
    pub plan: PlanId,
    pub at: Timestamp,
}

/// Whether the hub may start a backup of `plan` now, given `runs`, the
/// backups it started on the machine: none of `plan` within
/// [`HUB_RUN_SPACING`], and fewer than [`HUB_RUNS_PER_HOUR`] within an
/// hour. A run the machine's clock puts ahead of now, as after a clock set
/// back, counts by how far it lies either way. Refused, it says when the
/// hub may start one again.
pub fn hub_run_admitted(runs: &[HubRun], plan: &PlanId, now: Timestamp) -> Result<(), Timestamp> {
    let within = |run: &HubRun, span: SignedDuration| now.duration_since(run.at).abs() < span;
    let spaced = runs
        .iter()
        .filter(|run| run.plan == *plan && within(run, HUB_RUN_SPACING))
        .filter_map(|run| run.at.checked_add(HUB_RUN_SPACING).ok())
        .max();
    let mut hour: Vec<Timestamp> = runs
        .iter()
        .filter(|run| within(run, SignedDuration::from_hours(1)))
        .map(|run| run.at)
        .collect();
    hour.sort_unstable();
    let crowded = hour
        .len()
        .checked_sub(HUB_RUNS_PER_HOUR)
        .and_then(|past| hour.get(past))
        .and_then(|oldest| oldest.checked_add(SignedDuration::from_hours(1)).ok());
    match spaced.into_iter().chain(crowded).max() {
        Some(again) => Err(again),
        None => Ok(()),
    }
}

/// Whether a hub may send a machine one more command at `now`, on the
/// hub's monotonic clock, having sent it commands at `sent`: fewer than
/// [`HUB_COMMANDS_PER_HOUR`] within the hour before. Refused, it says when
/// on that clock the hub may send one again.
pub fn hub_command_admitted(
    sent: &[SignedDuration],
    now: SignedDuration,
) -> Result<(), SignedDuration> {
    todo!("{sent:?} {now:?}")
}

#[cfg(test)]
mod tests;
