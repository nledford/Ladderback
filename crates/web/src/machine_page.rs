//! A machine's page on a hub, `/machines/<machine-id>` (technical design
//! §4.4; §16, decision 175): its heading, the machine's hostname, "This
//! hub" under it on the hub's own page, then whether it is online now or
//! when it last connected, a warning when the hub let go of its oldest
//! reports, the shared progress bar of the operation it runs, a table of
//! its latest operations and one of its plans, with a warning when it left
//! some out. The hub's own page shows its own plans and operations, read as
//! the dashboard reads them for its card. The page reads, follows and
//! pauses as the dashboard does (`dashboard_page.rs`); what it shows is
//! `dashboard.rs`'s, decided on the host. It is a page not found for a
//! machine the hub hasn't approved, and on a daemon that isn't a hub. The
//! `@ui` scenarios of `features/ui/machine_page.feature` run it.

use std::sync::Arc;

use api::{HubMachineDto, IanaZone, MachineOperationDto, MachinePlanDto, MsgArg};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::components::alert::rules::AlertTone;
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Alert, Column, Name, Progress, Sentences, State, StatusBadge, Table, first_load, paced,
};
use crate::dashboard::{
    Fleet, connection_line, no_plans, not_found, outcome_badge, progress_reading,
};
use crate::dashboard_page::{follow, pause_control, shown_instant};
use crate::operations_page::badge_of;
use crate::routes::{NotFound, PAGE_HEADING_ID, PageHeading, machine_title, text_with, time_view};
use crate::settings::agents::hub_known;
use crate::text;
use crate::variants::Rem;

/// The route `/machines/:id`: on a daemon serving as a hub, the page of the
/// machine the address names; on one that isn't, what any path nothing
/// serves shows; nothing until the page knows which.
#[component]
pub(crate) fn MachineRoute(localizer: Arc<Localizer>) -> impl IntoView {
    let hub = hub_known();
    move || match hub() {
        None => ().into_any(),
        Some(true) => view! { <MachinePage localizer=Arc::clone(&localizer) /> }.into_any(),
        Some(false) => view! { <NotFound localizer=Arc::clone(&localizer) /> }.into_any(),
    }
}

/// Sets the page's title, for the browser's tab and history.
fn set_title(title: &str) {
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        document.set_title(title);
    }
}

/// The page of the machine the address names: its heading, its hostname
/// once read, then its section; a page not found once the machines and the
/// hub's own are read and it is neither, and while the hub's own goes
/// unread, a sentence saying so in its place (#1692).
#[component]
fn MachinePage(localizer: Arc<Localizer>) -> impl IntoView {
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|params| params.get("id").unwrap_or_default()));
    let fleet = Fleet::new();
    follow(fleet, &localizer);
    let viewer = l10n::viewer_zone();
    let shown = fleet.shown();
    let (state, pace) = paced(
        move || fleet.shown_state().get(),
        |state| *state == State::Loading,
    );
    // The machine the address names, its ID and the hostname it was
    // approved under, which a later read doesn't change: its section is
    // built once, and follows `shown` in place.
    let found = Memo::new(move |_| {
        let id = id.get();
        shown.with(|machines| {
            machines
                .iter()
                .find(|machine| machine.machine_id == id)
                .map(|machine| (machine.machine_id.clone(), machine.hostname.clone()))
        })
    });
    let own_unread = fleet.own_unread();
    let missing =
        Memo::new(move |_| not_found(state.get(), found.with(Option::is_some), own_unread.get()));
    let own = fleet.own_id();
    let is_own = Memo::new(move |_| {
        found.with(|found| {
            found
                .as_ref()
                .is_some_and(|(id, _)| own.with(|own| own.as_ref() == Some(id)))
        })
    });
    // Titled by the machine's hostname once read (WCAG 2.4.2), and as a page
    // not found once it is plain the hub hasn't approved it.
    let title_localizer = Arc::clone(&localizer);
    Effect::new(move |_| {
        if missing.get() {
            set_title(&text(&title_localizer, msg::UI_NOT_FOUND_TITLE));
        } else if let Some((_, hostname)) = found.get() {
            set_title(&machine_title(&title_localizer, Some(&hostname)));
        }
    });
    let waiting_heading = text(&localizer, msg::UI_MACHINE_PAGE_HEADING);
    let this_hub = text(&localizer, msg::UI_MACHINE_PAGE_THIS_HUB);
    let loading = text(&localizer, msg::UI_DASHBOARD_LOADING);
    let unreachable = text(&localizer, msg::UI_DASHBOARD_UNREACHABLE);
    let own_unanswered = text(&localizer, msg::UI_DASHBOARD_OWN_UNANSWERED);
    move || {
        if missing.get() {
            return view! { <NotFound localizer=Arc::clone(&localizer) /> }.into_any();
        }
        let heading = {
            let waiting = waiting_heading.clone();
            move || {
                found
                    .get()
                    .map_or_else(|| waiting.clone(), |(_, hostname)| hostname)
            }
        };
        let this_hub = this_hub.clone();
        let (loading, unreachable) = (loading.clone(), unreachable.clone());
        let own_unanswered = own_unanswered.clone();
        let pause = pause_control(&localizer, fleet);
        let localizer = Arc::clone(&localizer);
        let viewer = viewer.clone();
        let section = move || match (state.get(), found.get()) {
            (State::Loading, _) => first_load(loading.clone(), Awaited::table(2), pace).into_any(),
            (State::Unreachable, _) => view! { <p>{unreachable.clone()}</p> }.into_any(),
            (State::Ready, Some((agent_id, hostname))) => {
                machine_section(&localizer, shown, agent_id, hostname, viewer.clone()).into_any()
            }
            // Not missing, so the hub's own goes unread, and it may be this.
            (State::Ready, None) => view! { <p>{own_unanswered.clone()}</p> }.into_any(),
        };
        view! {
            <PageHeading>
                <bdi>{heading}</bdi>
            </PageHeading>
            {move || is_own.get().then(|| view! { <p>{this_hub.clone()}</p> })}
            {pause}
            {section}
        }
        .into_any()
    }
}

/// The section of the machine `agent_id`, named `hostname`, kept as `shown`
/// has it: when it last connected, the reports the hub let go of, the
/// progress of the operation it runs, its latest operations and its plans.
/// Its hostname is the one it was approved under, which a later read
/// doesn't change.
fn machine_section(
    localizer: &Arc<Localizer>,
    shown: Signal<Vec<HubMachineDto>>,
    agent_id: String,
    hostname: String,
    viewer: Option<IanaZone>,
) -> impl IntoView + use<> {
    let machine = Signal::derive(move || {
        shown.with(|machines| {
            machines
                .iter()
                .find(|machine| machine.machine_id == agent_id)
                .cloned()
        })
    });
    // The progress bar hands focus to the page's heading when it goes.
    let then_focus = PAGE_HEADING_ID.to_owned();
    let zone = Signal::derive(move || machine.with(|machine| machine.as_ref()?.zone.clone()));
    let last_connected = {
        let localizer = Arc::clone(localizer);
        let viewer = viewer.clone();
        move || {
            let (online, at) = machine.with(|machine| {
                machine
                    .as_ref()
                    .map(|machine| (machine.online, machine.last_seen.clone()))
                    .unwrap_or_default()
            });
            let when = shown_instant(&localizer, &at, viewer.as_ref(), zone.get().as_deref());
            text_with(
                &localizer,
                connection_line(online),
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
