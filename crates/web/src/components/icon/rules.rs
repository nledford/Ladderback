//! What an icon tells a screen reader and how it is drawn (components §4.5):
//! whether it is decoration or an image named by its label, the classes of
//! its tone and size, taken from the shared vocabulary (`variants.rs`), and
//! the tone and size it is drawn at when its caller names none.
//! `components/icon.rs`'s `Icon` is the glue that draws it.

use crate::variants::{Size, Tone};

/// What an icon is to a screen reader. A meaningful icon can't be made
/// without its label: the label is what makes it one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Meaning {
    /// Decoration beside words that say what it shows, or inside a control
    /// its own label names: hidden from a screen reader, so the words are
    /// read once. The default.
    #[default]
    Decorative,
    /// An image that says something no words beside it say, named by this
    /// label, which must say it: an empty one leaves an image without a name
    /// (1.1.1).
    Labelled(String),
}

impl Meaning {
    /// The drawing's `role`: an image only when it carries a label.
    pub fn role(&self) -> Option<&'static str> {
        todo!()
    }

    /// The drawing's `aria-hidden`: hidden when it is decoration.
    pub fn hidden(&self) -> Option<&'static str> {
        todo!()
    }

    /// The drawing's accessible name (`aria-label`): its label, if it has one.
    pub fn name(&self) -> Option<&str> {
        todo!()
    }
}

/// The tone an icon is drawn in when its caller names none: the color of
/// the text around it, as an icon beside words or inside a control is.
pub fn default_tone() -> Tone {
    todo!()
}

/// The size an icon is drawn at when its caller names none: as tall as the
/// text (1em).
pub fn default_size() -> Size {
    todo!()
}

/// The classes of the icon's drawing: its tone, which colors the glyph, and
/// its size, which `style.css`'s shared scale makes its length.
pub fn icon_class(_tone: Tone, _size: Size) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_is_decoration_in_the_texts_color_as_tall_as_the_text_unless_told_otherwise() {
        assert_eq!(Meaning::default(), Meaning::Decorative);
        assert_eq!(default_tone(), Tone::Inherit);
        assert_eq!(default_size(), Size::Small);
    }

    #[test]
    fn a_decorative_icon_is_hidden_from_a_screen_reader_and_has_no_name() {
        let decorative = Meaning::Decorative;
        assert_eq!(decorative.role(), None);
        assert_eq!(decorative.hidden(), Some("true"));
        assert_eq!(decorative.name(), None);
    }

    #[test]
    fn a_labelled_icon_is_an_image_named_by_its_label() {
        let labelled = Meaning::Labelled("Demo repository checked".to_owned());
        assert_eq!(labelled.role(), Some("img"));
        assert_eq!(labelled.hidden(), None);
        assert_eq!(labelled.name(), Some("Demo repository checked"));
    }

    #[test]
    fn an_icon_takes_its_tone_and_its_size_from_the_shared_vocabulary() {
        assert_eq!(
            icon_class(Tone::Inherit, Size::Small),
            "icon tone-inherit size-small"
        );
        assert_eq!(
            icon_class(Tone::Error, Size::XLarge),
            "icon tone-error size-xlarge"
        );
    }
}
