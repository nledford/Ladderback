//! The routes of the app (technical design §4.5, "Route changes"): the pages a
//! path shows. Each page has its own `<h1>`, the heading focus moves to when a
//! reader navigates to it, so it carries `tabindex="-1"`.

use std::collections::HashMap;
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
use crate::components::{ErrorSummary, FormField, Progress, StatusBadge, SummaryItem};
use crate::live::say;
use crate::repositories::{
    Action, Field, Form, Outcome, Sentence, advises_retry, error_of, field_of, password_source,
    problems_of, repository_notes, rereads_list,
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
struct Empty {}

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
#[component]
pub(crate) fn Operations(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_OPERATIONS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_OPERATIONS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let start_label = text(&localizer, msg::UI_START_BACKUP);
    let caption = text(&localizer, msg::UI_OPERATIONS_CAPTION);
    let (plan_column, status_column, started_column, actions_column) = (
        text(&localizer, msg::UI_COLUMN_PLAN),
        text(&localizer, msg::UI_COLUMN_STATUS),
        text(&localizer, msg::UI_COLUMN_STARTED),
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
    let seen = StoredValue::new(HashMap::<String, String>::new());
    let announced_ops = ops;
    let announced_localizer = Arc::clone(&localizer);
    Effect::new(move |_| {
        let rows = announced_ops.get().rows;
        seen.update_value(|seen| {
            for row in &rows {
                let status = row.summary.status.clone();
                let previous = seen.insert(row.summary.id.clone(), status.clone());
                let changed = previous.is_some_and(|previous| announces_end(&previous, &status));
                if changed {
                    // A run that warned its way to the end counts as
                    // finished: its warnings are the row's own story.
                    let (politeness, id) = end_display(&status);
                    say(politeness, &text_with(&announced_localizer, id, &[]));
                }
            }
        });
    });
    // What the page shows, and the ids of the rows in the order it shows them.
    // Each is a memo, so an update that leaves it as it was rebuilds nothing:
    // a progress report or a status change is an update to a row, which the
    // row follows by itself, and the table and the controls in it stay where
    // they are. History is the daemon's own order; a sort order is the sort
    // button's, and follows it as the reader presses it.
    let showing = Memo::new(move |_| {
        let phase = phase.get();
        ops.with(|state| shown(phase, &state.rows))
    });
    let order = Memo::new(move |_| ops.with(|state| ordered(&state.rows, sort.get())));
    let table_localizer = Arc::clone(&localizer);
    // The zone the browser reads its clock in, asked once: every instant the
    // page shows is written in it (§8.4).
    let viewer = l10n::viewer_zone();
    let cancel_localizer = Arc::clone(&localizer);
    let start_base = base.clone();
    let cancel_base = base.clone();
    let start_ops = ops;
    let cancel_ops = ops;
    view! {
        <PageHeading>
            {move || if phase.get() == Phase::Unreachable { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        {move || error.get().map(|text| view! { <p role="alert">{text}</p> })}
        // The plans sit outside the connection's phases: a reader can start a
        // backup while the history is still on its way.
        {move || {
            plans
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
                                                    say(Politeness::Polite, &started);
                                                    if let Some(operation) =
                                                        ws::started_operation(&answer)
                                                        && !ws::settle(&base, ops, &operation).await
                                                    {
                                                        say(Politeness::Polite, &still_waiting);
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
                                    {start_label.clone()}
                                </button>
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }}
        {move || {
            // Per render: the sort button's and rows' closures own what they
            // say and show. A render is a change of what the page shows, never
            // an update to a row.
            let newest = sorted_newest.clone();
            let oldest = sorted_oldest.clone();
            let rows_localizer = table_localizer.clone();
            let rows_errors = cancel_localizer.clone();
            let rows_base = cancel_base.clone();
            let rows_zone = viewer.clone();
            match showing.get() {
                Shown::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
                Shown::Unreachable => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
                Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
                Shown::Table => {
                    view! {
                        <table>
                            <caption class="text-muted">{caption.clone()}</caption>
                            <thead>
                                <tr>
                                    <th scope="col">{plan_column.clone()}</th>
                                    <th scope="col">{status_column.clone()}</th>
                                    <th
                                        scope="col"
                                        aria-sort=move || match sort.get() {
                                            SortOrder::OldestFirst => "ascending",
                                            _ => "descending",
                                        }
                                    >
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
                                                say(
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
                                    </th>
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
                                            cancel_ops,
                                            rows_errors.clone(),
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

/// One row of the operations table: the plan, its status (with a running
/// backup's progress), the instant it started — the raw instant in the cell's
/// `title`, so an order can be read without the locale's text — and what can
/// be done with it: a running or queued backup can be cancelled.
///
/// A row is built once, when its operation first shows, and follows the state
/// from then on. Each fact it shows is a memo of its own, and each cell binds
/// to the facts it shows, so an update reaches only the cells it is about: a
/// progress report moves the bar, a start changes the status and the Started
/// cell, and none of them replaces the Cancel button (WCAG 2.4.3). The button
/// goes only when the run ends, and then it is no longer a control.
fn row_view(
    localizer: Arc<Localizer>,
    base: String,
    ops: RwSignal<OperationsState>,
    error_localizer: Arc<Localizer>,
    id: String,
    error: RwSignal<Option<String>>,
    zone: Option<api::IanaZone>,
) -> impl IntoView {
    // The plan an operation belongs to does not change.
    let plan = ops
        .with_untracked(|state| state.row(&id).map(|row| row.summary.plan.clone()))
        .unwrap_or_default();
    // A row that has left the state, in the moment before its view is dropped,
    // reads as empty: never a panic, never a stale fact.
    let status = Memo::new({
        let id = id.clone();
        move |_| {
            ops.with(|state| {
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
            ops.with(|state| {
                state
                    .row(&id)
                    .map(|row| key(row).to_owned())
                    .unwrap_or_default()
            })
        }
    });
    let percent = Memo::new({
        let id = id.clone();
        move |_| ops.with(|state| state.row(&id).and_then(progress_percent))
    });
    let badge = Memo::new({
        let localizer = Arc::clone(&localizer);
        move |_| status.with(|status| badge_of(&localizer, status))
    });
    let can_cancel = Memo::new(move |_| status.with(|status| cancellable(status)));
    let has_progress = Memo::new(move |_| percent.get().is_some());
    let cancel_label = text_with(
        &localizer,
        msg::UI_CANCEL_PLAN,
        &[("plan", MsgArg::Text(plan.clone()))],
    );
    let progress_label = text(&localizer, msg::UI_BACKUP_PROGRESS);
    let still_waiting = text(&localizer, msg::UI_STILL_WAITING);
    let url = format!("{base}/api/v1/operations/{id}/cancel");
    let operation = id;
    view! {
        <tr>
            <td>{plan}</td>
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
                                        let ops = ops;
                                        spawn_local(async move {
                                            match ws::post(&url, &Empty {}).await {
                                                Some(Posted::Done(_)) => {
                                                    if !ws::settle(&base, ops, &operation).await {
                                                        say(Politeness::Polite, &still_waiting);
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
                                                None => {}
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
/// `rows`. Only a change of this rebuilds the table: an update to a row, or to
/// the order of the rows, leaves the table and the controls in it as they are
/// (WCAG 2.4.3).
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
/// message. A status this build doesn't know is the empty pill with the
/// status as it came, never a blank cell.
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

/// How a run's end is told: which politeness, which message. A failure is
/// said assertively; a cancellation and a finish (warnings included — they
/// are the row's own story) politely.
fn end_display(status: &str) -> (Politeness, MessageId) {
    match status {
        "failed" => (Politeness::Assertive, msg::UI_BACKUP_FAILED),
        "cancelled" => (Politeness::Polite, msg::UI_BACKUP_CANCELLED),
        _ => (Politeness::Polite, msg::UI_BACKUP_FINISHED),
    }
}

/// The text of message `id`, with `args` for its variables.
fn text_with(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
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

/// A refusal the daemon answered with, as the catalog writes it (§8.4): a
/// typed code with its arguments, never a bare status number.
fn refusal(
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

/// The snapshots page, the route `/snapshots`: one repository's snapshots,
/// newest first, behind a repository picker (§4.4). The repositories come
/// from one read; the snapshots of the chosen one from another, and a read
/// that fails says so in the page's heading, like the operations page.
#[component]
pub(crate) fn Snapshots(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_SNAPSHOTS_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_SNAPSHOTS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let caption = text(&localizer, msg::UI_SNAPSHOTS_CAPTION);
    let (snapshot_column, time_column) = (
        text(&localizer, msg::UI_COLUMN_SNAPSHOT),
        text(&localizer, msg::UI_COLUMN_TIME),
    );
    let empty = text(&localizer, msg::UI_NO_SNAPSHOTS);
    let base = ws::origin();
    let unreachable = RwSignal::new(false);
    // The repositories to pick from; `None` while the read is on its way.
    let repositories = RwSignal::<Option<Vec<String>>>::new(None);
    // The chosen repository and its snapshots. `None` snapshots of a chosen
    // repository is the read on its way.
    let chosen = RwSignal::<Option<String>>::new(None);
    let snapshots = RwSignal::<Option<Vec<api::SnapshotDto>>>::new(None);
    let repositories_url = format!("{base}/api/v1/repositories");
    spawn_local(async move {
        match ws::read::<api::RepositoriesDto>(&repositories_url).await {
            Some(read) => repositories.set(Some(
                read.repositories.into_iter().map(|row| row.alias).collect(),
            )),
            None => unreachable.set(true),
        }
    });
    let pick = move |alias: String| {
        chosen.set(Some(alias.clone()));
        snapshots.set(None);
        let url = format!("{base}/api/v1/repositories/{alias}/snapshots");
        spawn_local(async move {
            match ws::read::<api::SnapshotsDto>(&url).await {
                Some(read) => snapshots.set(Some(read.snapshots)),
                None => unreachable.set(true),
            }
        });
    };
    // Newest first, by the raw instant each row shows: RFC 3339 UTC orders
    // as its bytes do.
    let sorted = Memo::new(move |_| {
        let mut rows = snapshots.get().unwrap_or_default();
        rows.sort_by(|a, b| b.time.cmp(&a.time));
        rows
    });
    let picker_loading = loading.clone();
    let table_localizer = Arc::clone(&localizer);
    // The zone the browser reads its clock in: each time is written in it.
    let viewer = l10n::viewer_zone();
    view! {
        <PageHeading>
            {move || if unreachable.get() { unreachable_heading.clone() } else { heading.clone() }}
        </PageHeading>
        <p>{body}</p>
        {move || {
            unreachable
                .get()
                .then(|| view! { <p>{unreachable_body.clone()}</p> })
        }}
        {move || match repositories.get() {
            None => view! { <p>{picker_loading.clone()}</p> }.into_any(),
            Some(aliases) => aliases
                .into_iter()
                .map(|alias| {
                    let label = text_with(
                        &localizer,
                        msg::UI_SNAPSHOTS_OF,
                        &[("alias", MsgArg::Text(alias.clone()))],
                    );
                    let pick = pick.clone();
                    view! {
                        <button type="button" on:click=move |_| pick(alias.clone())>
                            {label.clone()}
                        </button>
                    }
                })
                .collect_view()
                .into_any(),
        }}
        {move || match chosen.get() {
            None => ().into_any(),
            Some(_) => match snapshots.get() {
                None => view! { <p>{loading.clone()}</p> }.into_any(),
                Some(rows) if rows.is_empty() => {
                    view! { <p>{empty.clone()}</p> }.into_any()
                }
                Some(_) => {
                    let rows_localizer = Arc::clone(&table_localizer);
                    let rows_zone = viewer.clone();
                    view! {
                    <table>
                        <caption class="text-muted">{caption.clone()}</caption>
                        <thead>
                            <tr>
                                <th scope="col">{snapshot_column.clone()}</th>
                                <th scope="col">{time_column.clone()}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <For
                                each=move || sorted.get()
                                key=|snapshot| snapshot.id.clone()
                                children=move |snapshot| {
                                    let raw = snapshot.time.clone();
                                    let shown = instant(&rows_localizer, &raw, rows_zone.as_ref());
                                    view! {
                                        <tr>
                                            <td>{short_id(&snapshot.id)}</td>
                                            <td title=raw.clone()>{shown}</td>
                                        </tr>
                                    }
                                }
                            />
                        </tbody>
                    </table>
                    }
                }
                    .into_any(),
            },
        }}
    }
}

/// What a person has typed into the Repositories form, one signal per field.
#[derive(Clone, Copy)]
struct Inputs {
    name: RwSignal<String>,
    location: RwSignal<String>,
    hot_storage: RwSignal<String>,
    password: RwSignal<String>,
}

impl Inputs {
    fn new() -> Self {
        Self {
            name: RwSignal::new(String::new()),
            location: RwSignal::new(String::new()),
            hot_storage: RwSignal::new(String::new()),
            password: RwSignal::new(String::new()),
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
        }
    }

    fn clear(self) {
        for field in FIELDS {
            self.of(field).set(String::new());
        }
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
    let (name_column, location_column, password_column) = (
        text(&localizer, msg::UI_COLUMN_NAME),
        text(&localizer, msg::UI_COLUMN_LOCATION),
        text(&localizer, msg::UI_COLUMN_PASSWORD),
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
        if let Some(items) = problems_of(&form, action, |id| text(&submit_localizer, id)) {
            problems.show(items, None);
            return;
        }
        busy.set(true);
        say(
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
                        say(Politeness::Polite, &text_with(&localizer, id, &args));
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
                    view! {
                        <table>
                            <caption class="text-muted">{caption.clone()}</caption>
                            <thead>
                                <tr>
                                    <th scope="col">{name_column.clone()}</th>
                                    <th scope="col">{location_column.clone()}</th>
                                    <th scope="col">{password_column.clone()}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {rows
                                    .into_iter()
                                    .map(|row| repository_row(&localizer, row))
                                    .collect_view()}
                            </tbody>
                        </table>
                    }
                        .into_any()
                }
                _ => view! { <p>{empty.clone()}</p> }.into_any(),
            },
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

/// One row of the repositories table: the name as the row's header, where the
/// repository is (the hot half on a line of its own), and where its password
/// comes from. Never the password itself (I4).
fn repository_row(localizer: &Localizer, row: RepositoryDto) -> impl IntoView {
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
    view! {
        <tr>
            <th scope="row">{row.alias}</th>
            <td>
                <div>{row.repository}</div>
                {hot.map(|hot| view! { <div>{hot}</div> })}
            </td>
            <td>
                <div>{password}</div>
                {notes.into_iter().map(|note| view! { <div class="text-muted">{note}</div> }).collect_view()}
            </td>
        </tr>
    }
}

/// A snapshot's short ID: its first eight hex digits, as rustic's own lists
/// show one. A shorter ID is shown whole.
fn short_id(id: &str) -> String {
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
        // `Form` has no `Debug`, so that nothing can print a password: compare.
        assert!(
            inputs.read()
                == Form {
                    name: "copy".to_owned(),
                    location: "/srv/main".to_owned(),
                    hot_storage: "/srv/hot".to_owned(),
                    password: "typed".to_owned(),
                }
        );
        for field in FIELDS {
            assert!(!inputs.of(field).get_untracked().is_empty(), "{field:?}");
        }
    }

    #[test]
    fn clearing_the_form_empties_every_input() {
        let inputs = Inputs::new();
        for field in FIELDS {
            inputs.of(field).set("typed".to_owned());
        }
        inputs.clear();
        assert!(inputs.read() == Form::default());
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

    #[test]
    fn the_end_of_a_run_is_told_by_its_own_politeness_and_message() {
        assert_eq!(
            end_display("failed"),
            (Politeness::Assertive, msg::UI_BACKUP_FAILED)
        );
        assert_eq!(
            end_display("cancelled"),
            (Politeness::Polite, msg::UI_BACKUP_CANCELLED)
        );
        assert_eq!(
            end_display("succeeded"),
            (Politeness::Polite, msg::UI_BACKUP_FINISHED)
        );
        assert_eq!(
            end_display("succeeded-with-warnings"),
            (Politeness::Polite, msg::UI_BACKUP_FINISHED),
            "a run that warned its way to the end counts as finished"
        );
    }
}
