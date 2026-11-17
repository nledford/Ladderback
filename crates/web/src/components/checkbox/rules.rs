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

/// A description or an error under the label, in its column.
pub const UNDER_LABEL: &str = "col-start-2 my-1";

/// The classes of a choice's box, a native `<input>`.
pub fn box_class(shape: Shape) -> String {
    let _ = shape;
    todo!()
}

/// The classes of the mark a chosen box shows over its fill.
pub fn mark_class(shape: Shape) -> &'static str {
    let _ = shape;
    todo!()
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
    fn a_box_never_under_24_pixels_grows_with_the_text() {
        for shape in SHAPES {
            let class = box_class(shape);
            let classes = classes(&class);
            assert!(classes.contains(&"inline-[max(24px,1.5em)]"), "{class}");
            assert!(classes.contains(&"block-[max(24px,1.5em)]"), "{class}");
        }
    }

    #[test]
    fn an_invalid_box_is_edged_thicker_in_the_error_tone() {
        for shape in SHAPES {
            let class = box_class(shape);
            let classes = classes(&class);
            assert!(classes.contains(&"aria-[invalid=true]:border-2"), "{class}");
            assert!(
                classes.contains(&"aria-[invalid=true]:border-error"),
                "{class}"
            );
        }
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
