//! What a spinner looks like and how a screen reader meets it (components
//! §4.5): its classes, its tone's and its size's from the shared vocabulary
//! (`variants.rs`) and the Tailwind utilities that draw its parts (decision
//! 117), the size it is drawn at when its page names none, and whether its
//! label is shown beside it or carried by the drawing as its name.
//! `components/spinner.rs`'s `Spinner` is the glue that draws it.

use crate::variants::{Placement, Size, Tone};

/// Whether a spinner's label is shown beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelShown {
    /// Shown beside the drawing as ordinary text, which a screen reader
    /// reads; the drawing is hidden from it.
    #[default]
    Visible,
    /// Not shown (a tight spot): the drawing is an image named by the label.
    Hidden,
}

impl LabelShown {
    /// Whether the label is written beside the drawing.
    pub fn shows_label(self) -> bool {
        self == Self::Visible
    }

    /// The drawing's `role`: an image only when it carries the label.
    pub fn graphic_role(self) -> Option<&'static str> {
        (!self.shows_label()).then_some("img")
    }

    /// The drawing's `aria-hidden`: hidden beside a label a screen reader
    /// reads already, so the label is read once.
    pub fn graphic_hidden(self) -> Option<&'static str> {
        self.shows_label().then_some("true")
    }

    /// The drawing's accessible name (`aria-label`): the label, when it
    /// isn't shown.
    pub fn graphic_name(self, label: &str) -> Option<String> {
        (!self.shows_label()).then(|| label.to_owned())
    }
}

/// The size a spinner is drawn at when its page names none: as tall as the
/// text (1em), for a spinner in a line of text or inside a button. The
/// vocabulary has no default size, so each component picks its own.
pub fn default_size() -> Size {
    Size::Small
}

/// The classes of the spinner, the element around its drawing and its label
/// inside its line: its tone, which colors the drawing, and its size, which
/// sets the drawing's length. On a line of its own (every placement but
/// Inline) it is a row, the drawing at its start, so a long label wraps
/// beside the drawing, never under it; `style.css` then centers the drawing
/// and the label's first line on each other.
pub fn spinner_class(tone: Tone, size: Size, placement: Placement) -> String {
    let row = match placement {
        Placement::Inline => "",
        Placement::Start | Placement::Center | Placement::End => " flex items-start",
    };
    format!("spinner {} {}{row}", tone.class(), size.class())
}

/// How the drawing is drawn, in Tailwind utilities, logical ones only: the
/// shared scale's length (`--size`), so it grows with the reader's text
/// (1.4.4), and stroked in its tone (`--tone`), whose tokens reach 4.5:1 on
/// the page and on a surface (the contrast gate's tone pairs), more than the
/// 3:1 a part needs (1.4.11). The label keeps the text's color: a tone
/// colors the drawing only. In a line of text it is centered on the text as
/// an inline image is, so its label stands on the baseline of the words
/// around it; in a row it keeps its length; before a label, half an em
/// stands between them.
pub const GRAPHIC: &str = "spinner-graphic flex-none inline-(--size) block-(--size) \
    align-middle text-(--tone) not-last:me-[0.5em]";

/// What turns: the circle inside the drawing, about the drawing's middle,
/// not the drawing, so its box keeps its length at every angle and a spinner
/// at the edge of a part that scrolls never changes how far it scrolls. It
/// turns once in 0.9 s, steadily, by `style.css`'s keyframes. When the
/// reader asks for less motion it is not drawn at all: stopped, a quarter
/// arc over a track reads as a gauge a quarter full, an amount the page
/// doesn't know, so [`STILL`] stands in its place.
pub const TURN: &str = "spinner-turn transform-view origin-center \
    animate-[spinner-turn_0.9s_linear_infinite] motion-reduce:hidden";

/// The track: the arc's color at a quarter of its strength (the drawing
/// sets that strength). Under forced colors it takes the color of
/// unavailable text, the arc the text's own, the reader's own two; only the
/// track opts out of the mode's colors, to keep `GrayText`: when the whole
/// drawing opted out, Chromium drew its arc in the author's text color, not
/// the reader's. Like the arc, it is not drawn when the reader asks for
/// less motion.
pub const TRACK: &str = "spinner-track forced-colors:forced-color-adjust-none \
    forced-colors:stroke-[color:GrayText] forced-colors:[stroke-opacity:1] motion-reduce:hidden";

/// The still form, drawn only when the reader asks for less motion: three
/// dots in a row across the drawing's middle, filled in its color (the
/// text's own under forced colors), which say "waiting" without moving and
/// without reading as an amount. The drawing's box, name and baseline are
/// the turning form's, so nothing around it moves when the form changes.
/// The reduced-motion rule in `style.css` takes away animations only; this
/// is a display, which it leaves alone.
pub const STILL: &str = "spinner-still hidden motion-reduce:inline";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spinner_is_small_with_its_label_shown_unless_told_otherwise() {
        assert_eq!(default_size(), Size::Small);
        assert_eq!(LabelShown::default(), LabelShown::Visible);
    }

    #[test]
    fn a_spinner_takes_its_tone_and_its_size_from_the_shared_vocabulary() {
        for tone in Tone::ALL {
            for size in Size::ALL {
                for placement in Placement::ALL {
                    let class = spinner_class(tone, size, placement);
                    let classes: Vec<&str> = class.split_whitespace().collect();
                    assert_eq!(classes.first(), Some(&"spinner"), "{class}");
                    assert!(classes.contains(&tone.class()), "{class}");
                    assert!(classes.contains(&size.class()), "{class}");
                }
            }
        }
    }

    #[test]
    fn a_spinner_on_a_line_of_its_own_is_a_row_and_one_in_a_line_of_text_is_not() {
        for placement in Placement::ALL {
            let class = spinner_class(Tone::Primary, Size::Small, placement);
            let classes: Vec<&str> = class.split_whitespace().collect();
            let row = classes.contains(&"flex") && classes.contains(&"items-start");
            assert_eq!(
                row,
                placement != Placement::Inline,
                "{placement:?}: {class}"
            );
        }
    }

    #[test]
    fn a_shown_label_is_read_as_text_and_the_drawing_is_hidden() {
        let shown = LabelShown::Visible;
        assert!(shown.shows_label());
        assert_eq!(shown.graphic_role(), None);
        assert_eq!(shown.graphic_hidden(), Some("true"));
        assert_eq!(shown.graphic_name("Loading snapshots…"), None);
    }

    #[test]
    fn a_hidden_label_names_the_drawing_as_an_image() {
        let hidden = LabelShown::Hidden;
        assert!(!hidden.shows_label());
        assert_eq!(hidden.graphic_role(), Some("img"));
        assert_eq!(hidden.graphic_hidden(), None);
        assert_eq!(
            hidden.graphic_name("Loading snapshots…").as_deref(),
            Some("Loading snapshots…")
        );
    }
}
