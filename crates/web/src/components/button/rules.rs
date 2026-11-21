//! What a button looks like, what a screen reader meets and whether it takes
//! a press (components §4.5): its appearance, its classes, its tone's and
//! its size's from the shared vocabulary (`variants.rs`) and the Tailwind
//! utilities that draw it (decision 117), the tone and size it is drawn at
//! when its page names none, what stands before its label, and the states
//! it tells with ARIA. `components/button.rs`'s `Button` is the glue that
//! draws it.

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

    /// The class that names the appearance a button is drawn in.
    pub fn class(self) -> &'static str {
        match self {
            Self::Filled => "button-filled",
            Self::Outlined => "button-outlined",
            Self::Ghost => "button-ghost",
        }
    }

    /// How a button of this appearance is colored, in Tailwind utilities.
    /// Filled: a fill in its tone, the label in the color drawn on fills
    /// (each pair at 4.5:1 or more in the contrast gate); its fill is its
    /// edge, at 3:1 or more on the page and on a surface (1.4.11). Outlined:
    /// an edge and a label in its tone. Ghost: the label alone, its edge
    /// drawn only while pressed, but under forced colors, whose edge is the
    /// button text's (`LOOK`).
    ///
    /// Under the pointer, while it takes presses: a fill a step darker
    /// (lighter in the dark scheme), or the surface under a label and edge a
    /// step darker, in its tone's hover color (`--tone-hover`, which
    /// [`hover`] sets), each pair in the contrast gate. Forced colors keep
    /// the reader's own.
    fn look(self) -> &'static str {
        match self {
            Self::Filled => {
                "bg-(--tone) border-(--tone) text-on-fill \
                 not-forced-colors:not-aria-disabled:hover:bg-(--tone-hover) \
                 not-forced-colors:not-aria-disabled:hover:border-(--tone-hover)"
            }
            Self::Outlined => {
                "bg-transparent border-(--tone) text-(--tone) \
                 not-forced-colors:not-aria-disabled:hover:bg-surface \
                 not-forced-colors:not-aria-disabled:hover:border-(--tone-hover) \
                 not-forced-colors:not-aria-disabled:hover:text-(--tone-hover)"
            }
            Self::Ghost => {
                "bg-transparent border-transparent text-(--tone) \
                 not-forced-colors:aria-pressed:border-(--tone) \
                 not-forced-colors:not-aria-disabled:hover:bg-surface \
                 not-forced-colors:not-aria-disabled:hover:text-(--tone-hover)"
            }
        }
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
        match self {
            Self::Button => "button",
            Self::Submit => "submit",
        }
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
        self != Self::IconOnly
    }

    /// The button's `aria-label`: the fuller name its page gives, which
    /// begins with the words the button shows (2.5.3), as a row's action is
    /// named with its row; otherwise its label, only where it isn't written
    /// in it, so a button's name is its visible words wherever it has them.
    pub fn name(self, label: &str, fuller: Option<String>) -> Option<String> {
        fuller.or_else(|| (!self.shows_label()).then(|| label.to_owned()))
    }

    /// What stands before the label: a busy button's spinner, in its icon's
    /// place or before a label that has none; otherwise its icon, if any.
    pub fn leading(self, busy: bool) -> Leading {
        match (self, busy) {
            (_, true) => Leading::Spinner,
            (Self::Text, false) => Leading::Nothing,
            (Self::IconAndText | Self::IconOnly, false) => Leading::Icon,
        }
    }
}

/// The tone a button is drawn in when its page names none: Secondary, the
/// neutral one, so a page says which action is its main one.
pub fn default_tone() -> Tone {
    Tone::Secondary
}

/// The size a button is drawn at when its page names none: Medium, 2em
/// tall, DESIGN.md's 2rem at the page's own text size.
pub fn default_size() -> Size {
    Size::Medium
}

/// The appearance and tone a button is drawn in. A fill in `Inherit`, the
/// color of the text around it, would need a label color checked against
/// every color that text could take, so a `Filled` button in `Inherit` is
/// drawn `Outlined`, its edge and label in the text's color.
pub fn drawn(appearance: Appearance, tone: Tone) -> (Appearance, Tone) {
    match (appearance, tone) {
        (Appearance::Filled, Tone::Inherit) => (Appearance::Outlined, Tone::Inherit),
        drawn => drawn,
    }
}

/// How every button is drawn, in Tailwind utilities (decision 117), logical
/// ones only: a native `<button>`, flat, its one small corner, its icon or
/// spinner and its label in a row centered both ways, half an em apart. It
/// is as tall as its size's control height and padded at its start and end
/// by its control padding (`Size::control`); a label too long for its line
/// wraps inside the button, which then grows taller. Its edge is a pixel,
/// solid; while pressed (a toggle) it is two, inset, so the state is never
/// color alone (The Weight Not Color Rule; `style.css` makes it bolder).
/// While it takes no press it stays on the page, where a keyboard and a
/// screen reader find it, and looks unavailable. Under forced colors it
/// takes the reader's button colors, every appearance edged so it still
/// reads as a button. Focus keeps the global ring.
const LOOK: &str = "inline-flex items-center justify-center gap-[0.5em] \
    min-block-(--control-height) py-0 border border-solid rounded-(--radius-small) \
    text-center align-middle aria-pressed:[border-width:2px] aria-pressed:[border-style:inset] \
    aria-disabled:cursor-not-allowed aria-disabled:opacity-60 \
    forced-colors:bg-[color:ButtonFace] forced-colors:border-[color:ButtonText] \
    forced-colors:text-[color:ButtonText]";

/// A button with words: its control padding at its start and end, a pixel
/// less while pressed, so the pressed edge's extra pixel doesn't widen it.
const PADDED: &str =
    "px-(--control-padding) aria-pressed:px-[max(0px,calc(var(--control-padding)_-_1px))]";

/// An icon-only button: square at every size, as wide as it is tall.
const SQUARE: &str = "inline-(--control-height) px-0";

/// Under forced colors, an unavailable button in the color of unavailable
/// text. Not a busy one, `aria-disabled` too: it is at work, not
/// unavailable, so it keeps the button text, and its spinner's arc takes
/// that color beside a `GrayText` track.
const UNAVAILABLE: &str = "forced-colors:aria-disabled:border-[color:GrayText] \
    forced-colors:aria-disabled:text-[color:GrayText]";

/// The color a button of `tone` takes under the pointer (`--tone-hover`): a
/// step darker than its tone, lighter in the dark scheme; for `Inherit` the
/// text's own color, as its tone is.
fn hover(tone: Tone) -> &'static str {
    match tone {
        Tone::Primary => "[--tone-hover:var(--accent-hover)]",
        Tone::Secondary => "[--tone-hover:var(--muted-hover)]",
        Tone::Success => "[--tone-hover:var(--success-hover)]",
        Tone::Warning => "[--tone-hover:var(--warning-hover)]",
        Tone::Error => "[--tone-hover:var(--error-hover)]",
        Tone::Inherit => "[--tone-hover:currentColor]",
    }
}

/// The classes of the button: its own, which names it, its appearance's and
/// tone's as drawn, its size's and its size's control height and padding,
/// then the utilities that draw it: every button's look, its appearance's
/// colors, its tone's hover color, square for an icon-only button and padded
/// otherwise, and, unless it is busy, the color of unavailable text under
/// forced colors while it takes no press.
pub fn button_class(
    appearance: Appearance,
    tone: Tone,
    size: Size,
    shape: Shape,
    busy: bool,
) -> String {
    let (appearance, tone) = drawn(appearance, tone);
    let padding = if shape == Shape::IconOnly {
        SQUARE
    } else {
        PADDED
    };
    let unavailable = if busy { "" } else { UNAVAILABLE };
    format!(
        "button {} {} {} {} {LOOK} {} {} {padding} {unavailable}",
        appearance.class(),
        tone.class(),
        size.class(),
        size.control(),
        appearance.look(),
        hover(tone),
    )
    .trim_end()
    .to_owned()
}

/// Whether a press reaches the page's handler: never while the button is
/// unavailable or busy.
pub fn takes_press(disabled: bool, busy: bool) -> bool {
    !disabled && !busy
}

/// The button's `aria-disabled`: `true` while it takes no press, so a
/// screen reader says it is unavailable while it keeps its place in the
/// Tab order. Never the `disabled` attribute, which would drop focus to the
/// page (A1).
pub fn unavailable(disabled: bool, busy: bool) -> Option<&'static str> {
    (!takes_press(disabled, busy)).then_some("true")
}

/// The button's `aria-pressed`: only on a toggle, the one a page gives a
/// pressed state.
pub fn pressed(pressed: Option<bool>) -> Option<&'static str> {
    pressed.map(|pressed| if pressed { "true" } else { "false" })
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

    fn classes(class: &str) -> Vec<&str> {
        class.split_whitespace().collect()
    }

    fn contains_all(class: &str, set: &str) -> bool {
        let classes = classes(class);
        set.split_whitespace().all(|one| classes.contains(&one))
    }

    #[test]
    fn a_button_takes_its_appearance_its_tone_and_its_size_from_the_shared_vocabulary() {
        for appearance in Appearance::ALL {
            for tone in Tone::ALL {
                for size in Size::ALL {
                    let class = button_class(appearance, tone, size, Shape::Text, false);
                    let (appearance, tone) = drawn(appearance, tone);
                    assert_eq!(classes(&class).first(), Some(&"button"), "{class}");
                    for set in [
                        appearance.class(),
                        appearance.look(),
                        tone.class(),
                        hover(tone),
                        size.class(),
                        size.control(),
                        LOOK,
                    ] {
                        assert!(contains_all(&class, set), "{set} in {class}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_hover_is_its_own_tones_step_darker_and_the_text_around_an_inherit_one() {
        for (tone, token) in [
            (Tone::Primary, "var(--accent-hover)"),
            (Tone::Secondary, "var(--muted-hover)"),
            (Tone::Success, "var(--success-hover)"),
            (Tone::Warning, "var(--warning-hover)"),
            (Tone::Error, "var(--error-hover)"),
            (Tone::Inherit, "currentColor"),
        ] {
            assert_eq!(hover(tone), format!("[--tone-hover:{token}]"), "{tone:?}");
        }
    }

    #[test]
    fn each_appearance_and_each_tone_is_drawn_in_colors_of_its_own() {
        let looks = Appearance::ALL.map(Appearance::look);
        let hovers = Tone::ALL.map(hover);
        for (i, look) in looks.iter().enumerate() {
            assert!(!looks[i + 1..].contains(look), "{look}");
        }
        for (i, hover) in hovers.iter().enumerate() {
            assert!(!hovers[i + 1..].contains(hover), "{hover}");
        }
    }

    #[test]
    fn an_icon_only_button_is_square_and_a_button_with_words_is_padded() {
        for shape in [Shape::Text, Shape::IconAndText, Shape::IconOnly] {
            let class = button_class(Appearance::Ghost, Tone::Primary, Size::Large, shape, false);
            let square = shape == Shape::IconOnly;
            assert_eq!(contains_all(&class, SQUARE), square, "{shape:?}: {class}");
            assert_eq!(contains_all(&class, PADDED), !square, "{shape:?}: {class}");
        }
    }

    #[test]
    fn a_busy_button_keeps_the_button_text_under_forced_colors_and_an_unavailable_one_does_not() {
        for shape in [Shape::Text, Shape::IconOnly] {
            let busy = button_class(Appearance::Filled, Tone::Primary, Size::Medium, shape, true);
            let idle = button_class(
                Appearance::Filled,
                Tone::Primary,
                Size::Medium,
                shape,
                false,
            );
            assert!(!contains_all(&busy, UNAVAILABLE), "{busy}");
            assert!(contains_all(&idle, UNAVAILABLE), "{idle}");
        }
    }

    #[test]
    fn a_filled_button_in_the_texts_color_is_drawn_outlined() {
        assert_eq!(
            drawn(Appearance::Filled, Tone::Inherit),
            (Appearance::Outlined, Tone::Inherit)
        );
        let class = button_class(
            Appearance::Filled,
            Tone::Inherit,
            Size::Medium,
            Shape::Text,
            false,
        );
        assert!(contains_all(&class, "button-outlined"), "{class}");
        assert!(!contains_all(&class, "button-filled"), "{class}");
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
        assert_eq!(Shape::Text.name("Save the settings", None), None);
        assert_eq!(Shape::IconAndText.name("Save the settings", None), None);
        assert_eq!(
            Shape::IconOnly.name("Refresh", None).as_deref(),
            Some("Refresh")
        );
    }

    #[test]
    fn a_fuller_name_the_page_gives_names_the_button_whatever_it_shows() {
        for shape in [Shape::Text, Shape::IconAndText, Shape::IconOnly] {
            assert_eq!(
                shape
                    .name("Cancel", Some("Cancel demo-home".to_owned()))
                    .as_deref(),
                Some("Cancel demo-home"),
                "{shape:?}"
            );
        }
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
