//! The hub's dashboard (technical design §4.3, §4.4; #569): each approved
//! machine as the hub last listed it, or the copy taken when the reader
//! paused live updates (A3, decision 65), how many changes wait, what the
//! resume says, which events read the machines again, and how a machine's
//! times read beside the viewer's (#115). Decided here, on the host;
//! `dashboard_page.rs` reads the machines and draws them.
//!
//! As on the Machines tab (`machines_list.rs`), the page goes on reading
//! while it is paused and shows the copy; resuming drops it.

use api::{
    AgainstHubDto, HubMachineDto, IanaZone, MachineDto, MachineOperationDto, MachineOutcomeDto,
    MachinePlanDto, MachinePlanRunDto, MachinePlansDto, MachineProgressDto, MachineVersionDto,
    MsgArg, StatusDto, UiEvent,
};
use l10n::{MessageId, msg};
use leptos::prelude::*;

use crate::backend::{OperationsState, Progress};
use crate::components::State;
use crate::components::status_badge::rules::BadgeState;
use crate::wording::Words;

/// The machines as they were when the reader paused them, the hub's own
/// and the time the cards were read at with them.
#[derive(Clone)]
struct Held {
    machines: Vec<HubMachineDto>,
    state: State,
    own: Own,
    now: i64,
}

/// What the page has read of the hub's own machine: nothing yet, or its
/// card's machine, none when the hub didn't say who it is (decision 175).
/// Boxed so that a `Waiting` doesn't take a whole machine's room.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Own {
    #[default]
    Waiting,
    Read(Option<Box<HubMachineDto>>),
}

/// The machines as the hub last listed them, whether a read has answered,
/// and the copy the page shows while the reader has paused it.
#[derive(Clone, Copy)]
pub(crate) struct Fleet {
    /// The approved machines as the hub last listed them, oldest first.
    pub(crate) machines: RwSignal<Vec<HubMachineDto>>,
    /// Whether a read has answered.
    pub(crate) state: RwSignal<State>,
    /// The hub's own machine, as the page reads it.
    pub(crate) own: RwSignal<Own>,
    /// The time the cards are read at (ms since the Unix epoch).
    pub(crate) now: RwSignal<i64>,
    held: RwSignal<Option<Held>>,
}

impl Fleet {
    pub(crate) fn new() -> Self {
        Self {
            machines: RwSignal::new(Vec::new()),
            state: RwSignal::new(State::Loading),
            own: RwSignal::new(Own::Waiting),
            now: RwSignal::new(0),
            held: RwSignal::new(None),
        }
    }

    /// The machine ID of the hub's own card, as the page shows it.
    pub(crate) fn own_id(self) -> Signal<Option<String>> {
        let own = self.shown_own();
        Signal::derive(move || {
            own.with(|own| match own {
                Own::Read(Some(machine)) => Some(machine.machine_id.clone()),
                Own::Read(None) | Own::Waiting => None,
            })
        })
    }

    /// The time the cards the page shows are read at.
    pub(crate) fn shown_now(self) -> Signal<i64> {
        Signal::derive(move || {
            self.held
                .with(|held| held.as_ref().map(|held| held.now))
                .unwrap_or_else(|| self.now.get())
        })
    }

    /// The hub's own machine, as the page shows it.
    fn shown_own(self) -> Signal<Own> {
        Signal::derive(move || {
            self.held
                .with(|held| held.as_ref().map(|held| held.own.clone()))
                .unwrap_or_else(|| self.own.get())
        })
    }

    /// Holds the machines as they are now; a page already paused keeps the
    /// copy it took then.
    pub(crate) fn pause(self) {
        if self.held.with_untracked(Option::is_some) {
            return;
        }
        self.held.set(Some(Held {
            machines: self.machines.get_untracked(),
            state: self.state.get_untracked(),
            own: self.own.get_untracked(),
            now: self.now.get_untracked(),
        }));
    }

    /// Shows the latest machines again, and what is said of it: how many
    /// changed while the page was paused.
    pub(crate) fn resume(self) -> Words {
        let changed = self.pending_untracked();
        self.held.set(None);
        resumed(changed)
    }

    /// Whether the reader has paused the page.
    pub(crate) fn paused(self) -> bool {
        self.held.with(Option::is_some)
    }

    /// Whether a read has answered, as the page shows it: the page waits
    /// for the hub's own machine too.
    pub(crate) fn shown_state(self) -> Signal<State> {
        Signal::derive(move || {
            let (state, own_read) = self
                .held
                .with(|held| {
                    held.as_ref()
                        .map(|held| (held.state, held.own != Own::Waiting))
                })
                .unwrap_or_else(|| (self.state.get(), self.own.with(|own| *own != Own::Waiting)));
            if state == State::Ready && !own_read {
                State::Loading
            } else {
                state
            }
        })
    }

    /// The machines the page shows: the hub's own first, then each
    /// approved one, oldest first.
    pub(crate) fn shown(self) -> Signal<Vec<HubMachineDto>> {
        let own = self.shown_own();
        Signal::derive(move || {
            let mut machines = self.held.with(|held| match held {
                Some(held) => held.machines.clone(),
                None => self.machines.get(),
            });
            if let Own::Read(Some(own)) = own.get() {
                // Never twice, should the hub list itself among its machines.
                machines.retain(|machine| machine.machine_id != own.machine_id);
                machines.insert(0, *own);
            }
            machines
        })
    }

    /// How many changes wait for the resume, the hub's own machine's among
    /// them; none when not paused.
    pub(crate) fn pending(self) -> usize {
        self.held.with(|held| {
            held.as_ref().map_or(0, |held| {
                self.machines
                    .with(|machines| changes_since(&held.machines, machines))
                    + self.own.with(|own| usize::from(*own != held.own))
            })
        })
    }

    fn pending_untracked(self) -> usize {
        self.held.with_untracked(|held| {
            held.as_ref().map_or(0, |held| {
                self.machines
                    .with_untracked(|machines| changes_since(&held.machines, machines))
                    + self.own.with_untracked(|own| usize::from(*own != held.own))
            })
        })
    }
}

/// Whether `event` asks the dashboard to read its machines again: a
/// machine was approved or removed, came online or went offline, or the
/// hub took one of a machine's reports, its plans or its running
/// operation's progress.
pub(crate) fn reads_again(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::MachineApproved { .. }
            | UiEvent::MachineRemoved { .. }
            | UiEvent::MachineOnline { .. }
            | UiEvent::MachineOffline { .. }
            | UiEvent::MachineReported { .. }
            | UiEvent::MachinePlans { .. }
            | UiEvent::MachineProgress { .. }
    )
}

/// What the shared progress bar draws of a machine's latest `progress`
/// (decision 83): heard at its `at`, which starts its measure too, since
/// the hub keeps only the latest; no time for an `at` that can't be read.
pub(crate) fn progress_reading(progress: &MachineProgressDto) -> Progress {
    let at = progress.at.parse::<jiff::Timestamp>().ok();
    Progress {
        step: progress.step,
        unit: progress.unit,
        current: progress.current,
        total: progress.total,
        reported_at: at,
        since: at,
    }
}

/// How many machines read otherwise in `now` than in `held`: each that
/// came, went or changed counts once.
pub(crate) fn changes_since(held: &[HubMachineDto], now: &[HubMachineDto]) -> usize {
    let went_or_changed = held.iter().filter(|before| !now.contains(before)).count();
    let came = now
        .iter()
        .filter(|machine| {
            !held
                .iter()
                .any(|before| before.machine_id == machine.machine_id)
        })
        .count();
    went_or_changed + came
}

/// What the resume says, with `changed` machines changed meanwhile.
pub(crate) fn resumed(changed: usize) -> Words {
    Words {
        id: msg::UI_DASHBOARD_RESUMED,
        args: vec![(
            "count",
            MsgArg::Count(u64::try_from(changed).unwrap_or(u64::MAX)),
        )],
    }
}

/// How an operation's outcome reads in its badge: its state, and its words.
pub(crate) fn outcome_badge(outcome: MachineOutcomeDto) -> (BadgeState, MessageId) {
    match outcome {
        MachineOutcomeDto::Running => (BadgeState::Running, msg::UI_STATUS_RUNNING),
        MachineOutcomeDto::Succeeded => (BadgeState::Succeeded, msg::UI_STATUS_SUCCEEDED),
        MachineOutcomeDto::Failed => (BadgeState::Failed, msg::UI_STATUS_FAILED),
        MachineOutcomeDto::Canceled => (BadgeState::Canceled, msg::UI_STATUS_CANCELED),
    }
}

/// What a machine's table of plans says while it lists none: that the
/// machine reported no plans, or, before its first report reached the
/// hub, that it hasn't reported them yet.
pub(crate) fn no_plans(plans: Option<&MachinePlansDto>) -> MessageId {
    match plans {
        Some(_) => msg::UI_DASHBOARD_PLANS_NONE,
        None => msg::UI_DASHBOARD_NO_PLANS,
    }
}

/// What a machine's section says of its connection: that it is online now,
/// while the hub holds its connection, or when it last connected (§16,
/// decision 159).
pub(crate) fn connection_line(online: bool) -> MessageId {
    if online {
        msg::UI_DASHBOARD_ONLINE_NOW
    } else {
        msg::UI_DASHBOARD_LAST_CONNECTED
    }
}

/// How the instant `at` (RFC 3339) reads on the dashboard: in the viewer's
/// zone, and after it in the machine's own `zone` when the machine gave one
/// and it differs from the viewer's (#115). `None` when `at` can't be read.
pub(crate) fn instant(at: &str, viewer: Option<&IanaZone>, zone: Option<&str>) -> Option<Words> {
    let utc_ms = at.parse::<jiff::Timestamp>().ok()?.as_millisecond();
    let time = MsgArg::Timestamp {
        utc_ms,
        zone: viewer.cloned(),
    };
    let there = zone
        .and_then(|zone| IanaZone::new(zone).ok())
        .filter(|zone| Some(zone) != viewer);
    Some(match there {
        None => Words {
            id: msg::UI_INSTANT,
            args: vec![("when", time)],
        },
        Some(there) => {
            let name = MsgArg::Text(there.as_str().to_owned());
            Words {
                id: msg::UI_DASHBOARD_TIME_ELSEWHERE,
                args: vec![
                    ("time", time),
                    (
                        "there",
                        MsgArg::Timestamp {
                            utc_ms,
                            zone: Some(there),
                        },
                    ),
                    ("zone", name),
                ],
            }
        }
    })
}

/// How long past its time a planned run may stand before it counts as due:
/// a machine reads its plans every 60 seconds and sends them when they
/// change (§7.2), and its clock and the viewer's may be a few minutes apart.
pub(crate) const DUE_AFTER_MS: i64 = 5 * 60 * 1000;

/// Why a machine needs attention (decision 175): the first of these that
/// holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Attention {
    /// Offline, though a planned run fell due: when it last connected, and
    /// when the earliest run was planned for.
    Offline { last_seen: String, due: String },
    /// The plan's last run failed.
    Failed { plan: String },
    /// The plan's planned run fell due while nothing ran.
    Due { plan: String, due: String },
}

/// What a card's status badge says: an operation runs, how the latest run
/// that ended came out (its status as the history names it), or that none
/// has run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Running,
    Last(String),
    Never,
}

/// When a card's machine runs next: not at all, at the earliest of its
/// plans' next runs, or that run fell due and hasn't run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NextRun {
    NotPlanned,
    At(String),
    WasDue(String),
}

/// What a machine's card tells (decision 175), each time in RFC 3339.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CardFacts {
    pub(crate) status: CardStatus,
    /// When the latest run that ended, ended.
    pub(crate) last_backup: Option<String>,
    pub(crate) next_run: NextRun,
    /// How many plans it has, those it left out among them; none before it
    /// reported them.
    pub(crate) plans: Option<u64>,
    pub(crate) attention: Option<Attention>,
}

/// The instant `at` (RFC 3339) in ms since the Unix epoch; none when it
/// can't be read.
fn millis(at: &str) -> Option<i64> {
    Some(at.parse::<jiff::Timestamp>().ok()?.as_millisecond())
}

/// Whether a run in `status` has ended: one queued or running hasn't.
fn ended(status: &str) -> bool {
    !matches!(status, "queued" | "running")
}

/// What the card of `machine` tells at `now_ms`: its status, when its
/// latest run that ended did, its earliest next run, how many plans it has,
/// and why it needs attention, the first reason that holds (decision 175):
/// offline past a planned run, a plan's last run failed, or a planned run
/// past due while nothing runs. A run counts as due once it is more than
/// [`DUE_AFTER_MS`] past.
pub(crate) fn card_facts(machine: &HubMachineDto, now_ms: i64) -> CardFacts {
    let listed: &[MachinePlanDto] = machine
        .plans
        .as_ref()
        .map_or(&[], |plans| plans.plans.as_slice());
    let running = machine.progress.is_some()
        || machine
            .operations
            .iter()
            .any(|operation| operation.outcome == MachineOutcomeDto::Running);
    let last = listed
        .iter()
        .filter_map(|plan| plan.last_run.as_ref())
        .filter(|run| ended(&run.status))
        .max_by_key(|run| millis(&run.at));
    let next = listed
        .iter()
        .filter_map(|plan| {
            let at = plan.next_run.as_deref()?;
            Some((plan.plan.as_str(), at, millis(at)?))
        })
        .min_by_key(|(_, _, ms)| *ms);
    let due = next.filter(|(_, _, ms)| ms.saturating_add(DUE_AFTER_MS) < now_ms);
    let status = if running {
        CardStatus::Running
    } else {
        last.map_or(CardStatus::Never, |run| {
            CardStatus::Last(run.status.clone())
        })
    };
    let next_run = match (next, due) {
        (None, _) => NextRun::NotPlanned,
        (Some((_, at, _)), Some(_)) if !running => NextRun::WasDue(at.to_owned()),
        (Some((_, at, _)), _) => NextRun::At(at.to_owned()),
    };
    let failed = listed.iter().find(|plan| {
        plan.last_run
            .as_ref()
            .is_some_and(|run| run.status == "failed")
    });
    let attention = match (due, failed) {
        (Some((_, at, _)), _) if !machine.online => Some(Attention::Offline {
            last_seen: machine.last_seen.clone(),
            due: at.to_owned(),
        }),
        (_, Some(plan)) => Some(Attention::Failed {
            plan: plan.plan.clone(),
        }),
        (Some((plan, at, _)), None) if !running => Some(Attention::Due {
            plan: plan.to_owned(),
            due: at.to_owned(),
        }),
        _ => None,
    };
    CardFacts {
        status,
        last_backup: last.map(|run| run.at.clone()),
        next_run,
        plans: machine.plans.as_ref().map(|plans| {
            u64::try_from(plans.plans.len())
                .unwrap_or(u64::MAX)
                .saturating_add(plans.left_out)
        }),
        attention,
    }
}

/// What a card says of its machine's version (#1707): as the machine
/// wrote it, with a quiet note after it when it is older or newer than the
/// hub's own; that it hasn't given one yet.
pub(crate) fn version_words(version: Option<&MachineVersionDto>) -> Words {
    let _ = version;
    todo!()
}

/// A count as a message's number.
fn count(n: usize) -> MsgArg {
    MsgArg::Count(u64::try_from(n).unwrap_or(u64::MAX))
}

/// What the sentence above the cards says of `count` machines, `attention`
/// of which need it.
pub(crate) fn summary(machines: usize, attention: usize) -> Words {
    Words {
        id: msg::UI_DASHBOARD_SUMMARY,
        args: vec![("count", count(machines)), ("attention", count(attention))],
    }
}

/// Why a card needs attention, in words, its times as `shown` writes them.
pub(crate) fn attention_words(attention: &Attention, shown: impl Fn(&str) -> String) -> Words {
    let text = |value: &str| MsgArg::Text(value.to_owned());
    match attention {
        Attention::Offline { last_seen, due } => Words {
            id: msg::UI_MACHINE_CARD_OFFLINE,
            args: vec![
                ("when", MsgArg::Text(shown(last_seen))),
                ("due", MsgArg::Text(shown(due))),
            ],
        },
        Attention::Failed { plan } => Words {
            id: msg::UI_MACHINE_CARD_FAILED,
            args: vec![("plan", text(plan))],
        },
        Attention::Due { plan, due } => Words {
            id: msg::UI_MACHINE_CARD_DUE,
            args: vec![("plan", text(plan)), ("due", MsgArg::Text(shown(due)))],
        },
    }
}

/// The words of the link after the sentence to the page of `hostname`,
/// which needs `attention`, its times as `shown` writes them.
pub(crate) fn attention_link(
    hostname: &str,
    attention: &Attention,
    shown: impl Fn(&str) -> String,
) -> Words {
    let name = ("hostname", MsgArg::Text(hostname.to_owned()));
    let text = |value: &str| MsgArg::Text(value.to_owned());
    match attention {
        Attention::Offline { last_seen, due } => Words {
            id: msg::UI_DASHBOARD_ATTENTION_OFFLINE,
            args: vec![
                name,
                ("when", MsgArg::Text(shown(last_seen))),
                ("due", MsgArg::Text(shown(due))),
            ],
        },
        Attention::Failed { plan } => Words {
            id: msg::UI_DASHBOARD_ATTENTION_FAILED,
            args: vec![name, ("plan", text(plan))],
        },
        Attention::Due { plan, due } => Words {
            id: msg::UI_DASHBOARD_ATTENTION_DUE,
            args: vec![
                name,
                ("plan", text(plan)),
                ("due", MsgArg::Text(shown(due))),
            ],
        },
    }
}

/// How many of the hub's own operations its card's machine lists, as a
/// paired machine's report holds at most.
const OWN_OPERATIONS: usize = 10;

/// A status of the hub's own history as a machine's outcome; none for one
/// that hasn't started, or that this build doesn't know.
fn own_outcome(status: &str) -> Option<MachineOutcomeDto> {
    match status {
        "running" => Some(MachineOutcomeDto::Running),
        "succeeded" | "succeeded-with-warnings" => Some(MachineOutcomeDto::Succeeded),
        "failed" => Some(MachineOutcomeDto::Failed),
        "cancelled" => Some(MachineOutcomeDto::Canceled),
        _ => None,
    }
}

/// The hub's own machine as a card shows one (decision 175): who
/// `identity` says it is, online, since the page reads it from the hub
/// itself; its `version`, the hub's own build as the footer shows it, with
/// no note, since it is the hub's; its plans from its `status` once read,
/// each one's last run when it ended, or else, while it waits or runs
/// again, its last success; its latest `operations` that started, newest
/// first, and the progress of the one that runs. None without a machine
/// ID or a hostname.
pub(crate) fn own_machine(
    identity: &MachineDto,
    status: Option<&StatusDto>,
    operations: &OperationsState,
    version: Option<&str>,
) -> Option<HubMachineDto> {
    let machine_id = identity.machine_id.clone()?;
    let hostname = identity.hostname.clone()?;
    let plans = status.map(|status| MachinePlansDto {
        plans: status
            .plans
            .iter()
            .map(|plan| MachinePlanDto {
                plan: plan.plan.id.clone(),
                last_run: match plan.last.as_ref() {
                    Some(last) if ended(&last.status) => Some(MachinePlanRunDto {
                        at: last
                            .finished_at
                            .as_ref()
                            .or(last.started_at.as_ref())
                            .unwrap_or(&last.queued_at)
                            .clone(),
                        status: last.status.clone(),
                    }),
                    _ => plan.last_success.as_ref().map(|success| MachinePlanRunDto {
                        at: success.finished_at.clone(),
                        status: "succeeded".to_owned(),
                    }),
                },
                next_run: plan.plan.next_run.clone(),
            })
            .collect(),
        left_out: 0,
    });
    let listed = operations
        .rows
        .iter()
        .filter_map(|row| {
            let summary = &row.summary;
            Some(MachineOperationDto {
                operation: summary.id.clone(),
                started_at: summary.started_at.clone(),
                ended_at: summary.finished_at.clone(),
                outcome: own_outcome(&summary.status)?,
                code: summary
                    .failure_code
                    .and_then(|code| serde_json::to_value(code).ok())
                    .and_then(|code| code.as_str().map(str::to_owned)),
            })
        })
        .take(OWN_OPERATIONS)
        .collect();
    let progress = operations.rows.iter().find_map(|row| {
        let progress = row.progress?;
        Some(MachineProgressDto {
            operation: row.summary.id.clone(),
            step: progress.step,
            unit: progress.unit,
            current: progress.current,
            total: progress.total,
            at: progress
                .reported_at
                .or(progress.since)
                .map(|at| at.to_string())
                .unwrap_or_default(),
        })
    });
    Some(HubMachineDto {
        machine_id,
        hostname,
        last_seen: String::new(),
        online: true,
        zone: identity.zone.clone(),
        version: None,
        reports_dropped: 0,
        operations: listed,
        plans,
        progress,
    })
}

/// Each of the hub's own operations and its status, as its plans' status
/// was last read for (decision 175). The card's status and last backup
/// come from that status, so the page reads it on opening, and again each
/// time an operation comes or its status changes; progress changes
/// neither.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OwnStory(Option<Vec<(String, String)>>);

impl OwnStory {
    /// Whether `operations` tells another story than the one the status
    /// was last read for, always so before the first read, which it then
    /// becomes: whether to read the status again.
    pub(crate) fn moves_on(&mut self, operations: &OperationsState) -> bool {
        let same = self.0.as_ref().is_some_and(|read| {
            read.len() == operations.rows.len()
                && read
                    .iter()
                    .zip(&operations.rows)
                    .all(|((id, status), row)| {
                        *id == row.summary.id && *status == row.summary.status
                    })
        });
        if same {
            return false;
        }
        self.0 = Some(
            operations
                .rows
                .iter()
                .map(|row| (row.summary.id.clone(), row.summary.status.clone()))
                .collect(),
        );
        true
    }
}

#[cfg(test)]
mod tests;
