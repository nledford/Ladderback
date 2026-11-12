//! The routes of the app (technical design §4.5, "Route changes"): the pages a
//! path shows. Each page has its own `<h1>`, the heading focus moves to when a
//! reader navigates to it, so it carries `tabindex="-1"`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use api::{ErrorCode, MsgArg, PlanSummaryDto, PlansDto, RepositoriesDto, RepositoryDto};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Serialize;
use std::str::FromStr;

use crate::announcer::Politeness;
use crate::backend::{OperationRow, OperationsState, ended};
use crate::components::{
    Checkbox, ErrorSummary, FormField, PauseUpdates, Progress, StatusBadge, SummaryItem,
};
use crate::live::{reply, say};
use crate::operations::{self, Name};
use crate::pause::{self, Ends, Pausable};
use crate::repositories::{
    Action, Field, Form, Outcome, RowCreated, Sentence, advises_retry, create_label,
    create_saved_path, error_of, field_of, password_source, problems_of, repository_notes,
    rereads_list, row_refusal,
};
use crate::ws::{self, Phase, Posted};
use crate::{page, text};

/// The `<h1>` of a page, with `tabindex="-1"` so that focus can move to it.
/// When a reader navigates to the page it takes focus as it is shown
/// ([`page::focus_on_arrival`]), so the heading is there when focus moves.
#[component]
pub(crate) fn PageHeading(children: Children) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H1>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading.get() {
            page::focus_on_arrival(&heading);
        }
    });
    view! { <h1 node_ref=heading tabindex="-1">{children()}</h1> }
}

/// The start page, the route `/`.
#[component]
pub(crate) fn Start(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_START_HEADING);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{text(&localizer, msg::UI_START_BODY)}</p>
    }
}

/// How the operations table is ordered (§4.4). `History` is the order the
/// daemon's own history page came in, newest first; the other two are the
/// sort button's, announced on each change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SortOrder {
    #[default]
    History,
    NewestFirst,
    OldestFirst,
}

/// The body a start request carries: the plan to back up.
#[derive(Serialize)]
struct StartBackup {
    plan: String,
}

/// The body a cancel request carries: the JSON-body rule applies to every
/// mutating method, so even an empty one is `{}`.
#[derive(Serialize)]
pub(crate) struct Empty {}

/// The operations page, the route `/operations`: the backup history, live —
/// a running operation's progress and its end arrive over the events
/// stream (§4.4). Its decisions are `backend::OperationsState`; its glue is
/// `ws::watch`. The heading follows the connection: a daemon that cannot be
/// reached says so, in place of the page's own heading.
///
/// The table is built once and its rows follow the state in place: an update
/// to a row changes the text, the bar or the button it is about, and nothing
/// else, so a control a reader has focused is never replaced while its row
/// updates (WCAG 2.4.3).
///
/// A reader can pause the page (§4.5, "Live updates"; WCAG 2.2.2): the stream
/// goes on filling the live state, while the heading and the table show the
/// copy taken at the pause ([`Pausable`]), and the runs that end meanwhile are
/// counted rather than announced, then told in one sentence on the resume.
#[component]
pub(crate) fn Operations(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_OPERATIONS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_OPERATIONS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let start_localizer = Arc::clone(&localizer);
    let caption = text(&localizer, msg::UI_OPERATIONS_CAPTION);
    let (operation_column, status_column, started_column, result_column, actions_column) = (
        text(&localizer, msg::UI_COLUMN_OPERATION),
        text(&localizer, msg::UI_COLUMN_STATUS),
        text(&localizer, msg::UI_COLUMN_STARTED),
        text(&localizer, msg::UI_COLUMN_RESULT),
        text(&localizer, msg::UI_COLUMN_ACTIONS),
    );
    let sort_label = text(&localizer, msg::UI_SORT_BY_STARTED);
    let sorted_newest = text(&localizer, msg::UI_SORTED_NEWEST);
    let sorted_oldest = text(&localizer, msg::UI_SORTED_OLDEST);
    let empty = text(&localizer, msg::UI_NO_OPERATIONS);
    let (started_text, still_waiting) = (
        text(&localizer, msg::UI_BACKUP_STARTED),
        text(&localizer, msg::UI_STILL_WAITING),
    );
    let (pause_label, paused_said) = (
        text(&localizer, msg::UI_LIVE_PAUSE),
        text(&localizer, msg::UI_LIVE_PAUSED),
    );
    let localizer_for_errors = Arc::clone(&localizer);
    // The state the page shows, and the connection that fills it: the history
    // first, then the events stream, with the reconnect's re-fetch between
    // them (§4.4). Plans come from a read of their own.
    let ops = RwSignal::new(OperationsState::default());
    let phase = RwSignal::new(Phase::Loading);
    let plans = RwSignal::<Option<Vec<PlanSummaryDto>>>::new(None);
    let error = RwSignal::<Option<String>>::new(None);
    let sort = RwSignal::new(SortOrder::History);
    let base = ws::origin();
    ws::watch(&base, ops, phase);
    // What the page shows: the live state, or the copy taken when the reader
    // paused it. Every view reads this; the stream, the settle-poll and the
    // end announcements read the live state.
    let page = Pausable::new(ops, phase);
    let plans_url = format!("{base}/api/v1/plans");
    let plans_ops = ops;
    spawn_local(async move {
        if let Some(read) = ws::read::<PlansDto>(&plans_url).await {
            plans.set(Some(read.plans));
        }
        let _ = plans_ops;
    });
    // The end of a run is announced from a status transition, so the events
    // stream and the settle-poll both arrive at one announcement, whichever
    // sees the end first. The first sight of a row only seeds its status.
    // The end of a run the reader cancelled answers that press, and is said
    // even while the page is paused. Any other end, while the page is paused,
    // is counted instead, and the resume tells the count in one sentence.
    let seen = StoredValue::new(HashMap::<String, String>::new());
    let cancelled = StoredValue::new(HashSet::<String>::new());
    let ends = StoredValue::new(Ends::default());
    let announced_ops = ops;
    let announced_localizer = Arc::clone(&localizer);
    Effect::new(move |_| {
        let rows = announced_ops.get().rows;
        let paused = page.paused_untracked();
        seen.update_value(|seen| {
            for row in &rows {
                let status = row.summary.status.clone();
                let previous = seen.insert(row.summary.id.clone(), status.clone());
                let changed = previous.is_some_and(|previous| announces_end(&previous, &status));
                if changed {
                    let asked = cancelled
                        .try_update_value(|cancelled| cancelled.remove(&row.summary.id))
                        .unwrap_or_default();
                    let (politeness, words) = operations::end(&row.summary);
                    let words = text_with(&announced_localizer, words.id, &words.args);
                    if asked {
                        reply(politeness, &words);
                    } else if paused {
                        ends.update_value(|ends| ends.count(&status));
                    } else {
                        say(politeness, &words);
                    }
                }
            }
        });
    });
    let banner_localizer = Arc::clone(&localizer);
    let resumed_localizer = Arc::clone(&localizer);
    let on_pause = Callback::new(move |()| {
        ends.set_value(Ends::default());
        page.pause();
    });
    let on_resume = Callback::new(move |()| {
        page.resume();
        let words = pause::resumed(ends.get_value());
        ends.set_value(Ends::default());
        text_with(&resumed_localizer, words.id, &words.args)
    });
    // What the page shows, and the ids of the rows in the order it shows them.
    // Each is a memo, so an update that leaves it as it was rebuilds nothing:
    // a progress report or a status change is an update to a row, which the
    // row follows by itself, and the table and the controls in it stay where
    // they are. History is the daemon's own order; a sort order is the sort
    // button's, and follows it as the reader presses it.
    let showing = Memo::new(move |_| {
        let phase = page.phase();
        page.with(|state| shown(phase, &state.rows))
    });
    let order = Memo::new(move |_| page.with(|state| ordered(&state.rows, sort.get())));
    let table_localizer = Arc::clone(&localizer);
    // The zone the browser reads its clock in, asked once: every instant the
    // page shows is written in it (§8.4).
    let viewer = l10n::viewer_zone();
    let start_base = base.clone();
    let cancel_base = base.clone();
    let start_ops = ops;
    view! {
        <PageHeading>
            {move || if page.phase() == Phase::Unreachable { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        <PauseUpdates
            label=pause_label
            paused_said=paused_said
            paused=Signal::derive(move || page.paused())
            banner=Signal::derive(move || {
                let words = pause::banner(page.pending());
                text_with(&banner_localizer, words.id, &words.args)
            })
            banner_id="operations-paused"
            on_pause=on_pause
            on_resume=on_resume
        />
        {move || error.get().map(|text| view! { <p role="alert">{text}</p> })}
        // The plans sit outside the connection's phases: a reader can start a
        // backup while the history is still on its way. Their buttons are a
        // row of actions, as a dialog's are, so each stands apart from the
        // next and they wrap on a small screen (§4.4).
        {move || {
            let buttons = plans
                .get()
                .map(|plans| {
                    plans
                        .into_iter()
                        .map(|plan| {
                            let started = started_text.clone();
                            let still_waiting = still_waiting.clone();
                            let url = format!("{}/api/v1/operations", start_base);
                            let ops = start_ops;
                            let base = start_base.clone();
                            let error_localizer = Arc::clone(&localizer_for_errors);
                            // Named with its plan, as a row's Cancel is: a
                            // reader tabbing through the plans hears which
                            // one each button runs (2.4.6, 4.1.2).
                            let label = text_with(
                                &start_localizer,
                                msg::UI_START_PLAN,
                                &[("plan", MsgArg::Text(plan.id.clone()))],
                            );
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        let request = StartBackup { plan: plan.id.clone() };
                                        let url = url.clone();
                                        let base = base.clone();
                                        let started = started.clone();
                                        let still_waiting = still_waiting.clone();
                                        let error_localizer = Arc::clone(&error_localizer);
                                        let ops = ops;
                                        spawn_local(async move {
                                            match ws::post(&url, &request).await {
                                                Some(Posted::Done(answer)) => {
                                                    reply(Politeness::Polite, &started);
                                                    if let Some(operation) =
                                                        ws::started_operation(&answer)
                                                        && !ws::settle(&base, ops, &operation).await
                                                    {
                                                        reply(Politeness::Polite, &still_waiting);
                                                    }
                                                }
                                                Some(Posted::Refused(code, args, detail)) => {
                                                    error.set(Some(refusal(
                                                        &error_localizer,
                                                        code,
                                                        &args,
                                                        detail.as_deref(),
                                                    )));
                                                }
                                                // No answer at all: the page's
                                                // phase says that its own way.
                                                None => {}
                                            }
                                        });
                                    }
                                >
                                    {label}
                                </button>
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!buttons.is_empty()).then(|| view! { <p class="actions">{buttons}</p> })
        }}
        {move || {
            // Per render: the sort button's and rows' closures own what they
            // say and show. A render is a change of what the page shows, never
            // an update to a row.
            let newest = sorted_newest.clone();
            let oldest = sorted_oldest.clone();
            let rows_localizer = table_localizer.clone();
            let rows_base = cancel_base.clone();
            let rows_zone = viewer.clone();
            match showing.get() {
                Shown::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
                Shown::Unreachable => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
                Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
                Shown::Table => {
                    view! {
                        <table class="operations">
                            <caption class="text-muted">{caption.clone()}</caption>
                            <thead>
                                <tr>
                                    <th scope="col">{operation_column.clone()}</th>
                                    <th scope="col">{status_column.clone()}</th>
                                    <th
                                        scope="col"
                                        aria-sort=move || match sort.get() {
                                            SortOrder::OldestFirst => "ascending",
                                            _ => "descending",
                                        }
                                    >
                                        <span class="sort-header">
                                            {started_column.clone()}
                                            <button
                                                type="button"
                                                on:click=move |_| {
                                                    // The first press is the newest
                                                    // first: the history is already
                                                    // the daemon's newest first, so
                                                    // the sort's own orders follow.
                                                    let next = match sort.get_untracked() {
                                                        SortOrder::History
                                                        | SortOrder::OldestFirst => SortOrder::NewestFirst,
                                                        SortOrder::NewestFirst => SortOrder::OldestFirst,
                                                    };
                                                    sort.set(next);
                                                    reply(
                                                        Politeness::Polite,
                                                        match next {
                                                            SortOrder::OldestFirst => &oldest,
                                                            _ => &newest,
                                                        },
                                                    );
                                                }
                                            >
                                                {sort_label.clone()}
                                            </button>
                                        </span>
                                    </th>
                                    <th scope="col">{result_column.clone()}</th>
                                    <th scope="col">{actions_column.clone()}</th>
                                </tr>
                            </thead>
                            <tbody>
                                <For
                                    each=move || order.get()
                                    key=|id| id.clone()
                                    children=move |id| {
                                        row_view(
                                            rows_localizer.clone(),
                                            rows_base.clone(),
                                            page,
                                            cancelled,
                                            id,
                                            error,
                                            rows_zone.clone(),
                                        )
                                    }
                                />
                            </tbody>
                        </table>
                    }
                    .into_any()
                }
            }
        }}
    }
}

/// One row of the operations table: its header, which names the operation (a
/// backup by its plan, a maintenance run by what ran and its repository), its
/// status (with a running backup's progress), the instant it started — the
/// raw instant in the cell's `title`, so an order can be read without the
/// locale's text — what a run that succeeded produced, and what can be done
/// with it: a running or queued run can be cancelled.
///
/// A row is built once, when its operation first shows, and follows the state
/// from then on. Each fact it shows is a memo of its own, and each cell binds
/// to the facts it shows, so an update reaches only the cells it is about: a
/// progress report moves the bar, a start changes the status and the Started
/// cell, and none of them replaces the Cancel button (WCAG 2.4.3). The button
/// goes only when the run ends, and then it is no longer a control.
///
/// What it shows is read from `page`, which holds still while the reader has
/// paused it; a cancel waits for its end in the live state, and puts its run
/// in `cancelled`, so that its end is said as the answer to the press.
fn row_view(
    localizer: Arc<Localizer>,
    base: String,
    page: Pausable,
    cancelled: StoredValue<HashSet<String>>,
    id: String,
    error: RwSignal<Option<String>>,
    zone: Option<api::IanaZone>,
) -> impl IntoView {
    // Its refusals are worded in the same language as the rest of it.
    let error_localizer = Arc::clone(&localizer);
    // What an operation is and where it ran do not change, so its name and its
    // Cancel button's label are worded once.
    let (name, cancel_label) = untrack(|| {
        page.with(|state| {
            state.row(&id).map(|row| {
                let name = match operations::name(&row.summary) {
                    Name::Plan(plan) => plan,
                    Name::Words(words) => text_with(&localizer, words.id, &words.args),
                };
                let cancel = operations::cancel_label(&row.summary);
                (name, text_with(&localizer, cancel.id, &cancel.args))
            })
        })
    })
    .unwrap_or_default();
    // A row that has left the state, in the moment before its view is dropped,
    // reads as empty: never a panic, never a stale fact.
    let status = Memo::new({
        let id = id.clone();
        move |_| {
            page.with(|state| {
                state
                    .row(&id)
                    .map(|row| row.summary.status.clone())
                    .unwrap_or_default()
            })
        }
    });
    let started = Memo::new({
        let id = id.clone();
        move |_| {
            page.with(|state| {
                state
                    .row(&id)
                    .map(|row| key(row).to_owned())
                    .unwrap_or_default()
            })
        }
    });
    let percent = Memo::new({
        let id = id.clone();
        move |_| page.with(|state| state.row(&id).and_then(progress_percent))
    });
    let result = Memo::new({
        let id = id.clone();
        move |_| {
            page.with(|state| {
                state
                    .row(&id)
                    .map(|row| operations::result(&row.summary))
                    .unwrap_or_default()
            })
        }
    });
    let badge = Memo::new({
        let localizer = Arc::clone(&localizer);
        move |_| status.with(|status| badge_of(&localizer, status))
    });
    let can_cancel = Memo::new(move |_| status.with(|status| cancellable(status)));
    let has_progress = Memo::new(move |_| percent.get().is_some());
    let result_localizer = Arc::clone(&localizer);
    let progress_label = text(&localizer, msg::UI_BACKUP_PROGRESS);
    let still_waiting = text(&localizer, msg::UI_STILL_WAITING);
    let url = format!("{base}/api/v1/operations/{id}/cancel");
    let operation = id;
    view! {
        <tr>
            <th scope="row">{name}</th>
            <td>
                <StatusBadge
                    state=Signal::derive(move || badge.with(|badge| badge.0))
                    label=Signal::derive(move || badge.with(|badge| badge.1.clone()))
                />
                {move || {
                    has_progress
                        .get()
                        .then(|| {
                            view! {
                                <Progress
                                    label=progress_label.clone()
                                    value=Signal::derive(move || percent.get().unwrap_or_default())
                                />
                            }
                        })
                }}
            </td>
            <td title=move || started.get()>
                {move || instant(&localizer, &started.get(), zone.as_ref())}
            </td>
            <td>
                {move || {
                    let facts = result.get();
                    (!facts.is_empty())
                        .then(|| {
                            let items = facts
                                .iter()
                                .map(|fact| {
                                    view! {
                                        <li>{text_with(&result_localizer, fact.id, &fact.args)}</li>
                                    }
                                })
                                .collect_view();
                            view! { <ul>{items}</ul> }
                        })
                }}
            </td>
            <td>
                {move || {
                    can_cancel
                        .get()
                        .then(|| {
                            let url = url.clone();
                            let base = base.clone();
                            let operation = operation.clone();
                            let still_waiting = still_waiting.clone();
                            let error_localizer = Arc::clone(&error_localizer);
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        let error_localizer = Arc::clone(&error_localizer);
                                        let base = base.clone();
                                        let still_waiting = still_waiting.clone();
                                        let operation = operation.clone();
                                        let url = url.clone();
                                        let ops = page.live();
                                        cancelled.update_value(|cancelled| {
                                            cancelled.insert(operation.clone());
                                        });
                                        spawn_local(async move {
                                            match ws::post(&url, &Empty {}).await {
                                                Some(Posted::Done(_)) => {
                                                    if !ws::settle(&base, ops, &operation).await {
                                                        reply(Politeness::Polite, &still_waiting);
                                                    }
                                                }
                                                Some(Posted::Refused(code, args, detail)) => {
                                                    cancelled.update_value(|cancelled| {
                                                        cancelled.remove(&operation);
                                                    });
                                                    error.set(Some(refusal(
                                                        &error_localizer,
                                                        code,
                                                        &args,
                                                        detail.as_deref(),
                                                    )));
                                                }
                                                None => {
                                                    cancelled.update_value(|cancelled| {
                                                        cancelled.remove(&operation);
                                                    });
                                                }
                                            }
                                        });
                                    }
                                >
                                    {cancel_label.clone()}
                                </button>
                            }
                        })
                }}
            </td>
        </tr>
    }
}

/// What the operations page shows in the place of its table, or the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// The history is on its way.
    Loading,
    /// The daemon cannot be reached.
    Unreachable,
    /// The history is here and holds no operation.
    Empty,
    /// The history is here and holds some.
    Table,
}

/// What the page shows while its connection is in `phase` and its state holds
/// `rows`. Only a change of this rebuilds the table: an update to a row leaves
/// the table and the controls in it as they are (WCAG 2.4.3), and a change in
/// the order of the rows leaves the table as it is and moves the rows that
/// change place.
fn shown(phase: Phase, rows: &[OperationRow]) -> Shown {
    match phase {
        Phase::Loading => Shown::Loading,
        Phase::Unreachable => Shown::Unreachable,
        Phase::Ready if rows.is_empty() => Shown::Empty,
        Phase::Ready => Shown::Table,
    }
}

/// The ids of `rows` in the order `sort` shows them. History is the daemon's
/// own order, newest first; a sort order compares the instant of the Started
/// cell, which as RFC 3339 in UTC orders as its bytes do, and keeps the
/// daemon's order between rows that tie.
fn ordered(rows: &[OperationRow], sort: SortOrder) -> Vec<String> {
    let mut rows: Vec<&OperationRow> = rows.iter().collect();
    match sort {
        SortOrder::History => {}
        SortOrder::NewestFirst => rows.sort_by(|a, b| key(b).cmp(key(a))),
        SortOrder::OldestFirst => rows.sort_by(|a, b| key(a).cmp(key(b))),
    }
    rows.into_iter().map(|row| row.summary.id.clone()).collect()
}

/// How far a row's run has come, as a whole percent: `None` until the run has
/// reported any progress, so a row that has not begun has no bar.
fn progress_percent(row: &OperationRow) -> Option<u8> {
    row.progress
        .map(|progress| percent(progress.current, progress.total))
}

/// A status as its badge shows it: which pill, and its text. A status this
/// build doesn't know is the empty pill with the status as it came, never a
/// blank cell.
fn badge_of(localizer: &Localizer, status: &str) -> (&'static str, String) {
    match status_display(status) {
        Some((state, label)) => (state, text(localizer, label)),
        None => ("empty", status.to_owned()),
    }
}

/// The order a row's life is told by: the instant its `Started` cell shows,
/// which for a queued run is the instant it was queued.
fn key(row: &OperationRow) -> &str {
    row.summary
        .started_at
        .as_deref()
        .unwrap_or(&row.summary.queued_at)
}

/// A status as the badge and the label show it: which pill, and which
/// message; `None` for a status this build doesn't know, which `badge_of`
/// shows as it came.
fn status_display(status: &str) -> Option<(&'static str, MessageId)> {
    match status {
        "queued" => Some(("empty", msg::UI_STATUS_QUEUED)),
        "running" => Some(("loading", msg::UI_STATUS_RUNNING)),
        "succeeded" => Some(("success", msg::UI_STATUS_SUCCEEDED)),
        "succeeded-with-warnings" => Some(("success", msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS)),
        "failed" => Some(("error", msg::UI_STATUS_FAILED)),
        "cancelled" => Some(("empty", msg::UI_STATUS_CANCELLED)),
        _ => None,
    }
}

/// Whether a row in `status` can still be stopped: a run that has not begun
/// or is still going. An ended run is history.
fn cancellable(status: &str) -> bool {
    status == "queued" || status == "running"
}

/// A run's progress as a whole percent of its total. A total of nothing or
/// zero cannot be a share, so it shows none rather than a wrong one.
fn percent(current: u64, total: Option<u64>) -> u8 {
    total
        .filter(|total| *total > 0)
        .map(|total| (current.saturating_mul(100) / total) as u8)
        .unwrap_or(0)
}

/// Whether the change from `previous` to `status` is a run's end arriving:
/// only a run that was still going can end, and only an end is news.
fn announces_end(previous: &str, status: &str) -> bool {
    (previous == "queued" || previous == "running") && ended(status)
}

/// The text of message `id`, with `args` for its variables.
pub(crate) fn text_with(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    localizer.message(id, args).into_text()
}

/// An RFC 3339 instant as the page's locale writes it (§8.4). One that
/// doesn't parse is shown as it came: never a blank cell, never a guess.
pub(crate) fn instant(localizer: &Localizer, raw: &str, zone: Option<&api::IanaZone>) -> String {
    match jiff::Timestamp::from_str(raw) {
        Ok(timestamp) => text_with(
            localizer,
            msg::UI_INSTANT,
            &[(
                "when",
                MsgArg::Timestamp {
                    utc_ms: timestamp.as_millisecond(),
                    zone: zone.cloned(),
                },
            )],
        ),
        Err(_) => raw.to_owned(),
    }
}

/// When a listed snapshot was taken, as a page shows it (§16, decision 56):
/// what the clock that took it read, on the day a calendar retention rule
/// counts it on, rather than the viewer's reading of the instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// A time that isn't RFC 3339, shown as it came.
    Unread,
    /// A time without the offset of its clock, from a daemon older than the
    /// field: the instant, read in the viewer's zone as before.
    Instant(jiff::Timestamp),
    /// What the snapshot's own clock read, at that clock's `offset`. The
    /// offset is `named` when it differs from the viewer's at that instant,
    /// so that someone in the zone the snapshot was taken in reads the time
    /// as their own clock does.
    Clock {
        reading: jiff::civil::DateTime,
        offset: jiff::tz::Offset,
        named: bool,
    },
}

/// When the snapshot whose time is `raw` (RFC 3339) was taken, by a clock at
/// `offset`, for a viewer whose own clock is at `viewer(instant)` at that
/// instant (the browser's, [`crate::snapshots::viewer_offset`]).
pub(crate) fn taken(
    raw: &str,
    offset: Option<api::ClockOffset>,
    viewer: impl FnOnce(jiff::Timestamp) -> jiff::tz::Offset,
) -> Taken {
    todo!("{raw} {offset:?} {:?}", viewer(jiff::Timestamp::UNIX_EPOCH))
}

/// A listed snapshot's time as the page writes it ([`taken`]). The host
/// writes the clock's reading as the locale does, with the offset after it
/// when it is named. A build whose host doesn't write instants
/// (`writes_instants` false) would write that reading as an instant in UTC
/// and call it so; the reading is written plainly instead, and its offset
/// always follows it, so nothing labels the reading of another clock UTC.
/// A time without its clock's offset is the instant in the viewer's `zone`,
/// as [`instant`] writes it.
pub(crate) fn snapshot_time(
    localizer: &Localizer,
    raw: &str,
    offset: Option<api::ClockOffset>,
    zone: Option<&api::IanaZone>,
    viewer: impl FnOnce(jiff::Timestamp) -> jiff::tz::Offset,
    writes_instants: bool,
) -> String {
    todo!(
        "{localizer:?} {raw} {offset:?} {zone:?} {:?} {writes_instants}",
        viewer(jiff::Timestamp::UNIX_EPOCH)
    )
}

/// A refusal the daemon answered with, as the catalog writes it (§8.4): a
/// typed code with its arguments, never a bare status number.
pub(crate) fn refusal(
    localizer: &Localizer,
    code: ErrorCode,
    args: &[(String, MsgArg)],
    detail: Option<&str>,
) -> String {
    let args = args
        .iter()
        .map(|(name, arg)| (name.as_str(), arg.clone()))
        .collect::<Vec<_>>();
    localizer.error(code, &args, detail).into_text()
}

/// What a person has typed into the Repositories form, one signal per field,
/// and whether they asked this agent to maintain the repository.
#[derive(Clone, Copy)]
struct Inputs {
    name: RwSignal<String>,
    location: RwSignal<String>,
    hot_storage: RwSignal<String>,
    password: RwSignal<String>,
    maintain_here: RwSignal<bool>,
}

impl Inputs {
    fn new() -> Self {
        Self {
            name: RwSignal::new(String::new()),
            location: RwSignal::new(String::new()),
            hot_storage: RwSignal::new(String::new()),
            password: RwSignal::new(String::new()),
            maintain_here: RwSignal::new(false),
        }
    }

    fn of(self, field: Field) -> RwSignal<String> {
        match field {
            Field::Name => self.name,
            Field::Location => self.location,
            Field::HotStorage => self.hot_storage,
            Field::Password => self.password,
        }
    }

    fn read(self) -> Form {
        Form {
            name: self.name.get_untracked(),
            location: self.location.get_untracked(),
            hot_storage: self.hot_storage.get_untracked(),
            password: self.password.get_untracked(),
            maintain_here: self.maintain_here.get_untracked(),
        }
    }

    fn clear(self) {
        for field in FIELDS {
            self.of(field).set(String::new());
        }
        self.maintain_here.set(false);
    }
}

/// The form's fields in the order the page shows them.
const FIELDS: [Field; 4] = [
    Field::Name,
    Field::Location,
    Field::HotStorage,
    Field::Password,
];

/// What is wrong with the form: the error each field shows, and the summary
/// that lists them all. Both are set together, so a field and the summary
/// never disagree.
#[derive(Clone, Copy)]
struct Problems {
    summary: RwSignal<Vec<SummaryItem>>,
    advice: RwSignal<Option<String>>,
    name: RwSignal<Option<String>>,
    location: RwSignal<Option<String>>,
    hot_storage: RwSignal<Option<String>>,
    password: RwSignal<Option<String>>,
}

impl Problems {
    fn new() -> Self {
        Self {
            summary: RwSignal::new(Vec::new()),
            advice: RwSignal::new(None),
            name: RwSignal::new(None),
            location: RwSignal::new(None),
            hot_storage: RwSignal::new(None),
            password: RwSignal::new(None),
        }
    }

    fn of(self, field: Field) -> RwSignal<Option<String>> {
        match field {
            Field::Name => self.name,
            Field::Location => self.location,
            Field::HotStorage => self.hot_storage,
            Field::Password => self.password,
        }
    }

    /// Shows `items`, and `advice` after them: each field about which an item
    /// is shows it, and every other field is clear. No items is a clear form.
    fn show(self, items: Vec<SummaryItem>, advice: Option<String>) {
        for field in FIELDS {
            let error = error_of(&items, field);
            self.of(field).set(error);
        }
        self.advice.set(advice);
        self.summary.set(items);
    }
}

/// The repositories page, the route `/repositories`: the repositories
/// Ladderback backs up to, and the form that tests one, connects one that
/// exists and creates one that doesn't (§4.4). Its decisions are
/// `repositories.rs`; this is the glue. A failed submit moves focus to the
/// error summary, and a typed password stays in its field until the
/// repository is saved, never shown again (I4, A5).
#[component]
pub(crate) fn Repositories(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_REPOSITORIES_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_REPOSITORIES_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let caption = text(&localizer, msg::UI_REPOSITORIES_CAPTION);
    let (name_column, location_column, password_column, actions_column) = (
        text(&localizer, msg::UI_COLUMN_NAME),
        text(&localizer, msg::UI_COLUMN_LOCATION),
        text(&localizer, msg::UI_COLUMN_PASSWORD),
        text(&localizer, msg::UI_COLUMN_ACTIONS),
    );
    let empty = text(&localizer, msg::UI_NO_REPOSITORIES);
    let form_heading = text(&localizer, msg::UI_REPOSITORIES_FORM_HEADING);
    let working = text(&localizer, msg::UI_REPOSITORIES_WORKING);
    let (test_label, connect_label, create_label) = (
        text(&localizer, msg::UI_REPOSITORIES_TEST),
        text(&localizer, msg::UI_REPOSITORIES_CONNECT),
        text(&localizer, msg::UI_REPOSITORIES_CREATE),
    );
    let base = StoredValue::new(ws::origin());
    let phase = RwSignal::new(Phase::Loading);
    // The configured repositories; `None` until the first read answers.
    let rows = RwSignal::<Option<Vec<RepositoryDto>>>::new(None);
    let load = Callback::new(move |()| {
        let url = format!("{}/api/v1/repositories", base.get_value());
        spawn_local(async move {
            match ws::read::<RepositoriesDto>(&url).await {
                Some(read) => {
                    rows.set(Some(read.repositories));
                    phase.set(Phase::Ready);
                }
                None => phase.set(Phase::Unreachable),
            }
        });
    });
    load.run(());
    // What the last of the rows' Creates came to, said below the list.
    let creates = RowCreates {
        phase,
        created: RwSignal::new(None),
        load,
    };

    let inputs = Inputs::new();
    let problems = Problems::new();
    // What the last answer said, and whether a request is still on its way.
    let result = RwSignal::<Option<String>>::new(None);
    let busy = RwSignal::new(false);
    let submit_localizer = Arc::clone(&localizer);
    let submit = Callback::new(move |action: Action| {
        if busy.get_untracked() {
            return;
        }
        let form = inputs.read();
        result.set(None);
        // What a row's Create came to is the page's last answer no more.
        creates.created.set(None);
        if let Some(items) = problems_of(&form, action, |id| text(&submit_localizer, id)) {
            problems.show(items, None);
            return;
        }
        busy.set(true);
        reply(
            Politeness::Polite,
            &text(&submit_localizer, msg::UI_REPOSITORIES_WORKING),
        );
        let localizer = Arc::clone(&submit_localizer);
        let url = format!("{}{}", base.get_value(), action.path());
        spawn_local(async move {
            let posted = match action {
                Action::Test => ws::post(&url, &form.test_request()).await,
                Action::Connect => ws::post(&url, &form.connect_request()).await,
                Action::Create => ws::post(&url, &form.create_request()).await,
            };
            // The reader may have followed a link while the engine worked: the
            // page's signals went with it, reading one would panic, and there
            // is nobody left to tell.
            let Some(phase_now) = phase.try_get_untracked() else {
                return;
            };
            busy.set(false);
            // An answer, of any kind, is a daemon that is there.
            if rereads_list(posted.is_some(), phase_now) {
                load.run(());
            }
            match posted {
                Some(Posted::Done(answer)) => match Outcome::of(action, &answer) {
                    Some(outcome) => {
                        let alias = form.alias();
                        let (id, args) = outcome.shown(alias);
                        result.set(Some(text_with(&localizer, id, &args)));
                        let (id, args) = outcome.announced(alias);
                        reply(Politeness::Polite, &text_with(&localizer, id, &args));
                        problems.show(Vec::new(), None);
                        if outcome.saved() {
                            inputs.clear();
                            load.run(());
                        }
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    let shown = refusal(&localizer, code, &args, detail.as_deref());
                    problems.show(
                        vec![SummaryItem {
                            field: field_of(code, &args).map(Field::id),
                            sentence: Sentence::new(&shown, detail.as_deref()),
                        }],
                        advises_retry(code)
                            .then(|| text(&localizer, msg::UI_REPOSITORIES_TRY_AGAIN)),
                    );
                }
                // No answer at all: the page's phase says that its own way.
                None => phase.set(Phase::Unreachable),
            }
        });
    });

    let list_localizer = Arc::clone(&localizer);
    let summary_localizer = Arc::clone(&localizer);
    let field_text = |id| text(&localizer, id);
    view! {
        <PageHeading>
            {move || if phase.get() == Phase::Unreachable { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        {move || match phase.get() {
            Phase::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
            Phase::Unreachable => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
            Phase::Ready => match rows.get() {
                Some(rows) if !rows.is_empty() => {
                    let localizer = Arc::clone(&list_localizer);
                    let base = base.get_value();
                    view! {
                        <table class="repositories">
                            <caption class="text-muted">{caption.clone()}</caption>
                            <thead>
                                <tr>
                                    <th scope="col">{name_column.clone()}</th>
                                    <th scope="col">{location_column.clone()}</th>
                                    <th scope="col">{password_column.clone()}</th>
                                    <th scope="col">{actions_column.clone()}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {rows
                                    .into_iter()
                                    .map(|row| repository_row(&localizer, &base, creates, row))
                                    .collect_view()}
                            </tbody>
                        </table>
                    }
                        .into_any()
                }
                _ => view! { <p>{empty.clone()}</p> }.into_any(),
            },
        }}
        {move || {
            creates
                .created
                .get()
                .map(|(confirmation, id_sentence)| {
                    view! { <CreatedNote confirmation=confirmation id_sentence=id_sentence /> }
                })
        }}
        <section aria-labelledby="repositories-form-heading">
            <h2 id="repositories-form-heading">{form_heading}</h2>
            {move || {
                let items = problems.summary.get();
                (!items.is_empty())
                    .then(|| {
                        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
                        let heading = text_with(
                            &summary_localizer,
                            msg::UI_REPOSITORIES_PROBLEMS,
                            &[("count", MsgArg::Count(count))],
                        );
                        view! {
                            <ErrorSummary
                                heading=heading
                                items=items
                                advice=problems.advice.get_untracked()
                            />
                        }
                    })
            }}
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    submit.run(Action::Test);
                }
            >
                <FormField
                    id=Field::Name.id()
                    label=field_text(msg::UI_REPOSITORIES_NAME_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_NAME_HINT)
                    error=problems.name
                    value=inputs.name
                />
                <FormField
                    id=Field::Location.id()
                    label=field_text(msg::UI_REPOSITORIES_LOCATION_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_LOCATION_HINT)
                    error=problems.location
                    value=inputs.location
                />
                <FormField
                    id=Field::HotStorage.id()
                    label=field_text(msg::UI_REPOSITORIES_HOT_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_HOT_HINT)
                    error=problems.hot_storage
                    value=inputs.hot_storage
                />
                <FormField
                    id=Field::Password.id()
                    label=field_text(msg::UI_REPOSITORIES_PASSWORD_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_PASSWORD_HINT)
                    error=problems.password
                    value=inputs.password
                    kind="password"
                />
                <Checkbox
                    id="repository-maintain-here"
                    label=field_text(msg::UI_REPOSITORIES_MAINTAIN_LABEL)
                    description=field_text(msg::UI_REPOSITORIES_MAINTAIN_HINT)
                    checked=inputs.maintain_here
                />
                <p class="actions">
                    <button type="submit">{test_label}</button>
                    <button type="button" on:click=move |_| submit.run(Action::Connect)>
                        {connect_label}
                    </button>
                    <button type="button" on:click=move |_| submit.run(Action::Create)>
                        {create_label}
                    </button>
                </p>
            </form>
            {move || busy.get().then(|| view! { <p>{working.clone()}</p> })}
            {move || result.get().map(|text| view! { <p>{text}</p> })}
        </section>
    }
}

/// What a row's Create needs from its page: the page's phase, which an
/// answer that never came sets; where the page says what a Create came to;
/// and the reading of the list, after which a created repository's row
/// offers Create no more.
#[derive(Clone, Copy)]
struct RowCreates {
    phase: RwSignal<Phase>,
    /// What a row's Create came to: the confirmation, then the ID's sentence.
    created: RwSignal<Option<(String, String)>>,
    load: Callback<()>,
}

/// One row of the repositories table: the name as the row's header, where the
/// repository is (the hot half on a line of its own), where its password
/// comes from, and its Create, when the row offers it (decision 53). Never
/// the password itself (I4).
fn repository_row(
    localizer: &Arc<Localizer>,
    base: &str,
    creates: RowCreates,
    row: RepositoryDto,
) -> impl IntoView {
    let (id, args) = password_source(&row.password);
    let password = text_with(localizer, id, &args);
    let notes = repository_notes(localizer, &row.warnings);
    let hot = row.repo_hot.map(|location| {
        text_with(
            localizer,
            msg::UI_REPOSITORY_HOT,
            &[("location", MsgArg::Text(location))],
        )
    });
    let create = row
        .create_offered
        .then(|| create_in_row(localizer, base, creates, &row.alias));
    view! {
        <tr>
            <th scope="row">{row.alias}</th>
            <td class="path">
                <div>{row.repository}</div>
                {hot.map(|hot| view! { <div>{hot}</div> })}
            </td>
            <td class="path">
                <div>{password}</div>
                {notes.into_iter().map(|note| view! { <div class="text-muted">{note}</div> }).collect_view()}
            </td>
            <td>{create}</td>
        </tr>
    }
}

/// A row's Create: the button, named after its row, that creates the saved
/// repository where its entry says. A created repository is said below the
/// list, which is read again, so the row's button goes and focus moves to
/// what was said ([`CreatedNote`]); a refusal is the row's alert, and the
/// row and its button stay.
fn create_in_row(
    localizer: &Arc<Localizer>,
    base: &str,
    creates: RowCreates,
    alias: &str,
) -> impl IntoView + use<> {
    let (id, args) = create_label(alias);
    let label = text_with(localizer, id, &args);
    let url = format!("{base}{}", create_saved_path(alias));
    let alias = alias.to_owned();
    let localizer = Arc::clone(localizer);
    // The last answer's refusal, as the row shows it, and whether the button
    // takes no press: from a press until the answer is a refusal, or, once the
    // repository is created, until the list is read again and the row goes.
    let refused = RwSignal::<Option<String>>::new(None);
    let busy = RwSignal::new(false);
    let create = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        creates.created.set(None);
        refused.set(None);
        reply(
            Politeness::Polite,
            &text(&localizer, msg::UI_REPOSITORIES_WORKING),
        );
        let (url, alias, localizer) = (url.clone(), alias.clone(), Arc::clone(&localizer));
        spawn_local(async move {
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            // The reader may have followed a link while the engine worked:
            // the page went with it, and there is nobody left to tell.
            if creates.phase.try_get_untracked().is_none() {
                return;
            }
            match posted {
                Some(Posted::Done(answer)) => match RowCreated::of(&answer) {
                    // The button stays busy: the list is read again, and
                    // the row and its button go with it.
                    Some(created) => {
                        let (id, args) = RowCreated::confirmation(&alias);
                        let confirmation = text_with(&localizer, id, &args);
                        let (id, args) = created.id_sentence();
                        let id_sentence = text_with(&localizer, id, &args);
                        creates.created.set(Some((confirmation, id_sentence)));
                        creates.load.run(());
                    }
                    // An answer that is not the one the route gives is a
                    // daemon the page cannot talk to.
                    None => creates.phase.set(Phase::Unreachable),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    // The list may have been drawn again while the engine
                    // worked, and the row's own signals went with the row.
                    busy.try_set(false);
                    let shown = match row_refusal(code, &alias) {
                        Some((id, args)) => text_with(&localizer, id, &args),
                        None => refusal(&localizer, code, &args, detail.as_deref()),
                    };
                    refused.try_set(Some(shown));
                }
                // No answer at all: the page's phase says that its own way.
                None => creates.phase.set(Phase::Unreachable),
            }
        });
    };
    view! {
        <button type="button" on:click=create>
            {label}
        </button>
        {move || refused.get().map(|text| view! { <p role="alert">{text}</p> })}
    }
}

/// What a row's Create came to, below the list: focus moves to the short
/// confirmation as it is shown, for the row's button goes when the list is
/// read again, and a screen reader reads it there, so nothing is announced
/// (§4.5). The ID is a sentence of its own after it, outside what has focus,
/// so a screen reader doesn't spell out 64 hex digits unasked.
#[component]
fn CreatedNote(confirmation: String, id_sentence: String) -> impl IntoView {
    let note = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        if let Some(note) = note.get() {
            // A failed focus leaves focus where it was.
            let _ = note.focus();
        }
    });
    view! {
        <p tabindex="-1" node_ref=note>{confirmation}</p>
        <p>{id_sentence}</p>
    }
}

/// A snapshot's short ID: its first eight hex digits, as rustic's own lists
/// show one. A shorter ID is shown whole.
pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// What any path nothing serves shows.
#[component]
pub(crate) fn NotFound(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_NOT_FOUND_HEADING);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{text(&localizer, msg::UI_NOT_FOUND_BODY)}</p>
        <p><A href="/">{text(&localizer, msg::UI_NOT_FOUND_LINK)}</A></p>
    }
}

/// The title of the page at `path`, for the browser's tab and history: the
/// page's own, in the page's language (technical design §4.5, "Route
/// changes"). A path with no page is the not-found page, and so is
/// `/_gallery` in a build without the gallery.
pub(crate) fn title(localizer: &Localizer, path: &str) -> String {
    title_in(localizer, path, cfg!(feature = "gallery"))
}

/// [`title`] for a build that has the gallery or not, so that both are tested
/// whichever one the tests are built as.
fn title_in(localizer: &Localizer, path: &str, gallery: bool) -> String {
    let page = if path == "/" {
        msg::UI_START_TITLE
    } else if gallery && path.strip_suffix('/').unwrap_or(path) == "/_gallery" {
        msg::UI_GALLERY_TITLE
    } else if path == "/operations" {
        msg::UI_OPERATIONS_TITLE
    } else if path == "/repositories" {
        msg::UI_REPOSITORIES_TITLE
    } else if path == "/snapshots" {
        msg::UI_SNAPSHOTS_TITLE
    } else {
        msg::UI_NOT_FOUND_TITLE
    };
    text(localizer, page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_of(path: &str) -> String {
        title(&Localizer::new(&[]), path)
    }

    #[test]
    fn the_start_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/"), "Ladderback");
    }

    #[test]
    fn the_operations_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/operations"), "Operations – Ladderback");
    }

    #[test]
    fn the_repositories_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/repositories"), "Repositories – Ladderback");
    }

    #[test]
    fn the_snapshots_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/snapshots"), "Snapshots – Ladderback");
    }

    #[test]
    fn a_path_with_no_page_is_titled_page_not_found() {
        assert_eq!(title_of("/no/such/page"), "Page not found – Ladderback");
    }

    #[test]
    fn the_gallery_has_its_title_only_in_a_build_that_has_it() {
        let with = title_in(&Localizer::new(&[]), "/_gallery", true);
        let without = title_in(&Localizer::new(&[]), "/_gallery", false);
        assert_eq!(with, "Component gallery – Ladderback");
        assert_eq!(without, "Page not found – Ladderback");
    }

    #[test]
    fn the_build_decides_whether_the_gallery_is_a_page() {
        let expected = if cfg!(feature = "gallery") {
            "Component gallery – Ladderback"
        } else {
            "Page not found – Ladderback"
        };
        assert_eq!(title_of("/_gallery"), expected);
    }

    #[test]
    fn a_trailing_slash_names_the_same_page() {
        let localizer = Localizer::new(&[]);
        assert_eq!(
            title_in(&localizer, "/_gallery/", true),
            "Component gallery – Ladderback"
        );
        assert_eq!(title_of("/no/such/page/"), "Page not found – Ladderback");
    }

    #[test]
    fn a_path_that_only_starts_like_a_page_is_not_that_page() {
        assert_eq!(title_of("/_gallery-2"), "Page not found – Ladderback");
        assert_eq!(title_of("//"), "Page not found – Ladderback");
    }

    #[test]
    fn each_field_has_its_own_input_and_the_form_reads_them_all() {
        let inputs = Inputs::new();
        for (field, value) in [
            (Field::Name, "copy"),
            (Field::Location, "/srv/main"),
            (Field::HotStorage, "/srv/hot"),
            (Field::Password, "typed"),
        ] {
            assert_eq!(
                inputs.of(field).get_untracked(),
                "",
                "{field:?} starts empty"
            );
            inputs.of(field).set(value.to_owned());
        }
        assert!(
            !inputs.maintain_here.get_untracked(),
            "the checkbox starts off"
        );
        inputs.maintain_here.set(true);
        // `Form` has no `Debug`, so that nothing can print a password: compare.
        assert!(
            inputs.read()
                == Form {
                    name: "copy".to_owned(),
                    location: "/srv/main".to_owned(),
                    hot_storage: "/srv/hot".to_owned(),
                    password: "typed".to_owned(),
                    maintain_here: true,
                }
        );
        for field in FIELDS {
            assert!(!inputs.of(field).get_untracked().is_empty(), "{field:?}");
        }
    }

    #[test]
    fn clearing_the_form_empties_every_input_and_turns_the_checkbox_off() {
        let inputs = Inputs::new();
        for field in FIELDS {
            inputs.of(field).set("typed".to_owned());
        }
        inputs.maintain_here.set(true);
        inputs.clear();
        assert!(inputs.read() == Form::default());
        assert!(!inputs.maintain_here.get_untracked());
    }

    #[test]
    fn the_problems_a_form_shows_are_the_summary_and_each_field_s_own_error() {
        let problems = Problems::new();
        let items = vec![SummaryItem {
            field: Some(Field::Location.id()),
            sentence: Sentence::new("Enter where the repository is.", None),
        }];
        problems.show(items.clone(), Some("Try again.".to_owned()));
        assert_eq!(problems.summary.get_untracked(), items);
        assert_eq!(
            problems.advice.get_untracked().as_deref(),
            Some("Try again.")
        );
        assert_eq!(
            problems.of(Field::Location).get_untracked().as_deref(),
            Some("Enter where the repository is.")
        );
        for field in [Field::Name, Field::HotStorage, Field::Password] {
            assert_eq!(problems.of(field).get_untracked(), None, "{field:?}");
        }
        // No items clears the form again, and the advice with it.
        problems.show(Vec::new(), None);
        assert!(problems.summary.get_untracked().is_empty());
        assert_eq!(problems.advice.get_untracked(), None);
        for field in FIELDS {
            assert_eq!(problems.of(field).get_untracked(), None, "{field:?}");
        }
    }

    #[test]
    fn each_field_has_its_own_error_signal() {
        let problems = Problems::new();
        for field in FIELDS {
            problems.of(field).set(Some(format!("{field:?}")));
        }
        for field in FIELDS {
            assert_eq!(
                problems.of(field).get_untracked(),
                Some(format!("{field:?}")),
                "{field:?} shares a signal"
            );
        }
    }

    /// The full operation ID of the run numbered `n`.
    fn id_of(n: u32) -> String {
        format!("018f0000-0000-7000-8000-{n:012}")
    }

    /// The run numbered `n` as the store sends it, with the fields the key
    /// reads.
    fn row(n: u32, started: Option<&str>, queued: &str) -> crate::backend::OperationRow {
        crate::backend::OperationRow {
            summary: api::OperationSummaryDto::new(
                id_of(n),
                "demo-home".to_owned(),
                "backup".to_owned(),
                "succeeded".to_owned(),
                queued.to_owned(),
                started.map(str::to_owned),
                None,
                Vec::new(),
                None,
                None,
                None,
            )
            .unwrap_or_else(|problem| panic!("a valid summary: {problem:?}")),
            progress: None,
        }
    }

    /// A summary as the store sends it, with the fields the key reads.
    fn summary(started: Option<&str>, queued: &str) -> crate::backend::OperationRow {
        row(1, started, queued)
    }

    #[test]
    fn the_row_order_key_is_when_the_row_started_or_was_queued() {
        let started = summary(Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:01Z");
        assert_eq!(key(&started), "2026-11-09T10:00:02Z");
        let queued = summary(None, "2026-11-09T10:00:01Z");
        assert_eq!(key(&queued), "2026-11-09T10:00:01Z");
    }

    #[test]
    fn the_page_shows_what_its_connection_and_its_history_allow() {
        let held = [summary(None, "2026-11-09T10:00:00Z")];
        assert_eq!(shown(Phase::Loading, &[]), Shown::Loading);
        assert_eq!(shown(Phase::Unreachable, &[]), Shown::Unreachable);
        assert_eq!(shown(Phase::Ready, &[]), Shown::Empty);
        assert_eq!(shown(Phase::Ready, &held), Shown::Table);
    }

    #[test]
    fn rows_held_from_before_do_not_show_a_table_while_the_connection_is_not_ready() {
        let held = [summary(None, "2026-11-09T10:00:00Z")];
        assert_eq!(shown(Phase::Loading, &held), Shown::Loading);
        assert_eq!(shown(Phase::Unreachable, &held), Shown::Unreachable);
    }

    #[test]
    fn the_history_is_shown_in_the_order_the_daemon_sent_it() {
        let rows = [
            row(1, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
            row(2, Some("2026-11-09T10:00:03Z"), "2026-11-09T10:00:00Z"),
            row(3, Some("2026-11-09T10:00:01Z"), "2026-11-09T10:00:00Z"),
        ];
        assert_eq!(
            ordered(&rows, SortOrder::History),
            [id_of(1), id_of(2), id_of(3)]
        );
    }

    #[test]
    fn newest_first_puts_the_latest_start_on_top() {
        let rows = [
            row(1, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
            row(2, Some("2026-11-09T10:00:03Z"), "2026-11-09T10:00:00Z"),
            row(3, Some("2026-11-09T10:00:01Z"), "2026-11-09T10:00:00Z"),
        ];
        assert_eq!(
            ordered(&rows, SortOrder::NewestFirst),
            [id_of(2), id_of(1), id_of(3)]
        );
    }

    #[test]
    fn oldest_first_puts_the_earliest_start_on_top() {
        let rows = [
            row(1, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
            row(2, Some("2026-11-09T10:00:03Z"), "2026-11-09T10:00:00Z"),
            row(3, Some("2026-11-09T10:00:01Z"), "2026-11-09T10:00:00Z"),
        ];
        assert_eq!(
            ordered(&rows, SortOrder::OldestFirst),
            [id_of(3), id_of(1), id_of(2)]
        );
    }

    #[test]
    fn a_run_that_has_not_begun_is_ordered_by_when_it_was_queued() {
        let rows = [
            row(1, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
            row(2, None, "2026-11-09T10:00:03Z"),
        ];
        assert_eq!(ordered(&rows, SortOrder::NewestFirst), [id_of(2), id_of(1)]);
        assert_eq!(ordered(&rows, SortOrder::OldestFirst), [id_of(1), id_of(2)]);
    }

    #[test]
    fn runs_that_started_together_keep_the_order_the_daemon_sent_them_in() {
        let rows = [
            row(1, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
            row(2, Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z"),
        ];
        for sort in [SortOrder::NewestFirst, SortOrder::OldestFirst] {
            assert_eq!(ordered(&rows, sort), [id_of(1), id_of(2)], "{sort:?}");
        }
    }

    #[test]
    fn a_row_shows_progress_once_its_run_has_reported_some() {
        let mut running = summary(Some("2026-11-09T10:00:02Z"), "2026-11-09T10:00:00Z");
        assert_eq!(
            progress_percent(&running),
            None,
            "a run that has reported nothing has no bar"
        );
        running.progress = Some(crate::backend::Progress {
            current: 3,
            total: Some(4),
        });
        assert_eq!(progress_percent(&running), Some(75));
        running.progress = Some(crate::backend::Progress {
            current: 9,
            total: None,
        });
        assert_eq!(
            progress_percent(&running),
            Some(0),
            "a run of unknown size has a bar at nothing, not no bar"
        );
    }

    #[test]
    fn a_status_is_badged_by_its_pill_and_its_text_and_an_unknown_one_stands_as_it_came() {
        let localizer = Localizer::new(&[]);
        assert_eq!(
            badge_of(&localizer, "running"),
            ("loading", "Running".to_owned())
        );
        assert_eq!(
            badge_of(&localizer, "succeeded-with-warnings"),
            ("success", "Succeeded with warnings".to_owned())
        );
        assert_eq!(
            badge_of(&localizer, "pausing"),
            ("empty", "pausing".to_owned()),
            "a status this build doesn't know is never a blank cell"
        );
    }

    #[test]
    fn every_status_the_store_names_has_its_pill_and_its_message() {
        let pills = [
            ("queued", "empty", msg::UI_STATUS_QUEUED),
            ("running", "loading", msg::UI_STATUS_RUNNING),
            ("succeeded", "success", msg::UI_STATUS_SUCCEEDED),
            (
                "succeeded-with-warnings",
                "success",
                msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS,
            ),
            ("failed", "error", msg::UI_STATUS_FAILED),
            ("cancelled", "empty", msg::UI_STATUS_CANCELLED),
        ];
        for (status, pill, label) in pills {
            assert_eq!(status_display(status), Some((pill, label)), "{status}");
        }
        assert_eq!(
            status_display("pausing"),
            None,
            "a status this build doesn't know is no pill, not a guess"
        );
    }

    #[test]
    fn an_instant_is_written_in_the_pages_locale_and_one_that_does_not_parse_stands_as_it_came() {
        let localizer = Localizer::new(&[]);
        assert_ne!(
            instant(&localizer, "2026-11-09T10:00:02Z", None),
            "2026-11-09T10:00:02Z",
            "a parsed instant is the locale's own writing, not the wire's"
        );
        assert_eq!(
            instant(&localizer, "not-an-instant", None),
            "not-an-instant",
            "never a blank cell, never a guess"
        );
    }

    #[test]
    fn a_refusal_is_written_through_the_catalog_with_its_argument() {
        let shown = refusal(
            &Localizer::new(&[]),
            ErrorCode::PlanUnknown,
            &[("plan".to_owned(), MsgArg::Text("demo-home".to_owned()))],
            None,
        );
        assert!(!shown.is_empty(), "a refusal is never a bare absence");
        assert!(
            shown.contains("demo-home"),
            "the argument is part of the message: {shown}"
        );
    }

    #[test]
    fn a_refusal_carries_the_engines_words_when_the_daemon_sent_them() {
        let localizer = Localizer::new(&[]);
        let with = refusal(
            &localizer,
            ErrorCode::OperationFailed,
            &[],
            Some("wrong password"),
        );
        assert!(
            with.starts_with("The operation failed: ") && with.contains("wrong password"),
            "{with}"
        );
        let without = refusal(&localizer, ErrorCode::OperationFailed, &[], None);
        assert_eq!(without, "The operation failed.");
    }

    #[test]
    fn a_snapshot_id_shows_its_first_eight_digits_and_a_short_one_whole() {
        assert_eq!(short_id("0123456789abcdef"), "01234567");
        assert_eq!(short_id("abc123"), "abc123");
    }
    #[test]
    fn a_message_with_its_arguments_is_the_catalogs_own_text() {
        assert_eq!(
            text_with(
                &Localizer::new(&[]),
                msg::UI_SNAPSHOTS_OF,
                &[("alias", MsgArg::Text("demo".to_owned()))],
            )
            .replace(['\u{2066}', '\u{2069}'], ""),
            "Snapshots of demo",
            "isolation marks aside, the catalog's text and nothing else"
        );
    }

    #[test]
    fn only_a_run_that_is_still_going_can_be_cancelled() {
        assert!(cancellable("queued"));
        assert!(cancellable("running"));
        for ended in [
            "succeeded",
            "succeeded-with-warnings",
            "failed",
            "cancelled",
        ] {
            assert!(!cancellable(ended), "{ended} is history");
        }
    }

    #[test]
    fn progress_is_a_whole_percent_of_a_known_total_and_nothing_without_one() {
        assert_eq!(percent(3, Some(4)), 75);
        assert_eq!(percent(1, Some(3)), 33);
        assert_eq!(percent(5, Some(0)), 0, "zero of nothing is no share");
        assert_eq!(percent(9, None), 0);
    }

    #[test]
    fn only_a_change_from_going_to_ended_is_announced() {
        assert!(announces_end("running", "succeeded"));
        assert!(announces_end("queued", "failed"));
        assert!(
            !announces_end("running", "running"),
            "still going is no news"
        );
        assert!(!announces_end("queued", "running"), "a start is not an end");
        assert!(
            !announces_end("succeeded", "failed"),
            "history does not move"
        );
    }

    fn clock(seconds: i32) -> Option<api::ClockOffset> {
        Some(api::ClockOffset::new(
            jiff::tz::Offset::from_seconds(seconds).unwrap(),
        ))
    }

    fn zone(name: &str) -> Option<api::IanaZone> {
        Some(api::IanaZone::new(name).unwrap())
    }

    /// The offset of a viewer's clock in the zone `name` at each instant, as
    /// the browser gives it.
    fn in_zone(name: &str) -> impl Fn(jiff::Timestamp) -> jiff::tz::Offset {
        let zone = jiff::tz::TimeZone::get(name).unwrap();
        move |at| zone.to_offset(at)
    }

    /// What the clock at `offset` read when it took the snapshot at `raw`,
    /// and whether the viewer in `viewer` is told the offset.
    fn read(raw: &str, offset: i32, viewer: &str) -> (String, bool) {
        match taken(raw, clock(offset), in_zone(viewer)) {
            Taken::Clock {
                reading,
                offset: read_at,
                named,
            } => {
                assert_eq!(read_at.seconds(), offset, "{raw}");
                (reading.to_string(), named)
            }
            other => panic!("{raw} at {offset} is a clock's reading: {other:?}"),
        }
    }

    /// Decision 56's examples: a container whose clock runs in UTC, a late
    /// evening in Chicago, and a laptop flown from Auckland to Hawaii.
    #[test]
    fn a_snapshot_is_shown_as_its_own_clock_read_it_with_an_offset_the_viewer_lacks() {
        let chicago = -6 * 3600;
        assert_eq!(
            read("2026-11-11T05:30:00Z", 0, "America/Chicago"),
            ("2026-11-11T05:30:00".to_owned(), true),
            "a container's clock in UTC, seen from Chicago"
        );
        assert_eq!(
            read("2026-11-11T05:30:00Z", chicago, "America/Chicago"),
            ("2026-11-10T23:30:00".to_owned(), false),
            "late evening in Chicago, seen from Chicago"
        );
        assert_eq!(
            read("2026-11-11T05:30:00Z", chicago, "America/New_York"),
            ("2026-11-10T23:30:00".to_owned(), true),
            "late evening in Chicago, seen from New York the next day"
        );
        assert_eq!(
            read("2026-11-09T11:30:00Z", 13 * 3600, "Pacific/Honolulu"),
            ("2026-11-10T00:30:00".to_owned(), true),
            "the laptop in Auckland, seen from Hawaii"
        );
        assert_eq!(
            read("2026-11-11T09:30:00Z", -10 * 3600, "Pacific/Honolulu"),
            ("2026-11-10T23:30:00".to_owned(), false),
            "the laptop in Hawaii, seen from Hawaii"
        );
    }

    /// The offset compared is the viewer's at the moment the snapshot was
    /// taken: the night daylight saving time ends in Chicago, both sides of
    /// it are the viewer's own, and a clock that kept the summer's offset
    /// after it is not.
    #[test]
    fn the_viewers_offset_is_the_one_in_force_when_the_snapshot_was_taken() {
        assert_eq!(
            read("2026-11-01T05:30:00Z", -5 * 3600, "America/Chicago"),
            ("2026-11-01T00:30:00".to_owned(), false)
        );
        assert_eq!(
            read("2026-11-02T05:30:00Z", -6 * 3600, "America/Chicago"),
            ("2026-11-01T23:30:00".to_owned(), false)
        );
        assert_eq!(
            read("2026-11-02T04:30:00Z", -5 * 3600, "America/Chicago"),
            ("2026-11-01T23:30:00".to_owned(), true)
        );
        assert_eq!(
            read("2026-07-01T05:30:00Z", -6 * 3600, "America/Chicago"),
            ("2026-06-30T23:30:00".to_owned(), true)
        );
    }

    /// The viewer's offset is asked for at the instant the snapshot was
    /// taken, and only it decides: a viewer at a fixed offset (a browser in
    /// UTC, or one without zone names) is told every other offset.
    #[test]
    fn the_viewers_offset_is_read_at_the_snapshots_instant() {
        let raw = "2026-11-11T05:30:00Z";
        let named = |offset: i32, viewer: i32| {
            let viewer = jiff::tz::Offset::from_seconds(viewer).unwrap();
            match taken(raw, clock(offset), |at| {
                assert_eq!(at, raw.parse().unwrap());
                viewer
            }) {
                Taken::Clock { named, .. } => named,
                other => panic!("a clock's reading: {other:?}"),
            }
        };
        assert!(!named(0, 0));
        assert!(named(-6 * 3600, 0));
        assert!(named(0, 5 * 3600 + 45 * 60));
        assert!(!named(5 * 3600 + 45 * 60, 5 * 3600 + 45 * 60));
    }

    #[test]
    fn a_time_without_its_clocks_offset_is_the_instant_in_the_viewers_zone_as_before() {
        let localizer = Localizer::new(&[]);
        let raw = "2026-11-11T05:30:00Z";
        let viewer = zone("America/Chicago");
        assert_eq!(
            taken(raw, None, in_zone("America/Chicago")),
            Taken::Instant(raw.parse().unwrap())
        );
        for host in [true, false] {
            assert_eq!(
                snapshot_time(
                    &localizer,
                    raw,
                    None,
                    viewer.as_ref(),
                    in_zone("America/Chicago"),
                    host
                ),
                instant(&localizer, raw, viewer.as_ref())
            );
        }
    }

    #[test]
    fn a_time_that_is_not_rfc_3339_is_shown_as_it_came() {
        let localizer = Localizer::new(&[]);
        let utc = |_| jiff::tz::Offset::UTC;
        assert_eq!(taken("yesterday", clock(0), utc), Taken::Unread);
        assert_eq!(taken("yesterday", None, utc), Taken::Unread);
        for host in [true, false] {
            assert_eq!(
                snapshot_time(&localizer, "yesterday", clock(0), None, utc, host),
                "yesterday"
            );
        }
    }

    /// The host writes the reading as it writes an instant in UTC, which
    /// shows no zone, and the offset follows only when it is named.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_host_writes_the_reading_and_the_offset_only_when_the_viewer_lacks_it() {
        let localizer = Localizer::new(&[]);
        if !localizer.writes_instants() {
            testkit::skip!("this build's l10n has no host that writes instants");
        }
        let raw = "2026-11-11T05:30:00Z";
        let reading = instant(&localizer, "2026-11-10T23:30:00Z", None);
        let shown = |offset: i32, viewer: &str| {
            snapshot_time(
                &localizer,
                raw,
                clock(offset),
                zone(viewer).as_ref(),
                in_zone(viewer),
                localizer.writes_instants(),
            )
        };
        assert_eq!(
            shown(-6 * 3600, "America/New_York"),
            format!("{reading} UTC-06:00")
        );
        assert_eq!(shown(-6 * 3600, "America/Chicago"), reading);
        assert_eq!(
            shown(0, "America/Chicago"),
            format!("{} UTC+00:00", instant(&localizer, raw, None))
        );
    }

    /// Without a host, `DATETIME` writes an instant in UTC and says "UTC":
    /// the reading of another clock is written plainly instead, and its
    /// offset always follows, even where the viewer's is the same.
    #[test]
    fn without_a_host_the_reading_is_written_plainly_and_never_called_utc() {
        let localizer = Localizer::new(&[]);
        let raw = "2026-11-11T05:30:00Z";
        let shown = |offset: i32, viewer: &str| {
            snapshot_time(
                &localizer,
                raw,
                clock(offset),
                zone(viewer).as_ref(),
                in_zone(viewer),
                false,
            )
        };
        for viewer in ["America/Chicago", "America/New_York"] {
            assert_eq!(
                shown(-6 * 3600, viewer),
                "2026-11-10 23:30:00 UTC-06:00",
                "{viewer}"
            );
        }
        assert_eq!(shown(0, "UTC"), "2026-11-11 05:30:00 UTC+00:00");
        assert_eq!(
            snapshot_time(
                &localizer,
                raw,
                clock(19 * 60 + 32),
                None,
                |_| jiff::tz::Offset::UTC,
                false
            ),
            "2026-11-11 05:49:32 UTC+00:19:32"
        );
    }

    /// A reading past the last instant (late on 30 December 9999, UTC) can't
    /// be handed to the host as one; it is written plainly, as it read.
    #[test]
    fn a_reading_past_the_last_instant_is_written_plainly() {
        let localizer = Localizer::new(&[]);
        let raw = "9999-12-30T22:00:00Z";
        let latest = 25 * 3600 + 59 * 60 + 59;
        assert_eq!(
            snapshot_time(
                &localizer,
                raw,
                clock(latest),
                None,
                |_| jiff::tz::Offset::UTC,
                true
            ),
            "9999-12-31 23:59:59 UTC+25:59:59"
        );
    }
}
