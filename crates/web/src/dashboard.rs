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
    HubMachineDto, IanaZone, MachineDto, MachineOutcomeDto, MachinePlansDto, MachineProgressDto,
    MsgArg, StatusDto, UiEvent,
};
#[cfg(test)]
use api::{MachinePlanDto, MachinePlanRunDto};
use l10n::{MessageId, msg};
use leptos::prelude::*;

use crate::backend::{OperationsState, Progress};
use crate::components::State;
use crate::components::status_badge::rules::BadgeState;
use crate::wording::Words;

/// The machines as they were when the reader paused them.
#[derive(Clone)]
struct Held {
    machines: Vec<HubMachineDto>,
    state: State,
}

/// What the page has read of the hub's own machine: nothing yet, or its
/// card's machine, none when the hub didn't say who it is (decision 175).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Own {
    #[default]
    Waiting,
    Read(Option<HubMachineDto>),
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
        todo!()
    }

    /// The time the cards the page shows are read at.
    pub(crate) fn shown_now(self) -> Signal<i64> {
        todo!()
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

    /// Whether a read has answered, as the page shows it.
    pub(crate) fn shown_state(self) -> Signal<State> {
        Signal::derive(move || {
            self.held
                .with(|held| held.as_ref().map(|held| held.state))
                .unwrap_or_else(|| self.state.get())
        })
    }

    /// The machines the page shows, oldest first.
    pub(crate) fn shown(self) -> Signal<Vec<HubMachineDto>> {
        Signal::derive(move || {
            self.held.with(|held| match held {
                Some(held) => held.machines.clone(),
                None => self.machines.get(),
            })
        })
    }

    /// How many changes wait for the resume; none when not paused.
    pub(crate) fn pending(self) -> usize {
        self.held.with(|held| {
            held.as_ref().map_or(0, |held| {
                self.machines
                    .with(|machines| changes_since(&held.machines, machines))
            })
        })
    }

    fn pending_untracked(self) -> usize {
        self.held.with_untracked(|held| {
            held.as_ref().map_or(0, |held| {
                self.machines
                    .with_untracked(|machines| changes_since(&held.machines, machines))
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

/// What the card of `machine` tells at `now_ms`.
pub(crate) fn card_facts(machine: &HubMachineDto, now_ms: i64) -> CardFacts {
    let _ = (machine, now_ms);
    todo!()
}

/// What the sentence above the cards says of `count` machines, `attention`
/// of which need it.
pub(crate) fn summary(count: usize, attention: usize) -> Words {
    let _ = (count, attention);
    todo!()
}

/// Why a card needs attention, in words, its times as `shown` writes them.
pub(crate) fn attention_words(attention: &Attention, shown: impl Fn(&str) -> String) -> Words {
    let _ = (attention, shown);
    todo!()
}

/// The words of the link after the sentence to the page of `hostname`,
/// which needs `attention`, its times as `shown` writes them.
pub(crate) fn attention_link(
    hostname: &str,
    attention: &Attention,
    shown: impl Fn(&str) -> String,
) -> Words {
    let _ = (hostname, attention, shown);
    todo!()
}

/// The hub's own machine as a card shows one: who `identity` says it is,
/// its plans from its `status` once read, and its `operations`; none
/// without a machine ID.
pub(crate) fn own_machine(
    identity: &MachineDto,
    status: Option<&StatusDto>,
    operations: &OperationsState,
) -> Option<HubMachineDto> {
    let _ = (identity, status, operations);
    todo!()
}

#[cfg(test)]
mod tests;
