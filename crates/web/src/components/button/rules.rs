//! What a button looks like, what a screen reader meets and whether it takes
//! a press (components §4.5): its appearance, the classes of its tone and
//! size, taken from the shared vocabulary (`variants.rs`), the tone and size
//! it is drawn at when its page names none, what stands before its label,
//! and the states it tells with ARIA. `components/button.rs`'s `Button` is
//! the glue that draws it.

use crate::variants::{Size, Tone};

/// How a button is drawn, from the loudest to the quietest. A page gives an
/// action row's main action the one `Filled` button (DESIGN.md, Buttons).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    /// A fill in its tone, its label in the color drawn on fills
    /// (`--on-fill`): the main action of its row.
    Filled,
    /// An edge and a label in its tone, on the page's own background. The
    /// default.
    #[default]
    Outlined,
    /// A label in its tone alone, its edge drawn only while pressed: an
    /// action beside others, in a row that has many.
    Ghost,
}

impl Appearance {
    /// Every appearance, loudest first.
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Filled, Self::Outlined, Self::Ghost];

    /// The class of `style.css` that draws a button so.
    pub fn class(self) -> &'static str {
        todo!()
    }
}

/// What a press does, as the `type` attribute says it: a button of the page
/// (the default), or one that sends the form it is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// It does what its page's handler does, and sends no form.
    #[default]
    Button,
    /// It sends the form it is in, as Enter in one of the form's fields does.
    Submit,
}

impl Kind {
    /// The button's `type`. Never left out: a `<button>` without one sends
    /// the form it is in.
    pub fn attribute(self) -> &'static str {
        todo!()
    }
}

/// What a button shows: its label alone, an icon before its label, or only
/// an icon, the label then its name (`aria-label`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Text,
    IconAndText,
    IconOnly,
}

/// What stands before a button's label (or alone, in an icon-only button).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leading {
    Nothing,
    Icon,
    Spinner,
}

impl Shape {
    /// Whether the label is written in the button, which names it: every
    /// button but an icon-only one.
    pub fn shows_label(self) -> bool {
        todo!()
    }

    /// The button's `aria-label`: its label, only where it isn't written in
    /// it, so a button's name is its visible words wherever it has them
    /// (2.5.3).
    pub fn name(self, _label: &str) -> Option<String> {
        todo!()
    }

    /// What stands before the label: a busy button's spinner, in its icon's
    /// place or before a label that has none; otherwise its icon, if any.
    pub fn leading(self, _busy: bool) -> Leading {
        todo!()
    }
}

/// The tone a button is drawn in when its page names none: Secondary, the
/// neutral one, so a page says which action is its main one.
pub fn default_tone() -> Tone {
    todo!()
}

/// The size a button is drawn at when its page names none: Medium, 2em
/// tall, DESIGN.md's 2rem at the page's own text size.
pub fn default_size() -> Size {
    todo!()
}

/// The appearance and tone a button is drawn in. A fill in `Inherit`, the
/// color of the text around it, would need a label color checked against
/// every color that text could take, so a `Filled` button in `Inherit` is
/// drawn `Outlined`, its edge and label in the text's color.
pub fn drawn(_appearance: Appearance, _tone: Tone) -> (Appearance, Tone) {
    todo!()
}

/// The classes of the button: its appearance and tone as drawn, its size,
/// which `style.css` makes its height and padding, and, for an icon-only
/// button, the class that makes it square.
pub fn button_class(_appearance: Appearance, _tone: Tone, _size: Size, _shape: Shape) -> String {
    todo!()
}

/// Whether a press reaches the page's handler: never while the button is
/// unavailable or busy.
pub fn takes_press(_disabled: bool, _busy: bool) -> bool {
    todo!()
}

/// The button's `aria-disabled`: `true` while it takes no press, so a
/// screen reader says it is unavailable while it keeps its place in the
/// Tab order. Never the `disabled` attribute, which would drop focus to the
/// page (A1).
pub fn unavailable(_disabled: bool, _busy: bool) -> Option<&'static str> {
    todo!()
}

/// The button's `aria-pressed`: only on a toggle, the one a page gives a
/// pressed state.
pub fn pressed(_pressed: Option<bool>) -> Option<&'static str> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_is_an_outlined_secondary_medium_button_of_the_page_unless_told_otherwise() {
        assert_eq!(Appearance::default(), Appearance::Outlined);
        assert_eq!(default_tone(), Tone::Secondary);
        assert_eq!(default_size(), Size::Medium);
        assert_eq!(Kind::default(), Kind::Button);
    }

    #[test]
    fn a_button_says_whether_it_sends_its_form() {
        assert_eq!(Kind::Button.attribute(), "button");
        assert_eq!(Kind::Submit.attribute(), "submit");
    }

    #[test]
    fn each_appearance_has_a_class_of_its_own() {
        let classes = [
            (Appearance::Filled, "button-filled"),
            (Appearance::Outlined, "button-outlined"),
            (Appearance::Ghost, "button-ghost"),
        ];
        for (appearance, class) in classes {
            assert_eq!(appearance.class(), class, "{appearance:?}");
        }
    }

    #[test]
    fn a_button_takes_its_tone_and_its_size_from_the_shared_vocabulary() {
        assert_eq!(
            button_class(
                Appearance::Outlined,
                Tone::Secondary,
                Size::Medium,
                Shape::Text
            ),
            "button button-outlined tone-secondary size-medium"
        );
        assert_eq!(
            button_class(
                Appearance::Filled,
                Tone::Error,
                Size::XLarge,
                Shape::IconAndText
            ),
            "button button-filled tone-error size-xlarge"
        );
        assert_eq!(
            button_class(Appearance::Ghost, Tone::Inherit, Size::Small, Shape::Text),
            "button button-ghost tone-inherit size-small"
        );
    }

    #[test]
    fn an_icon_only_button_is_square() {
        assert_eq!(
            button_class(
                Appearance::Ghost,
                Tone::Primary,
                Size::Large,
                Shape::IconOnly
            ),
            "button button-ghost tone-primary size-large button-icon-only"
        );
    }

    #[test]
    fn a_filled_button_in_the_texts_color_is_drawn_outlined() {
        assert_eq!(
            drawn(Appearance::Filled, Tone::Inherit),
            (Appearance::Outlined, Tone::Inherit)
        );
        assert_eq!(
            button_class(Appearance::Filled, Tone::Inherit, Size::Medium, Shape::Text),
            "button button-outlined tone-inherit size-medium"
        );
        for appearance in Appearance::ALL {
            for tone in Tone::ALL {
                let kept = !(appearance == Appearance::Filled && tone == Tone::Inherit);
                assert_eq!(
                    drawn(appearance, tone) == (appearance, tone),
                    kept,
                    "{appearance:?} {tone:?}"
                );
            }
        }
    }

    #[test]
    fn a_button_with_words_is_named_by_them_and_an_icon_only_one_by_its_label() {
        assert!(Shape::Text.shows_label());
        assert!(Shape::IconAndText.shows_label());
        assert!(!Shape::IconOnly.shows_label());
        assert_eq!(Shape::Text.name("Save the settings"), None);
        assert_eq!(Shape::IconAndText.name("Save the settings"), None);
        assert_eq!(Shape::IconOnly.name("Refresh").as_deref(), Some("Refresh"));
    }

    #[test]
    fn a_busy_buttons_spinner_takes_its_icons_place_or_stands_before_its_label() {
        assert_eq!(Shape::Text.leading(false), Leading::Nothing);
        assert_eq!(Shape::IconAndText.leading(false), Leading::Icon);
        assert_eq!(Shape::IconOnly.leading(false), Leading::Icon);
        assert_eq!(Shape::Text.leading(true), Leading::Spinner);
        assert_eq!(Shape::IconAndText.leading(true), Leading::Spinner);
        assert_eq!(Shape::IconOnly.leading(true), Leading::Spinner);
    }

    #[test]
    fn an_unavailable_or_busy_button_takes_no_press_and_says_so() {
        assert!(takes_press(false, false));
        assert_eq!(unavailable(false, false), None);
        for (disabled, busy) in [(true, false), (false, true), (true, true)] {
            assert!(!takes_press(disabled, busy), "{disabled} {busy}");
            assert_eq!(
                unavailable(disabled, busy),
                Some("true"),
                "{disabled} {busy}"
            );
        }
    }

    #[test]
    fn only_a_toggle_says_whether_it_is_pressed() {
        assert_eq!(pressed(None), None);
        assert_eq!(pressed(Some(false)), Some("false"));
        assert_eq!(pressed(Some(true)), Some("true"));
    }
}
