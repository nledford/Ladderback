//! The table (components §4.5): a real `<table>` of keyed rows, sortable by
//! its columns' buttons. [`rules`] decides the order of its rows, what its
//! headers tell a screen reader and what shows in its place, host-tested;
//! this module is the glue that draws it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use api::MsgArg;
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos::wasm_bindgen::closure::Closure;

use super::button::{Appearance, Button, Content};
use super::{Checkbox, Icon, Spinner, Tip, Tooltip, focus_element};
use crate::announcer::Politeness;
use crate::live::reply;
use crate::page::storage;
use crate::routes::text_with;
use crate::text;
use crate::variants::{Rem, Size};

pub(crate) mod rules;

pub use rules::{Align, Name, SortKey, State};
use rules::{Direction, Layout, Marks, Shown, Sorted, Step};

thread_local! {
    /// The number the next table's caption `id` takes, so no two on a page
    /// share one.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
    /// The columns each table `id` given a [`ColumnsChoice`] shows, as this
    /// browser keeps them, read once a page: the tables of one id share it.
    static KEPT: RefCell<HashMap<&'static str, ArcRwSignal<Option<String>>>> =
        RefCell::default();
}

/// What a table's choice of columns is kept under in the browser's local
/// storage, before the table's id.
const KEPT_KEY: &str = "ladderback-columns-";

/// A table's Columns panel, where the reader hides, shows and reorders the
/// columns after the row header's (decision 140): `id` names the table's
/// choice, which this browser keeps for every table of that id, and the
/// panel's words are `localizer`'s. Lower-case letters, digits and hyphens.
pub struct ColumnsChoice {
    id: &'static str,
    localizer: Arc<Localizer>,
}

impl ColumnsChoice {
    /// The Columns panel of the table `id`, its words `localizer`'s.
    pub fn new(id: &'static str, localizer: Arc<Localizer>) -> Self {
        Self { id, localizer }
    }
}

/// How a column draws its cell in a row: built when the row first shows, and
/// again when its column shows after the reader hid it, from the row as a
/// signal, so the cell follows the row's changes in place and a control in
/// it is never built again while its column shows.
type Draw<R> = Arc<dyn Fn(Signal<R>) -> AnyView + Send + Sync>;

/// One column of a [`Table`]: its id, its header's words, its width, how its
/// cells are aligned, how each is drawn and, if it can be sorted by, how. A
/// row header's column may keep the room its words need to read whole, and
/// the column a live update changes may carry the change mark ([`Cue`]).
pub struct Column<R: Send + Sync + 'static> {
    id: &'static str,
    header: String,
    width: Rem,
    whole: Option<Rem>,
    align: Align,
    sort: Option<Sort<R>>,
    cue: Option<Cue<R>>,
    draw: Draw<R>,
}

impl<R: Send + Sync + 'static> Column<R> {
    /// A column `id` headed `header`, `width` wide, its cells at the start
    /// of the line and drawn by `draw`, which is called once for each row as
    /// it first shows, and again as the column shows after it was hidden,
    /// and binds what it draws to the row's signal. The `id`,
    /// lower-case letters, digits and hyphens, unlike its table's others,
    /// names it in what the browser keeps of a table's Columns panel
    /// (decision 140), so it stays as releases come.
    pub fn new(
        id: &'static str,
        header: impl Into<String>,
        width: Rem,
        draw: impl Fn(Signal<R>) -> AnyView + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            header: header.into(),
            width,
            whole: None,
            align: Align::Start,
            sort: None,
            cue: None,
            draw: Arc::new(draw),
        }
    }

    /// The column, as a table's row header, `room` wide however narrow its
    /// frame, so its words read whole, as a snapshot's short ID must; the
    /// frame scrolls instead. It is still at most its width, and at most
    /// three fifths of the frame, so the cells beside it can be read. Only
    /// for a header beside cells that hold no control: the room it covers can
    /// be more than the share that keeps a focused control in view (2.4.11).
    /// A column that isn't the row header's is its width already.
    pub fn kept_whole(self, room: Rem) -> Self {
        Self {
            whole: Some(room),
            ..self
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

    /// The column, as the one a live update changes, such as a status:
    /// a change of its value marks its row (`cue`, decision 129). One
    /// column of a table carries it; the table watches the first that does.
    pub fn cued(self, cue: Cue<R>) -> Self {
        Self {
            cue: Some(cue),
            ..self
        }
    }
}

/// How a column sorts the table: the name of its button, which says what it
/// does ("Sort by size"), the two sentences a sort is said with ("Sorted by
/// size, smallest first.", "Sorted by size, largest first."), complete
/// sentences from the catalog, the key each row is sorted by, and which way
/// its first press sorts.
pub struct Sort<R> {
    label: String,
    ascending: String,
    descending: String,
    key: Arc<dyn Fn(&R) -> SortKey + Send + Sync>,
    first: Direction,
}

impl<R> Sort<R> {
    /// A sort whose first press sorts ascending.
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
            first: Direction::Ascending,
        }
    }

    /// The sort, its first press descending: for a column read newest first,
    /// such as a history's start times, whose rows already come newest
    /// first.
    pub fn descending_first(self) -> Self {
        Self {
            first: Direction::Descending,
            ..self
        }
    }
}

/// The change mark a column carries (decisions 129 and 130): `on`, what
/// a live update changes in a row, such as its status, and `changed`, the
/// words a screen reader hears in the column's cell while its row is
/// marked, a complete message from the catalog that reads on its own
/// ("Changed, not yet seen").
pub struct Cue<R> {
    on: fn(&R) -> &str,
    changed: String,
}

/// The column a table watches for the change mark: its place, what of a
/// row it reads there, and the words its marked cell says. `None` where no
/// column carries a [`Cue`].
type Watch<R> = Option<(usize, fn(&R) -> &str, Arc<str>)>;

impl<R> Cue<R> {
    /// A mark on a change of `on`, said `changed` to a screen reader.
    pub fn new(on: fn(&R) -> &str, changed: impl Into<String>) -> Self {
        Self {
            on,
            changed: changed.into(),
        }
    }
}

/// The change marks of a live table (decision 129): the rows a change has
/// marked that the reader hasn't seen yet. The page makes them and hands
/// them to its [`Table`], which marks its rows and sees one as focus enters
/// it, and to the [`MarkAllSeen`] it places in its own row of actions.
pub struct ChangeMarks<K: Send + Sync + 'static> {
    marks: RwSignal<Marks<K>>,
}

impl<K: Send + Sync + 'static> Clone for ChangeMarks<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: Send + Sync + 'static> Copy for ChangeMarks<K> {}

impl<K: Eq + Hash + Send + Sync + 'static> ChangeMarks<K> {
    /// No row marked.
    pub fn new() -> Self {
        Self {
            marks: RwSignal::new(Marks::default()),
        }
    }
}

impl<K: Eq + Hash + Send + Sync + 'static> Default for ChangeMarks<K> {
    fn default() -> Self {
        Self::new()
    }
}

/// The button that marks every row of a live table as seen ("Mark all as
/// seen", its `label`), which the page places in its row of actions, before
/// the table (decision 129). It is always there and hidden while no row is
/// marked, its room kept (`invisible`, and `inert`: no Tab stop, out of the
/// accessibility tree), so its coming and going moves nothing on the page,
/// however the row wraps. Its press clears `marks`, says how many it
/// cleared in the caller's `said` sentence, politely, and, as the button
/// hides, hands focus to `then_focus`, the `id` of something that stays,
/// such as the page's heading (2.4.3). When the last mark goes without its
/// press (its rows leave, as when a refusal takes the table's place), focus
/// on the button goes there too, before it hides, and nothing is said.
#[component]
pub fn MarkAllSeen<K>(
    marks: ChangeMarks<K>,
    label: String,
    said: Callback<usize, String>,
    then_focus: String,
) -> impl IntoView
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
{
    let marks = marks.marks;
    let hidden = RwSignal::new(true);
    let holder = NodeRef::<leptos::html::Span>::new();
    let after_press = then_focus.clone();
    // `hidden` follows the marks here rather than in a memo, so focus leaves
    // the button before `inert` would drop it to the page's body.
    Effect::new(move |_| {
        let none = !marks.with(Marks::any);
        if none
            && holder
                .get_untracked()
                .is_some_and(|holder| holds_focus_within(&holder))
        {
            focus_element(&then_focus);
        }
        hidden.set(none);
    });
    let press = Callback::new(move |()| {
        let seen = marks.try_update(Marks::see_all).unwrap_or_default();
        reply(Politeness::Polite, &said.run(seen));
        focus_element(&after_press);
    });
    // Its own flex box, centred in the row's line, as the row of actions
    // (`ACTIONS`, its `[&>.button]:self-center`) stands a button that is
    // the row's own item.
    view! {
        <span
            node_ref=holder
            class="flex items-center"
            class:invisible=hidden
            inert=move || hidden.get().then_some("")
        >
            <Button content=Content::Text(Signal::stored(label)) on_press=press />
        </span>
    }
}

/// The sentences that take the table's place while it can't show its rows,
/// the caller's, since only it knows what the rows are.
pub struct Sentences {
    /// Said beside a spinner while the rows are on their way, just before
    /// the busy region where the table will stand and outside it, since a
    /// screen reader may leave a busy region unread (§4.5).
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
/// `row_header_id`, where given, gives each row's header an `id` and lets a
/// script focus it (`tabindex="-1"`, no stop for Tab), so a control that
/// leaves the row can hand focus to the header that names it.
///
/// A column given a [`Cue`] ([`Column::cued`]) is what a live update can
/// change in a row, such as its status. A row whose value there changes
/// while it shows is marked "changed, not yet seen" in `marks` (decision
/// 129): that cell reads in bold, an edge stands at the row's start, in the
/// `--cue` color (`style.css`), and the cell says the cue's words to a
/// screen reader after its own, so the mark is never drawn alone (1.3.1).
/// The mark arrives once with a fade, its cells lit in the cue's fill and
/// settling back to their own over 600 ms (decision 130), and stays until
/// it is seen: focus enters the row, the reader presses the page's
/// [`MarkAllSeen`], or the row or the page goes. A change while focus is in
/// the row is seen as it comes. Its first sight, and an update that leaves
/// it as it was, mark nothing ([`rules::Marks`]); a page whose rows a pause
/// holds still marks nothing meanwhile, and its resume marks each changed
/// row once. Nothing is announced: the page's announcer says what it says
/// of the change itself. A table given no `marks` keeps its own, which only
/// focus sees.
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
///   apart by it: a row whose key an earlier row has doesn't show (#138).
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
    #[prop(optional)] row_header_id: Option<fn(&R) -> String>,
    #[prop(optional)] marks: Option<ChangeMarks<K>>,
    #[prop(optional)] columns_choice: Option<ColumnsChoice>,
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
    let whole = row_header.whole;
    let columns: Arc<Vec<Column<R>>> =
        Arc::new(std::iter::once(row_header).chain(columns).collect());
    // The columns after the row header's, by the caller's place: their ids,
    // and the layout the reader chose of them (decision 140), kept where the
    // table has a Columns panel.
    let ids: Arc<[&'static str]> = columns.iter().skip(1).map(|column| column.id).collect();
    let choice = columns_choice.map(|choice| (kept_columns(choice.id), choice));
    let layout = {
        let ids = Arc::clone(&ids);
        let kept = choice.as_ref().map(|(kept, _)| kept.clone());
        Memo::new(move |_| match &kept {
            Some(kept) => kept.with(|kept| Layout::from_kept(kept.as_deref(), &ids)),
            None => Layout::given(ids.len()),
        })
    };
    // The table's places (the row header's 0) of the columns it shows after
    // the row header's, in their order; placed once the focus a moving
    // column's cells may hold is noted (below).
    let shown_columns = Memo::new(move |_| {
        layout.with(|layout| {
            layout
                .shown()
                .into_iter()
                .map(|column| column + 1)
                .collect::<Vec<_>>()
        })
    });
    let placed_columns = RwSignal::new(shown_columns.get_untracked());
    let widths: Arc<[Rem]> = columns.iter().map(|column| column.width).collect();
    let width = Signal::derive({
        let widths = Arc::clone(&widths);
        move || {
            placed_columns.with(|placed| {
                rules::table_width(
                    pinned,
                    whole,
                    placed.iter().filter_map(|at| widths.get(*at).copied()),
                )
            })
        }
    });
    let pinned_room = rules::pinned_room(pinned, whole);
    let sorted = RwSignal::new(None::<Sorted>);
    // Hiding the column the rows are sorted by ends the sort.
    Effect::new(move |_| {
        let kept = layout.with(|layout| rules::sort_kept(sorted.get_untracked(), layout));
        if kept != sorted.get_untracked() {
            sorted.set(kept);
        }
    });
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
    let index = Memo::new(move |_| rows.with(|rows| rules::positions(rows, key)));
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
    // A column that moves takes its cells with it, as a row does, so a
    // control of the body that had focus gets it back: a pointer's drag in
    // the Columns panel leaves focus where it was.
    Effect::new(move |_| {
        let next = shown_columns.get();
        let focused = focused_in(body);
        placed_columns.set(next);
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
    // The column a live update changes, what of a row it watches there, and
    // what its marked cell says to a screen reader.
    let watch: Watch<R> = columns.iter().enumerate().find_map(|(at, column)| {
        column
            .cue
            .as_ref()
            .map(|cue| (at, cue.on, Arc::from(cue.changed.as_str())))
    });
    let marks = marks.unwrap_or_default().marks;
    // Whether the table shows the column a live update changes: while it
    // doesn't, a change marks nothing, and hiding it sees every mark, so no
    // row keeps an edge its words no longer say (1.3.1).
    let cue_shown = {
        let watched = watch.as_ref().map(|(at, _, _)| *at);
        Memo::new(move |_| {
            watched
                .and_then(|at| at.checked_sub(1))
                .is_none_or(|column| layout.with(|layout| layout.is_shown(column)))
        })
    };
    Effect::new(move |_| {
        if !cue_shown.get() {
            marks.try_maybe_update(|marks| {
                let any = marks.any();
                marks.see_all();
                (any, ())
            });
        }
    });
    let panel = choice.map(|(kept, choice)| {
        columns_panel(ColumnsPanel {
            table: id,
            choice,
            kept,
            ids,
            headers: columns
                .iter()
                .skip(1)
                .map(|column| column.header.clone())
                .collect(),
            layout,
            // A caption names the table only while it shows.
            described_by: {
                let (frame_name, captioned) = (frame_name.clone(), caption.is_some());
                Signal::derive(move || {
                    (!captioned || shown.get() == Shown::Table).then(|| frame_name.clone())
                })
            },
        })
    });
    view! {
        <div>
            {panel}
            {move || {
                (shown.get() == Shown::Loading).then(|| view! { <Spinner label=loading.clone() /> })
            }}
            <div aria-busy=move || {
                (shown.get() == Shown::Loading).then_some("true")
            }>
                {move || match shown.get() {
                    Shown::Loading => ().into_any(),
                    Shown::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
                    Shown::Empty => view! { <p>{empty.clone()}</p> }.into_any(),
                    Shown::Table => {
                        let columns = Arc::clone(&row_columns);
                        let watch = watch.clone();
                        let column_places = placed_columns.read_only();
                        let header_row = header_row(Arc::clone(&columns), sorted, column_places);
                        let footer_row = footer
                            .clone()
                            .map(|footer| footer_row(Arc::clone(&columns), footer, column_places));
                        let widths = Arc::clone(&widths);
                        let drawn = Drawn {
                            columns: Arc::clone(&columns),
                            placed: column_places,
                            cue_shown,
                        };
                        view! {
                            // The frame is the size container the table's width
                            // is measured against, and what scrolls. It is the
                            // containing block (`relative`) of every box a cell
                            // places absolutely, as the words a cell keeps out
                            // of sight are: such a box escapes the clip of each
                            // box that isn't positioned, so it would lie past
                            // the page's edge and widen the page (1.4.10).
                            <div
                                class="data-table-frame relative"
                                role="region"
                                aria-labelledby=frame_name.clone()
                                tabindex="0"
                                style:scroll-padding-inline-start=pinned_room.clone()
                            >
                                <table
                                    class="data-table"
                                    aria-labelledby=labelled_by.clone()
                                    style:inline-size=move || width.get()
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
                                        <col style:inline-size=rules::row_header_width(
                                            pinned,
                                            whole,
                                        ) />
                                        <For
                                            each=move || placed_columns.get()
                                            key=|at| *at
                                            children=move |at| {
                                                let width = widths
                                                    .get(at)
                                                    .map(ToString::to_string)
                                                    .unwrap_or_default();
                                                view! { <col style:inline-size=width /> }
                                            }
                                        />
                                    </colgroup>
                                    {header_row}
                                    <tbody node_ref=body>
                                        <For
                                            each=move || placed.get()
                                            key=|key| key.clone()
                                            children=move |key| {
                                                row_view(
                                                    &drawn,
                                                    rows,
                                                    index,
                                                    key,
                                                    row_header_id,
                                                    watch.clone(),
                                                    marks,
                                                )
                                            }
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
        </div>
    }
    .into_any()
}

/// The header row: the row header's words, then each shown column's
/// (`placed`), in their order, and a sortable column's button, whose press
/// sorts by it and says so.
fn header_row<R: Send + Sync + 'static>(
    columns: Arc<Vec<Column<R>>>,
    sorted: RwSignal<Option<Sorted>>,
    placed: ReadSignal<Vec<usize>>,
) -> AnyView {
    let first = columns.first().map(|column| header_cell(column, 0, sorted));
    view! {
        <thead>
            <tr>
                {first}
                <For
                    each=move || placed.get()
                    key=|at| *at
                    children=move |at| columns.get(at).map(|column| header_cell(column, at, sorted))
                />
            </tr>
        </thead>
    }
    .into_any()
}

/// The header cell of `column`, the table's place `at` (the row header's 0).
fn header_cell<R: Send + Sync + 'static>(
    column: &Column<R>,
    at: usize,
    sorted: RwSignal<Option<Sorted>>,
) -> AnyView {
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
            let first = sort.first;
            let press = Callback::new(move |()| {
                let next = rules::press(sorted.get_untracked(), at, first);
                sorted.set(Some(next));
                reply(
                    Politeness::Polite,
                    rules::sentence(next.direction, &ascending, &descending),
                );
            });
            view! {
                <th scope="col" class=class aria-sort=move || rules::aria_sort(sorted.get(), at)>
                    <span class="sort-header">
                        {header}
                        <Button
                            content=Content::IconAndText {
                                glyph: Signal::derive(move || rules::glyph(sorted.get(), at)),
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
}

/// The footer row: its header, then a cell for each shown column (`placed`),
/// in their order, each aligned as its column; a column the footer has no
/// cell for gets an empty one, so each column keeps its rule.
fn footer_row<R: Send + Sync + 'static>(
    columns: Arc<Vec<Column<R>>>,
    footer: Footer,
    placed: ReadSignal<Vec<usize>>,
) -> AnyView {
    let class = columns.first().and_then(|column| column.align.class());
    let Footer { header, cells } = footer;
    let cell = move |at: usize| {
        let class = columns.get(at).and_then(|column| column.align.class());
        let cell = at.checked_sub(1).and_then(|at| cells.get(at).copied());
        view! { <td class=class>{move || cell.map(|cell| cell.get())}</td> }
    };
    view! {
        <tfoot>
            <tr>
                <th scope="row" class=class>
                    {move || header.get()}
                </th>
                <For each=move || placed.get() key=|at| *at children=cell />
            </tr>
        </tfoot>
    }
    .into_any()
}

/// The columns a row draws: every one, by the table's place (the row
/// header's 0), the places of those it shows after the row header's, in
/// their order, and whether it shows the cued one.
#[derive(Clone)]
struct Drawn<R: Send + Sync + 'static> {
    columns: Arc<Vec<Column<R>>>,
    placed: ReadSignal<Vec<usize>>,
    cue_shown: Memo<bool>,
}

/// One row, built once as it first shows: its cells bind to the row as it
/// stands in `rows` now, or as it last stood while it is on its way out.
/// Where the table watches a column (`watch`, its place, what it reads and
/// what its marked cell says), a change there marks the row in `marks` and
/// lights its arrival.
fn row_view<R, K>(
    drawn: &Drawn<R>,
    rows: Signal<Vec<R>>,
    index: Memo<HashMap<K, usize>>,
    key: K,
    header_id: Option<fn(&R) -> String>,
    watch: Watch<R>,
    marks: RwSignal<Marks<K>>,
) -> AnyView
where
    R: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
{
    let Drawn {
        columns,
        placed,
        cue_shown,
    } = drawn;
    let (placed, cue_shown) = (*placed, *cue_shown);
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
    let id = header_id.map(|id_of| row.with_untracked(id_of));
    let unseen = Memo::new({
        let key = key.clone();
        move |_| marks.with(|marks| marks.is_unseen(&key))
    });
    let header_cell = columns.first().map(|column| {
        let class = column.align.class();
        let cell = (column.draw)(row);
        let tabindex = id.is_some().then_some("-1");
        view! {
            <th scope="row" class=class id=id.clone() tabindex=tabindex>
                {cell}
            </th>
        }
    });
    let cell = {
        let columns = Arc::clone(columns);
        let watched = watch
            .as_ref()
            .map(|(at, _, words)| (*at, Arc::clone(words)));
        move |at: usize| {
            let column = columns.get(at)?;
            let class = column.align.class();
            let cell = (column.draw)(row);
            Some(
                if let Some((_, words)) = watched.as_ref().filter(|(watched, _)| *watched == at) {
                    // The mark's words after the cell's own, out of sight, while
                    // the row is marked: its name then says it (1.3.1). The frame
                    // that scrolls holds their box, as it does every out-of-sight
                    // box in a cell.
                    let words = Arc::clone(words);
                    view! {
                        <td class=class data-cued="">
                            {cell}
                            {move || {
                                unseen
                                    .get()
                                    .then(|| {
                                        view! {
                                            {" "}
                                            <span class="sr-only">{words.to_string()}</span>
                                        }
                                    })
                            }}
                        </td>
                    }
                    .into_any()
                } else {
                    view! { <td class=class>{cell}</td> }.into_any()
                },
            )
        }
    };
    let arrival = RwSignal::new(Arrival::Settled);
    let line = NodeRef::<leptos::html::Tr>::new();
    let cued = watch.is_some();
    if let Some(on) = watch.map(|(_, on, _)| on) {
        // What the row shows there, owned anew only when it changes: a
        // progress report updates the row but not its status.
        let watched = Memo::new_owning(move |shown: Option<String>| {
            row.with(|row| match shown {
                Some(shown) if shown == on(row) => (shown, false),
                _ => (on(row).to_owned(), true),
            })
        });
        let changed = key.clone();
        Effect::new(move |shown: Option<String>| {
            let now = watched.get();
            let focused = line
                .get_untracked()
                .is_some_and(|line| holds_focus_within(&line));
            let cue_shown = cue_shown.get_untracked();
            let seen = rules::seen_as_it_comes(focused, cue_shown);
            // The marks' watchers hear of it only when a row is marked.
            let lights = marks
                .try_maybe_update(|marks| {
                    let lights = marks.change(&changed, shown.as_ref(), &now, seen);
                    (lights && !seen, lights)
                })
                .unwrap_or_default();
            // A hidden column's change lights nothing a cell could explain.
            if lights && cue_shown {
                light(arrival);
            }
            now
        });
        let gone = key.clone();
        on_cleanup(move || {
            marks.try_maybe_update(|marks| (marks.gone(&gone), ()));
        });
    }
    let state = move || cued.then(|| arrival.get().name());
    // The cells' fade ends, or a move of the row or a second change cuts it
    // short; a transition inside a cell (a progress bar's) isn't the cue's.
    let settle = move |event: web_sys::TransitionEvent| {
        if arrival.get_untracked() == Arrival::Settling && ends_the_cue(&event) {
            arrival.set(Arrival::Settled);
        }
    };
    // Focus entering the row sees its mark.
    let see = move |_: web_sys::FocusEvent| {
        if unseen.get_untracked() {
            marks.update(|marks| {
                marks.see(&key);
            });
        }
    };
    view! {
        <tr
            node_ref=line
            data-cue=state
            data-unseen=move || (unseen.get() && cue_shown.get()).then_some("")
            on:transitionend=settle
            on:transitioncancel=settle
            on:focusin=see
        >
            {header_cell}
            <For each=move || placed.get() key=|at| *at children=cell />
        </tr>
    }
    .into_any()
}

/// Where a row's arrival fade is (`data-cue`, which `style.css` draws): lit
/// in the cue's fill, settling back to its own fill, or settled. Only a
/// settling row's cells have a transition, so a change of the theme's
/// colors reaches a settled row at once, as it does the rest of the page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrival {
    Lit,
    Settling,
    Settled,
}

impl Arrival {
    fn name(self) -> &'static str {
        match self {
            Self::Lit => "lit",
            Self::Settling => "settling",
            Self::Settled => "settled",
        }
    }
}

/// Lights a row for its arrival fade, and lets it settle two frames on: the
/// first frame draws it lit, so at the second its cells' fill has a lit
/// color to settle back from over the transition `style.css` gives them. In
/// a tab the reader isn't looking at, the frames wait, and the row stays
/// lit until the tab shows again.
fn light(arrival: RwSignal<Arrival>) {
    arrival.set(Arrival::Lit);
    request_animation_frame(move || {
        request_animation_frame(move || {
            if arrival.try_get_untracked() == Some(Arrival::Lit) {
                arrival.set(Arrival::Settling);
            }
        });
    });
}

/// Whether focus is on `node` or inside it.
fn holds_focus_within(node: &web_sys::Node) -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .is_some_and(|active| node.contains(Some(&active)))
}

/// Whether a transition's event is the change cue's: a cell's fill, the
/// cell one of the row's own.
fn ends_the_cue(event: &web_sys::TransitionEvent) -> bool {
    let row = event
        .current_target()
        .and_then(|row| row.dyn_into::<web_sys::Node>().ok());
    event.property_name() == "background-color"
        && event
            .target()
            .and_then(|cell| cell.dyn_into::<web_sys::Node>().ok())
            .and_then(|cell| cell.parent_node())
            .is_some_and(|parent| parent.is_same_node(row.as_ref()))
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

/// The columns this browser keeps for the table `id`, as one signal every
/// table of that id shares, read from the browser's local storage the first
/// time a page asks. Outside input: [`Layout::from_kept`] reads it.
fn kept_columns(id: &'static str) -> ArcRwSignal<Option<String>> {
    KEPT.with(|kept| {
        kept.borrow_mut()
            .entry(id)
            .or_insert_with(|| {
                ArcRwSignal::new(storage().and_then(|storage| {
                    storage.get_item(&format!("{KEPT_KEY}{id}")).ok().flatten()
                }))
            })
            .clone()
    })
}

/// Keeps `value` as the columns of the table `id` in this browser, or
/// forgets them for none. A browser that refuses its storage keeps the
/// choice for as long as the page stays open.
fn keep_columns(id: &str, value: Option<&str>) {
    let Some(storage) = storage() else {
        return;
    };
    let key = format!("{KEPT_KEY}{id}");
    let _ = match value {
        Some(value) => storage.set_item(&key, value),
        None => storage.remove_item(&key),
    };
}

/// What a table's Columns panel draws from: the table's number, its
/// [`ColumnsChoice`], the choice kept, the ids and headers of the columns
/// after the row header's by the caller's place, their layout, and the
/// `id` of what names the table, which describes the panel's button.
struct ColumnsPanel {
    table: u32,
    choice: ColumnsChoice,
    kept: ArcRwSignal<Option<String>>,
    ids: Arc<[&'static str]>,
    headers: Arc<[String]>,
    layout: Memo<Layout>,
    described_by: Signal<Option<String>>,
}

/// The Columns panel (decision 140): a disclosure button, "Columns",
/// described by the table's name, and the panel it shows, a list named by
/// the button with an item for each column after the row header's, in the
/// table's order, then "Reset columns". Each item has a checkbox that shows
/// or hides its column and Move buttons that move it a place, unavailable
/// at the ends, focus staying on the button pressed until its column can't
/// move that way, then going to the other; a pointer may drag the item by
/// its handle instead, which a screen reader skips. Each change is said
/// politely, and kept. Escape closes the panel, focus back on its button,
/// once no tooltip took it.
fn columns_panel(panel: ColumnsPanel) -> AnyView {
    let ColumnsPanel {
        table,
        choice: ColumnsChoice { id, localizer },
        kept,
        ids,
        headers,
        layout,
        described_by,
    } = panel;
    let count = ids.len();
    let button_id = format!("table-{table}-columns");
    let panel_id = format!("{button_id}-panel");
    let open = RwSignal::new(false);
    let keep = Callback::new(move |next: Layout| {
        let value = next.to_keep(&ids);
        keep_columns(id, value.as_deref());
        kept.set(value);
    });
    let header_of = {
        let headers = Arc::clone(&headers);
        move |column: usize| headers.get(column).cloned().unwrap_or_default()
    };
    let say = {
        let localizer = Arc::clone(&localizer);
        Callback::new(
            move |(message, column, position): (l10n::MessageId, String, usize)| {
                let as_count = |n: usize| MsgArg::Count(u64::try_from(n).unwrap_or(u64::MAX));
                reply(
                    Politeness::Polite,
                    &text_with(
                        &localizer,
                        message,
                        &[
                            ("column", MsgArg::Text(column)),
                            ("position", as_count(position)),
                            ("count", as_count(count)),
                        ],
                    ),
                );
            },
        )
    };
    // A column moved: kept, and said with its place now, from 1.
    let moved = {
        let header_of = header_of.clone();
        Callback::new(move |(column, next): (usize, Layout)| {
            let position = next.position(column).map_or(0, |at| at + 1);
            keep.run(next);
            say.run((msg::UI_TABLE_COLUMNS_MOVED, header_of(column), position));
        })
    };
    // A pointer's drag of an item by its handle: the column it holds, and
    // the gap of the list it stands at.
    let dragging = RwSignal::new(None::<(usize, i32)>);
    let gap = RwSignal::new(None::<usize>);
    let list = NodeRef::<leptos::html::Ul>::new();
    let item = {
        let localizer = Arc::clone(&localizer);
        let button_id = button_id.clone();
        move |column: usize| {
            let header = header_of(column);
            let t = |message, header: &str| {
                text_with(
                    &localizer,
                    message,
                    &[("column", MsgArg::Text(header.to_owned()))],
                )
            };
            let (up_name, down_name) = (
                t(msg::UI_TABLE_COLUMNS_MOVE_UP, &header),
                t(msg::UI_TABLE_COLUMNS_MOVE_DOWN, &header),
            );
            let up_id = format!("{button_id}-{column}-up");
            let down_id = format!("{button_id}-{column}-down");
            let checked = RwSignal::new(layout.with_untracked(|layout| layout.is_shown(column)));
            // The box follows the layout (a reset), and the layout the box.
            Effect::new(move |_| {
                let shown = layout.with(|layout| layout.is_shown(column));
                if checked.get_untracked() != shown {
                    checked.set(shown);
                }
            });
            {
                let header = header.clone();
                Effect::watch(
                    move || checked.get(),
                    move |shown, _, _| {
                        let now = layout.get_untracked();
                        if now.is_shown(column) == *shown {
                            return;
                        }
                        keep.run(now.showing(column, *shown));
                        let message = if *shown {
                            msg::UI_TABLE_COLUMNS_SHOWN
                        } else {
                            msg::UI_TABLE_COLUMNS_HIDDEN
                        };
                        say.run((message, header.clone(), 0));
                    },
                    false,
                );
            }
            let step = move |step: Step, pressed: String, other: String| {
                Callback::new(move |()| {
                    let now = layout.get_untracked();
                    if !now.can_move(column, step) {
                        return;
                    }
                    let next = now.moved(column, step);
                    let to = if next.can_move(column, step) {
                        pressed.clone()
                    } else {
                        other.clone()
                    };
                    moved.run((column, next));
                    // The item moved, and its button lost focus with it.
                    request_animation_frame(move || {
                        focus_element(&to);
                    });
                })
            };
            let up = step(Step::Up, up_id.clone(), down_id.clone());
            let down = step(Step::Down, down_id.clone(), up_id.clone());
            let grab = move |event: web_sys::PointerEvent| {
                // One pointer drags at a time: a second one pressed meanwhile
                // moves nothing.
                if event.button() != 0 || dragging.get_untracked().is_some() {
                    return;
                }
                event.prevent_default();
                // On what the pointer pressed, inside the handle, so its
                // moves and its release come to the list however far it
                // goes (the page's delegated listeners make the handle no
                // `current_target`).
                if let Some(handle) = event
                    .target()
                    .and_then(|handle| handle.dyn_into::<web_sys::Element>().ok())
                {
                    let _ = handle.set_pointer_capture(event.pointer_id());
                }
                dragging.set(Some((column, event.pointer_id())));
                gap.set(None);
            };
            // Where a drop would place the dragged column: a line before
            // this item, or after it as the last; none where it stays put.
            let drop_mark = move || {
                let (dragged, gap) = (dragging.get()?.0, gap.get()?);
                layout.with(|layout| {
                    let from = layout.position(dragged)?;
                    let at = layout.position(column)?;
                    if rules::drop_place(from, gap) == from {
                        None
                    } else if gap == at {
                        Some("before")
                    } else if gap == at + 1 && gap == layout.order().len() {
                        Some("after")
                    } else {
                        None
                    }
                })
            };
            view! {
                // Where the panel has room (`@md`, in rem, so larger text
                // narrows it too), a row of the list's grid
                // (`grid-cols-subgrid`): every label shares one column and
                // the Move buttons line up however long a column's name is.
                // Without that room, as at 320 px with 200 % text, the row
                // wraps its buttons under the label instead (1.4.10). The
                // checkbox's own block margin is a form's spacing between
                // fields; in a list of rows it would open a gap above and
                // below each, so it goes.
                <li
                    class="group relative flex flex-wrap items-center gap-x-1 py-0.5 @md:col-span-4 @md:grid @md:grid-cols-subgrid [&>div]:my-0"
                    data-drop=drop_mark
                >
                    // The drop line: shown only where a drop would land, so
                    // forced colors, which paint every border drawn, draw
                    // no other.
                    <span
                        aria-hidden="true"
                        class="pointer-events-none absolute inset-0 hidden border-accent group-data-[drop=before]:block group-data-[drop=before]:border-bs-2 group-data-[drop=after]:block group-data-[drop=after]:border-be-2"
                    ></span>
                    <span
                        data-drag-handle=""
                        aria-hidden="true"
                        class="inline-flex min-inline-6 min-block-6 items-center justify-center cursor-grab touch-none text-muted"
                        on:pointerdown=grab
                    >
                        <Icon glyph=LucideGlyph::MoveVertical />
                    </span>
                    <Checkbox id=format!("{button_id}-{column}") label=header checked=checked />
                    <Tooltip tip=Tip::Names(up_name.clone())>
                        <Button
                            id=up_id
                            content=Content::IconOnly {
                                glyph: LucideGlyph::ArrowUp.into(),
                                label: up_name.into(),
                            }
                            appearance=Appearance::Ghost
                            size=Size::Small
                            disabled=Signal::derive(move || {
                                !layout.with(|layout| layout.can_move(column, Step::Up))
                            })
                            on_press=up
                        />
                    </Tooltip>
                    <Tooltip tip=Tip::Names(down_name.clone())>
                        <Button
                            id=down_id
                            content=Content::IconOnly {
                                glyph: LucideGlyph::ArrowDown.into(),
                                label: down_name.into(),
                            }
                            appearance=Appearance::Ghost
                            size=Size::Small
                            disabled=Signal::derive(move || {
                                !layout.with(|layout| layout.can_move(column, Step::Down))
                            })
                            on_press=down
                        />
                    </Tooltip>
                </li>
            }
        }
    };
    // The list hears the dragging pointer, held by the handle it pressed;
    // another pointer's moves and release are not the drag's.
    let held = move |event: &web_sys::PointerEvent| {
        dragging
            .get_untracked()
            .filter(|(_, pointer)| *pointer == event.pointer_id())
            .map(|(column, _)| column)
    };
    let drag = move |event: web_sys::PointerEvent| {
        if held(&event).is_none() {
            return;
        }
        if let Some(list) = list.get_untracked() {
            gap.set(Some(rules::gap_at(
                &middles(&list),
                f64::from(event.client_y()),
            )));
        }
    };
    let drop = move |event: web_sys::PointerEvent| {
        let Some(dragged) = held(&event) else {
            return;
        };
        let at_gap = gap.get_untracked();
        dragging.set(None);
        gap.set(None);
        let Some(at_gap) = at_gap else {
            return;
        };
        let now = layout.get_untracked();
        let Some(from) = now.position(dragged) else {
            return;
        };
        let to = rules::drop_place(from, at_gap);
        if to != from {
            moved.run((dragged, now.moved_to(dragged, to)));
        }
    };
    let cancel = move |event: web_sys::PointerEvent| {
        if held(&event).is_some() {
            dragging.set(None);
            gap.set(None);
        }
    };
    let reset = {
        let localizer = Arc::clone(&localizer);
        Callback::new(move |()| {
            keep.run(Layout::given(count));
            reply(
                Politeness::Polite,
                &text(&localizer, msg::UI_TABLE_COLUMNS_WAS_RESET),
            );
        })
    };
    // After the window's own listeners: an Escape that hid a tooltip did
    // its work, as a dialog's does.
    let close = {
        let button_id = button_id.clone();
        move |event: web_sys::KeyboardEvent| {
            if event.key() != "Escape" || !open.get_untracked() {
                return;
            }
            let button_id = button_id.clone();
            set_timeout(
                move || {
                    if !event.default_prevented() {
                        open.set(false);
                        focus_element(&button_id);
                    }
                },
                Duration::ZERO,
            );
        }
    };
    view! {
        <div class="@container flex flex-col items-start gap-2 mbe-2" on:keydown=close>
            <Button
                id=button_id.clone()
                content=Content::IconAndText {
                    glyph: Signal::derive(move || {
                        if open.get() { LucideGlyph::ChevronUp } else { LucideGlyph::ChevronDown }
                    }),
                    label: Signal::stored(text(&localizer, msg::UI_TABLE_COLUMNS)),
                }
                appearance=Appearance::Ghost
                size=Size::Small
                expanded=open
                controls=panel_id.clone()
                described_by=described_by
                on_press=Callback::new(move |()| open.update(|open| *open = !*open))
            />
            // The panel's own box shows (`hidden`); its flex box is inside.
            <div id=panel_id hidden=move || !open.get() class="border border-muted p-3">
                <div class="flex flex-col items-start gap-3">
                    // Preflight takes its markers, and with them Safari's
                    // list role, which the list's name and count need.
                    <ul
                        node_ref=list
                        role="list"
                        aria-labelledby=button_id
                        class="m-0 flex flex-col ps-0 @md:grid @md:grid-cols-[auto_1fr_auto_auto] @md:items-center @md:gap-x-1"
                        on:pointermove=drag
                        on:pointerup=drop
                        on:pointercancel=cancel
                    >
                        <For
                            each=move || layout.with(|layout| layout.order().to_vec())
                            key=|column| *column
                            children=item
                        />
                    </ul>
                    <Button
                        content=Content::Text(
                            Signal::stored(text(&localizer, msg::UI_TABLE_COLUMNS_RESET)),
                        )
                        size=Size::Small
                        on_press=reset
                    />
                </div>
            </div>
        </div>
    }
    .into_any()
}

/// The middles of a list's items from the top of the viewport, in their
/// order, for [`rules::gap_at`].
fn middles(list: &web_sys::Element) -> Vec<f64> {
    let items = list.children();
    (0..items.length())
        .filter_map(|at| items.item(at))
        .map(|item| {
            let rect = item.get_bounding_client_rect();
            rect.top() + rect.height() / 2.0
        })
        .collect()
}
