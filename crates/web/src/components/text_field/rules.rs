//! What a text field takes, how it is drawn and what a screen reader meets
//! (components §4.5): its kind and what each kind asks of the browser, the
//! classes of its parts (its size's from the shared vocabulary and the
//! Tailwind utilities that draw it, decision 117), the ids its label,
//! description, note and error are wired by and what it says in words of its
//! state. `components/text_field.rs`'s `TextField` is the glue that draws it.

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

/// How the field and its parts are drawn, in Tailwind utilities (decision
/// 117), logical ones only. The field: a rem above and below it (none in a
/// row of actions, `components::ACTIONS`), its label on a line of its own in
/// the Label weight, and each paragraph in it (its description, the note of
/// its state, its error, and any of its caller's) a quarter rem above and
/// below. Each rule reaches every label and paragraph inside the field, as
/// a caller's paragraph is one of them.
pub const FIELD: &str = "text-field my-4 [&_label]:block [&_label]:font-semibold [&_p]:my-1";

/// The row a quarter rem under the field's words: the box, then a
/// password's show button, at their tops, half a rem apart.
pub const ROW: &str = "text-field-row flex items-start gap-2 mbs-1";

/// The icon at the field's start, inside its edge by the control padding
/// and centered on the control's height, decoration that lets a press
/// through to the input under it.
pub const ICON: &str = "text-field-icon absolute inline-flex pointer-events-none \
    inset-bs-[calc((var(--control-height)_-_1em)/2)] inset-s-(--control-padding)";

/// The box, the place of the input and its icon: the icon's container,
/// filling its row beside a show button, and never wider than its row.
const BOX: &str = "relative block flex-auto min-inline-0";

/// As wide as its line, up to 32rem.
const LINE_WIDTH: &str = "max-inline-[32rem]";

/// A token's whole text: data, in the monospace a terminal shows it in,
/// which the input takes from its box (the base rule's `font: inherit` on
/// every input outranks any utility on the input itself), and wide enough
/// for its 66 characters where the screen has room.
const TOKEN: &str = "max-inline-[40rem] [font-family:ui-monospace,monospace]";

/// The classes of the field's box, the edge around its input: its size, and
/// its size's control height and padding (`Size::control`), a button of that
/// size's, which the input and its icon are drawn with, then the box's
/// utilities, its width, and the names of a box with a leading icon and of a
/// token's, whose monospace and width it takes.
pub fn box_class(size: Size, icon: bool, token: bool) -> String {
    let icon = if icon { " text-field-with-icon" } else { "" };
    let (width, token) = if token {
        (TOKEN, " text-field-token")
    } else {
        (LINE_WIDTH, "")
    };
    format!(
        "text-field-box {} {} {BOX} {width}{icon}{token}",
        size.class(),
        size.control()
    )
}

/// How every input is drawn: a block filling its box, at least the control's
/// height, a 1 px solid edge in `--field-edge` (3:1 or more on the page and
/// on a surface), the button's small corner, the page's colors. Flat: no
/// shadow. Read only: its edge dashed. Invalid: an Error-tone edge a pixel
/// thicker, beside its message, so the state is in its words and its weight,
/// never color alone. Disabled: muted text on the surface, a not-allowed
/// cursor. Under forced colors the reader's field colors, invalid keeping
/// its thicker edge and read only its dashes, a disabled field `GrayText`.
/// Its focus keeps the global ring.
const INPUT: &str = "block inline-full min-block-(--control-height) m-0 \
    border border-solid border-field-edge rounded-(--radius-small) bg-bg text-fg \
    [&[readonly]]:border-dashed aria-[invalid=true]:[border-width:2px] \
    not-forced-colors:aria-[invalid=true]:border-error disabled:cursor-not-allowed \
    not-forced-colors:disabled:bg-surface not-forced-colors:disabled:text-muted \
    forced-colors:bg-[color:Field] forced-colors:border-[color:FieldText] \
    forced-colors:text-[color:FieldText] forced-colors:disabled:border-[color:GrayText] \
    forced-colors:disabled:text-[color:GrayText]";

/// One line: exactly the control's height.
const ONE_LINE: &str = "block-(--control-height) py-0";

/// Many lines: padded above and below so its first line stands where a one
/// line field's text does, inside its edge.
const LINES: &str = "py-[calc((var(--control-height)_-_1lh)/2_-_1px)]";

/// The control padding at its start and end, a pixel less while invalid,
/// so its text stays where it was inside the thicker edge.
const PADDED: &str =
    "px-(--control-padding) aria-[invalid=true]:px-[calc(var(--control-padding)_-_1px)]";

/// With a leading icon: room for the icon (1em and half an em's gap) at its
/// start, a pixel less on each side while invalid.
const PADDED_FOR_ICON: &str = "pe-(--control-padding) \
    ps-[calc(var(--control-padding)_+_1.5em)] \
    aria-[invalid=true]:pe-[calc(var(--control-padding)_-_1px)] \
    aria-[invalid=true]:ps-[calc(var(--control-padding)_+_1.5em_-_1px)]";

/// The classes of the input (or the text area of a `Lines` field): its own,
/// which names it, and the utilities that draw it, its height for one line
/// or many, and its padding with or without a leading icon.
pub fn input_class(kind: Kind, icon: bool) -> String {
    let height = if kind.rows().is_some() {
        LINES
    } else {
        ONE_LINE
    };
    let padding = if icon { PADDED_FOR_ICON } else { PADDED };
    format!("text-field-input {INPUT} {height} {padding}")
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
/// and its error, each while the field shows it, in the order they stand,
/// then `also`, an element of the caller's that describes it too; none when
/// there is none of them.
pub fn described_by(
    id: &str,
    description: bool,
    access: Access,
    error: bool,
    also: Option<&str>,
) -> Option<String> {
    let parts: Vec<String> = [
        description.then(|| description_id(id)),
        access.note().map(|_| note_id(id)),
        error.then(|| error_id(id)),
        also.map(str::to_owned),
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

    fn classes(class: &str) -> Vec<&str> {
        class.split_whitespace().collect()
    }

    fn contains_all(class: &str, set: &str) -> bool {
        let classes = classes(class);
        set.split_whitespace().all(|one| classes.contains(&one))
    }

    #[test]
    fn the_box_takes_its_size_from_the_shared_vocabulary_and_makes_room_for_an_icon() {
        for size in Size::ALL {
            for (icon, token) in [(false, false), (true, false), (false, true), (true, true)] {
                let class = box_class(size, icon, token);
                assert_eq!(classes(&class).first(), Some(&"text-field-box"), "{class}");
                for set in [size.class(), size.control(), BOX] {
                    assert!(contains_all(&class, set), "{set} in {class}");
                }
                let names = classes(&class);
                assert_eq!(names.contains(&"text-field-with-icon"), icon, "{class}");
                assert_eq!(names.contains(&"text-field-token"), token, "{class}");
            }
        }
    }

    #[test]
    fn a_tokens_box_is_wider_and_in_monospace_and_any_other_as_wide_as_a_line() {
        let token = box_class(Size::Medium, false, true);
        assert!(contains_all(&token, TOKEN), "{token}");
        assert!(!contains_all(&token, LINE_WIDTH), "{token}");
        let line = box_class(Size::Medium, true, false);
        assert!(contains_all(&line, LINE_WIDTH), "{line}");
        let line_classes = classes(&line);
        assert!(
            !TOKEN
                .split_whitespace()
                .any(|one| line_classes.contains(&one)),
            "{line}"
        );
    }

    #[test]
    fn many_lines_are_padded_to_a_lines_place_and_one_line_is_the_controls_height() {
        for icon in [false, true] {
            let lines = input_class(Kind::Lines { rows: 3 }, icon);
            assert_eq!(
                classes(&lines).first(),
                Some(&"text-field-input"),
                "{lines}"
            );
            assert!(contains_all(&lines, INPUT), "{lines}");
            assert!(contains_all(&lines, LINES), "{lines}");
            assert!(!contains_all(&lines, ONE_LINE), "{lines}");
            for kind in [Kind::Line, Kind::Password, Kind::Number, Kind::Code] {
                let line = input_class(kind, icon);
                assert!(contains_all(&line, INPUT), "{line}");
                assert!(contains_all(&line, ONE_LINE), "{line}");
                assert!(!contains_all(&line, LINES), "{line}");
            }
        }
    }

    #[test]
    fn an_input_with_an_icon_leaves_room_for_it_at_its_start_and_any_other_is_padded_alike() {
        let with = input_class(Kind::Line, true);
        assert!(contains_all(&with, PADDED_FOR_ICON), "{with}");
        assert!(!contains_all(&with, PADDED), "{with}");
        let without = input_class(Kind::Line, false);
        assert!(contains_all(&without, PADDED), "{without}");
        assert!(!contains_all(&without, PADDED_FOR_ICON), "{without}");
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
        assert_eq!(
            described_by("f", false, Access::Editable, false, None),
            None
        );
        assert_eq!(
            described_by("f", true, Access::Editable, false, None).as_deref(),
            Some("f-description")
        );
        assert_eq!(
            described_by("f", true, Access::Editable, true, None).as_deref(),
            Some("f-description f-error")
        );
        assert_eq!(
            described_by("f", true, Access::ReadOnly, false, None).as_deref(),
            Some("f-description f-note")
        );
        assert_eq!(
            described_by("f", false, Access::Disabled, true, None).as_deref(),
            Some("f-note f-error")
        );
    }

    #[test]
    fn an_element_of_the_caller_s_describes_the_input_after_its_own_parts() {
        assert_eq!(
            described_by("f", false, Access::Editable, false, Some("warning")).as_deref(),
            Some("warning")
        );
        assert_eq!(
            described_by("f", true, Access::ReadOnly, true, Some("warning")).as_deref(),
            Some("f-description f-note f-error warning")
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
