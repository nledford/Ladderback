//! The table (components §4.5): a real `<table>` of keyed rows, sortable by
//! its columns' buttons. [`rules`] decides the order of its rows, what its
//! headers tell a screen reader and what shows in its place, host-tested;
//! this module is the glue that draws it.

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::Hash;
use std::iter;
use std::sync::Arc;

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos::wasm_bindgen::closure::Closure;

use super::Spinner;
use super::button::{Appearance, Button, Content};
use crate::announcer::Politeness;
use crate::live::reply;
use crate::variants::{Rem, Size};

pub(crate) mod rules;

pub use rules::{Align, Name, SortKey, State};
use rules::{Shown, Sorted};

thread_local! {
    /// The number the next table's caption `id` takes, so no two on a page
    /// share one.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

/// How a column draws its cell in a row: built once, when the row first
/// shows, from the row as a signal, so the cell follows the row's changes in
/// place and a control in it is never built again.
type Draw<R> = Arc<dyn Fn(Signal<R>) -> AnyView + Send + Sync>;

/// One column of a [`Table`]: its header's words, its width, how its cells
/// are aligned, how each is drawn and, if it can be sorted by, how.
pub struct Column<R: Send + Sync + 'static> {
    header: String,
    width: Rem,
    align: Align,
    sort: Option<Sort<R>>,
    draw: Draw<R>,
}

impl<R: Send + Sync + 'static> Column<R> {
    /// A column headed `header`, `width` wide, its cells at the start of the
    /// line and drawn by `draw`, which is called once for each row as it
    /// first shows and binds what it draws to the row's signal.
    pub fn new(
        header: impl Into<String>,
        width: Rem,
        draw: impl Fn(Signal<R>) -> AnyView + Send + Sync + 'static,
    ) -> Self {
        Self {
            header: header.into(),
            width,
            align: Align::Start,
            sort: None,
            draw: Arc::new(draw),
        }
    }

    /// The column with its cells, its header's and its footer's aligned so:
    /// numbers at the end ([`Align::End`]).
    pub fn align(self, align: Align) -> Self {
        Self { align, ..self }
    }

    /// The column, sortable by `sort`.
    pub fn sorted_by(self, sort: Sort<R>) -> Self {
        Self {
            sort: Some(sort),
            ..self
        }
    }
}

/// How a column sorts the table: the name of its button, which says what it
/// does ("Sort by size"), the two sentences a sort is said with ("Sorted by
/// size, smallest first.", "Sorted by size, largest first."), complete
/// sentences from the catalog, and the key each row is sorted by.
pub struct Sort<R> {
    label: String,
    ascending: String,
    descending: String,
    key: Arc<dyn Fn(&R) -> SortKey + Send + Sync>,
}

impl<R> Sort<R> {
    pub fn new(
        label: impl Into<String>,
        ascending: impl Into<String>,
        descending: impl Into<String>,
        key: impl Fn(&R) -> SortKey + Send + Sync + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            ascending: ascending.into(),
            descending: descending.into(),
            key: Arc::new(key),
        }
    }
}

/// The sentences that take the table's place while it can't show its rows,
/// the caller's, since only it knows what the rows are.
pub struct Sentences {
    /// Said beside a spinner while the rows are on their way.
    pub loading: String,
    /// Said when there are none.
    pub empty: String,
    /// Said when they can't be read.
    pub unreachable: String,
}

/// A footer row (`<tfoot>`), such as the totals: its header's words, then a
/// cell for each column after the row header's, in their order; columns
/// past the last cell given get empty ones.
#[derive(Clone)]
pub struct Footer {
    pub header: Signal<String>,
    pub cells: Vec<Signal<String>>,
}

/// A table (§4.5): a real `<table>`, named by its `name`, a caption or the
/// page's heading, with `columns` after the `row_header` column, whose cell
/// is each row's header (`<th scope="row">`), and an optional `footer`.
///
/// It sits in a frame that scrolls sideways where its columns are wider than
/// the page (a small screen, larger text): a region named as the table is,
/// which a keyboard focuses and scrolls, each row's header pinned at its
/// start. The columns are as wide as they say, in rem, never as their
/// content, so a cell that changes moves no column (#194).
///
/// `rows` are told apart by `key` and updated in place: a row is built once,
/// when it first shows, and a change to it reaches only what its cells bind
/// to, so a control in a row keeps its focus (2.4.3; #124). Only a change of
/// what shows (`state`, or rows coming to none) builds the table again. A
/// sort or a change that moves a row moves it, and focus that was in it
/// goes back to where it was.
///
/// A sortable column's header holds a button after its words that sorts by
/// it ascending, then descending, ties keeping the order given; the header
/// says which way (`aria-sort`), the button shows it, and the sort is said
/// politely.
///
/// What the caller owes WCAG 2.2 AA, since the table does none of it:
///
/// - **A row's actions are named with their row** ("Delete plan nightly"),
///   since a screen reader may reach them outside the row.
/// - **The sentences** say what the table holds, and a sort's two say which
///   way it runs, complete sentences from the catalog.
/// - **A sort key that orders as the reader expects**, such as a size in
///   bytes rather than its words.
/// - **The loading wait announced** where the reader caused it, as a
///   spinner's (4.1.3).
/// - **A key for each row unlike every other row's**, since rows are told
///   apart by it.
/// - **Focus moved on when its row goes**, such as to the next row or the
///   page's heading, since a removed row takes its focus with it (2.4.3).
#[component]
pub fn Table<R, K, KF>(
    name: Name,
    row_header: Column<R>,
    columns: Vec<Column<R>>,
    #[prop(into)] rows: Signal<Vec<R>>,
    key: KF,
    #[prop(into)] state: Signal<State>,
    sentences: Sentences,
    #[prop(optional)] footer: Option<Footer>,
) -> impl IntoView
where
    R: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    KF: Fn(&R) -> K + Copy + Send + Sync + 'static,
{
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        id
    });
    let caption_id = format!("table-{id}-caption");
    let frame_name = name.frame_labelled_by(&caption_id).to_owned();
    let pinned = row_header.width;
    let columns: Arc<Vec<Column<R>>> =
        Arc::new(std::iter::once(row_header).chain(columns).collect());
    let width = rules::table_width(pinned, columns.iter().skip(1).map(|column| column.width));
    let pinned_room = rules::pinned_room(pinned);
    let sorted = RwSignal::new(None::<Sorted>);
    let shown = Memo::new(move |_| rules::shown(state.get(), rows.with(Vec::len)));
    let order = {
        let columns = Arc::clone(&columns);
        Memo::new(move |_| {
            let by = sorted.get().and_then(|sorted| {
                let sort = columns.get(sorted.column)?.sort.as_ref()?;
                Some((sorted.direction, Arc::clone(&sort.key)))
            });
            rows.with(|rows| {
                rules::order(
                    rows,
                    key,
                    by.as_ref()
                        .map(|(direction, sort_key)| (*direction, &**sort_key as _)),
                )
            })
        })
    };
    let index = Memo::new(move |_| {
        rows.with(|rows| {
            rows.iter()
                .enumerate()
                .map(|(at, row)| (key(row), at))
                .collect::<HashMap<K, usize>>()
        })
    });
    // The order the rows stand in, which follows `order` a step behind: the
    // step notes the control that had focus in the body before a move takes
    // it away (a moved node loses focus), and gives it back after.
    let placed = RwSignal::new(order.get_untracked());
    let body = NodeRef::<leptos::html::Tbody>::new();
    Effect::new(move |_| {
        let next = order.get();
        let focused = focused_in(body);
        placed.set(next);
        if let Some(focused) = focused {
            give_back_focus(focused);
        }
    });
    let Sentences {
        loading,
        empty,
        unreachable,
    } = sentences;
    let caption = name.caption().map(str::to_owned);
    let labelled_by = name.table_labelled_by().map(str::to_owned);
    let row_columns = Arc::clone(&columns);
    view! {
        <div aria-busy=move || {
            (shown.get() == Shown::Loading).then_some("true")
        }>
            {move || match shown.get() {
                Shown::Loading => view! { <Spinner label=loading.clone() /> }.into_any(),
                Shown::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
                Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
                Shown::Table => {
                    let columns = Arc::clone(&row_columns);
                    let header_row = header_row(&columns, sorted);
                    let footer_row = footer.clone().map(|footer| footer_row(&columns, footer));
                    view! {
                        // The frame is the size container the table's width
                        // is measured against, and what scrolls.
                        <div
                            class="table-frame data-table-frame"
                            role="region"
                            aria-labelledby=frame_name.clone()
                            tabindex="0"
                            style:scroll-padding-inline-start=pinned_room.clone()
                        >
                            <table
                                class="data-table"
                                aria-labelledby=labelled_by.clone()
                                style:inline-size=width.clone()
                            >
                                {caption
                                    .clone()
                                    .map(|caption| {
                                        view! {
                                            <caption id=caption_id.clone() class="text-muted">
                                                {caption}
                                            </caption>
                                        }
                                    })}
                                <colgroup>
                                    {columns
                                        .iter()
                                        .enumerate()
                                        .map(|(at, column)| {
                                            let width = if at == 0 {
                                                rules::row_header_width(column.width)
                                            } else {
                                                column.width.to_string()
                                            };
                                            view! { <col style:inline-size=width /> }
                                        })
                                        .collect_view()}
                                </colgroup>
                                {header_row}
                                <tbody node_ref=body>
                                    <For
                                        each=move || placed.get()
                                        key=|key| key.clone()
                                        children=move |key| { row_view(&columns, rows, index, key) }
                                    />
                                </tbody>
                                {footer_row}
                            </table>
                        </div>
                    }
                        .into_any()
                }
            }}
        </div>
    }
    .into_any()
}

/// The header row: each column's words, and a sortable column's button,
/// whose press sorts by it and says so.
fn header_row<R: Send + Sync + 'static>(
    columns: &[Column<R>],
    sorted: RwSignal<Option<Sorted>>,
) -> AnyView {
    let cells = columns
        .iter()
        .enumerate()
        .map(|(at, column)| {
            let class = column.align.class();
            let header = column.header.clone();
            match &column.sort {
                None => view! {
                    <th scope="col" class=class>
                        {header}
                    </th>
                }
                .into_any(),
                Some(sort) => {
                    let (ascending, descending) = (sort.ascending.clone(), sort.descending.clone());
                    let press = Callback::new(move |()| {
                        let next = rules::press(sorted.get_untracked(), at);
                        sorted.set(Some(next));
                        reply(
                            Politeness::Polite,
                            rules::sentence(next.direction, &ascending, &descending),
                        );
                    });
                    view! {
                        <th
                            scope="col"
                            class=class
                            aria-sort=move || rules::aria_sort(sorted.get(), at)
                        >
                            <span class="sort-header">
                                {header}
                                <Button
                                    content=Content::IconAndText {
                                        glyph: Signal::derive(move || rules::glyph(
                                            sorted.get(),
                                            at,
                                        )),
                                        label: Signal::stored(sort.label.clone()),
                                    }
                                    appearance=Appearance::Ghost
                                    size=Size::Small
                                    on_press=press
                                />
                            </span>
                        </th>
                    }
                    .into_any()
                }
            }
        })
        .collect_view();
    view! {
        <thead>
            <tr>{cells}</tr>
        </thead>
    }
    .into_any()
}

/// The footer row: its header, then a cell for each column after the row
/// header's, each aligned as its column; a column the footer has no cell for
/// gets an empty one, so each column keeps its rule.
fn footer_row<R: Send + Sync + 'static>(columns: &[Column<R>], footer: Footer) -> AnyView {
    let cells = columns
        .iter()
        .skip(1)
        .zip(footer.cells.into_iter().map(Some).chain(iter::repeat(None)))
        .map(|(column, cell)| {
            view! { <td class=column.align.class()>{move || cell.map(|cell| cell.get())}</td> }
        })
        .collect_view();
    let class = columns.first().and_then(|column| column.align.class());
    let header = footer.header;
    view! {
        <tfoot>
            <tr>
                <th scope="row" class=class>
                    {move || header.get()}
                </th>
                {cells}
            </tr>
        </tfoot>
    }
    .into_any()
}

/// One row, built once as it first shows: its cells bind to the row as it
/// stands in `rows` now, or as it last stood while it is on its way out.
fn row_view<R, K>(
    columns: &[Column<R>],
    rows: Signal<Vec<R>>,
    index: Memo<HashMap<K, usize>>,
    key: K,
) -> AnyView
where
    R: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
{
    let find = {
        let key = key.clone();
        move || {
            index.with(|index| {
                let at = *index.get(&key)?;
                rows.with(|rows| rows.get(at).cloned())
            })
        }
    };
    let Some(first) = untrack(find.clone()) else {
        return ().into_any();
    };
    let row = Memo::new(move |last: Option<&R>| {
        find()
            .or_else(|| last.cloned())
            .unwrap_or_else(|| first.clone())
    });
    let row = Signal::from(row);
    let cells = columns
        .iter()
        .enumerate()
        .map(|(at, column)| {
            let class = column.align.class();
            let cell = (column.draw)(row);
            if at == 0 {
                view! {
                    <th scope="row" class=class>
                        {cell}
                    </th>
                }
                .into_any()
            } else {
                view! { <td class=class>{cell}</td> }.into_any()
            }
        })
        .collect_view();
    view! { <tr>{cells}</tr> }.into_any()
}

/// The element in the table's body that has focus, if any.
fn focused_in(body: NodeRef<leptos::html::Tbody>) -> Option<web_sys::HtmlElement> {
    let body = body.get_untracked()?;
    let active = web_sys::window()?.document()?.active_element()?;
    body.contains(Some(&active))
        .then(|| active.dyn_into::<web_sys::HtmlElement>().ok())
        .flatten()
}

/// Gives focus back to `focused` once the rows have moved: in the same turn,
/// after the move, and at the next frame should the move come later.
fn give_back_focus(focused: web_sys::HtmlElement) {
    let again = focused.clone();
    leptos::task::spawn_local(async move { refocus(&focused) });
    if let Some(window) = web_sys::window() {
        let frame = Closure::once_into_js(move || refocus(&again));
        let _ = window.request_animation_frame(frame.unchecked_ref());
    }
}

/// Focuses `element` if it is still on the page and focus fell to the page
/// itself (a moved node loses it), never taking it from where the reader has
/// since moved it.
fn refocus(element: &web_sys::HtmlElement) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let body: Option<web_sys::Element> = document.body().map(Into::into);
    let fell = document
        .active_element()
        .is_none_or(|active| Some(&active) == body.as_ref());
    if element.is_connected() && fell {
        let _ = element.focus();
    }
}
