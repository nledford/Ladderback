//! The hub's dashboard (technical design §4.3, §4.4; #569): each approved
//! machine as the hub last listed it, or the copy taken when the reader
//! paused live updates (A3, decision 65), how many changes wait, what the
//! resume says, which events read the machines again, and how a machine's
//! times read beside the viewer's (#115). Decided here, on the host;
//! `dashboard_page.rs` reads the machines and draws them.
//!
//! As on the Machines tab (`machines_list.rs`), the page goes on reading
//! while it is paused and shows the copy; resuming drops it.

use api::{HubMachineDto, IanaZone, MachineOutcomeDto, MsgArg, UiEvent};
use l10n::{MessageId, msg};
use leptos::prelude::*;

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

    /// Whether the page is paused, without following it.
    pub(crate) fn paused_untracked(self) -> bool {
        self.held.with_untracked(Option::is_some)
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
/// machine was approved, or the hub took one of a machine's reports.
pub(crate) fn reads_again(event: &UiEvent) -> bool {
    todo!("{event:?}")
}

/// How many machines read otherwise in `now` than in `held`: each that
/// came, went or changed counts once.
pub(crate) fn changes_since(held: &[HubMachineDto], now: &[HubMachineDto]) -> usize {
    todo!("{held:?} {now:?}")
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
    todo!("{outcome:?}")
}

/// How the instant `at` (RFC 3339) reads on the dashboard: in the viewer's
/// zone, and after it in the machine's own `zone` when the machine gave one
/// and it differs from the viewer's (#115). `None` when `at` can't be read.
pub(crate) fn instant(at: &str, viewer: Option<&IanaZone>, zone: Option<&str>) -> Option<Words> {
    todo!("{at} {viewer:?} {zone:?}")
}

#[cfg(test)]
mod tests;
