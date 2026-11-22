//! The Operations page, the route `/operations` (§4.4): the backup history,
//! live, a Start for each plan and a Cancel for each run still going. What a
//! row says is `operations.rs`'s, the live state `backend.rs`'s and a paused
//! page's `pause.rs`'s; this is the DOM glue, which the `@ui` scenarios of
//! `features/ui/` run, with the decisions not yet moved out of it (#739): the
//! order of its rows, what it shows in its table's place, how a status is
//! badged, and which runs can be cancelled or announce their end, tested on
//! the host below.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use api::{MsgArg, PlanSummaryDto, PlansDto};
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::announcer::Politeness;
use crate::backend::{OperationRow, OperationsState, ended};
use crate::components::ACTIONS;
use crate::components::button::Content;
use crate::components::skeleton::rules::Awaited;
use crate::components::status_badge::rules::BadgeState;
use crate::components::{
    Button, ChangeMarks, Column, ColumnsChoice, Cue, MarkAllSeen, Name as TableName, PauseUpdates,
    Progress, Sentences, Sort, SortKey, State, StatusBadge, Table, Toast, ToastTone, Toasts,
    cell_sentence_view, hand_focus_when_gone, refusal_alert, runs_view, say_refusals,
};
use crate::components::{first_load, paced};
use crate::live::{reply, say};
use crate::operations::{self, Name, Said};
use crate::pause::{self, Ends, Pausable};
use crate::repositories::Sentence;
use crate::routes::{Empty, PAGE_HEADING_ID, PageHeading, instant, refused_sentence, text_with};
use crate::text;
use crate::variants::{Rem, Size, Tone};
use crate::visit::Visit;
use crate::ws::{self, Got, Phase, Posted};

/// The body a start request carries: the plan to back up.
#[derive(Serialize)]
struct StartBackup {
    plan: String,
}

/// The page's connection to the daemon, as a row's Cancel uses it: the
/// daemon's origin, and the reader's visit, which the cancel's wait for the
/// run's end lasts no longer than (#300).
#[derive(Clone)]
struct Connection {
    base: String,
    visit: Visit,
}

/// Where the operations page answers a start or a cancel that its table
/// doesn't: a refusal in the page's error alert, and a wait for the run's
/// end that ran out in its toast, since the row still shows the run as it
/// was (#983).
#[derive(Clone, Copy)]
struct Answers {
    refused: RwSignal<Option<Sentence>>,
    toasts: Toasts,
}

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
    // A start or a cancel the daemon refused, its own words marked (§8.4).
    let error = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(error);
    let toasts = Toasts::new();
    let base = ws::origin();
    // The reader's visit to the page: the connection and the settle-polls of
    // the page's presses last as long as it, and end as the page goes (#300).
    let (visit, departure) = Visit::begin();
    on_cleanup(move || drop(departure));
    // A history read the daemon refused, said where the table would be, as a
    // read refused after the reader closed Sign in again is (§4.5).
    let history_refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(history_refused);
    let history_localizer = Arc::clone(&localizer);
    ws::watch(
        &base,
        ops,
        phase,
        history_refused,
        move |code, args, detail| refused_sentence(&history_localizer, code, args, detail),
        &visit,
    );
    // What the page shows: the live state, or the copy taken when the reader
    // paused it. Every view reads this; the stream, the settle-poll and the
    // end announcements read the live state.
    let page = Pausable::new(ops, phase);
    let plans_url = format!("{base}/api/v1/plans");
    let plans_ops = ops;
    // A list of plans the daemon refused, as a configuration with a mistake
    // in it is: said where the plans' buttons would be (#347).
    let plans_refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(plans_refused);
    let plans_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        match ws::get::<PlansDto>(&plans_url).await {
            Some(Got::Read(read)) => plans.set(Some(read.plans)),
            Some(Got::Refused(code, args, detail)) => plans_refused.set(Some(refused_sentence(
                &plans_localizer,
                code,
                &args,
                detail.as_deref(),
            ))),
            // No answer at all: the page's phase says that its own way.
            None => {}
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
    // What the page shows: a memo, so an update that leaves it as it was
    // rebuilds nothing. A progress report or a status change is an update to
    // a row, which the row follows by itself, and the table and the controls
    // in it stay where they are.
    let showing = Memo::new(move |_| {
        let phase = page.phase();
        let refused = history_refused.with(Option::is_some);
        page.with(|state| shown(phase, refused, &state.rows))
    });
    let (showing, pace) = paced(move || showing.get(), |showing| *showing == Shown::Loading);
    let table_localizer = Arc::clone(&localizer);
    let error_alert_localizer = Arc::clone(&localizer);
    let plans_alert_localizer = Arc::clone(&localizer);
    let history_alert_localizer = Arc::clone(&localizer);
    // The rows a live update changed that the reader hasn't seen yet: the
    // table marks them, and Mark all as seen, in the row of actions, clears
    // them (decision 129).
    let marks = ChangeMarks::<String>::new();
    let mark_all_seen = text(&localizer, msg::UI_MARK_ALL_SEEN);
    let marked_seen = Callback::new({
        let localizer = Arc::clone(&localizer);
        move |count: usize| {
            let count = u64::try_from(count).unwrap_or(u64::MAX);
            text_with(
                &localizer,
                msg::UI_MARKED_SEEN,
                &[("count", MsgArg::Count(count))],
            )
        }
    });
    // The zone the browser reads its clock in, asked once: every instant the
    // page shows is written in it (§8.4).
    let viewer = l10n::viewer_zone();
    let start_base = base.clone();
    let start_visit = visit.clone();
    let connection = Connection { base, visit };
    let start_ops = ops;
    view! {
        <PageHeading>
            {move || {
                if page.phase() == Phase::Unreachable {
                    unreachable_heading.clone()
                } else {
                    heading.clone()
                }
            }}
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
        {move || error.get().map(|sentence| refusal_alert(&error_alert_localizer, sentence))}
        {move || {
            plans_refused.get().map(|sentence| refusal_alert(&plans_alert_localizer, sentence))
        }}
        // The plans sit outside the connection's phases: a reader can start a
        // backup while the history is still on its way. Their buttons are a
        // row of actions, as a dialog's are, so each stands apart from the
        // next and they wrap on a small screen (§4.4). Mark all as seen ends
        // it, before the table, so a keyboard reaches it before the rows whose
        // focus would see their marks one by one; the row is there with no
        // plans too, as its room is (decision 129).
        <p class=ACTIONS>
            // A press that is refused again in the same words is said again
            // (§4.5).
            {move || {
                plans
                    .get()
                    .map_or_default(|plans| {
                        plans
                            .into_iter()
                            .map(|plan| {
                                let started = started_text.clone();
                                let still_waiting = still_waiting.clone();
                                let url = format!("{}/api/v1/operations", start_base);
                                let ops = start_ops;
                                let base = start_base.clone();
                                let visit = start_visit.clone();
                                let error_localizer = Arc::clone(&localizer_for_errors);
                                let label = text_with(
                                    &start_localizer,
                                    msg::UI_START_PLAN,
                                    &[("plan", MsgArg::Text(plan.id.clone()))],
                                );
                                let starting = RwSignal::new(false);
                                // `starting` keeps the button busy while the daemon
                                // answers the start; what the run then does, the
                                // table shows.
                                view! {
                                    // Named with its plan, as a row's Cancel is: a
                                    // reader tabbing through the plans hears which
                                    // one each button runs (2.4.6, 4.1.2). When a
                                    // start gets no answer at all, the page's phase
                                    // says that its own way.
                                    <Button
                                        content=Content::IconAndText {
                                            glyph: LucideGlyph::Play.into(),
                                            label: label.into(),
                                        }
                                        tone=Tone::Primary
                                        busy=starting
                                        on_press=Callback::new(move |()| {
                                            let request = StartBackup {
                                                plan: plan.id.clone(),
                                            };
                                            let url = url.clone();
                                            let base = base.clone();
                                            let started = started.clone();
                                            let visit = visit.clone();
                                            let still_waiting = still_waiting.clone();
                                            let error_localizer = Arc::clone(&error_localizer);
                                            let ops = ops;
                                            error.set(None);
                                            starting.set(true);
                                            spawn_local(async move {
                                                let answer = ws::post(&url, &request).await;
                                                starting.try_set(false);
                                                match answer {
                                                    Some(Posted::Done(answer)) => {
                                                        reply(Politeness::Polite, &started);
                                                        if let Some(operation) = ws::started_operation(&answer)
                                                            && visit.during(ws::settle(&base, ops, &operation)).await
                                                                == Some(false)
                                                        {
                                                            toasts.show(ToastTone::Primary, still_waiting);
                                                        }
                                                    }
                                                    Some(Posted::Refused(code, args, detail)) => {
                                                        error
                                                            .set(
                                                                Some(
                                                                    refused_sentence(
                                                                        &error_localizer,
                                                                        code,
                                                                        &args,
                                                                        detail.as_deref(),
                                                                    ),
                                                                ),
                                                            );
                                                    }
                                                    None => {}
                                                }
                                            });
                                        })
                                    />
                                }
                            })
                            .collect::<Vec<_>>()
                    })
            }}
            <MarkAllSeen
                marks=marks
                label=mark_all_seen
                said=marked_seen
                then_focus=PAGE_HEADING_ID.to_owned()
            />
        </p>
        // After the start buttons, whose press it mostly answers. Dismissed,
        // it hands focus to the page's heading: the button whose press it
        // answers may be a row's Cancel, gone with its run.
        <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=PAGE_HEADING_ID />
        // Per render: the table's columns own what they say and show. A
        // render is a change of what the page shows, never an update to a row.
        {move || {
            match showing.get() {
                Shown::Loading => first_load(loading.clone(), Awaited::table(5), pace).into_any(),
                Shown::Unreachable => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
                Shown::Refused => {
                    history_refused
                        .get()
                        .map(|sentence| refusal_alert(&history_alert_localizer, sentence))
                        .into_any()
                }
                Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
                Shown::Table => {
                    operations_table(
                        &table_localizer,
                        &connection,
                        page,
                        cancelled,
                        Answers { refused: error, toasts },
                        viewer.clone(),
                        marks,
                    )
                }
            }
        }}
    }
}

/// The operations table, named by the page's heading: each operation's name
/// as its row's header (a backup by its plan, a maintenance run by what ran
/// and its repository), its status (with a running backup's progress), the
/// instant it started, in its words in a `<time>` that gives it to a program
/// (#851), what a run that succeeded produced, and what can be done with it:
/// a running or queued run can be cancelled. Its rows come in the daemon's
/// order, newest first, and sort by Started, newest first on the first press
/// of its button, as that order already is.
///
/// A row is built once, when its operation first shows, and follows the state
/// from then on. Each fact it shows is a memo of its own, and each cell binds
/// to the facts it shows, so an update reaches only the cells it is about: a
/// progress report moves the bar, a start changes the status and the Started
/// cell, and none of them replaces the Cancel button (WCAG 2.4.3). The button
/// goes only when the run ends, and then it is no longer a control.
///
/// What it shows is read from `page`, which holds still while the reader has
/// paused it; a cancel waits for its end in the live state, as long as the
/// reader's visit lasts, and puts its run in `cancelled`, so that its end is
/// said as the answer to the press.
///
/// Its row header takes focus from a script, not from Tab: a control in the
/// row that goes while it holds focus, as the progress bar does when its run
/// ends, hands focus to it rather than to the page (WCAG 2.4.3).
fn operations_table(
    localizer: &Arc<Localizer>,
    connection: &Connection,
    page: Pausable,
    cancelled: StoredValue<HashSet<String>>,
    answers: Answers,
    zone: Option<api::IanaZone>,
    marks: ChangeMarks<String>,
) -> AnyView {
    let name_localizer = Arc::clone(localizer);
    let operation = Column::new(
        "operation",
        text(localizer, msg::UI_COLUMN_OPERATION),
        Rem::tenths(80),
        move |row: Signal<OperationRow>| {
            // What an operation is and where it ran do not change, so its
            // name is worded once.
            row.with_untracked(|row| match operations::name(&row.summary) {
                Name::Plan(plan) => plan,
                Name::Words(words) => text_with(&name_localizer, words.id, &words.args),
            })
            .into_any()
        },
    );
    let status_localizer = Arc::clone(localizer);
    let status_zone = zone.clone();
    let status = Column::new(
        "status",
        text(localizer, msg::UI_COLUMN_STATUS),
        Rem::tenths(95),
        move |row: Signal<OperationRow>| status_view(&status_localizer, status_zone.clone(), row),
    )
    .cued(Cue::new(
        status_of,
        text(localizer, msg::UI_TABLE_CUE_UNSEEN),
    ));
    let started_localizer = Arc::clone(localizer);
    let started = Column::new(
        "started",
        text(localizer, msg::UI_COLUMN_STARTED),
        // Room for the sort button's label, "Sort by started", and a whole
        // start time on one line each in the widest common sans-serif
        // (DejaVu Sans, Linux's default); 11 rem fit only narrower fonts.
        Rem::tenths(130),
        move |row: Signal<OperationRow>| {
            let localizer = Arc::clone(&started_localizer);
            let zone = zone.clone();
            let at = Memo::new(move |_| row.with(|row| key(row).to_owned()));
            view! {
                <time datetime=move || {
                    at.get()
                }>{move || at.with(|at| instant(&localizer, at, zone.as_ref()))}</time>
            }
            .into_any()
        },
    )
    .sorted_by(
        Sort::new(
            text(localizer, msg::UI_SORT_BY_STARTED),
            text(localizer, msg::UI_SORTED_OLDEST),
            text(localizer, msg::UI_SORTED_NEWEST),
            |row: &OperationRow| SortKey::Text(key(row).to_owned()),
        )
        .descending_first(),
    );
    let result_localizer = Arc::clone(localizer);
    let result = Column::new(
        "result",
        text(localizer, msg::UI_COLUMN_RESULT),
        // The rest of the page's 58 rem: room for a long path such as
        // "/srv/backups/home-laptop.password" (18.1 rem in DejaVu Sans,
        // Linux's default) on one line, beside its 1 rem of padding.
        Rem::tenths(195),
        move |row: Signal<OperationRow>| result_view(&result_localizer, row),
    );
    let cancel_localizer = Arc::clone(localizer);
    let cancel_connection = connection.clone();
    let actions = Column::new(
        "actions",
        text(localizer, msg::UI_COLUMN_ACTIONS),
        Rem::tenths(80),
        move |row: Signal<OperationRow>| {
            cancel_view(
                &cancel_localizer,
                &cancel_connection,
                page,
                cancelled,
                answers,
                row,
            )
        },
    );
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_NO_OPERATIONS),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=TableName::Heading(PAGE_HEADING_ID.to_owned())
            columns_choice=ColumnsChoice::new("operations", Arc::clone(localizer))
            row_header=operation
            columns=vec![status, started, result, actions]
            rows=Memo::new(move |_| page.with(|state| state.rows.clone()))
            key=|row: &OperationRow| row.summary.id.clone()
            state=State::Ready
            sentences=sentences
            row_header_id=header_id
            marks=marks
        />
    }
    .into_any()
}

/// The `id` of an operation's row header, which a control that leaves the
/// row hands focus to.
fn header_id(row: &OperationRow) -> String {
    format!("operation-{}", row.summary.id)
}

/// What a live update changes that marks an operation's row: its status
/// (a start, an end), never a progress report (decisions 129 and 130).
fn status_of(row: &OperationRow) -> &str {
    &row.summary.status
}

/// An operation's status: its badge, and a running backup's progress bar once
/// it has reported, whose name is worded once and whose last report is read
/// in `zone`, as the Started column is.
fn status_view(
    localizer: &Arc<Localizer>,
    zone: Option<api::IanaZone>,
    row: Signal<OperationRow>,
) -> AnyView {
    let (progress_label, header) = row.with_untracked(|row| {
        let bar = operations::progress_label(&row.summary);
        (text_with(localizer, bar.id, &bar.args), header_id(row))
    });
    let status = Memo::new(move |_| row.with(|row| row.summary.status.clone()));
    // The run's latest report: none until it has reported, so a row that has
    // not begun has no bar.
    let reading = Memo::new(move |_| row.with(|row| row.progress));
    let has_progress = Memo::new(move |_| reading.with(Option::is_some));
    let badge = Memo::new({
        let localizer = Arc::clone(localizer);
        move |_| status.with(|status| badge_of(&localizer, status))
    });
    let bar_localizer = Arc::clone(localizer);
    view! {
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
                            localizer=Arc::clone(&bar_localizer)
                            label=progress_label.clone()
                            then_focus=header.clone()
                            reading=Signal::derive(move || { reading.get().unwrap_or_default() })
                            zone=zone.clone()
                        />
                    }
                })
        }}
    }
    .into_any()
}

/// What a run did, after why it failed: one item alone as it is, two or more
/// as a list (#430).
fn result_view(localizer: &Arc<Localizer>, row: Signal<OperationRow>) -> AnyView {
    let result = Memo::new(move |_| row.with(|row| operations::result(&row.summary)));
    let failure = Memo::new({
        let localizer = Arc::clone(localizer);
        move |_| row.with(|row| operations::failure(&localizer, &row.summary))
    });
    let localizer = Arc::clone(localizer);
    (move || {
        let reason = failure
            .get()
            .map(|sentence| cell_sentence_view(sentence).into_any());
        let facts = result.with(|facts| {
            facts
                .iter()
                .map(|fact| runs_view(&text_with(&localizer, fact.id, &fact.args)).into_any())
                .collect::<Vec<_>>()
        });
        match Said::of(reason.into_iter().chain(facts).collect()) {
            Said::Nothing => ().into_any(),
            Said::One(item) => item,
            Said::List(items) => {
                view! { <ul>{items.into_iter().map(|item| view! { <li>{item}</li> }).collect_view()}</ul> }
                .into_any()
            }
        }
    })
    .into_any()
}

/// A running or queued run's Cancel button, which shows "Cancel" and is
/// named for its run: the Actions column has room for one word, and the name
/// begins with what the button shows (WCAG 2.5.3) (#430). A run that ends
/// takes it away; a button that held focus hands it to the row's header,
/// which names the run (#438). Its refusals are worded in the row's language.
fn cancel_view(
    localizer: &Arc<Localizer>,
    connection: &Connection,
    page: Pausable,
    cancelled: StoredValue<HashSet<String>>,
    answers: Answers,
    row: Signal<OperationRow>,
) -> AnyView {
    let cancel_shown = text(localizer, msg::UI_CANCEL);
    let still_waiting = text(localizer, msg::UI_STILL_WAITING);
    let (operation, cancel_label, header) = row.with_untracked(|row| {
        let cancel = operations::cancel_label(&row.summary);
        (
            row.summary.id.clone(),
            text_with(localizer, cancel.id, &cancel.args),
            header_id(row),
        )
    });
    let can_cancel = Memo::new(move |_| row.with(|row| cancellable(&row.summary.status)));
    let url = format!("{}/api/v1/operations/{operation}/cancel", connection.base);
    let error_localizer = Arc::clone(localizer);
    let connection = connection.clone();
    (move || {
        can_cancel.get().then(|| {
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone(button, header.clone());
            let url = url.clone();
            let connection = connection.clone();
            let operation = operation.clone();
            let still_waiting = still_waiting.clone();
            let error_localizer = Arc::clone(&error_localizer);
            let cancel_name = cancel_label.clone();
            // `cancelling` keeps the button busy while the daemon answers
            // the cancel; the run's end then takes the button away.
            let cancelling = RwSignal::new(false);
            view! {
                <Button
                    content=Content::Text(cancel_shown.clone().into())
                    size=Size::Small
                    node_ref=button
                    fuller_name=Signal::derive(move || cancel_name.clone())
                    busy=cancelling
                    on_press=Callback::new(move |()| {
                        let error_localizer = Arc::clone(&error_localizer);
                        let Connection { base, visit } = connection.clone();
                        let still_waiting = still_waiting.clone();
                        let operation = operation.clone();
                        let url = url.clone();
                        let ops = page.live();
                        cancelled
                            .update_value(|cancelled| {
                                cancelled.insert(operation.clone());
                            });
                        answers.refused.set(None);
                        cancelling.set(true);
                        spawn_local(async move {
                            let answer = ws::post(&url, &Empty {}).await;
                            cancelling.try_set(false);
                            match answer {
                                Some(Posted::Done(_)) => {
                                    if visit.during(ws::settle(&base, ops, &operation)).await
                                        == Some(false)
                                    {
                                        answers.toasts.show(ToastTone::Primary, still_waiting);
                                    }
                                }
                                Some(Posted::Refused(code, args, detail)) => {
                                    cancelled
                                        .update_value(|cancelled| {
                                            cancelled.remove(&operation);
                                        });
                                    answers
                                        .refused
                                        .set(
                                            Some(
                                                refused_sentence(
                                                    &error_localizer,
                                                    code,
                                                    &args,
                                                    detail.as_deref(),
                                                ),
                                            ),
                                        );
                                }
                                None => {
                                    cancelled
                                        .update_value(|cancelled| {
                                            cancelled.remove(&operation);
                                        });
                                }
                            }
                        });
                    })
                />
            }
        })
    })
    .into_any()
}

/// What the operations page shows in the place of its table, or the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// The history is on its way.
    Loading,
    /// The daemon cannot be reached.
    Unreachable,
    /// The daemon answered with a refusal rather than the history.
    Refused,
    /// The history is here and holds no operation.
    Empty,
    /// The history is here and holds some.
    Table,
}

/// What the page shows while its connection is in `phase`, its last read of
/// the history was `refused` or not, and its state holds `rows`. Only a
/// change of this rebuilds the table: an update to a row leaves the table
/// and the controls in it as they are (WCAG 2.4.3), and a change in the order
/// of the rows leaves the table as it is and moves the rows that change
/// place.
fn shown(phase: Phase, refused: bool, rows: &[OperationRow]) -> Shown {
    match phase {
        Phase::Loading => Shown::Loading,
        Phase::Unreachable => Shown::Unreachable,
        Phase::Ready if refused => Shown::Refused,
        Phase::Ready if rows.is_empty() => Shown::Empty,
        Phase::Ready => Shown::Table,
    }
}

/// A status as its badge shows it: its state, and its text. A status this
/// build doesn't know is the empty state with the status as it came, never a
/// blank cell.
fn badge_of(localizer: &Localizer, status: &str) -> (BadgeState, String) {
    match status_display(status) {
        Some((state, label)) => (state, text(localizer, label)),
        None => (BadgeState::Empty, status.to_owned()),
    }
}

/// The order a row's life is told by: the instant its `Started` cell shows,
/// which for a queued run is the instant it was queued. As RFC 3339 in UTC it
/// orders as its bytes do.
fn key(row: &OperationRow) -> &str {
    row.summary
        .started_at
        .as_deref()
        .unwrap_or(&row.summary.queued_at)
}

/// A status as the badge and the label show it: which state, and which
/// message; `None` for a status this build doesn't know, which `badge_of`
/// shows as it came.
fn status_display(status: &str) -> Option<(BadgeState, MessageId)> {
    match status {
        "queued" => Some((BadgeState::Queued, msg::UI_STATUS_QUEUED)),
        "running" => Some((BadgeState::Running, msg::UI_STATUS_RUNNING)),
        "succeeded" => Some((BadgeState::Succeeded, msg::UI_STATUS_SUCCEEDED)),
        "succeeded-with-warnings" => Some((
            BadgeState::SucceededWithWarnings,
            msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS,
        )),
        "failed" => Some((BadgeState::Failed, msg::UI_STATUS_FAILED)),
        "cancelled" => Some((BadgeState::Cancelled, msg::UI_STATUS_CANCELLED)),
        _ => None,
    }
}

/// Whether a row in `status` can still be stopped: a run that has not begun
/// or is still going. An ended run is history.
fn cancellable(status: &str) -> bool {
    status == "queued" || status == "running"
}

/// Whether the change from `previous` to `status` is a run's end arriving:
/// only a run that was still going can end, and only an end is news.
fn announces_end(previous: &str, status: &str) -> bool {
    (previous == "queued" || previous == "running") && ended(status)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(shown(Phase::Loading, false, &[]), Shown::Loading);
        assert_eq!(shown(Phase::Unreachable, false, &[]), Shown::Unreachable);
        assert_eq!(shown(Phase::Ready, false, &[]), Shown::Empty);
        assert_eq!(shown(Phase::Ready, false, &held), Shown::Table);
    }

    #[test]
    fn rows_held_from_before_do_not_show_a_table_while_the_connection_is_not_ready() {
        let held = [summary(None, "2026-11-09T10:00:00Z")];
        assert_eq!(shown(Phase::Loading, false, &held), Shown::Loading);
        assert_eq!(shown(Phase::Unreachable, false, &held), Shown::Unreachable);
    }

    #[test]
    fn a_refused_history_read_shows_the_refusal_in_the_place_of_the_table() {
        let held = [summary(None, "2026-11-09T10:00:00Z")];
        assert_eq!(shown(Phase::Ready, true, &[]), Shown::Refused);
        assert_eq!(shown(Phase::Ready, true, &held), Shown::Refused);
        assert_eq!(shown(Phase::Loading, true, &held), Shown::Loading);
        assert_eq!(shown(Phase::Unreachable, true, &held), Shown::Unreachable);
    }

    #[test]
    fn a_status_is_badged_by_its_state_and_its_text_and_an_unknown_one_stands_as_it_came() {
        let localizer = Localizer::new(&[]);
        assert_eq!(
            badge_of(&localizer, "running"),
            (BadgeState::Running, "Running".to_owned())
        );
        assert_eq!(
            badge_of(&localizer, "succeeded-with-warnings"),
            (
                BadgeState::SucceededWithWarnings,
                "Succeeded with warnings".to_owned()
            )
        );
        assert_eq!(
            badge_of(&localizer, "pausing"),
            (BadgeState::Empty, "pausing".to_owned()),
            "a status this build doesn't know is never a blank cell"
        );
    }

    #[test]
    fn every_status_the_store_names_has_its_state_and_its_message() {
        let states = [
            ("queued", BadgeState::Queued, msg::UI_STATUS_QUEUED),
            ("running", BadgeState::Running, msg::UI_STATUS_RUNNING),
            ("succeeded", BadgeState::Succeeded, msg::UI_STATUS_SUCCEEDED),
            (
                "succeeded-with-warnings",
                BadgeState::SucceededWithWarnings,
                msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS,
            ),
            ("failed", BadgeState::Failed, msg::UI_STATUS_FAILED),
            ("cancelled", BadgeState::Cancelled, msg::UI_STATUS_CANCELLED),
        ];
        for (status, state, label) in states {
            assert_eq!(status_display(status), Some((state, label)), "{status}");
        }
        assert_eq!(
            status_display("pausing"),
            None,
            "a status this build doesn't know is no pill, not a guess"
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
}
