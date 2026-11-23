//! The hub's dashboard (technical design §4.3, §4.4; #569): each approved
//! machine as the hub last listed it, or the copy taken when the reader
//! paused live updates (A3, decision 65), how many changes wait, what the
//! resume says, which events read the machines again, and how a machine's
//! times read beside the viewer's (#115). Decided here, on the host;
//! `dashboard_page.rs` reads the machines and draws them.
//!
//! As on the Machines tab (`machines_list.rs`), the page goes on reading
//! while it is paused and shows the copy; resuming drops it.

use api::{HubMachineDto, IanaZone, MachineOutcomeDto, MachineProgressDto, MsgArg, UiEvent};
use l10n::{MessageId, msg};
use leptos::prelude::*;

use crate::backend::Progress;
use crate::components::State;
use crate::components::status_badge::rules::BadgeState;
use crate::wording::Words;

/// The machines as they were when the reader paused them.
#[derive(Clone)]
struct Held {
    machines: Vec<HubMachineDto>,
    state: State,
}

/// The machines as the hub last listed them, whether a read has answered,
/// and the copy the page shows while the reader has paused it.
#[derive(Clone, Copy)]
pub(crate) struct Fleet {
    /// The approved machines as the hub last listed them, oldest first.
    pub(crate) machines: RwSignal<Vec<HubMachineDto>>,
    /// Whether a read has answered.
    pub(crate) state: RwSignal<State>,
    held: RwSignal<Option<Held>>,
}

impl Fleet {
    pub(crate) fn new() -> Self {
        Self {
            machines: RwSignal::new(Vec::new()),
            state: RwSignal::new(State::Loading),
            held: RwSignal::new(None),
        }
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
/// machine was approved, or the hub took one of a machine's reports, its
/// plans or its running operation's progress.
pub(crate) fn reads_again(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::AgentApproved { .. }
            | UiEvent::AgentReported { .. }
            | UiEvent::AgentPlans { .. }
            | UiEvent::AgentProgress { .. }
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
                .any(|before| before.agent_id == machine.agent_id)
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
        MachineOutcomeDto::Cancelled => (BadgeState::Cancelled, msg::UI_STATUS_CANCELLED),
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

#[cfg(test)]
mod tests;
