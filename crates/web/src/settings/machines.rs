//! The Machines tab's machines, on a hub (§16, decision 121, "Waiting for
//! approval"): the machines waiting for approval, each with its hostname,
//! agent ID, key fingerprint in full, how long it has waited and a note when
//! its hostname looks like another machine's, and the machines approved.
//! Approve opens a review that names the machine, shows its fingerprint to
//! compare with the machine's own screen and asks for the step-up each time;
//! it starts on Cancel. The page approves the agent ID under the key it
//! listed, never a hostname alone (decision 120, "Pending approval"). Reject,
//! beside Approve, and Remove, on each approved machine, open a review of
//! their own that names the machine and shows its fingerprint, with no
//! step-up: either only takes authority away (decision 154).
//!
//! The list is read when the tab is built (Settings builds every tab's panel
//! once, so this one listens while another tab is shown too), again each
//! time the events stream opens, the first time and after a reconnect, and
//! at once after an approval. Between reads, the hub's events say when a
//! machine starts waiting for approval, is approved or is removed (#569):
//! each reads the list again, and a machine that starts waiting is said
//! politely while the tables aren't paused. "Pause live updates" holds both
//! tables still (A3, decision 65); what they show while paused, and what the
//! resume says, is `machines_list.rs`'s. A link leads to the hub's dashboard.

use std::sync::Arc;
use std::time::Duration;

use api::{
    AgentStatusDto, ApproveAgentDto, ErrorCode, HUB_AGENTS_PATH, HubAgentDto, HubAgentsDto, MsgArg,
    RemoveAgentDto, agent_approval_path, agent_removal_path,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::backend::Rereads;
use crate::components::alert::rules::AlertTone;
use crate::components::button::Content;
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::{
    ACTIONS, Alert, Button, Column, Confirmation, Fingerprint, Link, Name, PauseUpdates,
    ReviewDialog, Sentences, State, StepUp, StepUpField, Table, Working, dialog_refusal_alert,
    focus_element, hand_focus_when_gone,
};
use crate::live::{reply, say};
use crate::machines_list::{Heard, Listed, heard};
use crate::pause;
use crate::repositories::Sentence;
use crate::routes::{refused_sentence, text_with};
use crate::text;
use crate::variants::{Rem, Size, Tone};
use crate::visit::Visit;
use crate::ws::{self, Posted};

/// The waiting machines' heading, which names their table and takes focus
/// when the review's way back is gone: the machine it approved has left the
/// table.
const WAITING_HEADING_ID: &str = "machines-waiting-heading";
/// The approved machines' heading, which names their table.
const APPROVED_HEADING_ID: &str = "machines-approved-heading";
/// What the review says first, its description.
const REVIEW_BODY_ID: &str = "machines-review-body";
/// What the review of rejecting or removing a machine says first.
const LEAVE_BODY_ID: &str = "machines-leave-body";
/// The review's step-up field, where focus goes when the step-up is refused.
const STEP_UP_ID: &str = "machines-confirmation";
/// The banner beside the pause button while the tables are paused.
const PAUSED_ID: &str = "machines-paused";
/// How long one read of the list may take before it counts as unanswered.
const READ_PATIENCE: Duration = Duration::from_secs(5);
/// How long the tab waits before it asks again after a read that went
/// unanswered while none had answered: the "few seconds" it says.
const ASK_AGAIN: Duration = Duration::from_secs(3);

/// The id of the button that approves the agent `agent_id`, where focus
/// returns when its review is dismissed.
fn approve_id(agent_id: &str) -> String {
    format!("machines-approve-{agent_id}")
}

/// The id of the button that rejects or removes the agent `agent_id`, where
/// focus returns when its review is dismissed.
fn leave_id(agent_id: &str) -> String {
    format!("machines-leave-{agent_id}")
}

/// What rejecting a waiting machine, or removing an approved one, says, and
/// the heading of its table, where focus goes once its row is gone.
struct LeaveWords {
    button: l10n::MessageId,
    title: l10n::MessageId,
    body: l10n::MessageId,
    working: l10n::MessageId,
    done: l10n::MessageId,
    heading: &'static str,
}

impl LeaveWords {
    fn of(status: &AgentStatusDto) -> Self {
        match status {
            AgentStatusDto::Pending => Self {
                button: msg::UI_MACHINES_REJECT,
                title: msg::UI_MACHINES_REJECT_TITLE,
                body: msg::UI_MACHINES_REJECT_BODY,
                working: msg::UI_MACHINES_REJECTING,
                done: msg::UI_MACHINES_REJECTED,
                heading: WAITING_HEADING_ID,
            },
            AgentStatusDto::Approved => Self {
                button: msg::UI_MACHINES_REMOVE,
                title: msg::UI_MACHINES_REMOVE_TITLE,
                body: msg::UI_MACHINES_REMOVE_BODY,
                working: msg::UI_MACHINES_REMOVING,
                done: msg::UI_MACHINES_REMOVED,
                heading: APPROVED_HEADING_ID,
            },
        }
    }
}

/// What `id` says of a machine named `hostname`.
fn of_machine(localizer: &Localizer, id: l10n::MessageId, hostname: &str) -> String {
    text_with(
        localizer,
        id,
        &[("hostname", MsgArg::Text(hostname.to_owned()))],
    )
}

/// Lands one read's answer in `listed`, or its lack of one: an answer
/// replaces the rows, and no answer keeps them, or says the hub didn't
/// answer while none has. Whether the tab still waits for its first answer.
fn land(listed: Listed, read: Option<HubAgentsDto>) -> bool {
    if let Some(read) = read {
        let _ = listed.agents.try_set(read.agents);
        let _ = listed.state.try_set(State::Ready);
        return false;
    }
    let waiting = matches!(
        listed.state.try_get_untracked(),
        Some(State::Loading | State::Unreachable)
    );
    if waiting {
        let _ = listed.state.try_set(State::Unreachable);
    }
    waiting
}

/// Reads the list into `listed`, paced by `rereads` (`ws::reread_hub`).
fn read(listed: Listed, rereads: StoredValue<Rereads>, visit: &Visit) {
    ws::reread_hub(
        format!("{}{HUB_AGENTS_PATH}", ws::origin()),
        rereads,
        visit,
        (READ_PATIENCE, ASK_AGAIN),
        move |read| land(listed, read),
    );
}

/// Reads the list into `listed` now, and again when the events stream
/// opens and when an event of the hub's asks for it, until Settings is
/// left, paused or not: a pause holds what the page shows, not the reads.
/// A machine that starts waiting is said politely unless the tables are
/// paused, whose resume counts it instead.
fn follow(localizer: &Arc<Localizer>, listed: Listed) {
    let rereads = StoredValue::new(Rereads::default());
    let (visit, departure) = Visit::begin();
    on_cleanup(move || drop(departure));
    read(listed, rereads, &visit);
    let localizer = Arc::clone(localizer);
    let (opening, hearing) = (visit.clone(), visit.clone());
    ws::listen(
        &ws::origin(),
        &visit,
        move || read(listed, rereads, &opening),
        move |event| match heard(&event) {
            Heard::Nothing => {}
            Heard::ReadAgain => read(listed, rereads, &hearing),
            Heard::Waiting(hostname) => {
                if !listed.paused_untracked() {
                    let words = of_machine(&localizer, msg::UI_MACHINES_STARTED_WAITING, &hostname);
                    say(Politeness::Polite, &words);
                }
                read(listed, rereads, &hearing);
            }
        },
    );
}

#[component]
pub(crate) fn Machines(localizer: Arc<Localizer>, step_up: StepUp) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let listed = Listed::new();
    follow(&localizer, listed);
    let (pause_label, paused_said) = (
        text(&localizer, msg::UI_LIVE_PAUSE),
        text(&localizer, msg::UI_LIVE_PAUSED),
    );
    let banner_localizer = Arc::clone(&localizer);
    let resume_localizer = Arc::clone(&localizer);
    let on_resume = Callback::new(move |()| {
        let words = listed.resume();
        text_with(&resume_localizer, words.id, &words.args)
    });
    let chosen = RwSignal::new(None::<HubAgentDto>);
    let open = RwSignal::new(false);
    let confirmation = RwSignal::new(String::new());
    let step_up_error = RwSignal::new(None::<String>);
    let problem = RwSignal::new(None::<Sentence>);
    let said = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    let choose = Callback::new(move |agent: HubAgentDto| {
        // The step-up is asked for again each time (decision 121).
        confirmation.set(String::new());
        step_up_error.set(None);
        problem.set(None);
        chosen.set(Some(agent));
        open.set(true);
    });

    let approve_localizer = Arc::clone(&localizer);
    let approve = Callback::new(move |()| {
        let Some(agent) = chosen.get_untracked() else {
            return;
        };
        if busy.get_untracked() {
            return;
        }
        problem.set(None);
        step_up_error.set(None);
        let ask = ApproveAgentDto {
            key: agent.key.clone(),
            confirmation: confirmation.get_untracked(),
        };
        confirmation.set(String::new());
        busy.set(true);
        let localizer = Arc::clone(&approve_localizer);
        spawn_local(async move {
            let url = format!("{}{}", ws::origin(), agent_approval_path(&agent.agent_id));
            let answer = ws::post(&url, &ask).await;
            if busy.try_set(false).is_some() {
                // The tab was left while the approval was on its way.
                return;
            }
            match answer {
                Some(Posted::Done(_)) => {
                    listed.approved(&agent.agent_id);
                    let words = of_machine(&localizer, msg::UI_MACHINES_APPROVED, &agent.hostname);
                    // Said through the announcer: the line below is set
                    // while the modal still makes it inert, so it changes
                    // no live region a reader hears.
                    reply(Politeness::Polite, &words);
                    said.set(Some(words));
                    open.set(false);
                    // The hub's `AgentApproved` reads the list again.
                }
                Some(Posted::Refused(ErrorCode::StepUpFailed, ..)) => {
                    step_up_error.set(Some(text(&localizer, msg::UI_MACHINES_STEP_UP_FAILED)));
                    focus_element(STEP_UP_ID);
                }
                Some(Posted::Refused(code, args, detail)) => {
                    problem.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    // The list may have changed under the review: another
                    // tab approved it, or it paired again under a new key.
                    // The refusal answers the reader, so that row shows as
                    // this read of it finds it, even while paused.
                    let url = format!("{}{HUB_AGENTS_PATH}", ws::origin());
                    land(listed, ws::poll::<HubAgentsDto>(&url, READ_PATIENCE).await);
                    listed.refused(&agent.agent_id);
                }
                None => problem.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_PAIRING_UNANSWERED),
                    None,
                ))),
            }
        });
    });

    let leaving = RwSignal::new(None::<HubAgentDto>);
    let leave_open = RwSignal::new(false);
    let leave_problem = RwSignal::new(None::<Sentence>);
    let leave_busy = RwSignal::new(false);
    let choose_leave = Callback::new(move |agent: HubAgentDto| {
        leave_problem.set(None);
        leaving.set(Some(agent));
        leave_open.set(true);
    });
    let leave_localizer = Arc::clone(&localizer);
    let leave = Callback::new(move |()| {
        let Some(agent) = leaving.get_untracked() else {
            return;
        };
        if leave_busy.get_untracked() {
            return;
        }
        leave_problem.set(None);
        let ask = RemoveAgentDto {
            key: agent.key.clone(),
        };
        leave_busy.set(true);
        let localizer = Arc::clone(&leave_localizer);
        spawn_local(async move {
            let url = format!("{}{}", ws::origin(), agent_removal_path(&agent.agent_id));
            let answer = ws::post(&url, &ask).await;
            if leave_busy.try_set(false).is_some() {
                // The tab was left while the removal was on its way.
                return;
            }
            match answer {
                Some(Posted::Done(_)) => {
                    listed.removed(&agent.agent_id);
                    let done = LeaveWords::of(&agent.status).done;
                    let words = of_machine(&localizer, done, &agent.hostname);
                    // Said through the announcer, as an approval is.
                    reply(Politeness::Polite, &words);
                    said.set(Some(words));
                    leave_open.set(false);
                    // The hub's `AgentRemoved` reads the list again.
                }
                Some(Posted::Refused(code, args, detail)) => {
                    leave_problem.set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                    // Another tab removed it, or it paired again under a
                    // new key: the row shows as this read finds it.
                    let url = format!("{}{HUB_AGENTS_PATH}", ws::origin());
                    land(listed, ws::poll::<HubAgentsDto>(&url, READ_PATIENCE).await);
                    listed.refused(&agent.agent_id);
                }
                None => leave_problem.set(Some(Sentence::new(
                    &text(&localizer, msg::UI_PAIRING_UNANSWERED),
                    None,
                ))),
            }
        });
    });

    let (waiting_header, waiting_columns) = waiting_columns(&localizer, choose, choose_leave);
    let (approved_header, approved_columns) = approved_columns(&localizer, choose_leave);
    let review_localizer = Arc::clone(&localizer);
    let leave_review_localizer = Arc::clone(&localizer);
    let link_localizer = Arc::clone(&localizer);
    let dashboard_link = t(msg::UI_MACHINES_DASHBOARD_LINK);
    view! {
        <PauseUpdates
            label=pause_label
            paused_said=paused_said
            paused=Signal::derive(move || listed.paused())
            banner=Signal::derive(move || {
                let words = pause::banner(listed.pending());
                text_with(&banner_localizer, words.id, &words.args)
            })
            banner_id=PAUSED_ID
            on_pause=Callback::new(move |()| listed.pause())
            on_resume=on_resume
        />
        <p>{move || said.get()}</p>
        <p>
            {Destination::route("/dashboard", Matching::Exact)
                .map(|to| {
                    view! {
                        <Link
                            to=to
                            localizer=Arc::clone(&link_localizer)
                            placement=LinkPlacement::InText
                        >
                            {dashboard_link}
                        </Link>
                    }
                })}
        </p>
        <h3 id=WAITING_HEADING_ID tabindex="-1">
            {t(msg::UI_MACHINES_WAITING_HEADING)}
        </h3>
        <Table
            name=Name::Heading(WAITING_HEADING_ID.to_owned())
            row_header=waiting_header
            columns=waiting_columns
            rows=listed.of(AgentStatusDto::Pending)
            key=|row: &HubAgentDto| row.agent_id.clone()
            state=listed.shown_state()
            sentences=Sentences {
                loading: t(msg::UI_MACHINES_LOADING),
                empty: t(msg::UI_MACHINES_NONE_WAITING),
                unreachable: t(msg::UI_MACHINES_UNREACHABLE),
            }
        />
        <h3 id=APPROVED_HEADING_ID tabindex="-1">
            {t(msg::UI_MACHINES_APPROVED_HEADING)}
        </h3>
        <Table
            name=Name::Heading(APPROVED_HEADING_ID.to_owned())
            row_header=approved_header
            columns=approved_columns
            rows=listed.of(AgentStatusDto::Approved)
            key=|row: &HubAgentDto| row.agent_id.clone()
            state=listed.shown_state()
            sentences=Sentences {
                loading: t(msg::UI_MACHINES_LOADING),
                empty: t(msg::UI_MACHINES_NONE_APPROVED),
                unreachable: t(msg::UI_MACHINES_UNREACHABLE),
            }
        />
        {move || {
            chosen
                .get()
                .map(|agent| {
                    review(
                        &review_localizer,
                        agent,
                        Review {
                            open,
                            step_up: step_up.clone(),
                            confirmation,
                            step_up_error,
                            problem,
                            approve,
                            busy,
                        },
                    )
                })
        }}
        {move || {
            leaving
                .get()
                .map(|agent| {
                    leave_review(
                        &leave_review_localizer,
                        agent,
                        LeaveReview {
                            open: leave_open,
                            problem: leave_problem,
                            leave,
                            busy: leave_busy,
                        },
                    )
                })
        }}
    }
}

/// The signals and the action a review shares with its tab.
struct Review {
    open: RwSignal<bool>,
    step_up: StepUp,
    confirmation: RwSignal<String>,
    step_up_error: RwSignal<Option<String>>,
    problem: RwSignal<Option<Sentence>>,
    approve: Callback<()>,
    /// Whether the approval is on its way, while the review holds still.
    busy: RwSignal<bool>,
}

/// The review of approving `agent`: its fingerprint in full, a warning when
/// its hostname looks like another machine's, and the step-up.
fn review(localizer: &Arc<Localizer>, agent: HubAgentDto, review: Review) -> impl IntoView + use<> {
    let hostname = &agent.hostname;
    let lookalike = agent.looks_like.as_ref().map(|like| {
        let words = of_machine(localizer, msg::UI_MACHINES_REVIEW_LOOKALIKE, &like.hostname);
        let localizer = Arc::clone(localizer);
        view! {
            <Alert localizer=localizer tone=AlertTone::Warning>
                {words}
            </Alert>
        }
    });
    let title = of_machine(localizer, msg::UI_MACHINES_REVIEW_TITLE, hostname);
    let label = of_machine(localizer, msg::UI_MACHINES_APPROVE, hostname);
    let body = of_machine(localizer, msg::UI_MACHINES_REVIEW_BODY, hostname);
    let fingerprint_name = of_machine(localizer, msg::UI_MACHINES_FINGERPRINT, hostname);
    let returns_to = approve_id(&agent.agent_id);
    let working = Working {
        active: review.busy.into(),
        status: of_machine(localizer, msg::UI_MACHINES_APPROVING, hostname),
    };
    let fingerprint = agent.fingerprint;
    let field_localizer = Arc::clone(localizer);
    let problem_localizer = Arc::clone(localizer);
    let problem = review.problem;
    view! {
        <ReviewDialog
            open=review.open
            title=title
            dismiss_label=text(localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label,
                tone: Tone::Primary,
                action: review.approve,
            })
            returns_to=returns_to
            described_by=REVIEW_BODY_ID.to_owned()
            working=working
            fallback=WAITING_HEADING_ID.to_owned()
        >
            <p id=REVIEW_BODY_ID>{body}</p>
            <Fingerprint value=fingerprint name=fingerprint_name />
            {lookalike}
            {move || {
                problem.get().map(|sentence| dialog_refusal_alert(&problem_localizer, sentence))
            }}
            <StepUpField
                id=STEP_UP_ID
                localizer=field_localizer
                step_up=review.step_up
                value=review.confirmation
                error=review.step_up_error
            />
        </ReviewDialog>
    }
}

/// The signals and the action a review of rejecting or removing shares with
/// its tab.
struct LeaveReview {
    open: RwSignal<bool>,
    problem: RwSignal<Option<Sentence>>,
    leave: Callback<()>,
    /// Whether the rejection or removal is on its way.
    busy: RwSignal<bool>,
}

/// The review of rejecting `agent` while it waits, or removing it once
/// approved: its fingerprint in full, and no step-up (decision 154).
fn leave_review(
    localizer: &Arc<Localizer>,
    agent: HubAgentDto,
    review: LeaveReview,
) -> impl IntoView + use<> {
    let hostname = &agent.hostname;
    let words = LeaveWords::of(&agent.status);
    let working = Working {
        active: review.busy.into(),
        status: of_machine(localizer, words.working, hostname),
    };
    let fingerprint_name = of_machine(localizer, msg::UI_MACHINES_FINGERPRINT, hostname);
    let body = of_machine(localizer, words.body, hostname);
    let problem_localizer = Arc::clone(localizer);
    let problem = review.problem;
    view! {
        <ReviewDialog
            open=review.open
            title=of_machine(localizer, words.title, hostname)
            dismiss_label=text(localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label: of_machine(localizer, words.button, hostname),
                tone: Tone::Error,
                action: review.leave,
            })
            returns_to=leave_id(&agent.agent_id)
            described_by=LEAVE_BODY_ID.to_owned()
            working=working
            fallback=words.heading.to_owned()
        >
            <p id=LEAVE_BODY_ID>{body}</p>
            <Fingerprint value=agent.fingerprint name=fingerprint_name />
            {move || {
                problem.get().map(|sentence| dialog_refusal_alert(&problem_localizer, sentence))
            }}
        </ReviewDialog>
    }
}

/// A cell of the row's Reject or Remove button, which `choose` opens the
/// review with. Drawn once, as the Approve button is; focus goes to
/// `heading` when its row leaves the table.
fn leave_button(
    localizer: &Localizer,
    row: Signal<HubAgentDto>,
    choose: Callback<HubAgentDto>,
) -> AnyView {
    let (hostname, agent_id, words) = row.with_untracked(|row| {
        (
            row.hostname.clone(),
            row.agent_id.clone(),
            LeaveWords::of(&row.status),
        )
    });
    let label = of_machine(localizer, words.button, &hostname);
    let button = NodeRef::<leptos::html::Button>::new();
    hand_focus_when_gone(button, words.heading.to_owned());
    let press = Callback::new(move |()| choose.run(row.get_untracked()));
    view! {
        <Button
            id=leave_id(&agent_id)
            content=Content::Text(label.into())
            size=Size::Small
            node_ref=button
            on_press=press
        />
    }
    .into_any()
}

/// A cell of the row's hostname.
fn hostname_column(localizer: &Localizer) -> Column<HubAgentDto> {
    Column::new(
        "hostname",
        text(localizer, msg::UI_MACHINES_HOSTNAME),
        Rem::tenths(80),
        |row: Signal<HubAgentDto>| {
            view! { <bdi>{move || row.with(|row| row.hostname.clone())}</bdi> }.into_any()
        },
    )
}

/// A cell of the row's agent ID, which wraps at its hyphens.
fn agent_id_column(localizer: &Localizer) -> Column<HubAgentDto> {
    Column::new(
        "agent-id",
        text(localizer, msg::UI_MACHINES_AGENT_ID),
        Rem::tenths(70),
        |row: Signal<HubAgentDto>| {
            view! { <bdi>{move || row.with(|row| row.agent_id.clone())}</bdi> }.into_any()
        },
    )
}

/// A cell of the row's key fingerprint, in full.
fn fingerprint_column(localizer: &Arc<Localizer>) -> Column<HubAgentDto> {
    let names = Arc::clone(localizer);
    Column::new(
        "key",
        text(localizer, msg::UI_MACHINES_KEY),
        Rem::tenths(80),
        move |row: Signal<HubAgentDto>| {
            // A row's key doesn't change under its agent ID, so the cell is
            // drawn once rather than again at each read of the list.
            let (value, hostname) =
                row.with_untracked(|row| (row.fingerprint.clone(), row.hostname.clone()));
            let name = of_machine(&names, msg::UI_MACHINES_FINGERPRINT, &hostname);
            view! { <Fingerprint value=value name=name /> }.into_any()
        },
    )
}

/// A column `column` of how long ago the row's status began, in `id`'s words.
fn ago_column(
    localizer: &Arc<Localizer>,
    column: &'static str,
    header: l10n::MessageId,
    id: l10n::MessageId,
) -> Column<HubAgentDto> {
    let words = Arc::clone(localizer);
    Column::new(
        column,
        text(localizer, header),
        Rem::tenths(60),
        move |row: Signal<HubAgentDto>| {
            let words = Arc::clone(&words);
            (move || {
                let seconds = row.with(|row| row.seconds_ago);
                text_with(
                    &words,
                    id,
                    &[("duration", MsgArg::DurationMs(shown_ms(seconds)))],
                )
            })
            .into_any()
        },
    )
}

/// The waiting machines' columns: the hostname, the row's header, its agent
/// ID, its fingerprint, how long it has waited, the machine its hostname
/// looks like, and its Approve and Reject buttons, which `choose` and
/// `leave` open their reviews with. The columns add up to 42 rem, under the
/// Machines panel's 44 rem at the page's default width with CI's wider
/// fonts, so the table fits and no row action is cut off at the frame's
/// edge; each button's label wraps inside it.
fn waiting_columns(
    localizer: &Arc<Localizer>,
    choose: Callback<HubAgentDto>,
    leave: Callback<HubAgentDto>,
) -> (Column<HubAgentDto>, Vec<Column<HubAgentDto>>) {
    let notes = Arc::clone(localizer);
    let note = Column::new(
        "note",
        text(localizer, msg::UI_MACHINES_NOTE),
        Rem::tenths(60),
        move |row: Signal<HubAgentDto>| {
            let notes = Arc::clone(&notes);
            (move || {
                row.with(|row| {
                    row.looks_like
                        .as_ref()
                        .map(|like| of_machine(&notes, msg::UI_MACHINES_LOOKS_LIKE, &like.hostname))
                })
            })
            .into_any()
        },
    );
    let labels = Arc::clone(localizer);
    let action = Column::new(
        "action",
        text(localizer, msg::UI_MACHINES_ACTION),
        Rem::tenths(70),
        move |row: Signal<HubAgentDto>| {
            // Drawn once, so a read of the list every few seconds never
            // replaces the button focus is on; focus goes to the heading
            // when its row leaves the table.
            let (hostname, agent_id) =
                row.with_untracked(|row| (row.hostname.clone(), row.agent_id.clone()));
            let label = of_machine(&labels, msg::UI_MACHINES_APPROVE, &hostname);
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone(button, WAITING_HEADING_ID.to_owned());
            let press = Callback::new(move |()| choose.run(row.get_untracked()));
            let reject = leave_button(&labels, row, leave);
            view! {
                <div class=ACTIONS>
                    <Button
                        id=approve_id(&agent_id)
                        content=Content::Text(label.into())
                        size=Size::Small
                        node_ref=button
                        on_press=press
                    />
                    {reject}
                </div>
            }
            .into_any()
        },
    );
    (
        hostname_column(localizer),
        vec![
            agent_id_column(localizer),
            fingerprint_column(localizer),
            ago_column(
                localizer,
                "waited",
                msg::UI_MACHINES_WAITED,
                msg::UI_MACHINES_WAITED_FOR,
            ),
            note,
            action,
        ],
    )
}

/// The approved machines' columns: the hostname, the row's header, its agent
/// ID, its fingerprint, how long ago it was approved, and its Remove button,
/// which `leave` opens the review with.
fn approved_columns(
    localizer: &Arc<Localizer>,
    leave: Callback<HubAgentDto>,
) -> (Column<HubAgentDto>, Vec<Column<HubAgentDto>>) {
    let labels = Arc::clone(localizer);
    let action = Column::new(
        "action",
        text(localizer, msg::UI_MACHINES_ACTION),
        Rem::tenths(70),
        move |row: Signal<HubAgentDto>| leave_button(&labels, row, leave),
    );
    (
        hostname_column(localizer),
        vec![
            agent_id_column(localizer),
            fingerprint_column(localizer),
            ago_column(
                localizer,
                "approved",
                msg::UI_MACHINES_APPROVED_SINCE,
                msg::UI_MACHINES_APPROVED_AGO,
            ),
            action,
        ],
    )
}

/// How long `seconds` reads, in milliseconds: the hub counts whole seconds,
/// so a status that began under a second ago reads as one second, never as
/// "0 ms".
fn shown_ms(seconds: u64) -> u64 {
    seconds.max(1).saturating_mul(1000)
}

#[cfg(test)]
mod tests {
    use super::shown_ms;

    #[test]
    fn a_status_under_a_second_old_reads_as_one_second() {
        assert_eq!(shown_ms(0), 1000);
        assert_eq!(shown_ms(1), 1000);
        assert_eq!(shown_ms(65), 65_000);
        assert_eq!(shown_ms(u64::MAX), u64::MAX);
    }
}
