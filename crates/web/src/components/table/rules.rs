//! What a table decides (components §4.5), on the host: the order its rows
//! take and how a press on a sort button changes it, what its headers tell a
//! screen reader (`aria-sort`) and show (the direction's glyph), which
//! sentence a sort is said with, what shows in the table's place, how a
//! cell is aligned and how wide the columns are. `components/table.rs`'s
//! `Table` is the glue that draws it.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use lepticons_data::LucideGlyph;

use crate::variants::Rem;

/// Which way a column is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Smallest, earliest or first in the alphabet first.
    Ascending,
    /// Largest, latest or last in the alphabet first.
    Descending,
}

impl Direction {
    /// The other way.
    fn flipped(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// The column a table is sorted by, counted from the row header's (0), and
/// which way. A table that isn't sorted shows its rows in the order its
/// caller gave them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sorted {
    pub column: usize,
    pub direction: Direction,
}

/// The sort after a press on the sort button of `column`, whose first press
/// sorts it `first` (ascending, or descending for a column read newest
/// first): the other way if the table is already sorted by that column, and
/// `first` if it is sorted by another or not at all.
pub fn press(sorted: Option<Sorted>, column: usize, first: Direction) -> Sorted {
    let direction = match sorted {
        Some(Sorted {
            column: last,
            direction,
        }) if last == column => direction.flipped(),
        _ => first,
    };
    Sorted { column, direction }
}

/// The `aria-sort` of `column`'s header: `ascending` or `descending` on the
/// column the table is sorted by, and none on any other, as the WAI-ARIA
/// table pattern asks (one sorted column at a time).
pub fn aria_sort(sorted: Option<Sorted>, column: usize) -> Option<&'static str> {
    match sorted? {
        Sorted {
            column: by,
            direction: Direction::Ascending,
        } if by == column => Some("ascending"),
        Sorted {
            column: by,
            direction: Direction::Descending,
        } if by == column => Some("descending"),
        _ => None,
    }
}

/// The glyph of `column`'s sort button, decoration beside its words: up and
/// down while the table isn't sorted by it, up while it is sorted ascending,
/// down while descending.
pub fn glyph(sorted: Option<Sorted>, column: usize) -> LucideGlyph {
    match sorted {
        Some(Sorted {
            column: by,
            direction: Direction::Ascending,
        }) if by == column => LucideGlyph::ArrowUp,
        Some(Sorted {
            column: by,
            direction: Direction::Descending,
        }) if by == column => LucideGlyph::ArrowDown,
        _ => LucideGlyph::ArrowUpDown,
    }
}

/// Which of a column's two sentences says a sort `direction`, such as
/// "Sorted by size, smallest first." and "Sorted by size, largest first.".
pub fn sentence<'a>(direction: Direction, ascending: &'a str, descending: &'a str) -> &'a str {
    match direction {
        Direction::Ascending => ascending,
        Direction::Descending => descending,
    }
}

/// What a column is sorted by in a row: a number (a size in bytes, a
/// count), a text, compared as its bytes are, so its caller passes a text
/// that orders as the reader expects (a name in lower case), or an instant,
/// compared as the time it is, whatever precision and offset it was written
/// with (#138).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortKey {
    Number(i64),
    Text(String),
    Instant(jiff::Timestamp),
}

/// A sort: which way, and a column's sort key.
pub type SortBy<'a, R> = (Direction, &'a dyn Fn(&R) -> SortKey);

/// The keys of `rows` in the order the table shows them: the order given,
/// or, sorted `by` a direction and a column's sort key, that order, rows
/// whose keys tie keeping the order given (a stable sort), whichever way it
/// runs. A key given again is dropped, its first row kept, since the rows
/// come from the daemon and a keyed list given one key twice panics in the
/// browser (#138). Each row's sort key is read once.
pub fn order<R, K: Clone + Eq + Hash>(
    rows: &[R],
    key: impl Fn(&R) -> K,
    by: Option<SortBy<'_, R>>,
) -> Vec<K> {
    let mut seen = HashSet::with_capacity(rows.len());
    let firsts = rows.iter().filter_map(|row| {
        let key = key(row);
        seen.insert(key.clone()).then_some((row, key))
    });
    let Some((direction, sort_key)) = by else {
        return firsts.map(|(_, key)| key).collect();
    };
    // The rows' sort keys, each with where its row stands among `keys`, are
    // sorted by one function for every row type (#1429).
    let mut keys = Vec::with_capacity(rows.len());
    let mut keyed = Vec::with_capacity(rows.len());
    for (at, (row, key)) in firsts.enumerate() {
        keyed.push((sort_key(row), at));
        keys.push(Some(key));
    }
    sort_keys(&mut keyed, direction);
    keyed
        .into_iter()
        .filter_map(|(_, at)| keys.get_mut(at).and_then(Option::take))
        .collect()
}

/// Sorts `keyed`, each a row's sort key and where the row stands, the way
/// `direction` runs. A stable sort: rows whose keys tie keep the order given,
/// whichever way the keys run. Not generic, so the standard library's sort
/// is built once for every table rather than once for each row type (#1429).
fn sort_keys(keyed: &mut [(SortKey, usize)], direction: Direction) {
    keyed.sort_by(|(a, _), (b, _)| match direction {
        Direction::Ascending => a.cmp(b),
        Direction::Descending => b.cmp(a),
    });
}

/// Where each key of `rows` first stands in them: a key given twice is the
/// first row's, the row the table shows ([`order`]), so its cells read that
/// row (#138).
pub fn positions<R, K: Eq + Hash>(rows: &[R], key: impl Fn(&R) -> K) -> HashMap<K, usize> {
    let mut positions = HashMap::with_capacity(rows.len());
    for (at, row) in rows.iter().enumerate() {
        positions.entry(key(row)).or_insert(at);
    }
    positions
}

/// Where the rows a table shows stand, as its caller knows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// On their way.
    #[default]
    Loading,
    /// They can't be read: the daemon can't be reached.
    Unreachable,
    /// Here, however many there are.
    Ready,
}

/// What shows in the table's place: a sentence for each state the rows can't
/// be shown in, or the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Loading,
    Unreachable,
    Empty,
    Table,
}

/// What shows while the rows are in `state` and there are `rows` of them:
/// only a change of this builds the table again.
pub fn shown(state: State, rows: usize) -> Shown {
    match state {
        State::Loading => Shown::Loading,
        State::Unreachable => Shown::Unreachable,
        State::Ready if rows == 0 => Shown::Empty,
        State::Ready => Shown::Table,
    }
}

/// Whether a row takes the change cue (decision 130): the part of it the
/// table watches (the column given a `Cue`, a status) was `shown` as the row last
/// showed it and is `now`. Only a change of a value the row already showed
/// lights it: a row's first sight (the page's first rows, a row that
/// arrives, the table built again) only seeds it, and an update that leaves
/// the value as it was (a progress report, a re-sort) lights nothing.
pub fn cues<T: PartialEq>(shown: Option<&T>, now: &T) -> bool {
    shown.is_some_and(|shown| shown != now)
}

/// The change marks of a table's rows (decision 129): which rows a live
/// change marked "changed, not yet seen". A mark stays until it is seen:
/// focus enters its row ([`Marks::see`]), the reader marks every row as seen
/// at once ([`Marks::see_all`]), or the row leaves the table
/// ([`Marks::gone`]); leaving the page drops them all, since nothing is
/// stored.
#[derive(Debug)]
pub struct Marks<K> {
    unseen: HashSet<K>,
}

impl<K> Default for Marks<K> {
    fn default() -> Self {
        Self {
            unseen: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> Marks<K> {
    /// The row `key` shows `now` where it last showed `shown`: whether it
    /// takes the change cue's arrival fade, as [`cues`] decides. A row that
    /// takes it is marked, unless focus is in it as it changes (`focused`):
    /// the reader is there, so it is seen as it arrives.
    pub fn change<T: PartialEq>(
        &mut self,
        key: &K,
        shown: Option<&T>,
        now: &T,
        focused: bool,
    ) -> bool {
        let lights = cues(shown, now);
        if lights && !focused {
            self.unseen.insert(key.clone());
        }
        lights
    }

    /// Focus entered the row `key`: its mark, if it had one, is seen.
    /// Whether it had one.
    pub fn see(&mut self, key: &K) -> bool {
        self.unseen.remove(key)
    }

    /// The reader marked every row as seen: how many were marked.
    pub fn see_all(&mut self) -> usize {
        let marked = self.unseen.len();
        self.unseen.clear();
        marked
    }

    /// The row `key` left the table, and its mark with it. Whether it had
    /// one.
    pub fn gone(&mut self, key: &K) -> bool {
        self.unseen.remove(key)
    }

    /// Whether the row `key` is marked, changed and not yet seen.
    pub fn is_unseen(&self, key: &K) -> bool {
        self.unseen.contains(key)
    }

    /// Whether any row is marked, which shows the Mark all as seen button.
    pub fn any(&self) -> bool {
        !self.unseen.is_empty()
    }
}

/// How a column's cells are aligned: at the start of the line, or, for
/// numbers, at its end in figures of one width, so the digits of a column
/// line up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    End,
}

impl Align {
    /// The class of `style.css` a cell, its header's and its footer's take:
    /// none at the start, where every cell already is.
    pub fn class(self) -> Option<&'static str> {
        match self {
            Self::Start => None,
            Self::End => Some("cell-end"),
        }
    }
}

/// The share of its frame a pinned row header takes at most, in percent: a
/// header pinned wider would leave the cells scrolling past it too little
/// room to show a focused control (2.4.11) on a small screen with larger
/// text, where a frame is 8rem wide. Its words break inside it there, unless
/// they must read whole (`whole` below), beside cells that hold no control.
const PINNED_SHARE: u8 = 40;

/// The share of its frame a row header kept whole takes at most, in percent:
/// the cells beside it keep two fifths to be read in, so at 320 px with 200 %
/// text, where the forget review's frame is 180 px, its words break inside it
/// rather than cover every cell.
const WHOLE_SHARE: u8 = 60;

/// The row header column's width: `width`, or, in a frame too narrow for it
/// to leave the others room, its share of the frame (`cqi`: the frame is the
/// size container around the table). Where its header's words must read
/// whole, that share is `whole`, but at most three fifths of the frame.
pub fn row_header_width(width: Rem, whole: Option<Rem>) -> String {
    match whole {
        None => format!("min({width}, {PINNED_SHARE}cqi)"),
        Some(whole) => {
            format!("min({width}, max({PINNED_SHARE}cqi, min({whole}, {WHOLE_SHARE}cqi)))")
        }
    }
}

/// The frame's `scroll-padding-inline-start`: the room the pinned row header
/// covers, so a control focused in a cell scrolled under it is scrolled out
/// from beneath it. A percentage of the frame's own width, as a padding of a
/// scroller's is.
pub fn pinned_room(width: Rem, whole: Option<Rem>) -> String {
    match whole {
        None => format!("min({width}, {PINNED_SHARE}%)"),
        Some(whole) => format!("min({width}, max({PINNED_SHARE}%, min({whole}, {WHOLE_SHARE}%)))"),
    }
}

/// The table's own width: the frame's (`100cqi`), or, where that is less
/// than the row header's and the `columns`' widths added up, those widths,
/// and the frame scrolls.
pub fn table_width(
    row_header: Rem,
    whole: Option<Rem>,
    columns: impl IntoIterator<Item = Rem>,
) -> String {
    let sum = columns
        .into_iter()
        .fold(Rem::tenths(0), Rem::saturating_add);
    format!(
        "max(100cqi, calc({} + {sum}))",
        row_header_width(row_header, whole)
    )
}

/// What names a table: a caption above it, or the page's heading (a
/// `<caption>` saying it again under the heading only repeats it). The frame
/// around the table, a region that scrolls, takes the same name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Name {
    /// A visible caption, its words.
    Caption(String),
    /// The page's heading, by its `id`.
    Heading(String),
}

impl Name {
    /// The words of the table's `<caption>`, if a caption names it.
    pub fn caption(&self) -> Option<&str> {
        match self {
            Self::Caption(words) => Some(words),
            Self::Heading(_) => None,
        }
    }

    /// The table's `aria-labelledby`: the heading's `id`, for a table named
    /// by one. A caption names its table without it.
    pub fn table_labelled_by(&self) -> Option<&str> {
        match self {
            Self::Caption(_) => None,
            Self::Heading(id) => Some(id),
        }
    }

    /// The frame's `aria-labelledby`: the caption's `id`, `caption_id`, or
    /// the heading's.
    pub fn frame_labelled_by<'a>(&'a self, caption_id: &'a str) -> &'a str {
        match self {
            Self::Caption(_) => caption_id,
            Self::Heading(id) => id,
        }
    }
}

/// How a table's columns after its row header's stand, as the reader chose
/// them in its Columns panel (decision 140): their order, and which of
/// them are hidden. A column is told by its place among the columns its
/// caller gives (`0`, the first after the row header's), whatever place
/// the reader has moved it to. Until the reader changes it, the caller's
/// order with every column shown ([`Layout::given`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The caller's places, in the order the table shows them.
    order: Vec<usize>,
    /// Whether each column, by its caller's place, is hidden.
    hidden: Vec<bool>,
}

/// Which way a Move button moves its column in the Columns panel's list:
/// up, toward the row header, or down, toward the table's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up,
    Down,
}

/// The most entries a kept layout is read with: more than any table has
/// columns, so a longer value is no layout Ladderback kept.
const KEPT_ENTRIES_MAX: usize = 64;

/// The longest id a kept layout may name, in bytes.
const ID_BYTES_MAX: usize = 64;

/// What marks a hidden column's id in a kept layout.
const HIDDEN_MARK: char = '!';

/// Whether `id` is one a column could have: lower-case ASCII letters,
/// digits and hyphens, at most [`ID_BYTES_MAX`] of them.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_BYTES_MAX
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl Layout {
    /// The caller's order of its `count` columns, every one shown.
    pub fn given(count: usize) -> Self {
        Self {
            order: (0..count).collect(),
            hidden: vec![false; count],
        }
    }

    /// The layout this browser `kept` for a table whose columns' ids are
    /// `ids`, in the caller's order. What the browser kept is outside
    /// input: an id no column has any more drops, a column it doesn't name
    /// (one a later release adds) comes after the others, shown, in the
    /// caller's order, and a value that isn't one Ladderback keeps (an id
    /// named twice, words that are no id, more than the cap) is the given
    /// layout.
    pub fn from_kept(kept: Option<&str>, ids: &[&str]) -> Self {
        Self::read(kept, ids).unwrap_or_else(|| Self::given(ids.len()))
    }

    /// The layout `kept` holds, or `None` where it holds none.
    fn read(kept: Option<&str>, ids: &[&str]) -> Option<Self> {
        let kept = kept?;
        let mut layout = Self {
            order: Vec::with_capacity(ids.len()),
            hidden: vec![false; ids.len()],
        };
        for (read, entry) in kept.split(',').enumerate() {
            if read >= KEPT_ENTRIES_MAX {
                return None;
            }
            let (hidden, id) = match entry.strip_prefix(HIDDEN_MARK) {
                Some(id) => (true, id),
                None => (false, entry),
            };
            if !is_id(id) {
                return None;
            }
            let Some(column) = ids.iter().position(|known| *known == id) else {
                continue;
            };
            if layout.order.contains(&column) {
                return None;
            }
            layout.order.push(column);
            if let Some(slot) = layout.hidden.get_mut(column) {
                *slot = hidden;
            }
        }
        for column in 0..ids.len() {
            if !layout.order.contains(&column) {
                layout.order.push(column);
            }
        }
        Some(layout)
    }

    /// What the browser keeps for the layout of a table whose columns' ids
    /// are `ids`: each id in its order, a hidden one after a `!`, between
    /// commas (`size,!changed,files`), or nothing for the given layout,
    /// which forgets the one kept.
    pub fn to_keep(&self, ids: &[&str]) -> Option<String> {
        if *self == Self::given(ids.len()) {
            return None;
        }
        let entries: Vec<String> = self
            .order
            .iter()
            .filter_map(|&column| {
                let id = ids.get(column)?;
                Some(if self.is_shown(column) {
                    (*id).to_owned()
                } else {
                    format!("{HIDDEN_MARK}{id}")
                })
            })
            .collect();
        Some(entries.join(","))
    }

    /// Every column, shown or hidden, in its order: the Columns panel's
    /// list.
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// The columns the table shows, in their order.
    pub fn shown(&self) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&column| self.is_shown(column))
            .collect()
    }

    /// Whether the table shows `column`; one it doesn't have isn't.
    pub fn is_shown(&self, column: usize) -> bool {
        self.hidden.get(column).is_some_and(|hidden| !hidden)
    }

    /// Where `column` stands in the order, from 0.
    pub fn position(&self, column: usize) -> Option<usize> {
        self.order.iter().position(|&at| at == column)
    }

    /// The layout with `column` shown or hidden. Every column but the row
    /// header's may be hidden; the row header always shows.
    pub fn showing(&self, column: usize, shown: bool) -> Self {
        let mut next = self.clone();
        if let Some(hidden) = next.hidden.get_mut(column) {
            *hidden = !shown;
        }
        next
    }

    /// Whether `column` can move a `step`: not up from the first place, nor
    /// down from the last.
    pub fn can_move(&self, column: usize, step: Step) -> bool {
        self.position(column).is_some_and(|at| match step {
            Step::Up => at > 0,
            Step::Down => at + 1 < self.order.len(),
        })
    }

    /// The layout with `column` moved one place a `step`; the same where it
    /// can't move so.
    pub fn moved(&self, column: usize, step: Step) -> Self {
        match self.position(column) {
            Some(at) if self.can_move(column, step) => {
                let to = match step {
                    Step::Up => at - 1,
                    Step::Down => at + 1,
                };
                self.moved_to(column, to)
            }
            _ => self.clone(),
        }
    }

    /// The layout with `column` moved to the place `to`, from 0, or the
    /// last place where `to` is past it.
    pub fn moved_to(&self, column: usize, to: usize) -> Self {
        let mut next = self.clone();
        if let Some(at) = self.position(column) {
            next.order.remove(at);
            next.order.insert(to.min(next.order.len()), column);
        }
        next
    }
}

/// The gap of the Columns panel's list a pointer at `y` stands at: 0 above
/// the first item's middle, one more past each item's middle, `middles`
/// the items' middles from the top.
pub fn gap_at(middles: &[f64], y: f64) -> usize {
    middles.iter().filter(|&&middle| middle < y).count()
}

/// The place a column dragged from the place `from` takes, dropped at the
/// gap `gap` ([`gap_at`]): the gap counts the column itself, which leaves
/// its place as it moves.
pub fn drop_place(from: usize, gap: usize) -> usize {
    if gap > from { gap - 1 } else { gap }
}

/// The sort the table keeps once its columns are laid out as `layout`:
/// none, the caller's order, once the column it sorts by (`Sorted`
/// counts the row header's as 0) is hidden, so no row is ordered by a
/// value the table no longer shows or says (decision 140).
pub fn sort_kept(sorted: Option<Sorted>, layout: &Layout) -> Option<Sorted> {
    sorted.filter(|sorted| {
        sorted
            .column
            .checked_sub(1)
            .is_none_or(|column| layout.is_shown(column))
    })
}

/// Whether a change to the cued column of a row is seen as it comes, and
/// so marks nothing: focus is in the row (`focused`), or the column is
/// hidden (`!cue_shown`), where no cell could say the mark (decision 140).
pub fn seen_as_it_comes(focused: bool, cue_shown: bool) -> bool {
    focused || !cue_shown
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 1;
    const FILES: usize = 3;

    fn sorted(column: usize, direction: Direction) -> Option<Sorted> {
        Some(Sorted { column, direction })
    }

    #[test]
    fn a_first_press_sorts_ascending_a_second_descending_and_a_third_ascending_again() {
        assert_eq!(
            press(None, SIZE, Direction::Ascending),
            Sorted {
                column: SIZE,
                direction: Direction::Ascending
            }
        );
        assert_eq!(
            press(
                sorted(SIZE, Direction::Ascending),
                SIZE,
                Direction::Ascending
            ),
            Sorted {
                column: SIZE,
                direction: Direction::Descending
            }
        );
        assert_eq!(
            press(
                sorted(SIZE, Direction::Descending),
                SIZE,
                Direction::Ascending
            ),
            Sorted {
                column: SIZE,
                direction: Direction::Ascending
            }
        );
    }

    #[test]
    fn a_column_read_newest_first_sorts_descending_on_its_first_press_and_ascending_on_its_second()
    {
        assert_eq!(
            press(None, SIZE, Direction::Descending),
            Sorted {
                column: SIZE,
                direction: Direction::Descending
            }
        );
        assert_eq!(
            press(
                sorted(SIZE, Direction::Descending),
                SIZE,
                Direction::Descending
            ),
            Sorted {
                column: SIZE,
                direction: Direction::Ascending
            }
        );
        assert_eq!(
            press(
                sorted(SIZE, Direction::Ascending),
                SIZE,
                Direction::Descending
            ),
            Sorted {
                column: SIZE,
                direction: Direction::Descending
            }
        );
    }

    #[test]
    fn a_press_on_another_column_sorts_it_its_first_way_whichever_way_the_last_ran() {
        for first in [Direction::Ascending, Direction::Descending] {
            for direction in [Direction::Ascending, Direction::Descending] {
                assert_eq!(
                    press(sorted(SIZE, direction), FILES, first),
                    Sorted {
                        column: FILES,
                        direction: first
                    }
                );
            }
        }
    }

    #[test]
    fn only_the_sorted_column_says_which_way_it_is_sorted() {
        assert_eq!(aria_sort(None, SIZE), None);
        assert_eq!(
            aria_sort(sorted(SIZE, Direction::Ascending), SIZE),
            Some("ascending")
        );
        assert_eq!(
            aria_sort(sorted(SIZE, Direction::Descending), SIZE),
            Some("descending")
        );
        assert_eq!(aria_sort(sorted(SIZE, Direction::Ascending), FILES), None);
        assert_eq!(aria_sort(sorted(SIZE, Direction::Descending), FILES), None);
    }

    #[test]
    fn the_glyph_shows_the_direction_of_the_sorted_column_and_both_on_the_others() {
        assert_eq!(glyph(None, SIZE), LucideGlyph::ArrowUpDown);
        assert_eq!(
            glyph(sorted(SIZE, Direction::Ascending), SIZE),
            LucideGlyph::ArrowUp
        );
        assert_eq!(
            glyph(sorted(SIZE, Direction::Descending), SIZE),
            LucideGlyph::ArrowDown
        );
        assert_eq!(
            glyph(sorted(SIZE, Direction::Ascending), FILES),
            LucideGlyph::ArrowUpDown
        );
        assert_eq!(
            glyph(sorted(SIZE, Direction::Descending), FILES),
            LucideGlyph::ArrowUpDown
        );
    }

    #[test]
    fn a_sort_is_said_by_its_directions_sentence() {
        let (up, down) = (
            "Sorted by size, smallest first.",
            "Sorted by size, largest first.",
        );
        assert_eq!(sentence(Direction::Ascending, up, down), up);
        assert_eq!(sentence(Direction::Descending, up, down), down);
    }

    /// A demo row: its name, the table's key, and a count.
    struct Row(&'static str, i64);

    fn rows() -> Vec<Row> {
        vec![
            Row("photos", 1204),
            Row("music", 12),
            Row("documents", 312),
            Row("notes", 12),
        ]
    }

    fn by_count(row: &Row) -> SortKey {
        SortKey::Number(row.1)
    }

    fn by_name(row: &Row) -> SortKey {
        SortKey::Text(row.0.to_owned())
    }

    #[test]
    fn an_unsorted_table_shows_its_rows_in_the_order_given() {
        assert_eq!(
            order(&rows(), |row| row.0, None),
            ["photos", "music", "documents", "notes"]
        );
    }

    #[test]
    fn a_sorted_table_orders_its_rows_by_the_columns_key_either_way() {
        assert_eq!(
            order(&rows(), |row| row.0, Some((Direction::Ascending, &by_name))),
            ["documents", "music", "notes", "photos"]
        );
        assert_eq!(
            order(
                &rows(),
                |row| row.0,
                Some((Direction::Descending, &by_name))
            ),
            ["photos", "notes", "music", "documents"]
        );
    }

    #[test]
    fn rows_that_tie_keep_the_order_given_whichever_way_the_sort_runs() {
        assert_eq!(
            order(
                &rows(),
                |row| row.0,
                Some((Direction::Ascending, &by_count))
            ),
            ["music", "notes", "documents", "photos"]
        );
        assert_eq!(
            order(
                &rows(),
                |row| row.0,
                Some((Direction::Descending, &by_count))
            ),
            ["photos", "documents", "music", "notes"]
        );
    }

    #[test]
    fn many_rows_that_tie_keep_the_order_given_whichever_way_the_sort_runs() {
        // More rows than the standard library sorts by insertion alone, which
        // keeps ties in order whatever the sort, so a sort that isn't stable
        // shows here.
        let rows: Vec<(i64, i64)> = (0..64).map(|at| (at, at % 3)).collect();
        let by_value = |row: &(i64, i64)| SortKey::Number(row.1);
        let ties = |value| (0..64).filter(move |at| at % 3 == value);
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Ascending, &by_value))),
            [0, 1, 2].into_iter().flat_map(ties).collect::<Vec<_>>()
        );
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Descending, &by_value))),
            [2, 1, 0].into_iter().flat_map(ties).collect::<Vec<_>>()
        );
    }

    #[test]
    fn numbers_order_by_value_not_by_their_digits() {
        let rows = [Row("nine", 9), Row("ten", 10), Row("minus", -1)];
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Ascending, &by_count))),
            ["minus", "nine", "ten"]
        );
    }

    // The rows come from the daemon, and a keyed list given one key twice
    // panics in the browser: a key given again is dropped, the first row of
    // it kept, sorted or not (#138).
    #[test]
    fn a_key_given_twice_shows_once_as_its_first_row_sorted_or_not() {
        let rows = [
            Row("music", 12),
            Row("photos", 1204),
            Row("music", 3),
            Row("notes", 7),
        ];
        assert_eq!(
            order(&rows, |row| row.0, None),
            ["music", "photos", "notes"]
        );
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Ascending, &by_count))),
            ["notes", "music", "photos"],
            "the second music's 3 sorts nothing"
        );
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Descending, &by_count))),
            ["photos", "music", "notes"]
        );
    }

    #[test]
    fn each_key_stands_where_its_first_row_does() {
        let rows = [Row("music", 12), Row("photos", 1204), Row("music", 3)];
        let at = positions(&rows, |row| row.0);
        assert_eq!(at.len(), 2);
        assert_eq!(at.get("music"), Some(&0), "the first music, not the last");
        assert_eq!(at.get("photos"), Some(&1));
    }

    #[test]
    fn a_state_the_rows_cant_be_shown_in_takes_the_tables_place() {
        assert_eq!(shown(State::Loading, 0), Shown::Loading);
        assert_eq!(shown(State::Loading, 3), Shown::Loading);
        assert_eq!(shown(State::Unreachable, 0), Shown::Unreachable);
        assert_eq!(shown(State::Unreachable, 3), Shown::Unreachable);
    }

    #[test]
    fn ready_rows_show_as_the_table_or_say_there_are_none() {
        assert_eq!(shown(State::Ready, 0), Shown::Empty);
        assert_eq!(shown(State::Ready, 1), Shown::Table);
    }

    #[test]
    fn numbers_align_at_the_end_and_everything_else_at_the_start() {
        assert_eq!(Align::default(), Align::Start);
        assert_eq!(Align::Start.class(), None);
        assert_eq!(Align::End.class(), Some("cell-end"));
    }

    #[test]
    fn the_table_is_as_wide_as_its_frame_or_its_columns_whichever_is_wider() {
        assert_eq!(
            table_width(Rem::tenths(80), None, [Rem::tenths(55), Rem::tenths(110)]),
            "max(100cqi, calc(min(8.0rem, 40cqi) + 16.5rem))"
        );
        assert_eq!(
            table_width(Rem::tenths(80), None, []),
            "max(100cqi, calc(min(8.0rem, 40cqi) + 0.0rem))"
        );
    }

    #[test]
    fn a_pinned_row_header_takes_at_most_two_fifths_of_its_frame() {
        assert_eq!(
            row_header_width(Rem::tenths(65), None),
            "min(6.5rem, 40cqi)"
        );
        assert_eq!(pinned_room(Rem::tenths(65), None), "min(6.5rem, 40%)");
    }

    /// A snapshot's short ID breaks across lines in two fifths of a frame at
    /// 320 px (#1129), so a header whose words must read whole keeps their
    /// room in a narrow frame, up to three fifths of it: the cells beside it
    /// keep the rest to be read in, at 200 % text too.
    #[test]
    fn a_row_header_kept_whole_keeps_its_room_up_to_three_fifths_of_its_frame() {
        let whole = Some(Rem::tenths(65));
        assert_eq!(
            row_header_width(Rem::tenths(100), whole),
            "min(10.0rem, max(40cqi, min(6.5rem, 60cqi)))"
        );
        assert_eq!(
            pinned_room(Rem::tenths(100), whole),
            "min(10.0rem, max(40%, min(6.5rem, 60%)))"
        );
        assert_eq!(
            table_width(Rem::tenths(100), whole, [Rem::tenths(200)]),
            "max(100cqi, calc(min(10.0rem, max(40cqi, min(6.5rem, 60cqi))) + 20.0rem))"
        );
    }

    #[test]
    fn a_caption_names_its_table_and_its_frame() {
        let name = Name::Caption("Demo folders".to_owned());
        assert_eq!(name.caption(), Some("Demo folders"));
        assert_eq!(name.table_labelled_by(), None);
        assert_eq!(name.frame_labelled_by("table-1-caption"), "table-1-caption");
    }

    #[test]
    fn the_pages_heading_names_the_table_and_its_frame() {
        let name = Name::Heading("page-heading".to_owned());
        assert_eq!(name.caption(), None);
        assert_eq!(name.table_labelled_by(), Some("page-heading"));
        assert_eq!(name.frame_labelled_by("table-1-caption"), "page-heading");
    }

    #[test]
    fn a_rows_first_sight_only_seeds_its_status_and_lights_nothing() {
        assert!(!cues(None, &"running"));
    }

    #[test]
    fn a_status_the_row_showed_lights_the_row_when_it_changes() {
        assert!(cues(Some(&"running"), &"succeeded"));
        assert!(cues(Some(&"queued"), &"running"));
    }

    #[test]
    fn an_update_that_leaves_the_status_as_it_was_lights_nothing() {
        assert!(!cues(Some(&"running"), &"running"));
    }

    #[test]
    fn a_changed_row_stays_marked_until_focus_enters_it() {
        let mut marks = Marks::default();
        assert!(marks.change(&"docs", Some(&"running"), &"succeeded", false));
        assert!(marks.is_unseen(&"docs"));
        assert!(marks.any());
        assert!(marks.see(&"docs"));
        assert!(!marks.is_unseen(&"docs"));
        assert!(!marks.any());
        assert!(
            !marks.see(&"docs"),
            "a row seen once has no mark left to see"
        );
    }

    #[test]
    fn a_first_sight_and_an_unchanged_status_mark_nothing() {
        let mut marks = Marks::default();
        assert!(!marks.change(&"home", None, &"running", false));
        assert!(!marks.change(&"home", Some(&"running"), &"running", false));
        assert!(!marks.is_unseen(&"home"));
        assert!(!marks.any());
    }

    #[test]
    fn a_row_that_changes_with_focus_in_it_takes_the_fade_but_no_mark() {
        let mut marks = Marks::default();
        assert!(marks.change(&"home", Some(&"running"), &"cancelled", true));
        assert!(!marks.is_unseen(&"home"));
        assert!(!marks.any());
    }

    #[test]
    fn a_second_change_fades_again_and_keeps_one_mark() {
        let mut marks = Marks::default();
        assert!(marks.change(&"home", Some(&"queued"), &"running", false));
        assert!(marks.change(&"home", Some(&"running"), &"failed", false));
        assert_eq!(marks.see_all(), 1);
    }

    #[test]
    fn marking_all_as_seen_counts_the_marks_and_clears_them() {
        let mut marks = Marks::default();
        marks.change(&"home", Some(&"queued"), &"running", false);
        marks.change(&"docs", Some(&"running"), &"succeeded", false);
        marks.change(&"music", Some(&"running"), &"running", false);
        assert_eq!(marks.see_all(), 2);
        assert!(!marks.is_unseen(&"home"));
        assert!(!marks.is_unseen(&"docs"));
        assert!(!marks.any());
        assert_eq!(marks.see_all(), 0);
    }

    #[test]
    fn a_row_that_leaves_the_table_takes_its_mark_and_leaves_the_others() {
        let mut marks = Marks::default();
        marks.change(&"home", Some(&"queued"), &"running", false);
        marks.change(&"docs", Some(&"running"), &"succeeded", false);
        assert!(marks.gone(&"home"));
        assert!(!marks.gone(&"music"));
        assert!(!marks.is_unseen(&"home"));
        assert!(marks.is_unseen(&"docs"));
        assert_eq!(marks.see_all(), 1);
    }

    const IDS: [&str; 4] = ["size", "changed", "files", "actions"];
    const CHANGED: usize = 1;
    const FILES_PLACE: usize = 2;
    const ACTIONS: usize = 3;

    fn all_hidden() -> Layout {
        (0..4).fold(Layout::given(4), |layout, column| {
            layout.showing(column, false)
        })
    }

    #[test]
    fn the_given_layout_shows_every_column_in_the_callers_order() {
        let given = Layout::given(4);
        assert_eq!(given.order(), [0, 1, 2, 3]);
        assert_eq!(given.shown(), [0, 1, 2, 3]);
        assert!(given.is_shown(ACTIONS));
        assert!(!given.is_shown(4));
        assert_eq!(given.position(FILES_PLACE), Some(2));
        assert_eq!(given.position(4), None);
    }

    #[test]
    fn a_hidden_column_keeps_its_place_in_the_list_and_leaves_the_table() {
        let hidden = Layout::given(4).showing(CHANGED, false);
        assert_eq!(hidden.order(), [0, 1, 2, 3]);
        assert_eq!(hidden.shown(), [0, 2, 3]);
        assert!(!hidden.is_shown(CHANGED));
        assert_eq!(hidden.showing(CHANGED, true), Layout::given(4));
        assert_eq!(hidden.showing(4, false), hidden);
    }

    #[test]
    fn every_column_after_the_row_headers_may_be_hidden() {
        let none = all_hidden();
        assert!(none.shown().is_empty());
        assert_eq!(none.order(), [0, 1, 2, 3]);
    }

    #[test]
    fn a_column_moves_one_place_up_or_down_and_never_past_an_end() {
        let given = Layout::given(4);
        assert_eq!(given.moved(CHANGED, Step::Up).order(), [1, 0, 2, 3]);
        assert_eq!(given.moved(CHANGED, Step::Down).order(), [0, 2, 1, 3]);
        assert!(!given.can_move(0, Step::Up));
        assert!(given.can_move(0, Step::Down));
        assert!(given.can_move(ACTIONS, Step::Up));
        assert!(!given.can_move(ACTIONS, Step::Down));
        assert!(!given.can_move(4, Step::Up));
        assert_eq!(given.moved(0, Step::Up), given);
        assert_eq!(given.moved(ACTIONS, Step::Down), given);
        assert_eq!(given.moved(4, Step::Down), given);
    }

    #[test]
    fn a_moved_column_keeps_whether_it_is_hidden() {
        let moved = Layout::given(4)
            .showing(CHANGED, false)
            .moved(CHANGED, Step::Down);
        assert_eq!(moved.order(), [0, 2, 1, 3]);
        assert_eq!(moved.shown(), [0, 2, 3]);
        assert_eq!(moved.position(CHANGED), Some(2));
    }

    #[test]
    fn a_column_moved_to_a_place_takes_it_or_the_last_place_past_the_end() {
        let given = Layout::given(4);
        assert_eq!(given.moved_to(ACTIONS, 0).order(), [3, 0, 1, 2]);
        assert_eq!(given.moved_to(0, 2).order(), [1, 2, 0, 3]);
        assert_eq!(given.moved_to(0, 9).order(), [1, 2, 3, 0]);
        assert_eq!(given.moved_to(CHANGED, 1), given);
        assert_eq!(given.moved_to(4, 0), given);
    }

    #[test]
    fn the_given_layout_keeps_nothing_and_a_changed_one_keeps_its_ids_in_order() {
        assert_eq!(Layout::given(4).to_keep(&IDS), None);
        let changed = Layout::given(4)
            .moved(FILES_PLACE, Step::Up)
            .showing(CHANGED, false);
        assert_eq!(
            changed.to_keep(&IDS).as_deref(),
            Some("size,files,!changed,actions")
        );
        assert_eq!(
            Layout::from_kept(changed.to_keep(&IDS).as_deref(), &IDS),
            changed
        );
    }

    #[test]
    fn a_kept_layout_is_read_back_whole() {
        let kept = Layout::from_kept(Some("actions,!size,files,changed"), &IDS);
        assert_eq!(kept.order(), [3, 0, 2, 1]);
        assert_eq!(kept.shown(), [3, 2, 1]);
    }

    #[test]
    fn a_kept_id_no_column_has_drops_and_a_column_it_misses_comes_last_shown() {
        let kept = Layout::from_kept(Some("files,gone,!size"), &IDS);
        assert_eq!(kept.order(), [2, 0, 1, 3]);
        assert_eq!(kept.shown(), [2, 1, 3]);
        assert_eq!(Layout::from_kept(Some("gone"), &IDS), Layout::given(4));
    }

    #[test]
    fn nothing_kept_is_the_given_layout() {
        assert_eq!(Layout::from_kept(None, &IDS), Layout::given(4));
    }

    #[test]
    fn a_kept_value_ladderback_doesnt_keep_is_the_given_layout() {
        for junk in [
            "",
            "size,size",
            "size,!size",
            "size,,files",
            "Size",
            "size;files",
            "!!size",
            "size ,files",
            "{\"order\":[1]}",
        ] {
            assert_eq!(
                Layout::from_kept(Some(junk), &IDS),
                Layout::given(4),
                "{junk:?}"
            );
        }
    }

    #[test]
    fn a_kept_value_past_the_cap_is_the_given_layout() {
        // Well formed, its ids ones no column has, which drop: 64 entries
        // are the most read, the 65th is past the cap.
        let gone = |count: usize| vec!["gone"; count].join(",");
        let at_cap = format!("files,{}", gone(63));
        assert_eq!(Layout::from_kept(Some(&at_cap), &IDS).order(), [2, 0, 1, 3]);
        let past_cap = format!("files,{}", gone(64));
        assert_eq!(Layout::from_kept(Some(&past_cap), &IDS), Layout::given(4));
        let longest_id = "a".repeat(64);
        assert_eq!(
            Layout::from_kept(Some(&format!("files,{longest_id}")), &IDS).order(),
            [2, 0, 1, 3]
        );
        let long_id = "a".repeat(65);
        assert_eq!(
            Layout::from_kept(Some(&format!("files,{long_id}")), &IDS),
            Layout::given(4)
        );
    }

    #[test]
    fn ids_may_hold_digits_and_hyphens() {
        let ids = ["last-run", "files2"];
        assert_eq!(
            Layout::from_kept(Some("files2,!last-run"), &ids).order(),
            [1, 0]
        );
    }

    #[test]
    fn a_pointer_stands_at_the_gap_past_the_middles_above_it() {
        let middles = [10.0, 30.0, 50.0];
        assert_eq!(gap_at(&middles, 0.0), 0);
        assert_eq!(gap_at(&middles, 10.0), 0);
        assert_eq!(gap_at(&middles, 11.0), 1);
        assert_eq!(gap_at(&middles, 49.0), 2);
        assert_eq!(gap_at(&middles, 99.0), 3);
        assert_eq!(gap_at(&[], 5.0), 0);
    }

    #[test]
    fn a_column_dropped_below_its_own_place_takes_the_gap_less_itself() {
        assert_eq!(drop_place(1, 0), 0);
        assert_eq!(drop_place(1, 1), 1);
        assert_eq!(drop_place(1, 2), 1);
        assert_eq!(drop_place(1, 4), 3);
        assert_eq!(drop_place(0, 0), 0);
    }

    #[test]
    fn hiding_the_sorted_column_ends_the_sort_and_hiding_another_keeps_it() {
        let by_changed = sorted(CHANGED + 1, Direction::Descending);
        let changed_hidden = Layout::given(4).showing(CHANGED, false);
        assert_eq!(sort_kept(by_changed, &changed_hidden), None);
        let files_hidden = Layout::given(4).showing(FILES_PLACE, false);
        assert_eq!(sort_kept(by_changed, &files_hidden), by_changed);
        let moved = Layout::given(4).moved(CHANGED, Step::Up);
        assert_eq!(sort_kept(by_changed, &moved), by_changed);
        let by_name = sorted(0, Direction::Ascending);
        assert_eq!(sort_kept(by_name, &all_hidden()), by_name);
        assert_eq!(sort_kept(None, &all_hidden()), None);
    }

    #[test]
    fn a_change_to_a_hidden_cued_column_is_seen_as_it_comes() {
        assert!(!seen_as_it_comes(false, true));
        assert!(seen_as_it_comes(true, true));
        assert!(seen_as_it_comes(false, false));
        assert!(seen_as_it_comes(true, false));
    }
}
