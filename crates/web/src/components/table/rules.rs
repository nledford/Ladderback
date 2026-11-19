//! What a table decides (components §4.5), on the host: the order its rows
//! take and how a press on a sort button changes it, what its headers tell a
//! screen reader (`aria-sort`) and show (the direction's glyph), which
//! sentence a sort is said with, what shows in the table's place, how a
//! cell is aligned and how wide the columns are. `components/table.rs`'s
//! `Table` is the glue that draws it.

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

/// What a column is sorted by in a row: a number (a size in bytes, an
/// instant in milliseconds, a count) or a text, compared as its bytes are,
/// so its caller passes a text that orders as the reader expects (a name in
/// lower case, an instant as RFC 3339 in UTC).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortKey {
    Number(i64),
    Text(String),
}

/// A sort: which way, and a column's sort key.
pub type SortBy<'a, R> = (Direction, &'a dyn Fn(&R) -> SortKey);

/// The keys of `rows` in the order the table shows them: the order given,
/// or, sorted `by` a direction and a column's sort key, that order, rows
/// whose keys tie keeping the order given (a stable sort), whichever way it
/// runs. Each row's sort key is read once.
pub fn order<R, K>(rows: &[R], key: impl Fn(&R) -> K, by: Option<SortBy<'_, R>>) -> Vec<K> {
    let Some((direction, sort_key)) = by else {
        return rows.iter().map(key).collect();
    };
    let mut keyed: Vec<(SortKey, usize)> = rows
        .iter()
        .enumerate()
        .map(|(at, row)| (sort_key(row), at))
        .collect();
    // The position given breaks a tie, ascending whichever way the keys run.
    keyed.sort_by(|(a, at_a), (b, at_b)| {
        let by_key = match direction {
            Direction::Ascending => a.cmp(b),
            Direction::Descending => b.cmp(a),
        };
        by_key.then(at_a.cmp(at_b))
    });
    keyed
        .into_iter()
        .filter_map(|(_, at)| rows.get(at).map(&key))
        .collect()
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
/// text, where a frame is 8rem wide. Its words break inside it there.
const PINNED_SHARE: u8 = 40;

/// The row header column's width: `width`, or, in a frame too narrow for it
/// to leave the others room, its share of the frame (`cqi`: the frame is the
/// size container around the table), but never less than `whole`, where its
/// header's words must read whole.
pub fn row_header_width(width: Rem, whole: Option<Rem>) -> String {
    match whole {
        None => format!("min({width}, {PINNED_SHARE}cqi)"),
        Some(_) => todo!(),
    }
}

/// The frame's `scroll-padding-inline-start`: the room the pinned row header
/// covers, so a control focused in a cell scrolled under it is scrolled out
/// from beneath it. A percentage of the frame's own width, as a padding of a
/// scroller's is.
pub fn pinned_room(width: Rem, whole: Option<Rem>) -> String {
    match whole {
        None => format!("min({width}, {PINNED_SHARE}%)"),
        Some(_) => todo!(),
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
    fn numbers_order_by_value_not_by_their_digits() {
        let rows = [Row("nine", 9), Row("ten", 10), Row("minus", -1)];
        assert_eq!(
            order(&rows, |row| row.0, Some((Direction::Ascending, &by_count))),
            ["minus", "nine", "ten"]
        );
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
}
