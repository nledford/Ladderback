//! The gallery's table demo (technical design §4.5, "Tests").

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::link::RUSTIC_DOCS;
use super::{DemoHeading, demo_link};
use crate::components::button::Content;
use crate::components::link::rules::Destination;
use crate::components::{
    Align, Button, ChangeMarks, Column, Cue, Footer, MarkAllSeen, Name, RadioGroup, Sentences,
    Sort, SortKey, State, Table,
};
use crate::routes::text_with;
use crate::text;
use crate::variants::Rem;

/// The table's demo (`#table-demo`).
#[component]
pub(super) fn TableDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    let table_localizer = Arc::clone(&localizer);
    view! {
        <section id="table-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABLE_HEADING) />
            {demo_table(table_localizer)}
        </section>
    }
}

/// A folder of the table demo: neutral demo data.
#[derive(Clone, Debug, PartialEq)]
struct DemoFolder {
    name: String,
    size: u64,
    changed_ms: i64,
    files: u64,
}

/// The table demo: a choice of what the table shows, then a table of demo
/// folders sortable by each column, with a total and a row action that
/// changes its row, then the same table in a narrow frame, which scrolls
/// with each row's header pinned at its start, and right to left.
fn demo_table(localizer: Arc<Localizer>) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let folder = |name, size, changed_ms, files| DemoFolder {
        name: t(name),
        size,
        changed_ms,
        files,
    };
    let data = RwSignal::new(vec![
        folder(
            msg::UI_GALLERY_TABLE_PHOTOS,
            4_509_715_660,
            1_791_072_000_000,
            1204,
        ),
        folder(
            msg::UI_GALLERY_TABLE_MUSIC,
            13_529_146_982,
            1_787_270_400_000,
            12,
        ),
        folder(
            msg::UI_GALLERY_TABLE_DOCUMENTS,
            859_832_320,
            1_793_577_600_000,
            312,
        ),
        folder(
            msg::UI_GALLERY_TABLE_NOTES,
            1_572_864,
            1_794_441_600_000,
            12,
        ),
    ]);
    let chosen = RwSignal::new("rows");
    let state = Signal::derive(move || match chosen.get() {
        "loading" => State::Loading,
        "unreachable" => State::Unreachable,
        _ => State::Ready,
    });
    let rows = Signal::derive(move || {
        if chosen.get() == "empty" {
            Vec::new()
        } else {
            data.get()
        }
    });
    let sentences = || Sentences {
        loading: t(msg::UI_GALLERY_TABLE_LOADING),
        empty: t(msg::UI_GALLERY_TABLE_EMPTY),
        unreachable: t(msg::UI_GALLERY_TABLE_UNREACHABLE),
    };
    let total = |localizer: &Arc<Localizer>| {
        let (sizes, counts) = (Arc::clone(localizer), Arc::clone(localizer));
        Footer {
            header: Signal::stored(text(localizer, msg::UI_GALLERY_TABLE_TOTAL)),
            cells: vec![
                Signal::derive(move || {
                    let size = data.with(|rows| rows.iter().map(|row| row.size).sum());
                    text_with(
                        &sizes,
                        msg::UI_GALLERY_TABLE_SIZE,
                        &[("size", MsgArg::Bytes(size))],
                    )
                }),
                Signal::stored(String::new()),
                Signal::derive(move || {
                    let files = data.with(|rows| rows.iter().map(|row| row.files).sum());
                    text_with(
                        &counts,
                        msg::UI_GALLERY_TABLE_FILES,
                        &[("count", MsgArg::Count(files))],
                    )
                }),
            ],
        }
    };
    let (row_header, columns) = folder_columns(&localizer, Some(data));
    let (narrow_header, narrow_columns) = folder_columns(&localizer, None);
    let (rtl_header, rtl_columns) = folder_columns(&localizer, None);
    view! {
        // Before the table it changes, so what it changes follows it.
        <RadioGroup
            name="gallery-table-shows"
            legend=t(msg::UI_GALLERY_TABLE_SHOWS)
            options=vec![
                ("rows", t(msg::UI_GALLERY_TABLE_SHOWS_ROWS)),
                ("loading", t(msg::UI_GALLERY_TABLE_SHOWS_LOADING)),
                ("empty", t(msg::UI_GALLERY_TABLE_SHOWS_EMPTY)),
                ("unreachable", t(msg::UI_GALLERY_TABLE_SHOWS_UNREACHABLE)),
            ]
            chosen=chosen
        />
        <Table
            name=Name::Caption(t(msg::UI_GALLERY_TABLE_CAPTION))
            row_header=row_header
            columns=columns
            rows=rows
            key=|row: &DemoFolder| row.name.clone()
            state=state
            sentences=sentences()
            footer=total(&localizer)
        />
        <div class="gallery-narrow">
            <Table
                name=Name::Caption(t(msg::UI_GALLERY_TABLE_NARROW_CAPTION))
                row_header=narrow_header
                columns=narrow_columns
                rows=data
                key=|row: &DemoFolder| row.name.clone()
                state=State::Ready
                sentences=sentences()
            />
        </div>
        <div class="gallery-narrow" dir="rtl">
            <Table
                name=Name::Caption(t(msg::UI_GALLERY_TABLE_RTL_CAPTION))
                row_header=rtl_header
                columns=rtl_columns
                rows=data
                key=|row: &DemoFolder| row.name.clone()
                state=State::Ready
                sentences=sentences()
            />
        </div>
    }
}

/// The demo folders' columns: the name, the row's header, its size and the
/// number of its files, numbers at the end, and when it changed. Given the
/// folders' `data`, each is sortable and a last column holds the row's
/// action, which adds a file to its folder. Each value is isolated
/// (`<bdi>`): the demo's words are English in every frame, and in the
/// right-to-left one an English size would otherwise read "GiB 4.2" (a page
/// in a right-to-left language gets Fluent's isolation marks instead).
fn folder_columns(
    localizer: &Arc<Localizer>,
    data: Option<RwSignal<Vec<DemoFolder>>>,
) -> (Column<DemoFolder>, Vec<Column<DemoFolder>>) {
    let t = |id| text(localizer, id);
    let sortable = data.is_some();
    let sort = |column: Column<DemoFolder>,
                label,
                ascending,
                descending,
                key: fn(&DemoFolder) -> SortKey| {
        if sortable {
            column.sorted_by(Sort::new(t(label), t(ascending), t(descending), key))
        } else {
            column
        }
    };
    let name = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_NAME),
            Rem::tenths(80),
            |row: Signal<DemoFolder>| (move || row.with(|row| row.name.clone())).into_any(),
        ),
        msg::UI_GALLERY_TABLE_SORT_NAME,
        msg::UI_GALLERY_TABLE_SORTED_NAME_UP,
        msg::UI_GALLERY_TABLE_SORTED_NAME_DOWN,
        |row: &DemoFolder| SortKey::Text(row.name.to_lowercase()),
    );
    let sizes = Arc::clone(localizer);
    let size = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_SIZE_COLUMN),
            Rem::tenths(70),
            move |row: Signal<DemoFolder>| {
                let sizes = Arc::clone(&sizes);
                view! {
                    <bdi>
                        {move || {
                            let size = row.with(|row| row.size);
                            text_with(
                                &sizes,
                                msg::UI_GALLERY_TABLE_SIZE,
                                &[("size", MsgArg::Bytes(size))],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        )
        .align(Align::End),
        msg::UI_GALLERY_TABLE_SORT_SIZE,
        msg::UI_GALLERY_TABLE_SORTED_SIZE_UP,
        msg::UI_GALLERY_TABLE_SORTED_SIZE_DOWN,
        |row: &DemoFolder| SortKey::Number(i64::try_from(row.size).unwrap_or(i64::MAX)),
    );
    let dates = Arc::clone(localizer);
    let changed = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_CHANGED),
            Rem::tenths(90),
            move |row: Signal<DemoFolder>| {
                let dates = Arc::clone(&dates);
                view! {
                    <bdi>
                        {move || {
                            let utc_ms = row.with(|row| row.changed_ms);
                            text_with(
                                &dates,
                                msg::UI_GALLERY_TABLE_CHANGED_ON,
                                &[
                                    (
                                        "when",
                                        MsgArg::Timestamp {
                                            utc_ms,
                                            zone: None,
                                        },
                                    ),
                                ],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        ),
        msg::UI_GALLERY_TABLE_SORT_CHANGED,
        msg::UI_GALLERY_TABLE_SORTED_CHANGED_UP,
        msg::UI_GALLERY_TABLE_SORTED_CHANGED_DOWN,
        |row: &DemoFolder| SortKey::Number(row.changed_ms),
    );
    let counts = Arc::clone(localizer);
    let files = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_FILES_COLUMN),
            // Wide enough for "Files" and its sort button on one line at
            // 1280 px; at 6rem the button wrapped under the word.
            Rem::tenths(80),
            move |row: Signal<DemoFolder>| {
                let counts = Arc::clone(&counts);
                view! {
                    <bdi>
                        {move || {
                            let files = row.with(|row| row.files);
                            text_with(
                                &counts,
                                msg::UI_GALLERY_TABLE_FILES,
                                &[("count", MsgArg::Count(files))],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        )
        .align(Align::End),
        msg::UI_GALLERY_TABLE_SORT_FILES,
        msg::UI_GALLERY_TABLE_SORTED_FILES_UP,
        msg::UI_GALLERY_TABLE_SORTED_FILES_DOWN,
        |row: &DemoFolder| SortKey::Number(i64::try_from(row.files).unwrap_or(i64::MAX)),
    );
    let mut columns = vec![size, changed, files];
    if let Some(data) = data {
        // The row's action, named with its row, adds a file to the folder,
        // which changes the row and, sorted by files, moves it.
        let labels = Arc::clone(localizer);
        columns.push(Column::new(
            t(msg::UI_GALLERY_TABLE_ACTIONS),
            Rem::tenths(90),
            move |row: Signal<DemoFolder>| {
                let name = row.with_untracked(|row| row.name.clone());
                let label = text_with(
                    &labels,
                    msg::UI_GALLERY_TABLE_ADD_FILE,
                    &[("folder", MsgArg::Text(name.clone()))],
                );
                let press = Callback::new(move |()| {
                    data.update(|rows| {
                        if let Some(row) = rows.iter_mut().find(|row| row.name == name) {
                            row.files = row.files.saturating_add(1);
                        }
                    });
                });
                view! { <Button content=Content::Text(label.into()) on_press=press /> }.into_any()
            },
        ));
    }
    (name, columns)
}

/// A job of the live table demo: neutral demo data, and the step its status
/// has reached: 0 waiting, 1 running, 2 done.
#[derive(Clone, Debug, PartialEq)]
struct DemoJob {
    name: String,
    step: usize,
}

/// The live table's demo (`#live-table-demo`): a button that moves the next
/// demo job's status on, as a live update would (photos, then music, then
/// documents, each waiting, running, done and round again), and a table
/// whose status column carries the change mark (decision 129), each row
/// with a toggle a keyboard moves focus into the row with, and last a link
/// to another site.
#[component]
pub(super) fn LiveTableDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let jobs = RwSignal::new(
        [
            msg::UI_GALLERY_TABLE_PHOTOS,
            msg::UI_GALLERY_TABLE_MUSIC,
            msg::UI_GALLERY_TABLE_DOCUMENTS,
        ]
        .map(|name| DemoJob {
            name: t(name),
            step: 0,
        })
        .to_vec(),
    );
    let next = StoredValue::new(0_usize);
    let change = Callback::new(move |()| {
        let at = next.get_value();
        jobs.update(|jobs| {
            if let Some(job) = jobs.get_mut(at) {
                job.step = (job.step + 1) % 3;
            }
            next.set_value((at + 1) % jobs.len().max(1));
        });
    });
    let statuses: Arc<[String; 3]> = Arc::new([
        t(msg::UI_GALLERY_LIVE_TABLE_WAITING),
        t(msg::UI_GALLERY_LIVE_TABLE_RUNNING),
        t(msg::UI_GALLERY_LIVE_TABLE_DONE),
    ]);
    let status_of = |job: &DemoJob| job.step.to_string();
    let job = Column::new(
        t(msg::UI_GALLERY_LIVE_TABLE_JOB),
        Rem::tenths(100),
        |row: Signal<DemoJob>| row.with_untracked(|row| row.name.clone()).into_any(),
    );
    let status = Column::new(
        t(msg::UI_GALLERY_LIVE_TABLE_STATUS),
        Rem::tenths(100),
        move |row: Signal<DemoJob>| {
            let statuses = Arc::clone(&statuses);
            (move || row.with(|row| statuses.get(row.step).cloned().unwrap_or_default())).into_any()
        },
    )
    .cued(Cue::new(status_of, t(msg::UI_TABLE_CUE_UNSEEN)));
    let marks = ChangeMarks::<String>::new();
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
    let star_localizer = Arc::clone(&localizer);
    let star = Column::new(
        t(msg::UI_GALLERY_TABLE_ACTIONS),
        Rem::tenths(140),
        move |row: Signal<DemoJob>| {
            let label = row.with_untracked(|row| {
                text_with(
                    &star_localizer,
                    msg::UI_GALLERY_LIVE_TABLE_STAR,
                    &[("job", MsgArg::Text(row.name.clone()))],
                )
            });
            let starred = RwSignal::new(false);
            let press = Callback::new(move |()| starred.update(|starred| *starred = !*starred));
            view! { <Button content=Content::Text(label.into()) pressed=starred on_press=press /> }
                .into_any()
        },
    );
    // Another site's link in the row's last cell, far past a small screen's
    // edge: what its name adds after its words is out of sight, as the
    // mark's word is, and both stay inside the table's frame.
    let guide_localizer = Arc::clone(&localizer);
    let guide = Column::new(
        t(msg::UI_GALLERY_LIVE_TABLE_GUIDE),
        Rem::tenths(140),
        move |row: Signal<DemoJob>| {
            let words = row.with_untracked(|row| {
                text_with(
                    &guide_localizer,
                    msg::UI_GALLERY_LIVE_TABLE_GUIDE_LINK,
                    &[("job", MsgArg::Text(row.name.clone()))],
                )
            });
            view! { {demo_link(&guide_localizer, Destination::outside(RUSTIC_DOCS), words)} }
                .into_any()
        },
    );
    view! {
        <section id="live-table-demo">
            <DemoHeading text=t(msg::UI_GALLERY_LIVE_TABLE_HEADING) id=LIVE_TABLE_HEADING_ID />
            // Its row of actions, as a page's: the stand-in for a live update,
            // then Mark all as seen, whose press hands focus to the heading.
            <p class="actions">
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_LIVE_TABLE_CHANGE).into())
                    on_press=change
                />
                <MarkAllSeen
                    marks=marks
                    label=t(msg::UI_MARK_ALL_SEEN)
                    said=marked_seen
                    then_focus=LIVE_TABLE_HEADING_ID.to_owned()
                />
            </p>
            <Table
                name=Name::Caption(t(msg::UI_GALLERY_LIVE_TABLE_CAPTION))
                row_header=job
                columns=vec![status, star, guide]
                rows=jobs
                key=|row: &DemoJob| row.name.clone()
                state=State::Ready
                sentences=Sentences {
                    loading: t(msg::UI_GALLERY_TABLE_LOADING),
                    empty: t(msg::UI_GALLERY_TABLE_EMPTY),
                    unreachable: t(msg::UI_GALLERY_TABLE_UNREACHABLE),
                }
                marks=marks
            />
        </section>
    }
}

/// The `id` of the live table demo's heading, where Mark all as seen hands
/// focus as it goes.
const LIVE_TABLE_HEADING_ID: &str = "live-table-demo-heading";
