//! What a spinner looks like and how a screen reader meets it (components
//! §4.5): the classes of its tone and size, taken from the shared vocabulary
//! (`variants.rs`), the size it is drawn at when its page names none, and
//! whether its label is shown beside it or carried by the drawing as its
//! name. `components.rs`'s `Spinner` is the glue that draws it.

use crate::variants::{Size, Tone};

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
/// `style.css` maps to the drawing's length.
pub fn spinner_class(tone: Tone, size: Size) -> String {
    format!("spinner {} {}", tone.class(), size.class())
}

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
        assert_eq!(
            spinner_class(Tone::Primary, Size::Small),
            "spinner tone-primary size-small"
        );
        assert_eq!(
            spinner_class(Tone::Inherit, Size::XLarge),
            "spinner tone-inherit size-xlarge"
        );
        for tone in Tone::ALL {
            for size in Size::ALL {
                assert_eq!(
                    spinner_class(tone, size),
                    format!("spinner {} {}", tone.class(), size.class()),
                    "{tone:?} {size:?}"
                );
            }
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
