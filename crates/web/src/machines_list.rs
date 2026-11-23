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
                .find(|agent| agent.agent_id == agent_id)
                .cloned()
        });
        self.held.update(|held| {
            if let Some(held) = held {
                match &now {
                    Some(now) => {
                        for row in held
                            .agents
                            .iter_mut()
                            .filter(|row| row.agent_id == agent_id)
                        {
                            row.clone_from(now);
                        }
                    }
                    None => held.agents.retain(|row| row.agent_id != agent_id),
                }
            }
        });
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

/// Marks `agent_id` approved just now.
fn mark_approved(agents: &mut [HubAgentDto], agent_id: &str) {
    for agent in agents.iter_mut().filter(|agent| agent.agent_id == agent_id) {
        agent.status = AgentStatusDto::Approved;
        agent.seconds_ago = 0;
    }
}

/// Whether the rows `a` and `b` read the same but for how long ago their
/// status began, which every read changes: `seconds_ago`, and `since`,
/// which the page doesn't show and an approval made on the page moves.
fn reads_the_same(a: &HubAgentDto, b: &HubAgentDto) -> bool {
    let HubAgentDto {
        agent_id,
        hostname,
        key,
        fingerprint,
        status,
        since: _,
        seconds_ago: _,
        looks_like,
        second_machine_at: _,
    } = a;
    *agent_id == b.agent_id
        && *hostname == b.hostname
        && *key == b.key
        && *fingerprint == b.fingerprint
        && *status == b.status
        && notes_the_same(looks_like.as_ref(), b.looks_like.as_ref())
}

/// Whether two rows' notes name the same machine: the page shows only its
/// hostname, and when the other machine was last seen moves each time it
/// connects.
fn notes_the_same(a: Option<&LookalikeDto>, b: Option<&LookalikeDto>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let LookalikeDto {
                agent_id,
                hostname,
                last_seen: _,
            } = a;
            *agent_id == b.agent_id && *hostname == b.hostname
        }
        _ => false,
    }
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
                .any(|before| before.agent_id == now.agent_id)
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
                .any(|before| before.agent_id == now.agent_id && waiting(before))
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
        UiEvent::AgentPending { hostname, .. } => Heard::Waiting(hostname.clone()),
        UiEvent::AgentApproved { .. } | UiEvent::AgentRemoved { .. } => Heard::ReadAgain,
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
            agent_id: format!("agent-{hostname}"),
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
        let pending = UiEvent::AgentPending {
            agent: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&pending), Heard::Waiting("demo-laptop".to_owned()));
        let approved = UiEvent::AgentApproved {
            agent: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&approved), Heard::ReadAgain);
        let removed = UiEvent::AgentRemoved {
            agent: "agent-demo-laptop".to_owned(),
            hostname: "demo-laptop".to_owned(),
            at: at(),
        };
        assert_eq!(heard(&removed), Heard::ReadAgain);
        for other in [
            UiEvent::AgentReported {
                agent: "agent-demo-laptop".to_owned(),
                operation: "op-1".to_owned(),
                outcome: api::MachineOutcomeDto::Running,
            },
            UiEvent::AgentOnline {
                agent: "agent-demo-laptop".to_owned(),
                at: at(),
            },
            UiEvent::AgentOffline {
                agent: "agent-demo-laptop".to_owned(),
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
        let changes: [fn(&mut HubAgentDto); 5] = [
            |row| row.hostname = "demo-laptop-2".to_owned(),
            |row| row.key = "key-other".to_owned(),
            |row| row.fingerprint = "fingerprint-other".to_owned(),
            |row| row.status = AgentStatusDto::Approved,
            |row| {
                row.looks_like = Some(LookalikeDto {
                    agent_id: "agent-demo-macbook".to_owned(),
                    hostname: "demo-macbook".to_owned(),
                    last_seen: "2026-11-21T09:00:00Z".to_owned(),
                });
            },
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
            agent_id: "agent-demo-macbook".to_owned(),
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
            like.agent_id = "agent-other".to_owned();
        }
        assert_eq!(
            changes_since(&noted, &[seen_again, macbook.clone()], State::Ready),
            1
        );
        let mut renamed = laptop;
        renamed.agent_id = "agent-other".to_owned();
        assert_eq!(changes_since(&held, &[renamed, macbook], State::Ready), 2);
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
}
