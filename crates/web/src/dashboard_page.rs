//! The hub's dashboard, the route `/dashboard` (technical design §4.3, §4.4;
//! #569, #1440): each approved machine in a section of its own, named by its
//! hostname, with when it last connected, a warning when the hub let go of
//! its oldest reports, the shared progress bar of the operation it runs, a
//! table of its latest operations and one of its plans, with a warning when
//! it left some out. What it shows while paused, counts, says, reads again
//! for and how its times and progress read is `dashboard.rs`'s, decided on
//! the host; this is the DOM glue, which the `@ui` scenarios of
//! `features/ui/hub_dashboard.feature` run.
//!
//! The machines are read when the page opens, again each time the events
//! stream opens, the first time and after a reconnect, and when an event of
//! the hub's says a machine was approved, reported an operation, its plans
//! or its progress. A pause holds what the page shows, not the reads.

use std::sync::Arc;
use std::time::Duration;

use api::{
    HUB_MACHINES_PATH, HubMachineDto, HubMachinesDto, IanaZone, MachineOperationDto,
    MachinePlanDto, MsgArg,
};
use l10n::{Localizer, msg};
use leptos::prelude::*;

use crate::backend::Rereads;
use crate::components::alert::rules::AlertTone;
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Alert, Column, Link, Name, PauseUpdates, Progress, Sentences, State, StatusBadge, Table,
    first_load, paced,
};
use crate::dashboard::{Fleet, instant, no_plans, outcome_badge, progress_reading, reads_again};
use crate::operations_page::badge_of;
use crate::pause;
use crate::routes::{NotFound, PageHeading, text_with, time_view};
use crate::settings::agents::hub_known;
use crate::text;
use crate::variants::Rem;
use crate::visit::Visit;
use crate::wording::Words;
use crate::ws;

/// The banner beside the pause button while the page is paused.
const PAUSED_ID: &str = "dashboard-paused";
/// How long one read of the machines may take before it counts as
/// unanswered.
const READ_PATIENCE: Duration = Duration::from_secs(5);
/// How long the page waits before it asks again after a read that went
/// unanswered while none had answered: the "few seconds" it says.
const ASK_AGAIN: Duration = Duration::from_secs(3);

/// The route `/dashboard`: the dashboard on a daemon serving as a hub, and
/// what any path nothing serves shows on one that isn't; nothing until the
/// page knows which.
#[component]
pub(crate) fn DashboardRoute(localizer: Arc<Localizer>) -> impl IntoView {
    let hub = hub_known();
    move || match hub() {
        None => ().into_any(),
        Some(true) => view! { <Dashboard localizer=Arc::clone(&localizer) /> }.into_any(),
        Some(false) => view! { <NotFound localizer=Arc::clone(&localizer) /> }.into_any(),
    }
}

/// On a hub, the start page's way to the dashboard; nothing elsewhere.
#[component]
pub(crate) fn DashboardLink(localizer: Arc<Localizer>) -> impl IntoView {
    let hub = hub_known();
    move || {
        (hub() == Some(true)).then(|| {
            let words = text(&localizer, msg::UI_MACHINES_DASHBOARD_LINK);
            Destination::route("/dashboard", Matching::Exact).map(|to| {
                view! {
                    <p>
                        <Link
                            to=to
                            localizer=Arc::clone(&localizer)
                            placement=LinkPlacement::InText
                        >
                            {words}
                        </Link>
                    </p>
                }
            })
        })
    }
}

/// Lands one read's answer in `fleet`, or its lack of one: an answer
/// replaces the machines shown, and no answer keeps them, or says the hub
/// didn't answer while none has. Whether the page still waits for its
/// first answer.
fn land(fleet: Fleet, read: Option<HubMachinesDto>) -> bool {
    if let Some(read) = read {
        let _ = fleet.machines.try_set(read.machines);
        let _ = fleet.state.try_set(State::Ready);
        return false;
    }
    let waiting = matches!(
        fleet.state.try_get_untracked(),
        Some(State::Loading | State::Unreachable)
    );
    if waiting {
        let _ = fleet.state.try_set(State::Unreachable);
    }
    waiting
}

/// Reads the machines into `fleet`, paced by `rereads` (`ws::reread_hub`).
fn read(fleet: Fleet, rereads: StoredValue<Rereads>, visit: &Visit) {
    ws::reread_hub(
        format!("{}{HUB_MACHINES_PATH}", ws::origin()),
        rereads,
        visit,
        (READ_PATIENCE, ASK_AGAIN),
        move |read| land(fleet, read),
    );
}

/// Reads the machines into `fleet` now, when the events stream opens and
/// when an event asks for it, as long as the page shows.
fn follow(fleet: Fleet) {
    let rereads = StoredValue::new(Rereads::default());
    let (visit, departure) = Visit::begin();
    on_cleanup(move || drop(departure));
    read(fleet, rereads, &visit);
    let (opening, hearing) = (visit.clone(), visit.clone());
    ws::listen(
        &ws::origin(),
        &visit,
        move || read(fleet, rereads, &opening),
        move |event| {
            if reads_again(&event) {
                read(fleet, rereads, &hearing);
            }
        },
    );
}

/// The dashboard itself.
#[component]
fn Dashboard(localizer: Arc<Localizer>) -> impl IntoView {
    let fleet = Fleet::new();
    follow(fleet);
    let viewer = l10n::viewer_zone();
    let (pause_label, paused_said) = (
        text(&localizer, msg::UI_LIVE_PAUSE),
        text(&localizer, msg::UI_LIVE_PAUSED),
    );
    let banner_localizer = Arc::clone(&localizer);
    let resume_localizer = Arc::clone(&localizer);
    let on_resume = Callback::new(move |()| {
        let words = fleet.resume();
        text_with(&resume_localizer, words.id, &words.args)
    });
    let loading = text(&localizer, msg::UI_DASHBOARD_LOADING);
    let unreachable = text(&localizer, msg::UI_DASHBOARD_UNREACHABLE);
    let none = text(&localizer, msg::UI_DASHBOARD_NONE);
    let shown = fleet.shown();
    // Whether there is a machine to show, apart from the machines: their
    // sections update in place, and only this builds them again.
    let empty = Memo::new(move |_| shown.with(Vec::is_empty));
    let (state, pace) = paced(
        move || fleet.shown_state().get(),
        |state| *state == State::Loading,
    );
    let heading = text(&localizer, msg::UI_DASHBOARD_HEADING);
    let intro = text(&localizer, msg::UI_DASHBOARD_INTRO);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{intro}</p>
        <PauseUpdates
            label=pause_label
            paused_said=paused_said
            paused=Signal::derive(move || fleet.paused())
            banner=Signal::derive(move || {
                let words = pause::banner(fleet.pending());
                text_with(&banner_localizer, words.id, &words.args)
            })
            banner_id=PAUSED_ID
            on_pause=Callback::new(move |()| fleet.pause())
            on_resume=on_resume
        />
        {move || match (state.get(), empty.get()) {
            (State::Loading, _) => first_load(loading.clone(), Awaited::table(2), pace).into_any(),
            (State::Unreachable, _) => view! { <p>{unreachable.clone()}</p> }.into_any(),
            (State::Ready, true) => view! { <p>{none.clone()}</p> }.into_any(),
            (State::Ready, false) => {
                let localizer = Arc::clone(&localizer);
                let viewer = viewer.clone();
                view! {
                    <For
                        each=move || shown.get()
                        key=|machine: &HubMachineDto| machine.agent_id.clone()
                        children=move |machine: HubMachineDto| {
                            machine_section(&localizer, shown, machine, viewer.clone())
                        }
                    />
                }
                    .into_any()
            }
        }}
    }
}

/// The section of `first`, a machine as the page first showed it, kept as
/// `shown` has it since: its heading, when it last connected, the reports
/// the hub let go of, the progress of the operation it runs, its latest
/// operations and its plans. Its hostname is the one it was approved under,
/// which a later read doesn't change.
fn machine_section(
    localizer: &Arc<Localizer>,
    shown: Signal<Vec<HubMachineDto>>,
    first: HubMachineDto,
    viewer: Option<IanaZone>,
) -> impl IntoView + use<> {
    let HubMachineDto {
        agent_id, hostname, ..
    } = first;
    let heading_id = format!("dashboard-machine-{agent_id}");
    let labelled_by = heading_id.clone();
    let then_focus = heading_id.clone();
    let machine = Signal::derive(move || {
        shown.with(|machines| {
            machines
                .iter()
                .find(|machine| machine.agent_id == agent_id)
                .cloned()
        })
    });
    let zone = Signal::derive(move || machine.with(|machine| machine.as_ref()?.zone.clone()));
    let last_connected = {
        let localizer = Arc::clone(localizer);
        let viewer = viewer.clone();
        move || {
            let at = machine.with(|machine| {
                machine
                    .as_ref()
                    .map(|machine| machine.last_seen.clone())
                    .unwrap_or_default()
            });
            let when = shown_instant(&localizer, &at, viewer.as_ref(), zone.get().as_deref());
            text_with(
                &localizer,
                msg::UI_DASHBOARD_LAST_CONNECTED,
                &[("when", MsgArg::Text(when))],
            )
        }
    };
    let dropped = {
        let localizer = Arc::clone(localizer);
        move || {
            let count = machine.with(|machine| machine.as_ref().map_or(0, |m| m.reports_dropped));
            (count > 0).then(|| {
                let words = text_with(
                    &localizer,
                    msg::UI_DASHBOARD_DROPPED,
                    &[("count", MsgArg::Count(count))],
                );
                view! {
                    <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Warning>
                        {words}
                    </Alert>
                }
            })
        }
    };
    let caption = text_with(
        localizer,
        msg::UI_DASHBOARD_OPERATIONS,
        &[("hostname", MsgArg::Text(hostname.clone()))],
    );
    let operations = Signal::derive(move || {
        machine.with(|machine| {
            machine
                .as_ref()
                .map(|machine| machine.operations.clone())
                .unwrap_or_default()
        })
    });
    let (row_header, columns) = operation_columns(localizer, viewer.clone(), zone);
    let t = |id| text(localizer, id);
    let sentences = Sentences {
        loading: t(msg::UI_DASHBOARD_LOADING),
        empty: t(msg::UI_DASHBOARD_NO_OPERATIONS),
        unreachable: t(msg::UI_DASHBOARD_UNREACHABLE),
    };
    let progress = {
        let localizer = Arc::clone(localizer);
        let label = text_with(
            &localizer,
            msg::UI_DASHBOARD_PROGRESS,
            &[("hostname", MsgArg::Text(hostname.clone()))],
        );
        let viewer = viewer.clone();
        let reading = Memo::new(move |_| {
            machine.with(|machine| machine.as_ref()?.progress.as_ref().map(progress_reading))
        });
        let running = Memo::new(move |_| reading.with(Option::is_some));
        // Progress is never announced (decision 83): the bar tells its value
        // when the reader comes to it.
        move || {
            running.get().then(|| {
                view! {
                    <Progress
                        localizer=Arc::clone(&localizer)
                        label=label.clone()
                        then_focus=then_focus.clone()
                        reading=Signal::derive(move || reading.get().unwrap_or_default())
                        zone=viewer.clone()
                    />
                }
            })
        }
    };
    let plans_section = plans_view(localizer, &hostname, machine, viewer, zone);
    view! {
        <section aria-labelledby=labelled_by>
            <h2 id=heading_id tabindex="-1">
                <bdi>{hostname}</bdi>
            </h2>
            <p>{last_connected}</p>
            {dropped}
            {progress}
            <Table
                name=Name::Caption(caption)
                row_header=row_header
                columns=columns
                rows=operations
                key=|row: &MachineOperationDto| row.operation.clone()
                state=State::Ready
                sentences=sentences
            />
            {plans_section}
        </section>
    }
}

/// A machine's plans as it last reported them: a warning when it left some
/// out, then their table, which says none was reported before the first.
fn plans_view(
    localizer: &Arc<Localizer>,
    hostname: &str,
    machine: Signal<Option<HubMachineDto>>,
    viewer: Option<IanaZone>,
    zone: Signal<Option<String>>,
) -> impl IntoView + use<> {
    let left_out = {
        let localizer = Arc::clone(localizer);
        move || {
            let count = machine.with(|machine| {
                machine
                    .as_ref()
                    .and_then(|machine| machine.plans.as_ref())
                    .map_or(0, |plans| plans.left_out)
            });
            (count > 0).then(|| {
                let words = text_with(
                    &localizer,
                    msg::UI_DASHBOARD_PLANS_LEFT_OUT,
                    &[("count", MsgArg::Count(count))],
                );
                view! {
                    <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Warning>
                        {words}
                    </Alert>
                }
            })
        }
    };
    let caption = text_with(
        localizer,
        msg::UI_DASHBOARD_PLANS,
        &[("hostname", MsgArg::Text(hostname.to_owned()))],
    );
    let plans = Signal::derive(move || {
        machine.with(|machine| {
            machine
                .as_ref()
                .and_then(|machine| machine.plans.as_ref())
                .map(|plans| plans.plans.clone())
                .unwrap_or_default()
        })
    });
    // Which sentence the empty table says changes only when the machine's
    // first report of its plans arrives; the table is drawn again then.
    let empty = Memo::new(move |_| {
        machine
            .with(|machine| no_plans(machine.as_ref().and_then(|machine| machine.plans.as_ref())))
    });
    let localizer = Arc::clone(localizer);
    let table = move || {
        let (row_header, columns) = plan_columns(&localizer, viewer.clone(), zone);
        let t = |id| text(&localizer, id);
        let sentences = Sentences {
            loading: t(msg::UI_DASHBOARD_LOADING),
            empty: t(empty.get()),
            unreachable: t(msg::UI_DASHBOARD_UNREACHABLE),
        };
        view! {
            <Table
                name=Name::Caption(caption.clone())
                row_header=row_header
                columns=columns
                rows=plans
                key=|row: &MachinePlanDto| row.plan.clone()
                state=State::Ready
                sentences=sentences
            />
        }
    };
    view! {
        {left_out}
        {table}
    }
}

/// The instant `at` as the dashboard writes it, in the viewer's zone and the
/// machine's `zone` beside it when they differ; as it came when it can't be
/// read, never a blank.
fn shown_instant(
    localizer: &Localizer,
    at: &str,
    viewer: Option<&IanaZone>,
    zone: Option<&str>,
) -> String {
    instant(at, viewer, zone).map_or_else(
        || at.to_owned(),
        |Words { id, args }| text_with(localizer, id, &args),
    )
}

/// The columns of a machine's operations: when each started, its row's
/// header, when it ended, how it came out and the error code it failed with.
fn operation_columns(
    localizer: &Arc<Localizer>,
    viewer: Option<IanaZone>,
    zone: Signal<Option<String>>,
) -> (
    Column<MachineOperationDto>,
    Vec<Column<MachineOperationDto>>,
) {
    let time_column = |id: &'static str, header, at: fn(&MachineOperationDto) -> Option<String>| {
        let localizer = Arc::clone(localizer);
        let viewer = viewer.clone();
        Column::new(
            id,
            text(&localizer, header),
            Rem::tenths(160),
            move |row: Signal<MachineOperationDto>| {
                let localizer = Arc::clone(&localizer);
                let viewer = viewer.clone();
                (move || match row.with(at) {
                    Some(raw) => {
                        let shown =
                            shown_instant(&localizer, &raw, viewer.as_ref(), zone.get().as_deref());
                        time_view(shown, Some(raw))
                    }
                    None => text(&localizer, msg::UI_DASHBOARD_UNKNOWN).into_any(),
                })
                .into_any()
            },
        )
    };
    let started = time_column("started", msg::UI_DASHBOARD_STARTED, |row| {
        row.started_at.clone()
    });
    let ended = time_column("ended", msg::UI_DASHBOARD_ENDED, |row| row.ended_at.clone());
    let outcome = {
        let localizer = Arc::clone(localizer);
        Column::new(
            "outcome",
            text(&localizer, msg::UI_DASHBOARD_OUTCOME),
            Rem::tenths(80),
            move |row: Signal<MachineOperationDto>| {
                let localizer = Arc::clone(&localizer);
                let badge = Signal::derive(move || row.with(|row| outcome_badge(row.outcome)));
                view! {
                    <StatusBadge
                        state=Signal::derive(move || badge.get().0)
                        label=Signal::derive(move || text(&localizer, badge.get().1))
                    />
                }
                .into_any()
            },
        )
    };
    let code = Column::new(
        "code",
        text(localizer, msg::UI_DASHBOARD_CODE),
        Rem::tenths(120),
        |row: Signal<MachineOperationDto>| {
            view! { <bdi>{move || row.with(|row| row.code.clone().unwrap_or_default())}</bdi> }
                .into_any()
        },
    );
    (started, vec![ended, outcome, code])
}

/// The columns of a machine's plans: each plan, its row's header, how and
/// when it last ran, and when it runs next, each by the machine's clock.
fn plan_columns(
    localizer: &Arc<Localizer>,
    viewer: Option<IanaZone>,
    zone: Signal<Option<String>>,
) -> (Column<MachinePlanDto>, Vec<Column<MachinePlanDto>>) {
    let plan = Column::new(
        "plan",
        text(localizer, msg::UI_DASHBOARD_PLAN),
        Rem::tenths(160),
        |row: Signal<MachinePlanDto>| {
            view! { <bdi>{move || row.with(|row| row.plan.clone())}</bdi> }.into_any()
        },
    );
    let last_run = {
        let localizer = Arc::clone(localizer);
        let viewer = viewer.clone();
        Column::new(
            "last-run",
            text(&localizer, msg::UI_DASHBOARD_LAST_RUN),
            Rem::tenths(240),
            move |row: Signal<MachinePlanDto>| {
                let localizer = Arc::clone(&localizer);
                let viewer = viewer.clone();
                (move || match row.with(|row| row.last_run.clone()) {
                    Some(run) => {
                        let shown = shown_instant(
                            &localizer,
                            &run.at,
                            viewer.as_ref(),
                            zone.get().as_deref(),
                        );
                        let (state, label) = badge_of(&localizer, &run.status);
                        view! {
                            <StatusBadge state=state label=label />
                            " "
                            {time_view(shown, Some(run.at))}
                        }
                        .into_any()
                    }
                    None => text(&localizer, msg::UI_DASHBOARD_NEVER_RUN).into_any(),
                })
                .into_any()
            },
        )
    };
    let next_run = {
        let localizer = Arc::clone(localizer);
        Column::new(
            "next-run",
            text(&localizer, msg::UI_DASHBOARD_NEXT_RUN),
            Rem::tenths(160),
            move |row: Signal<MachinePlanDto>| {
                let localizer = Arc::clone(&localizer);
                let viewer = viewer.clone();
                (move || match row.with(|row| row.next_run.clone()) {
                    Some(raw) => {
                        let shown =
                            shown_instant(&localizer, &raw, viewer.as_ref(), zone.get().as_deref());
                        time_view(shown, Some(raw))
                    }
                    None => text(&localizer, msg::UI_DASHBOARD_NOT_PLANNED).into_any(),
                })
                .into_any()
            },
        )
    };
    (plan, vec![last_run, next_run])
}
