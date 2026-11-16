//! What a text field takes, how it is drawn and what a screen reader meets
//! (components §4.5): its kind and what each kind asks of the browser, the
//! classes of its size, the ids its label, description, note and error are
//! wired by and what it says in words of its state.
//! `components/text_field.rs`'s `TextField` is the glue that draws it.

use l10n::{MessageId, msg};
use lepticons_data::LucideGlyph;

use crate::variants::Size;

/// What a text field takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// One line of text. The default.
    #[default]
    Line,
    /// Many lines, a native `<textarea>` `rows` lines tall, for a list typed
    /// an entry a line: Enter starts a line there and never sends the form.
    Lines { rows: u32 },
    /// A secret, hidden as it is typed, with a button that shows it.
    Password,
    /// A whole number, typed in digits: a text field that asks for the
    /// keyboard of digits, never `type="number"`, whose value a scroll of
    /// the wheel changes. It takes whatever is typed or pasted, as every
    /// field does: a key that silently did nothing would say nothing of
    /// why (3.3.1), so the form refuses a value that isn't a number when
    /// it is checked, its error above the field.
    Number,
    /// A code typed or pasted back, such as a snapshot's short ID or a
    /// token: one line that isn't words, so never spell-checked and never
    /// capitalized by a phone's keyboard.
    Code,
}

impl Kind {
    /// The rows of a many-lines field, which is a `<textarea>`; `None` for
    /// every one-line kind, an `<input>`.
    pub fn rows(self) -> Option<u32> {
        match self {
            Self::Lines { rows } => Some(rows),
            Self::Line | Self::Password | Self::Number | Self::Code => None,
        }
    }

    /// The `<input>`'s `type`: `password` for a password while it is hidden,
    /// `text` for everything else, a number and a shown password too.
    pub fn input_type(self, shown: bool) -> &'static str {
        if self == Self::Password && !shown {
            "password"
        } else {
            "text"
        }
    }

    /// The `inputmode`: the keyboard of digits for a number, and the
    /// browser's own choice otherwise.
    pub fn input_mode(self) -> Option<&'static str> {
        (self == Self::Number).then_some("numeric")
    }

    /// The `spellcheck`: off for what isn't words (a list of paths, a
    /// password, a number, a code). A shown password is never sent to a
    /// spelling service.
    pub fn spellcheck(self) -> Option<&'static str> {
        (self != Self::Line).then_some("false")
    }

    /// The `autocapitalize`: off for a password, a number and a code, so a
    /// phone's keyboard changes nothing typed.
    pub fn autocapitalize(self) -> Option<&'static str> {
        matches!(self, Self::Password | Self::Number | Self::Code).then_some("off")
    }
}

/// Whether a text field can be changed: told apart in words (its note) and
/// by its edge, never by graying alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    /// It takes what is typed. The default.
    #[default]
    Editable,
    /// Its value can be read, selected and copied, never changed: `readonly`,
    /// still in the Tab order, its edge dashed.
    ReadOnly,
    /// Unavailable: the native `disabled`, out of the Tab order, as HTML
    /// makes a disabled field. A field that must still be reached is
    /// `ReadOnly` instead.
    Disabled,
}

impl Access {
    /// The note that says the field's state in words, under its
    /// description, which describes the field too.
    pub fn note(self) -> Option<MessageId> {
        match self {
            Self::Editable => None,
            Self::ReadOnly => Some(msg::UI_TEXT_FIELD_READ_ONLY),
            Self::Disabled => Some(msg::UI_TEXT_FIELD_DISABLED),
        }
    }
}

/// The size a text field is drawn at when its page names none: Medium, 2em
/// tall, a Medium button's height.
pub fn default_size() -> Size {
    Size::Medium
}

/// The classes of the field's box, the edge around its input: its size,
/// which `style.css` makes the height and padding of a button of that size,
/// with a leading icon the room for it, and for a token's whole text the
/// monospace and the width it takes.
pub fn box_class(size: Size, icon: bool, token: bool) -> String {
    let icon = if icon { " text-field-with-icon" } else { "" };
    let token = if token { " text-field-token" } else { "" };
    format!("text-field-box {}{icon}{token}", size.class())
}

/// The ids a field's parts are wired by, each from the input's `id`.
pub fn label_id(id: &str) -> String {
    format!("{id}-label")
}

pub fn description_id(id: &str) -> String {
    format!("{id}-description")
}

pub fn note_id(id: &str) -> String {
    format!("{id}-note")
}

pub fn error_id(id: &str) -> String {
    format!("{id}-error")
}

/// The input's `aria-describedby`: its description, the note of its state
/// and its error, each while the field shows it, in the order they stand;
/// none when it shows none.
pub fn described_by(id: &str, description: bool, access: Access, error: bool) -> Option<String> {
    let parts: Vec<String> = [
        description.then(|| description_id(id)),
        access.note().map(|_| note_id(id)),
        error.then(|| error_id(id)),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The marker a required field shows after its label, "(required)", or
/// none. It is said once, by `aria-required`, so a screen reader skips the
/// marker (`aria-hidden`) and the field keeps its label as its name, which
/// still begins with the words it shows (2.5.3).
pub fn required_marker(required: bool) -> Option<MessageId> {
    required.then_some(msg::UI_TEXT_FIELD_REQUIRED)
}

/// The input's `aria-required`, beside the words after its label. Never
/// the `required` attribute: a form checks its own fields (`novalidate`),
/// and the browser's own check would draw an invalid state of its own.
pub fn aria_required(required: bool) -> Option<&'static str> {
    required.then_some("true")
}

/// The input's `aria-invalid`: `true` while it shows an error.
pub fn invalid(error: bool) -> Option<&'static str> {
    error.then_some("true")
}

/// The glyph of a password's show button, a toggle named "Show password":
/// an eye while the password is shown (pressed), crossed out while it is
/// hidden, as the icon toggle's bell is crossed out while off.
pub fn show_glyph(shown: bool) -> LucideGlyph {
    if shown {
        LucideGlyph::Eye
    } else {
        LucideGlyph::EyeOff
    }
}

/// The role of the field's error: an alert, said as it appears, for a field
/// in a dialog, as the dialog contract asks (§4.5); none otherwise, the page
/// saying the error through its announcer.
pub fn error_role(alert: bool) -> Option<&'static str> {
    alert.then_some("alert")
}

/// Whether a password's show button stands beside the field: beside a
/// password only, and never beside an unavailable one, which leaves the Tab
/// order with its field rather than leave a stop there that does nothing.
pub fn shows_toggle(kind: Kind, access: Access) -> bool {
    kind == Kind::Password && access != Access::Disabled
}

/// Whether a shown password is hidden again as what the field holds changes
/// to `value`: when its page sets it, clearing its form once it is sent,
/// rather than the reader typing `typed`. The next password typed there
/// stays hidden until the reader asks to see it.
pub fn hides_again(typed: &str, value: &str) -> bool {
    typed != value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_field_takes_one_line_and_is_medium_unless_told_otherwise() {
        assert_eq!(Kind::default(), Kind::Line);
        assert_eq!(Access::default(), Access::Editable);
        assert_eq!(default_size(), Size::Medium);
    }

    #[test]
    fn only_a_many_lines_field_is_a_textarea_of_its_rows() {
        assert_eq!(Kind::Lines { rows: 3 }.rows(), Some(3));
        for kind in [Kind::Line, Kind::Password, Kind::Number, Kind::Code] {
            assert_eq!(kind.rows(), None, "{kind:?}");
        }
    }

    #[test]
    fn a_password_is_hidden_until_shown_and_a_number_is_text() {
        assert_eq!(Kind::Password.input_type(false), "password");
        assert_eq!(Kind::Password.input_type(true), "text");
        for shown in [false, true] {
            assert_eq!(Kind::Line.input_type(shown), "text");
            assert_eq!(Kind::Number.input_type(shown), "text");
            assert_eq!(Kind::Code.input_type(shown), "text");
        }
    }

    #[test]
    fn a_number_asks_for_the_keyboard_of_digits_and_nothing_else_does() {
        assert_eq!(Kind::Number.input_mode(), Some("numeric"));
        for kind in [
            Kind::Line,
            Kind::Lines { rows: 2 },
            Kind::Password,
            Kind::Code,
        ] {
            assert_eq!(kind.input_mode(), None, "{kind:?}");
        }
    }

    #[test]
    fn only_words_are_spellchecked_and_only_secrets_numbers_and_codes_keep_their_case() {
        assert_eq!(Kind::Line.spellcheck(), None);
        for kind in [
            Kind::Lines { rows: 2 },
            Kind::Password,
            Kind::Number,
            Kind::Code,
        ] {
            assert_eq!(kind.spellcheck(), Some("false"), "{kind:?}");
        }
        for kind in [Kind::Password, Kind::Number, Kind::Code] {
            assert_eq!(kind.autocapitalize(), Some("off"), "{kind:?}");
        }
        assert_eq!(Kind::Line.autocapitalize(), None);
        assert_eq!(Kind::Lines { rows: 2 }.autocapitalize(), None);
    }

    #[test]
    fn read_only_and_disabled_each_say_so_in_words_of_their_own() {
        assert_eq!(Access::Editable.note(), None);
        assert_eq!(Access::ReadOnly.note(), Some(msg::UI_TEXT_FIELD_READ_ONLY));
        assert_eq!(Access::Disabled.note(), Some(msg::UI_TEXT_FIELD_DISABLED));
    }

    #[test]
    fn the_box_takes_its_size_from_the_shared_vocabulary_and_makes_room_for_an_icon() {
        assert_eq!(
            box_class(Size::Medium, false, false),
            "text-field-box size-medium"
        );
        assert_eq!(
            box_class(Size::XLarge, true, false),
            "text-field-box size-xlarge text-field-with-icon"
        );
        assert_eq!(
            box_class(Size::Small, false, true),
            "text-field-box size-small text-field-token"
        );
    }

    #[test]
    fn each_part_has_an_id_of_its_own_from_the_inputs() {
        assert_eq!(label_id("name"), "name-label");
        assert_eq!(description_id("name"), "name-description");
        assert_eq!(note_id("name"), "name-note");
        assert_eq!(error_id("name"), "name-error");
    }

    #[test]
    fn the_input_is_described_by_what_the_field_shows_in_order() {
        assert_eq!(described_by("f", false, Access::Editable, false), None);
        assert_eq!(
            described_by("f", true, Access::Editable, false).as_deref(),
            Some("f-description")
        );
        assert_eq!(
            described_by("f", true, Access::Editable, true).as_deref(),
            Some("f-description f-error")
        );
        assert_eq!(
            described_by("f", true, Access::ReadOnly, false).as_deref(),
            Some("f-description f-note")
        );
        assert_eq!(
            described_by("f", false, Access::Disabled, true).as_deref(),
            Some("f-note f-error")
        );
    }

    #[test]
    fn a_required_field_shows_a_marker_and_says_so_to_a_screen_reader_once() {
        assert_eq!(required_marker(false), None);
        assert_eq!(required_marker(true), Some(msg::UI_TEXT_FIELD_REQUIRED));
        assert_eq!(aria_required(false), None);
        assert_eq!(aria_required(true), Some("true"));
    }

    #[test]
    fn a_field_is_invalid_only_while_it_shows_an_error() {
        assert_eq!(invalid(false), None);
        assert_eq!(invalid(true), Some("true"));
    }

    #[test]
    fn only_a_field_in_a_dialog_says_its_error_as_an_alert() {
        assert_eq!(error_role(false), None);
        assert_eq!(error_role(true), Some("alert"));
    }

    #[test]
    fn the_show_button_crosses_its_eye_out_while_the_password_is_hidden() {
        assert_eq!(show_glyph(false), LucideGlyph::EyeOff);
        assert_eq!(show_glyph(true), LucideGlyph::Eye);
    }

    #[test]
    fn only_a_password_that_can_be_used_has_a_show_button() {
        assert!(shows_toggle(Kind::Password, Access::Editable));
        assert!(shows_toggle(Kind::Password, Access::ReadOnly));
        assert!(!shows_toggle(Kind::Password, Access::Disabled));
        for kind in [
            Kind::Line,
            Kind::Lines { rows: 2 },
            Kind::Number,
            Kind::Code,
        ] {
            for access in [Access::Editable, Access::ReadOnly, Access::Disabled] {
                assert!(!shows_toggle(kind, access), "{kind:?} {access:?}");
            }
        }
    }

    #[test]
    fn a_shown_password_hides_again_when_its_page_sets_what_it_holds() {
        assert!(!hides_again("demo-secret", "demo-secret"));
        assert!(hides_again("demo-secret", ""));
        assert!(hides_again("", "demo-secret"));
    }

    #[test]
    fn the_required_marker_holds_the_space_before_it_for_a_translator_to_set() {
        let marker = crate::text(&l10n::Localizer::new(&[]), msg::UI_TEXT_FIELD_REQUIRED);
        assert_eq!(marker, " (required)");
    }
}
