//! How a checkbox and a radio are drawn (components §4.5): the box, its
//! mark once chosen, and the line its label, description and error stand
//! in. The ids those parts are wired by, and what describes the box, are the
//! text field's (`components/text_field/rules.rs`), since they are its parts.
//! `components/checkbox.rs`'s `Checkbox` and `components/radio_group.rs`'s
//! `RadioGroup` are the glue that draws them.

/// The shape of a choice's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A checkbox's: a square with the small corner, a text field's, whose
    /// mark is a check.
    Square,
    /// A radio's: a circle, whose mark is a dot.
    Round,
}

/// The line a choice stands on, in a grid: its box at the start, its label
/// beside it, and its description and error under the label.
pub const LINE: &str = "grid grid-cols-[auto_1fr] gap-x-2 items-start my-4";

/// The cell that holds the box and, over it, its mark, centered in it.
pub const CELL: &str = "grid place-items-center";

/// A description under the label, in its column.
pub const UNDER_LABEL: &str = "col-start-2 my-1";

/// An error under the label, after the description: a text field's error,
/// in its color and weight.
pub const ERROR_UNDER_LABEL: &str = "form-error col-start-2 my-1";

/// The label beside the box, in the Label weight, as wide as its words, so
/// a label written right to left in a left-to-right page still starts
/// beside its box, not at the far end of the line.
pub const LABEL: &str = "font-semibold justify-self-start";

/// What every box takes, in Tailwind utilities (decision 117), logical ones
/// only: the native control's own drawing dropped, never under 24px a side
/// (2.5.8) and 1.5 times its text, so it grows with the reader's text, a
/// 1px edge in Field Edge (3:1 on the page and on a surface, 1.4.11) round
/// the page's color, the edge and the fill in Ladder Blue once chosen, and
/// the edge 2px in the Error tone while it is invalid, as a text field's.
/// Under forced colors, the reader's field colors, chosen or not (the
/// chosen variants named too, since `:checked` outranks the media query
/// alone), the mark telling them apart; invalid keeps its thicker edge.
const BOX: &str = "peer col-start-1 row-start-1 appearance-none m-0 p-0 \
    inline-[max(24px,1.5em)] block-[max(24px,1.5em)] \
    border border-solid border-(--field-edge) bg-bg \
    checked:border-accent checked:bg-accent \
    forced-colors:border-[color:FieldText] forced-colors:bg-[color:Field] \
    forced-colors:checked:border-[color:FieldText] forced-colors:checked:bg-[color:Field]";

/// A checkbox is invalid itself (`aria-invalid` on it), edged 2px in the
/// Error tone, which stays under forced colors.
const SQUARE_INVALID: &str = "aria-[invalid=true]:border-2 aria-[invalid=true]:border-error \
    forced-colors:aria-[invalid=true]:border-[color:FieldText]";

/// A radio is invalid with its group, which carries `aria-invalid` (ARIA
/// 1.2 allows it on a `radiogroup`, not on a `radio`), so each is edged from
/// the group it stands in, the `group` of its fieldset ([`GROUP`]).
const ROUND_INVALID: &str = "group-aria-[invalid=true]:border-2 \
    group-aria-[invalid=true]:border-error \
    forced-colors:group-aria-[invalid=true]:border-[color:FieldText]";

/// The class of a radio group's fieldset, the `group` its radios' invalid
/// edge is keyed on.
pub const GROUP: &str = "choices group";

/// The classes of a choice's box, a native `<input>`: its shape's corner,
/// a text field's small one for a checkbox, round for a radio.
pub fn box_class(shape: Shape) -> String {
    let (corner, invalid) = match shape {
        Shape::Square => ("rounded-(--radius-small)", SQUARE_INVALID),
        Shape::Round => ("rounded-full", ROUND_INVALID),
    };
    format!("{BOX} {corner} {invalid}")
}

/// The classes of the mark a chosen box shows over its fill, in its middle
/// and in the page's color (as far from Ladder Blue as Ladder Blue is from
/// the page), shown only while the box before it is checked, and never in
/// the way of a press on it: a checkbox's check, an icon as tall as the
/// text, or a radio's dot. Under forced colors, the reader's field text,
/// kept as it is named (`forced-color-adjust: none`), so the dot isn't
/// dropped with the page's fills.
pub fn mark_class(shape: Shape) -> &'static str {
    match shape {
        Shape::Square => {
            "col-start-1 row-start-1 hidden peer-checked:flex pointer-events-none text-bg \
             forced-color-adjust-none forced-colors:text-[color:FieldText]"
        }
        Shape::Round => {
            "col-start-1 row-start-1 hidden peer-checked:block pointer-events-none \
             rounded-full inline-[max(10px,0.625em)] block-[max(10px,0.625em)] bg-bg \
             forced-color-adjust-none forced-colors:bg-[color:FieldText]"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPES: [Shape; 2] = [Shape::Square, Shape::Round];

    fn classes(class: &str) -> Vec<&str> {
        class.split_whitespace().collect()
    }

    #[test]
    fn a_box_is_edged_in_field_edge_and_filled_in_ladder_blue_once_chosen() {
        for shape in SHAPES {
            let class = box_class(shape);
            let classes = classes(&class);
            for wanted in [
                "appearance-none",
                "border",
                "border-(--field-edge)",
                "bg-bg",
                "checked:border-accent",
                "checked:bg-accent",
            ] {
                assert!(
                    classes.contains(&wanted),
                    "{shape:?} lacks {wanted}: {class}"
                );
            }
        }
    }

    #[test]
    fn under_forced_colors_a_chosen_box_keeps_the_readers_field_colors() {
        for shape in SHAPES {
            let class = box_class(shape);
            let classes = classes(&class);
            for wanted in [
                "forced-colors:bg-[color:Field]",
                "forced-colors:checked:bg-[color:Field]",
                "forced-colors:checked:border-[color:FieldText]",
            ] {
                assert!(
                    classes.contains(&wanted),
                    "{shape:?} lacks {wanted}: {class}"
                );
            }
        }
    }

    #[test]
    fn a_box_never_under_24_pixels_grows_with_the_text() {
        for shape in SHAPES {
            let class = box_class(shape);
            let classes = classes(&class);
            assert!(classes.contains(&"inline-[max(24px,1.5em)]"), "{class}");
            assert!(classes.contains(&"block-[max(24px,1.5em)]"), "{class}");
        }
    }

    #[test]
    fn an_invalid_checkbox_is_edged_thicker_in_the_error_tone() {
        let class = box_class(Shape::Square);
        let classes = classes(&class);
        for wanted in [
            "aria-[invalid=true]:border-2",
            "aria-[invalid=true]:border-error",
            "forced-colors:aria-[invalid=true]:border-[color:FieldText]",
        ] {
            assert!(classes.contains(&wanted), "lacks {wanted}: {class}");
        }
    }

    #[test]
    fn a_radio_is_edged_thicker_when_its_group_is_invalid_not_itself() {
        let class = box_class(Shape::Round);
        let classes = classes(&class);
        for wanted in [
            "group-aria-[invalid=true]:border-2",
            "group-aria-[invalid=true]:border-error",
            "forced-colors:group-aria-[invalid=true]:border-[color:FieldText]",
        ] {
            assert!(classes.contains(&wanted), "lacks {wanted}: {class}");
        }
        assert!(
            !classes.iter().any(|c| c.starts_with("aria-[invalid")),
            "{class}"
        );
        assert!(GROUP.split_whitespace().any(|c| c == "group"));
    }

    #[test]
    fn a_checkbox_has_the_small_corner_and_a_radio_is_round() {
        let square = box_class(Shape::Square);
        let round = box_class(Shape::Round);
        assert!(
            classes(&square).contains(&"rounded-(--radius-small)"),
            "{square}"
        );
        assert!(!classes(&square).contains(&"rounded-full"), "{square}");
        assert!(classes(&round).contains(&"rounded-full"), "{round}");
        assert!(
            !classes(&round).contains(&"rounded-(--radius-small)"),
            "{round}"
        );
    }

    #[test]
    fn a_mark_shows_only_over_a_chosen_box_in_the_pages_color() {
        for shape in SHAPES {
            let class = mark_class(shape);
            let classes = classes(class);
            assert!(classes.contains(&"hidden"), "{shape:?}: {class}");
            assert!(
                classes
                    .iter()
                    .any(|class| class.starts_with("peer-checked:")),
                "{shape:?}: {class}"
            );
            assert!(
                classes.contains(&"pointer-events-none"),
                "{shape:?}: {class}"
            );
        }
        assert!(classes(mark_class(Shape::Square)).contains(&"text-bg"));
        let dot = classes(mark_class(Shape::Round));
        assert!(dot.contains(&"bg-bg"), "{dot:?}");
        assert!(dot.contains(&"rounded-full"), "{dot:?}");
    }
}
