//! The shape a skeleton draws (components §4.5), decided on the host: how
//! many lines of text it stands for and how long the last one is, or the
//! lengths of the block it stands for. `components/skeleton.rs`'s `Skeleton`
//! is the glue that draws it, and `style.css` the rest of its look: each
//! line centered on a line of the text's height, and every bar capped at
//! the room it is given.

use std::fmt;
use std::num::NonZeroU8;

use crate::variants::Rem;

/// What a skeleton stands for, which sets its shape. Its lengths come from
/// the reader's text, so the room it takes is the content's, and the page
/// doesn't jump when the content arrives. Its types can't hold a shape that
/// can't be drawn: at least one line, a last line no longer than its room,
/// and a block's lengths never below 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `count` lines of text, each the full width of the room it is given
    /// but the last, which is `last` of that width.
    Lines { count: NonZeroU8, last: Share },
    /// A box `inline` wide and `block` tall, such as a field or a button's
    /// place, never wider than the room it is given.
    Block { inline: Rem, block: Rem },
}

/// The share of its room a skeleton's last line takes: a whole percent,
/// from 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Share(u8);

impl Share {
    /// `percent` of the room; over 100 is the whole room, since a line is
    /// never longer than its room.
    pub fn percent(percent: u8) -> Self {
        Self(percent.min(100))
    }
}

impl fmt::Display for Share {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// One bar a skeleton draws: its inline and block sizes, as CSS lengths.
/// The page sets them as style properties, which its content security
/// policy allows, never as a `style` attribute, which it refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub inline: String,
    pub block: String,
}

impl Shape {
    /// The skeleton's classes: what it stands for, which `style.css` lays
    /// out.
    pub fn class(self) -> &'static str {
        match self {
            Self::Lines { .. } => "skeleton skeleton-lines",
            Self::Block { .. } => "skeleton skeleton-block",
        }
    }

    /// The bars the skeleton draws, in order: a line of text each, as tall
    /// as the text (1em), or the one block.
    pub fn bars(self) -> Vec<Bar> {
        match self {
            Self::Lines { count, last } => {
                let line = |inline: String| Bar {
                    inline,
                    block: "1em".to_owned(),
                };
                // `count` is at least 1, so the last line is always there.
                let full = usize::from(count.get()) - 1;
                let mut bars = vec![line("100%".to_owned()); full];
                bars.push(line(last.to_string()));
                bars
            }
            Self::Block { inline, block } => vec![Bar {
                inline: inline.to_string(),
                block: block.to_string(),
            }],
        }
    }
}

/// What a region's first load waits for (§4.5), which sets the skeletons
/// standing in its place until it comes: the shape of what is coming, as
/// far as the page knows it before the daemon answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Awaited {
    /// A table: its header, then a few rows, each a line of text per column.
    Table { columns: NonZeroU8 },
    /// A form: each field a label above its box, then the button that sends
    /// it.
    Form { fields: NonZeroU8 },
    /// Words, a list or a few buttons, whose shape is known only once they
    /// come: lines of text.
    Text,
}

impl Awaited {
    /// A table of `columns` columns; none is one, since a table has one at
    /// least.
    pub fn table(columns: u8) -> Self {
        todo!("{columns}")
    }

    /// A form of `fields` fields; none is one, since a form has one at
    /// least.
    pub fn form(fields: u8) -> Self {
        todo!("{fields}")
    }

    /// The skeletons that stand for it, row by row: a row's skeletons stand
    /// side by side, each in an equal share of the row.
    pub fn rows(self) -> Vec<Vec<Shape>> {
        todo!("{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(count: u8, percent: u8) -> Shape {
        Shape::Lines {
            count: NonZeroU8::new(count).unwrap(),
            last: Share::percent(percent),
        }
    }

    fn inline_sizes(shape: Shape) -> Vec<String> {
        shape.bars().into_iter().map(|bar| bar.inline).collect()
    }

    #[test]
    fn lines_fill_their_room_but_the_last_which_is_its_share_of_it() {
        assert_eq!(inline_sizes(lines(3, 60)), ["100%", "100%", "60%"]);
    }

    #[test]
    fn each_line_is_as_tall_as_the_text() {
        let bars = lines(2, 50).bars();
        assert_eq!(bars.len(), 2);
        assert!(bars.iter().all(|bar| bar.block == "1em"), "{bars:?}");
    }

    #[test]
    fn one_line_is_the_last_one() {
        assert_eq!(inline_sizes(lines(1, 33)), ["33%"]);
    }

    #[test]
    fn the_last_line_is_never_longer_than_its_room() {
        let last = |percent| inline_sizes(lines(1, percent)).concat();
        assert_eq!(last(0), "0%");
        assert_eq!(last(100), "100%");
        assert_eq!(last(101), "100%");
        assert_eq!(last(u8::MAX), "100%");
    }

    #[test]
    fn a_block_is_sized_in_rem_to_a_tenth() {
        let block = |inline, block| {
            Shape::Block {
                inline: Rem::tenths(inline),
                block: Rem::tenths(block),
            }
            .bars()
        };
        assert_eq!(
            block(200, 25),
            [Bar {
                inline: "20.0rem".to_owned(),
                block: "2.5rem".to_owned()
            }]
        );
        assert_eq!(
            block(0, 9),
            [Bar {
                inline: "0.0rem".to_owned(),
                block: "0.9rem".to_owned()
            }]
        );
    }

    #[test]
    fn lines_and_a_block_are_laid_out_by_their_own_classes() {
        assert_eq!(lines(2, 50).class(), "skeleton skeleton-lines");
        assert_eq!(
            Shape::Block {
                inline: Rem::tenths(10),
                block: Rem::tenths(10)
            }
            .class(),
            "skeleton skeleton-block"
        );
    }

    /// The share of its room a one-line skeleton takes, in percent.
    fn share(shape: Shape) -> u8 {
        match shape {
            Shape::Lines { count, last } if count == NonZeroU8::MIN => last.0,
            other => panic!("{other:?} is not one line"),
        }
    }

    fn field() -> Shape {
        Shape::Block {
            inline: Rem::tenths(200),
            block: Rem::tenths(25),
        }
    }

    #[test]
    fn a_table_stands_as_its_header_then_three_rows_each_a_line_per_column() {
        let rows = Awaited::table(3).rows();
        assert_eq!(rows.len(), 4, "{rows:?}");
        for row in &rows {
            assert_eq!(row.len(), 3, "{rows:?}");
            row.iter().for_each(|&cell| {
                share(cell);
            });
        }
    }

    #[test]
    fn a_table_s_rows_differ_as_rows_of_content_do() {
        let rows = Awaited::table(1).rows();
        let shares: Vec<u8> = rows[1..].iter().map(|row| share(row[0])).collect();
        assert!(shares.windows(2).all(|pair| pair[0] != pair[1]), "{rows:?}");
    }

    #[test]
    fn a_table_s_header_is_shorter_than_each_of_its_rows() {
        let rows = Awaited::table(2).rows();
        let header = rows[0].iter().map(|&cell| share(cell)).max().unwrap();
        for row in &rows[1..] {
            assert!(row.iter().all(|&cell| share(cell) > header), "{rows:?}");
        }
    }

    #[test]
    fn a_table_of_no_columns_has_one() {
        assert_eq!(Awaited::table(0), Awaited::table(1));
        assert!(Awaited::table(0).rows().iter().all(|row| row.len() == 1));
    }

    #[test]
    fn a_form_stands_as_each_field_s_label_above_its_box_then_its_button() {
        let rows = Awaited::form(2).rows();
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert!(rows.iter().all(|row| row.len() == 1), "{rows:?}");
        let parts: Vec<Shape> = rows.into_iter().flatten().collect();
        share(parts[0]);
        share(parts[2]);
        assert_eq!([parts[1], parts[3]], [field(), field()]);
        // The button, as tall as a field and narrower.
        let [button] = parts[4].bars().try_into().unwrap();
        let [field] = field().bars().try_into().unwrap();
        assert_eq!(button.block, field.block);
        assert!(
            button
                .inline
                .trim_end_matches("rem")
                .parse::<f32>()
                .unwrap()
                < field.inline.trim_end_matches("rem").parse::<f32>().unwrap(),
            "{button:?}"
        );
    }

    #[test]
    fn a_form_of_no_fields_has_one() {
        assert_eq!(Awaited::form(0), Awaited::form(1));
        assert_eq!(Awaited::form(0).rows().len(), 3);
    }

    #[test]
    fn words_stand_as_lines_of_text_the_last_one_shorter() {
        let rows = Awaited::Text.rows();
        assert_eq!(rows.len(), 1, "{rows:?}");
        let [Shape::Lines { count, last }] = rows[0][..] else {
            panic!("{rows:?}");
        };
        assert!(count.get() > 1, "{rows:?}");
        assert!(last.0 < 100, "{rows:?}");
    }
}
