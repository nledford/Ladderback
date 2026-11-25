//! The Machines tab's lists on a hub (§16, decisions 121 and 65): the
//! machines as the page last read them, or the copy taken when the reader
//! paused live updates, how many changes wait, and what the resume says.
//! Decided here, on the host; `settings/machines.rs` reads the list and
//! draws it.
//!
//! As on the Operations page (`pause.rs`), the page goes on reading the list
//! while it is paused, and shows the copy: the rows and how long each machine
//! has waited stay as they were at the press. Resuming drops the copy, so the
//! latest list shows at once.

use api::{AgentStatusDto, HubAgentDto, LookalikeDto, MsgArg, UiEvent};
use l10n::msg;
use leptos::prelude::*;

use crate::components::State;
use crate::wording::Words;

/// The lists as they were when the reader paused them.
#[derive(Clone)]
struct Held {
    agents: Vec<HubAgentDto>,
    state: State,
}

/// The list as the hub last answered it, whether a read has answered, and
/// the copy the page shows while the reader has paused it. Reading the rows
/// while paused reads only the copy, so a read reaches nothing the page
/// shows.
#[derive(Clone, Copy)]
pub(crate) struct Listed {
    /// The machines as the hub last listed them.
    pub(crate) agents: RwSignal<Vec<HubAgentDto>>,
    /// Whether a read has answered.
    pub(crate) state: RwSignal<State>,
    held: RwSignal<Option<Held>>,
}

impl Listed {
    pub(crate) fn new() -> Self {
        Self {
            agents: RwSignal::new(Vec::new()),
            state: RwSignal::new(State::Loading),
            held: RwSignal::new(None),
        }
    }

    /// Holds the lists as they are now. Lists already paused keep the copy
    /// they took then.
    pub(crate) fn pause(self) {
        if self.held.with_untracked(Option::is_some) {
            return;
        }
        self.held.set(Some(Held {
            agents: self.agents.get_untracked(),
            state: self.state.get_untracked(),
        }));
    }

    /// Shows the latest list again, and what is said of it: how many
    /// machines started to wait while the lists were paused, or no count
    /// when the copy listed no machine yet (a pause before a read answered),
    /// since none of them can then be told apart as new.
    pub(crate) fn resume(self) -> Words {
        let came = self.held.with_untracked(|held| {
            held.as_ref()
                .filter(|held| held.state == State::Ready)
                .map(|held| {
                    self.agents
                        .with_untracked(|agents| started_waiting(&held.agents, agents))
                })
        });
        self.held.set(None);
        match came {
            Some(came) => Words {
                id: msg::UI_MACHINES_RESUMED,
                args: vec![(
                    "count",
                    MsgArg::Count(u64::try_from(came).unwrap_or(u64::MAX)),
                )],
            },
            None => Words {
                id: msg::UI_MACHINES_RESUMED_UNLISTED,
                args: Vec::new(),
            },
        }
    }

    /// Whether the reader has paused the lists.
    pub(crate) fn paused(self) -> bool {
        self.held.with(Option::is_some)
    }

    /// Whether the reader has paused the lists, without following it.
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

    /// The machines of `status` the page shows, oldest first.
    pub(crate) fn of(self, status: AgentStatusDto) -> Signal<Vec<HubAgentDto>> {
        Signal::derive(move || {
            self.held.with(|held| match held {
                Some(held) => of_status(&held.agents, status),
                None => self.agents.with(|agents| of_status(agents, status)),
            })
        })
    }

    /// How many changes wait for the resume: each machine whose row will
    /// read otherwise, came or went, and whether a read has answered when
    /// that changed. None when the lists are not paused.
    pub(crate) fn pending(self) -> usize {
        self.held.with(|held| {
            held.as_ref().map_or(0, |held| {
                self.agents
                    .with(|agents| changes_since(held, agents, self.state.get()))
            })
        })
    }

    /// Shows `agent_id` approved at once, in the copy too: the approval
    /// answers the reader's own press, so its row leaves the waiting table
    /// before the review closes, paused or not, and focus goes to the
    /// heading, not the page.
    pub(crate) fn approved(self, agent_id: &str) {
        self.agents.update(|agents| mark_approved(agents, agent_id));
        if self.held.with_untracked(Option::is_some) {
            self.held.update(|held| {
                if let Some(held) = held {
                    mark_approved(&mut held.agents, agent_id);
                }
            });
        }
    }

    /// Drops `agent_id` at once, from the copy too: the rejection or the
    /// removal answers the reader's own press, so its row leaves its table
    /// before the review closes, paused or not.
    pub(crate) fn removed(self, agent_id: &str) {
        let keep = |agent: &HubAgentDto| agent.machine_id != agent_id;
        self.agents.update(|agents| agents.retain(keep));
        if self.held.with_untracked(Option::is_some) {
            self.held.update(|held| {
                if let Some(held) = held {
                    held.agents.retain(keep);
                }
            });
        }
    }

    /// Shows `agent_id` as the hub last listed it, in the copy too, once its
    /// approval was refused and the list read again: the refusal answers the
    /// reader's own press, so the row they compare and approve next is the
    /// hub's, not the one taken at the pause. Gone from the list, it leaves
    /// the copy too.
    pub(crate) fn refused(self, agent_id: &str) {
        if self.held.with_untracked(Option::is_none) {
            return;
        }
        let now = self.agents.with_untracked(|agents| {
            agents
                .iter()
                .find(|agent| agent.machine_id == agent_id)
                .cloned()
        });
        self.held.update(|held| {
            if let Some(held) = held {
                match &now {
                    Some(now) => {
                        for row in held
                            .agents
                            .iter_mut()
                            .filter(|row| row.machine_id == agent_id)
                        {
                            row.clone_from(now);
                        }
                    }
                    None => held.agents.retain(|row| row.machine_id != agent_id),
                }
            }
        });
    }

    /// The hostnames of the machines a read that answered while the lists
    /// were paused flagged for a refused second computer, which the copy
    /// didn't: what the resume says, beside how many started waiting. None
    /// when the lists aren't paused or the copy listed no machine yet.
    pub(crate) fn refused_while_paused(self) -> Vec<String> {
        self.held.with_untracked(|held| {
            held.as_ref()
                .filter(|held| held.state == State::Ready)
                .map(|held| {
                    self.agents
                        .with_untracked(|agents| newly_refused(&held.agents, agents))
                })
                .unwrap_or_default()
        })
    }
}

/// The machines of `agents` whose status is `status`, in their order.
fn of_status(agents: &[HubAgentDto], status: AgentStatusDto) -> Vec<HubAgentDto> {
    agents
        .iter()
        .filter(|agent| agent.status == status)
        .cloned()
        .collect()
}

/// How long `seconds` reads, in milliseconds: the hub counts whole seconds,
/// so a status that began under a second ago reads as one second, never as
/// "0 ms".
pub(crate) fn shown_ms(seconds: u64) -> u64 {
    seconds.max(1).saturating_mul(1000)
}

/// Marks `agent_id` approved just now, by the hub's clock as the row read
/// it: its status begins at the row's now (when it began and `seconds_ago`
/// after), so how long ago the hub refused a second computer still reads
/// right. A `since` the page can't read is kept as it is.
fn mark_approved(agents: &mut [HubAgentDto], agent_id: &str) {
    for agent in agents
        .iter_mut()
        .filter(|agent| agent.machine_id == agent_id)
    {
        let now = agent
            .since
            .parse::<jiff::Timestamp>()
            .ok()
            .and_then(|since| {
                let ago = i64::try_from(agent.seconds_ago).ok()?;
                since.checked_add(jiff::SignedDuration::from_secs(ago)).ok()
            });
        if let Some(now) = now {
            agent.since = now.to_string();
        }
        agent.status = AgentStatusDto::Approved;
        agent.seconds_ago = 0;
    }
}

/// Whether the rows `a` and `b` read the same but for how long ago their
/// status began, which every read changes: `seconds_ago`, and `since`,
/// which the page doesn't show and an approval made on the page moves. When
/// the hub last refused a second computer reads as a time too, as a wait
/// does, so only whether it refused one counts.
fn reads_the_same(a: &HubAgentDto, b: &HubAgentDto) -> bool {
    let HubAgentDto {
        machine_id: agent_id,
        hostname,
        key,
        fingerprint,
        status,
        since: _,
        seconds_ago: _,
        looks_like,
        second_machine_at,
    } = a;
    *agent_id == b.machine_id
        && *hostname == b.hostname
        && *key == b.key
        && *fingerprint == b.fingerprint
        && *status == b.status
        && notes_the_same(looks_like.as_ref(), b.looks_like.as_ref())
        && second_machine_at.is_some() == b.second_machine_at.is_some()
}

/// Whether two rows' notes name the same machine: the page shows only its
/// hostname, and when the other machine was last seen moves each time it
/// connects.
fn notes_the_same(a: Option<&LookalikeDto>, b: Option<&LookalikeDto>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let LookalikeDto {
                machine_id: agent_id,
                hostname,
                last_seen: _,
            } = a;
            *agent_id == b.machine_id && *hostname == b.hostname
        }
        _ => false,
    }
}

/// What the row's note says of the second computer the hub last refused
/// under its machine ID (decision 150): that the hub refused it, how long
/// ago, and that the ways back are in the warning about the machine above
/// the tables. `None` when the hub refused none. A time the page can't read
/// still flags the machine, without how long ago: the flag is what the
/// person mustn't miss.
pub(crate) fn second_computer(row: &HubAgentDto) -> Option<Words> {
    let at = row.second_machine_at.as_deref()?;
    let hostname = ("hostname", MsgArg::Text(row.hostname.clone()));
    Some(match refused_seconds_ago(row, at) {
        Some(seconds) => Words {
            id: msg::UI_MACHINES_SECOND_COMPUTER,
            args: vec![
                hostname,
                ("duration", MsgArg::DurationMs(shown_ms(seconds))),
            ],
        },
        None => Words {
            id: msg::UI_MACHINES_SECOND_COMPUTER_UNTIMED,
            args: vec![hostname],
        },
    })
}

/// The warning above the tables for one machine the hub refused a second
/// computer for: its words, which name the machine and end where the
/// command is to be typed, and that command's words, each kept whole on a
/// line (decision 150).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaysBack {
    pub(crate) words: Words,
    pub(crate) command: Vec<String>,
}

/// The warnings for the machines of `rows` the hub refused a second
/// computer for, in their order: the page gives the waiting machines', then
/// the approved ones', as its tables stand.
pub(crate) fn ways_back<'a>(rows: impl IntoIterator<Item = &'a HubAgentDto>) -> Vec<WaysBack> {
    rows.into_iter()
        .filter(|row| row.second_machine_at.is_some())
        .map(|row| WaysBack {
            words: Words {
                id: msg::UI_MACHINES_SECOND_COMPUTER_WAYS_BACK,
                args: vec![("hostname", MsgArg::Text(row.hostname.clone()))],
            },
            command: forget_command(&row.hostname),
        })
        .collect()
}

/// The command that forgets the computer the hub took as `hostname`, word
/// by word, as a POSIX shell reads it: a hostname of anything but ASCII
/// letters, digits, dots, hyphens and underscores (a Mac's "Demo's MacBook
/// Pro") is quoted, and one that starts with a hyphen is joined to the
/// option, so it isn't read as an option itself. Quoted here rather than
/// by `shell_words`, as the daemon quotes, since the web UI doesn't depend
/// on it for one rule.
fn forget_command(hostname: &str) -> Vec<String> {
    let plain = hostname
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    let quoted = if plain {
        hostname.to_owned()
    } else {
        format!("'{}'", hostname.replace('\'', r"'\''"))
    };
    let mut command = vec!["ladderback".to_owned(), "pairing-token".to_owned()];
    if hostname.starts_with('-') {
        command.push(format!("--forget-computer={quoted}"));
    } else {
        command.extend(["--forget-computer".to_owned(), quoted]);
    }
    command
}

/// How many whole seconds before the hub's latest read of `row` the hub
/// refused a second computer, `at` (RFC 3339), by the hub's clock, as the
/// row's other times: its now is when the row's status began and
/// `seconds_ago` after, so a browser whose clock is wrong reads it right. A
/// refusal after that now, by a clock set back, was none ago. `None` when
/// either time can't be read or the hub's now is out of range.
fn refused_seconds_ago(row: &HubAgentDto, at: &str) -> Option<u64> {
    let at = at.parse::<jiff::Timestamp>().ok()?.as_millisecond();
    let since = row.since.parse::<jiff::Timestamp>().ok()?.as_millisecond();
    let now = i64::try_from(row.seconds_ago)
        .ok()?
        .checked_mul(1000)?
        .checked_add(since)?;
    Some(u64::try_from(now.saturating_sub(at) / 1000).unwrap_or(0))
}

/// The hostnames of the machines in `now` the hub refused a second computer
/// for that `before` didn't flag, in `now`'s order: what the page says when
/// a read brings in a warning (the warning is no live region). A later
/// refusal for a machine already flagged brings no new warning, so isn't
/// said again.
pub(crate) fn newly_refused(before: &[HubAgentDto], now: &[HubAgentDto]) -> Vec<String> {
    now.iter()
        .filter(|row| row.second_machine_at.is_some())
        .filter(|row| {
            !before
                .iter()
                .any(|was| was.machine_id == row.machine_id && was.second_machine_at.is_some())
        })
        .map(|row| row.hostname.clone())
        .collect()
}

/// How many changes `agents` and `state` hold that `held` doesn't: each
/// machine whose row reads otherwise or went, each that came, and one when
/// whether a read has answered changed.
fn changes_since(held: &Held, agents: &[HubAgentDto], state: State) -> usize {
    let changed_or_gone = held
        .agents
        .iter()
        .filter(|before| !agents.iter().any(|now| reads_the_same(before, now)))
        .count();
    let came = agents
        .iter()
        .filter(|now| {
            !held
                .agents
                .iter()
                .any(|before| before.machine_id == now.machine_id)
        })
        .count();
    changed_or_gone
        .saturating_add(came)
        .saturating_add(usize::from(state != held.state))
}

/// How many machines of `agents` wait for approval that didn't in `held`.
fn started_waiting(held: &[HubAgentDto], agents: &[HubAgentDto]) -> usize {
    let waiting = |agent: &HubAgentDto| agent.status == AgentStatusDto::Pending;
    agents
        .iter()
        .filter(|now| waiting(now))
        .filter(|now| {
            !held
                .iter()
                .any(|before| before.machine_id == now.machine_id && waiting(before))
        })
        .count()
}

/// What the Machines tab does on hearing an event of the hub's (#569).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// Nothing its lists show changed.
    Nothing,
    /// A machine was approved: the lists are read again.
    ReadAgain,
    /// The machine named here started waiting for approval: the lists are
    /// read again, and the page says so politely while it isn't paused.
    Waiting(String),
}

/// What the Machines tab does on hearing `event`.
pub(crate) fn heard(event: &UiEvent) -> Heard {
    match event {
        UiEvent::MachinePending { hostname, .. } => Heard::Waiting(hostname.clone()),
        UiEvent::MachineApproved { .. } | UiEvent::MachineRemoved { .. } => Heard::ReadAgain,
        _ => Heard::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use l10n::Localizer;

    use super::*;
    use crate::routes::text_with;

    fn agent(hostname: &str, status: AgentStatusDto, seconds_ago: u64) -> HubAgentDto {
        HubAgentDto {
            machine_id: format!("agent-{hostname}"),
            hostname: hostname.to_owned(),
            key: format!("key-{hostname}"),
            fingerprint: format!("fingerprint-{hostname}"),
            status,
            since: "2026-11-21T10:00:00Z".to_owned(),
            seconds_ago,
            looks_like: None,
            second_machine_at: None,
        }
    }

    /// Lists that answered with `agents`.
    fn listed(agents: Vec<HubAgentDto>) -> Listed {
        let listed = Listed::new();
        listed.agents.set(agents);
        listed.state.set(State::Ready);
        listed
    }

    fn hostnames(listed: Listed, status: AgentStatusDto) -> Vec<String> {
        listed
            .of(status)
            .get_untracked()
            .into_iter()
            .map(|agent| agent.hostname)
            .collect()
    }

    /// A machine that starts waiting is read and said by its hostname, an
    /// approval is read, and nothing else changes the lists: a report, a
    /// machine's connection or an operation's events.
    #[test]
    fn a_machine_starting_to_wait_is_read_and_said_and_an_approval_is_read() {
        let at = || "2026-11-21T10:00:00Z".to_owned();
        let pending = UiEvent::MachinePending {
            machine: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&pending), Heard::Waiting("demo-laptop".to_owned()));
        let approved = UiEvent::MachineApproved {
            machine: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&approved), Heard::ReadAgain);
        let removed = UiEvent::MachineRemoved {
            machine: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&removed), Heard::ReadAgain);
        for other in [
            UiEvent::MachineReported {
                machine: "agent-demo-laptop".to_owned(),
                operation: "op-1".to_owned(),
                outcome: api::MachineOutcomeDto::Running,
            },
            UiEvent::MachineOnline {
                machine: "agent-demo-laptop".to_owned(),
                at: at(),
            },
            UiEvent::MachineOffline {
                machine: "agent-demo-laptop".to_owned(),
                at: at(),
            },
        ] {
            assert_eq!(heard(&other), Heard::Nothing, "{other:?}");
        }
    }

    /// The words' text, isolation marks aside.
    fn said(words: &Words) -> String {
        text_with(&Localizer::new(&[]), words.id, &words.args).replace(['\u{2066}', '\u{2069}'], "")
    }

    #[test]
    fn lists_not_paused_show_each_read_by_status() {
        let listed = listed(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 5),
            agent("demo-macbook", AgentStatusDto::Approved, 60),
        ]);
        assert!(!listed.paused());
        assert!(!listed.paused_untracked());
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-laptop"]);
        assert_eq!(
            hostnames(listed, AgentStatusDto::Approved),
            ["demo-macbook"]
        );
        listed
            .agents
            .set(vec![agent("demo-desktop", AgentStatusDto::Pending, 1)]);
        listed.state.set(State::Unreachable);
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-desktop"]);
        assert_eq!(listed.shown_state().get_untracked(), State::Unreachable);
        assert_eq!(listed.pending(), 0);
    }

    #[test]
    fn paused_lists_show_the_rows_and_waits_of_the_press() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.pause();
        assert!(listed.paused());
        assert!(listed.paused_untracked());
        listed.agents.set(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 10),
            agent("demo-desktop", AgentStatusDto::Pending, 1),
        ]);
        listed.state.set(State::Unreachable);
        let shown = listed.of(AgentStatusDto::Pending).get_untracked();
        assert_eq!(shown, [agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        assert_eq!(listed.shown_state().get_untracked(), State::Ready);
    }

    #[test]
    fn a_second_pause_keeps_the_first_copy() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.pause();
        listed.agents.set(Vec::new());
        listed.pause();
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-laptop"]);
    }

    #[test]
    fn resuming_shows_the_latest_list_at_once_and_counts_who_started_waiting() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.pause();
        listed.agents.set(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 10),
            agent("demo-desktop", AgentStatusDto::Pending, 1),
        ]);
        let words = listed.resume();
        assert!(!listed.paused());
        assert_eq!(
            hostnames(listed, AgentStatusDto::Pending),
            ["demo-laptop", "demo-desktop"]
        );
        assert_eq!(
            said(&words),
            "Live updates are on again. One machine started waiting while they were paused."
        );
    }

    #[test]
    fn a_resume_with_nobody_new_says_so() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.pause();
        listed.agents.set(Vec::new());
        assert_eq!(
            said(&listed.resume()),
            "Live updates are on again. No machine started waiting while they were paused."
        );
    }

    #[test]
    fn only_a_machine_newly_waiting_counts_as_started() {
        let laptop = agent("demo-laptop", AgentStatusDto::Pending, 5);
        let desktop = agent("demo-desktop", AgentStatusDto::Approved, 5);
        let held = [laptop.clone(), desktop.clone()];
        // Still waiting, waiting again after its approval, newly approved,
        // and newly waiting.
        let mut again = desktop;
        again.status = AgentStatusDto::Pending;
        let now = [
            laptop,
            again,
            agent("demo-tower", AgentStatusDto::Approved, 1),
            agent("demo-mini", AgentStatusDto::Pending, 1),
            agent("demo-nas", AgentStatusDto::Pending, 1),
        ];
        assert_eq!(started_waiting(&held, &now), 3);
        assert_eq!(started_waiting(&now, &now), 0);
    }

    #[test]
    fn the_banner_counts_rows_that_came_went_or_changed_not_ones_that_only_aged() {
        let laptop = agent("demo-laptop", AgentStatusDto::Pending, 5);
        let macbook = agent("demo-macbook", AgentStatusDto::Approved, 60);
        let held = Held {
            agents: vec![laptop.clone(), macbook.clone()],
            state: State::Ready,
        };
        let mut aged = laptop.clone();
        aged.seconds_ago = 30;
        aged.since = "2026-11-21T10:00:05Z".to_owned();
        assert_eq!(
            changes_since(&held, &[aged, macbook.clone()], State::Ready),
            0
        );
        assert_eq!(
            changes_since(
                &held,
                &[laptop.clone(), macbook.clone()],
                State::Unreachable
            ),
            1
        );
        assert_eq!(
            changes_since(&held, std::slice::from_ref(&laptop), State::Ready),
            1
        );
        let desktop = agent("demo-desktop", AgentStatusDto::Pending, 1);
        assert_eq!(
            changes_since(
                &held,
                &[laptop.clone(), macbook.clone(), desktop],
                State::Ready
            ),
            1
        );
        let changes: [fn(&mut HubAgentDto); 6] = [
            |row| row.hostname = "demo-laptop-2".to_owned(),
            |row| row.key = "key-other".to_owned(),
            |row| row.fingerprint = "fingerprint-other".to_owned(),
            |row| row.status = AgentStatusDto::Approved,
            |row| {
                row.looks_like = Some(LookalikeDto {
                    machine_id: "agent-demo-macbook".to_owned(),
                    hostname: "demo-macbook".to_owned(),
                    last_seen: "2026-11-21T09:00:00Z".to_owned(),
                });
            },
            |row| row.second_machine_at = Some("2026-11-21T09:00:00Z".to_owned()),
        ];
        for change in changes {
            let mut changed = laptop.clone();
            change(&mut changed);
            assert_eq!(
                changes_since(&held, &[changed, macbook.clone()], State::Ready),
                1
            );
        }
        let mut seen_again = laptop.clone();
        seen_again.looks_like = Some(LookalikeDto {
            machine_id: "agent-demo-macbook".to_owned(),
            hostname: "demo-macbook".to_owned(),
            last_seen: "2026-11-21T09:00:00Z".to_owned(),
        });
        let noted = Held {
            agents: vec![seen_again.clone(), macbook.clone()],
            state: State::Ready,
        };
        if let Some(like) = seen_again.looks_like.as_mut() {
            like.last_seen = "2026-11-21T10:30:00Z".to_owned();
        }
        assert_eq!(
            changes_since(&noted, &[seen_again.clone(), macbook.clone()], State::Ready),
            0
        );
        if let Some(like) = seen_again.looks_like.as_mut() {
            like.machine_id = "agent-other".to_owned();
        }
        assert_eq!(
            changes_since(&noted, &[seen_again, macbook.clone()], State::Ready),
            1
        );
        let mut renamed = laptop;
        renamed.machine_id = "agent-other".to_owned();
        assert_eq!(changes_since(&held, &[renamed, macbook], State::Ready), 2);
    }

    /// A second refusal under a machine ID already flagged moves only when
    /// the hub last refused one, which reads as a time, as a wait does: the
    /// row reads the same.
    #[test]
    fn a_later_refusal_of_a_second_computer_is_no_change_to_count() {
        let mut flagged = agent("demo-laptop", AgentStatusDto::Approved, 60);
        flagged.second_machine_at = Some("2026-11-21T09:00:00Z".to_owned());
        let held = Held {
            agents: vec![flagged.clone()],
            state: State::Ready,
        };
        flagged.second_machine_at = Some("2026-11-21T10:00:30Z".to_owned());
        assert_eq!(changes_since(&held, &[flagged.clone()], State::Ready), 0);
        flagged.second_machine_at = None;
        assert_eq!(changes_since(&held, &[flagged], State::Ready), 1);
    }

    /// The machine `demo-laptop`, approved at 10:00:00 and read
    /// `seconds_ago` later, under whose ID the hub last refused a second
    /// computer `at`.
    fn refused(seconds_ago: u64, at: Option<&str>) -> HubAgentDto {
        let mut row = agent("demo-laptop", AgentStatusDto::Approved, seconds_ago);
        row.second_machine_at = at.map(str::to_owned);
        row
    }

    /// What the note says after how long ago: where the ways back are.
    const POINTER: &str = "Its warning above the tables says what to do.";

    /// How long ago is the hub's: from when the row's status began and how
    /// long ago that was, never the browser's clock, in whole seconds as the
    /// row's other times.
    #[test]
    fn a_refused_second_computer_reads_how_long_ago_by_the_hubs_clock() {
        // The hub's now is 10:00:00 and 600 s: 10:10:00.
        let note = second_computer(&refused(600, Some("2026-11-21T10:05:00.400Z")));
        assert_eq!(
            note.as_ref().map(said),
            Some(format!(
                "The hub refused another computer as demo-laptop 4 min 59 s ago. {POINTER}"
            ))
        );
        let hours = second_computer(&refused(7200, Some("2026-11-21T10:30:00Z")));
        assert_eq!(
            hours.as_ref().map(said),
            Some(format!(
                "The hub refused another computer as demo-laptop 1 h 30 min ago. {POINTER}"
            ))
        );
    }

    /// A refusal at the hub's now, or after it by a clock set back, reads as
    /// a second ago, as a wait under a second does.
    #[test]
    fn a_refusal_at_or_after_the_hubs_now_reads_as_a_second_ago() {
        for at in [
            "2026-11-21T10:01:00Z",
            "2026-11-21T10:01:00.900Z",
            "2026-11-21T11:00:00Z",
        ] {
            let note = second_computer(&refused(60, Some(at)));
            assert_eq!(
                note.as_ref().map(said),
                Some(format!(
                    "The hub refused another computer as demo-laptop 1 s ago. {POINTER}"
                )),
                "{at}"
            );
        }
    }

    /// Times the page can't read still flag the machine, without how long
    /// ago: the flag is what the person must not miss.
    #[test]
    fn a_refusal_whose_times_cant_be_read_still_flags_the_machine() {
        let mut unread_since = refused(60, Some("2026-11-21T09:00:00Z"));
        unread_since.since = "yesterday".to_owned();
        for row in [
            refused(60, Some("an hour ago")),
            unread_since,
            refused(u64::MAX, Some("2026-11-21T09:00:00Z")),
        ] {
            assert_eq!(
                second_computer(&row).as_ref().map(said),
                Some(format!(
                    "The hub refused another computer as demo-laptop. {POINTER}"
                ))
            );
        }
    }

    #[test]
    fn a_machine_the_hub_refused_no_second_computer_for_notes_nothing() {
        let row = refused(60, None);
        assert_eq!(second_computer(&row), None);
        assert_eq!(ways_back([&row]), []);
    }

    /// One warning for each machine the hub refused a second computer for,
    /// in the order the page gives them, each naming its machine and ending
    /// where the command that forgets the computer the hub took is typed.
    #[test]
    fn each_refused_machine_gets_its_ways_back_in_the_order_given() {
        let mut desktop = agent("demo-desktop", AgentStatusDto::Pending, 5);
        desktop.second_machine_at = Some("2026-11-21T10:00:01Z".to_owned());
        let quiet = agent("demo-server", AgentStatusDto::Approved, 5);
        let laptop = refused(60, Some("2026-11-21T10:00:30Z"));
        let warnings = ways_back([&desktop, &quiet, &laptop]);
        let said: Vec<_> = warnings
            .iter()
            .map(|ways| (said(&ways.words), ways.command.join(" ")))
            .collect();
        let ways = |hostname: &str| {
            (
                format!(
                    "The hub refused another computer as {hostname}. This warning stays until \
                     the command below has run, whichever way back is taken. If the refused \
                     computer is the copy, take a new machine ID on its start page, restart \
                     Ladderback and pair it again. If it should keep this ID, stop Ladderback \
                     on the computer the hub took or give that one a new ID, then stop the \
                     hub's daemon and run this at the hub's terminal:"
                ),
                format!("ladderback pairing-token --forget-computer {hostname}"),
            )
        };
        assert_eq!(said, [ways("demo-desktop"), ways("demo-laptop")]);
    }

    /// The command is typed into a POSIX shell: a hostname with anything
    /// but letters, digits, dots, hyphens and underscores is quoted, and
    /// one that starts with a hyphen is joined to its option, so neither the
    /// shell nor the option parser reads it as something else.
    #[test]
    fn the_command_reads_the_hostname_as_one_value_in_a_shell() {
        for (hostname, last) in [
            (
                "demo_laptop.local",
                &["--forget-computer", "demo_laptop.local"][..],
            ),
            (
                "Demo's MacBook Pro",
                &["--forget-computer", r"'Demo'\''s MacBook Pro'"],
            ),
            ("café", &["--forget-computer", "'café'"]),
            ("-demo", &["--forget-computer=-demo"]),
            ("-demo laptop", &["--forget-computer='-demo laptop'"]),
        ] {
            let command = forget_command(hostname);
            assert_eq!(command[..2], ["ladderback", "pairing-token"], "{hostname}");
            assert_eq!(command[2..], *last, "{hostname}");
        }
    }

    #[test]
    fn a_status_under_a_second_old_reads_as_one_second() {
        assert_eq!(shown_ms(0), 1000);
        assert_eq!(shown_ms(1), 1000);
        assert_eq!(shown_ms(65), 65_000);
        assert_eq!(shown_ms(u64::MAX), u64::MAX);
    }

    #[test]
    fn a_resume_after_a_pause_before_any_answer_counts_nobody() {
        for state in [State::Loading, State::Unreachable] {
            let listed = Listed::new();
            listed.state.set(state);
            listed.pause();
            listed
                .agents
                .set(vec![agent("demo-laptop", AgentStatusDto::Pending, 3600)]);
            listed.state.set(State::Ready);
            assert_eq!(listed.shown_state().get_untracked(), state);
            assert_eq!(said(&listed.resume()), "Live updates are on again.");
            assert_eq!(listed.shown_state().get_untracked(), State::Ready);
        }
    }

    #[test]
    fn a_refusal_while_paused_shows_the_hubs_row_for_that_machine_alone() {
        let listed = listed(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 5),
            agent("demo-mini", AgentStatusDto::Pending, 5),
        ]);
        listed.pause();
        // demo-laptop paired again under another key, demo-mini left, and
        // demo-desktop started to wait.
        let mut repaired = agent("demo-laptop", AgentStatusDto::Pending, 1);
        repaired.key = "key-again".to_owned();
        listed.agents.set(vec![
            repaired.clone(),
            agent("demo-desktop", AgentStatusDto::Pending, 1),
        ]);
        listed.refused("agent-demo-laptop");
        let shown = listed.of(AgentStatusDto::Pending).get_untracked();
        assert_eq!(
            shown,
            [repaired, agent("demo-mini", AgentStatusDto::Pending, 5)]
        );
        listed.refused("agent-demo-mini");
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-laptop"]);
        assert!(listed.paused());
        assert_eq!(listed.pending(), 1);
    }

    #[test]
    fn a_refusal_while_not_paused_changes_nothing() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.refused("agent-demo-laptop");
        assert!(!listed.paused());
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-laptop"]);
    }

    #[test]
    fn paused_lists_count_what_waits() {
        let listed = listed(vec![agent("demo-laptop", AgentStatusDto::Pending, 5)]);
        listed.pause();
        assert_eq!(listed.pending(), 0);
        listed.agents.set(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 10),
            agent("demo-desktop", AgentStatusDto::Pending, 1),
        ]);
        assert_eq!(listed.pending(), 1);
    }

    #[test]
    fn an_approval_shows_at_once_paused_or_not() {
        let listed = listed(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 5),
            agent("demo-desktop", AgentStatusDto::Pending, 5),
        ]);
        listed.approved("agent-demo-laptop");
        assert_eq!(hostnames(listed, AgentStatusDto::Pending), ["demo-desktop"]);
        let approved = listed.of(AgentStatusDto::Approved).get_untracked();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved.first().map(|agent| agent.seconds_ago), Some(0));

        listed.pause();
        listed.approved("agent-demo-desktop");
        assert!(hostnames(listed, AgentStatusDto::Pending).is_empty());
        assert_eq!(
            hostnames(listed, AgentStatusDto::Approved),
            ["demo-laptop", "demo-desktop"]
        );
        assert_eq!(listed.pending(), 0);
        listed.resume();
        assert!(hostnames(listed, AgentStatusDto::Pending).is_empty());
    }

    #[test]
    fn a_removal_drops_its_row_at_once_paused_or_not() {
        let listed = listed(vec![
            agent("demo-laptop", AgentStatusDto::Pending, 5),
            agent("demo-desktop", AgentStatusDto::Approved, 5),
            agent("demo-macbook", AgentStatusDto::Approved, 5),
        ]);
        listed.removed("agent-demo-laptop");
        assert!(hostnames(listed, AgentStatusDto::Pending).is_empty());
        assert_eq!(
            hostnames(listed, AgentStatusDto::Approved),
            ["demo-desktop", "demo-macbook"]
        );

        listed.pause();
        listed.removed("agent-demo-desktop");
        assert_eq!(
            hostnames(listed, AgentStatusDto::Approved),
            ["demo-macbook"]
        );
        listed.resume();
        assert_eq!(
            hostnames(listed, AgentStatusDto::Approved),
            ["demo-macbook"]
        );
    }

    /// The machine `hostname`, approved, flagged for a second computer the
    /// hub refused `at`, when given.
    fn flagged(hostname: &str, at: Option<&str>) -> HubAgentDto {
        let mut row = agent(hostname, AgentStatusDto::Approved, 60);
        row.second_machine_at = at.map(str::to_owned);
        row
    }

    /// A read says only the machines it newly flags: not one already
    /// flagged, even refused again since, and not one whose flag went.
    #[test]
    fn a_read_says_only_the_machines_it_newly_flags() {
        let before = [
            flagged("demo-laptop", Some("2026-11-21T09:00:00Z")),
            flagged("demo-desktop", None),
            flagged("demo-server", Some("2026-11-21T09:00:00Z")),
        ];
        let now = [
            flagged("demo-laptop", Some("2026-11-21T09:30:00Z")),
            flagged("demo-desktop", Some("2026-11-21T09:40:00Z")),
            flagged("demo-server", None),
            flagged("demo-tablet", Some("2026-11-21T09:50:00Z")),
        ];
        assert_eq!(
            newly_refused(&before, &now),
            ["demo-desktop", "demo-tablet"]
        );
        assert!(newly_refused(&now, &now).is_empty());
    }

    /// The resume says each machine a read flagged while the lists were
    /// paused; lists that aren't paused, or a copy taken before any read
    /// answered, say none.
    #[test]
    fn a_refusal_while_paused_is_told_on_resume() {
        let listed = listed(vec![flagged("demo-laptop", None)]);
        assert!(listed.refused_while_paused().is_empty());
        listed.pause();
        listed
            .agents
            .set(vec![flagged("demo-laptop", Some("2026-11-21T10:00:30Z"))]);
        assert_eq!(listed.refused_while_paused(), ["demo-laptop"]);

        let early = Listed::new();
        early.pause();
        early
            .agents
            .set(vec![flagged("demo-laptop", Some("2026-11-21T10:00:30Z"))]);
        early.state.set(State::Ready);
        assert!(early.refused_while_paused().is_empty());
    }

    /// An approval made on the page keeps how long ago the hub refused a
    /// second computer: the row's now stays the hub's, not when the machine
    /// started waiting.
    #[test]
    fn an_approval_on_the_page_keeps_how_long_ago_a_refusal_was() {
        // Waiting since 10:00:00, read 3600 s later (11:00:00); refused at 10:30:00.
        let mut desktop = agent("demo-desktop", AgentStatusDto::Pending, 3600);
        desktop.second_machine_at = Some("2026-11-21T10:30:00Z".to_owned());
        let listed = listed(vec![desktop]);
        listed.pause();
        listed.approved("agent-demo-desktop");
        let approved = listed.of(AgentStatusDto::Approved).get_untracked();
        let note = approved.first().and_then(second_computer);
        assert_eq!(
            note.as_ref().map(said),
            Some(format!(
                "The hub refused another computer as demo-desktop 30 min 0 s ago. {POINTER}"
            ))
        );
    }
}
