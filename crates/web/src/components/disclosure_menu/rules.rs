//! What the disclosure menu decides (components §4.5): when its panel shows
//! and when it closes again, the chevron its button shows, and whether its
//! button marks the current page's group. Decided here and tested on the
//! host; `components/disclosure_menu.rs`'s `DisclosureMenu` is the glue.

use lepticons_data::LucideGlyph;

/// Where focus went as it left an element of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusMoved {
    /// To another element of the menu: its button, a row of its panel.
    Inside,
    /// To an element outside the menu.
    Outside,
    /// To no element: a press on something that takes no focus, or one
    /// in a browser that doesn't focus what it presses, as WebKit doesn't
    /// a link or a button. A press outside closes the menu on its own
    /// ([`closes_on_press`]); a press inside must still reach its row.
    Nowhere,
}

/// Whether the panel shows once the button is pressed, given whether it
/// showed before.
pub(crate) fn toggled(open: bool) -> bool {
    let _ = open;
    todo!()
}

/// The chevron after the button's words: down while the panel is hidden,
/// up while it shows, as a table's Columns button's.
pub(crate) fn chevron(open: bool) -> LucideGlyph {
    let _ = open;
    todo!()
}

/// Whether a key pressed in the menu closes it and returns focus to its
/// button: Escape, only while the panel shows and only while the button is
/// drawn. A page may leave the button undrawn and the panel's rows shown
/// in a menu of its own, as the header's hamburger does below its
/// breakpoint; that menu's Escape is then its own.
pub(crate) fn escape_closes(open: bool, key: &str, button_drawn: bool) -> bool {
    let _ = (open, key, button_drawn);
    todo!()
}

/// Whether focus leaving an element of the menu closes it: only to an
/// element outside, so the panel never covers what focus moved to (2.4.11)
/// and focus is never kept in it.
pub(crate) fn closes_when_focus_moves(open: bool, moved: FocusMoved) -> bool {
    let _ = (open, moved);
    todo!()
}

/// Whether a press of the pointer closes the menu: one outside it, while it
/// is open.
pub(crate) fn closes_on_press(open: bool, inside: bool) -> bool {
    let _ = (open, inside);
    todo!()
}

/// Whether focus goes back to the button once a row's link is chosen, given
/// the link's `aria-current`: only for the current page's link, which
/// changes no route, so no page's heading takes focus and the link it was on
/// is no longer drawn (2.4.3). Any other link's route change moves focus to
/// the new page's heading, as every link's does.
pub(crate) fn focus_returns_on_choice(link_current: Option<&str>) -> bool {
    todo!("{link_current:?}")
}

/// The button's `aria-current`: `true` while the page the reader is on is
/// one of the panel's, so the closed menu still says where the reader is.
pub(crate) fn current(group_holds_page: bool) -> Option<&'static str> {
    let _ = group_holds_page;
    todo!()
}

/// Whether a box of this size is drawn: a `display: none` box has none.
/// The glue reads the button's box and asks this, so the decision is the
/// host's and the glue only reads the DOM.
pub(crate) fn is_drawn(width: f64, height: f64) -> bool {
    let _ = (width, height);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_shows_what_was_hidden_and_hides_what_was_shown() {
        assert!(toggled(false));
        assert!(!toggled(true));
    }

    #[test]
    fn the_chevron_points_down_while_the_panel_is_hidden_and_up_while_it_shows() {
        assert_eq!(chevron(false), LucideGlyph::ChevronDown);
        assert_eq!(chevron(true), LucideGlyph::ChevronUp);
    }

    #[test]
    fn escape_closes_the_menu_only_while_it_shows_and_its_button_is_drawn() {
        assert!(escape_closes(true, "Escape", true));
        for (open, key, drawn) in [
            (false, "Escape", true),
            (true, "Enter", true),
            (true, "Tab", true),
            (true, "escape", true),
            // The rows shown in another menu, the button undrawn: that
            // menu's Escape is its own.
            (true, "Escape", false),
            (false, "Escape", false),
        ] {
            assert!(
                !escape_closes(open, key, drawn),
                "open {open}, key {key}, button drawn {drawn}"
            );
        }
    }

    #[test]
    fn focus_moving_outside_closes_the_menu_and_focus_moving_inside_or_nowhere_does_not() {
        assert!(closes_when_focus_moves(true, FocusMoved::Outside));
        for (open, moved) in [
            (true, FocusMoved::Inside),
            (true, FocusMoved::Nowhere),
            (false, FocusMoved::Outside),
            (false, FocusMoved::Inside),
            (false, FocusMoved::Nowhere),
        ] {
            assert!(
                !closes_when_focus_moves(open, moved),
                "open {open}, moved {moved:?}"
            );
        }
    }

    #[test]
    fn a_press_outside_the_open_menu_closes_it_and_a_press_inside_does_not() {
        assert!(closes_on_press(true, false));
        assert!(!closes_on_press(true, true));
        assert!(!closes_on_press(false, false));
        assert!(!closes_on_press(false, true));
    }

    #[test]
    fn the_button_is_current_only_while_its_group_holds_the_page() {
        assert_eq!(current(true), Some("true"));
        assert_eq!(current(false), None);
    }

    #[test]
    fn choosing_the_current_pages_link_returns_focus_to_the_button() {
        assert!(focus_returns_on_choice(Some("page")));
        assert!(!focus_returns_on_choice(None));
        assert!(!focus_returns_on_choice(Some("true")));
    }

    #[test]
    fn a_box_is_drawn_only_with_a_width_and_a_height() {
        assert!(is_drawn(1.0, 1.0));
        assert!(is_drawn(0.5, 0.5));
        for (width, height) in [(0.0, 48.0), (48.0, 0.0), (0.0, 0.0), (-1.0, 48.0)] {
            assert!(!is_drawn(width, height), "{width} by {height}");
        }
    }
}
