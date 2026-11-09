//! The routes of the app (technical design §4.5, "Route changes"): the pages a
//! path shows. Each page has its own `<h1>`, the heading focus moves to when a
//! reader navigates to it, so it carries `tabindex="-1"`.

use std::collections::HashMap;
use std::sync::Arc;

use api::{ErrorCode, MsgArg, PlanSummaryDto, PlansDto};
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use serde::Serialize;
use std::cmp::Reverse;
use std::str::FromStr;

use crate::announcer::Politeness;
use crate::backend::{OperationRow, OperationsState};
use crate::components::{Progress, StatusBadge};
use crate::live::say;
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
    let (started_text, backup_cancelled, backup_failed, backup_finished) = (
        text(&localizer, msg::UI_BACKUP_STARTED),
        text(&localizer, msg::UI_BACKUP_CANCELLED),
        text(&localizer, msg::UI_BACKUP_FAILED),
        text(&localizer, msg::UI_BACKUP_FINISHED),
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
    Effect::new(move |_| {
        let rows = announced_ops.get().rows;
        seen.update_value(|seen| {
            for row in &rows {
                let status = row.summary.status.clone();
                let previous = seen.insert(row.summary.id.clone(), status.clone());
                let changed = previous.is_some_and(|previous| {
                    (previous == "queued" || previous == "running") && ws::ended(&status)
                });
                if changed {
                    match status.as_str() {
                        "failed" => say(Politeness::Assertive, &backup_failed),
                        "cancelled" => say(Politeness::Polite, &backup_cancelled),
                        // A run that warned its way to the end counts as
                        // finished: its warnings are the row's own story.
                        _ => say(Politeness::Polite, &backup_finished),
                    }
                }
            }
        });
    });
    // The rows as the current sort shows them. History is the daemon's own
    // order, newest first; a sort order compares the Started cell's instant,
    // RFC 3339 UTC, which orders as its bytes do.
    let sorted = Memo::new(move |_| {
        let mut rows = ops.get().rows;
        match sort.get_untracked() {
            SortOrder::History => {}
            SortOrder::NewestFirst => rows.sort_by_key(|row| Reverse(key(row))),
            SortOrder::OldestFirst => rows.sort_by_key(key),
        }
        rows
    });
    let table_localizer = Arc::clone(&localizer);
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
                                        let error_localizer = Arc::clone(&error_localizer);
                                        let ops = ops;
                                        spawn_local(async move {
                                            match ws::post(&url, &request).await {
                                                Some(Posted::Done(answer)) => {
                                                    say(Politeness::Polite, &started);
                                                    if let Some(operation) =
                                                        ws::started_operation(&answer)
                                                    {
                                                        ws::settle(&base, ops, &operation).await;
                                                    }
                                                }
                                                Some(Posted::Refused(code, args)) => {
                                                    error.set(Some(refusal(
                                                        &error_localizer,
                                                        code,
                                                        &args,
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
            // say and show.
            let newest = sorted_newest.clone();
            let oldest = sorted_oldest.clone();
            let rows_localizer = table_localizer.clone();
            let rows_errors = cancel_localizer.clone();
            let rows_base = cancel_base.clone();
            match phase.get() {
            Phase::Loading => view! { <p>{loading.clone()}</p> }.into_any(),
            Phase::Unreachable => {
                view! { <p>{unreachable_body.clone()}</p> }.into_any()
            }
            Phase::Ready => {
                if sorted.get().is_empty() {
                    view! { <p>{empty.clone()}</p> }.into_any()
                } else {
                    view! {
                        <table>
                            <caption class="text-muted">{caption.clone()}</caption>
                            <thead>
                                <tr>
                                    <th scope="col">{plan_column.clone()}</th>
                                    <th scope="col">{status_column.clone()}</th>
                                    <th
                                        scope="col"
                                        aria-sort=move || match sort.get_untracked() {
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
                                    each=move || sorted.get()
                                    key=|row| row.summary.id.clone()
                                    children=move |row| {
                                        row_view(
                                            rows_localizer.clone(),
                                            rows_base.clone(),
                                            cancel_ops,
                                            rows_errors.clone(),
                                            row,
                                            error,
                                        )
                                    }
                                />
                            </tbody>
                        </table>
                    }
                    .into_any()
                }
            }
        }
        }}
    }
}

/// One row of the operations table: the plan, its status (with a running
/// backup's progress), the instant it started — the raw instant in the cell's
/// `title`, so an order can be read without the locale's text — and what can
/// be done with it: a running or queued backup can be cancelled.
fn row_view(
    localizer: Arc<Localizer>,
    base: String,
    ops: RwSignal<OperationsState>,
    error_localizer: Arc<Localizer>,
    row: OperationRow,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let (state, label) = match status_display(&row.summary.status) {
        Some((state, label_id)) => (state, text(&localizer, label_id)),
        // A status this build doesn't know: the empty pill with the status
        // as it came, never a blank cell.
        None => ("empty", row.summary.status.clone()),
    };
    let raw = row
        .summary
        .started_at
        .clone()
        .unwrap_or_else(|| row.summary.queued_at.clone());
    let shown = instant(&localizer, &raw);
    let plan = row.summary.plan.clone();
    let cancel_label = text_with(
        &localizer,
        msg::UI_CANCEL_PLAN,
        &[("plan", MsgArg::Text(plan.clone()))],
    );
    let cancellable = row.summary.status == "queued" || row.summary.status == "running";
    let progress = row.progress.map(|progress| {
        let percent = progress
            .total
            .filter(|total| *total > 0)
            .map(|total| ((progress.current.saturating_mul(100)) / total) as u8)
            .unwrap_or(0);
        (
            text(&localizer, msg::UI_BACKUP_PROGRESS),
            RwSignal::new(percent),
        )
    });
    let url = format!("{base}/api/v1/operations/{}/cancel", row.summary.id);
    let operation = row.summary.id.clone();
    let base = base.clone();
    view! {
        <tr>
            <td>{plan}</td>
            <td>
                <StatusBadge state=state label=label />
                {progress
                    .map(|(progress_label, value)| {
                        view! { <Progress label=progress_label value=value /> }
                    })}
            </td>
            <td title=raw.clone()>{shown}</td>
            <td>
                {cancellable.then(|| {
                    let url = url.clone();
                    let base = base.clone();
                    view! {
                        <button
                            type="button"
                            on:click=move |_| {
                                let error_localizer = Arc::clone(&error_localizer);
                                let base = base.clone();
                                let operation = operation.clone();
                                let url = url.clone();
                                let ops = ops;
                                spawn_local(async move {
                                    match ws::post(&url, &Empty {}).await {
                                        Some(Posted::Done(_)) => {
                                            ws::settle(&base, ops, &operation).await;
                                        }
                                        Some(Posted::Refused(code, args)) => {
                                            error.set(Some(refusal(
                                                &error_localizer,
                                                code,
                                                &args,
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
                })}
            </td>
        </tr>
    }
}

/// The order a row's life is told by: the instant its `Started` cell shows,
/// which for a queued run is the instant it was queued.
fn key(row: &OperationRow) -> String {
    row.summary
        .started_at
        .clone()
        .unwrap_or_else(|| row.summary.queued_at.clone())
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

/// The text of message `id`, with `args` for its variables.
fn text_with(localizer: &Localizer, id: MessageId, args: &[(&str, MsgArg)]) -> String {
    localizer.message(id, args).into_text()
}

/// An RFC 3339 instant as the page's locale writes it (§8.4). One that
/// doesn't parse is shown as it came: never a blank cell, never a guess.
fn instant(localizer: &Localizer, raw: &str) -> String {
    match jiff::Timestamp::from_str(raw) {
        Ok(timestamp) => text_with(
            localizer,
            msg::UI_INSTANT,
            &[(
                "when",
                MsgArg::Timestamp {
                    utc_ms: timestamp.as_millisecond(),
                    zone: None,
                },
            )],
        ),
        Err(_) => raw.to_owned(),
    }
}

/// A refusal the daemon answered with, as the catalog writes it (§8.4): a
/// typed code with its arguments, never a bare status number.
fn refusal(localizer: &Localizer, code: ErrorCode, args: &[(String, MsgArg)]) -> String {
    let args = args
        .iter()
        .map(|(name, arg)| (name.as_str(), arg.clone()))
        .collect::<Vec<_>>();
    localizer.error(code, &args, None).into_text()
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
            Some(read) => repositories.set(Some(read.repositories)),
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
                                    let shown = instant(&rows_localizer, &raw);
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
}
