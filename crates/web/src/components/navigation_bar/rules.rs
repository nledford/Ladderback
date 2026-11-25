//! What the navigation bar decides (components §4.5, decisions 143, 152 and
//! 160): below 54rem its pages' links and its group's rows sit in a panel
//! behind a hamburger button, which shows and hides it. Decided here and
//! tested on the host; `components/navigation_bar.rs`'s `NavigationBar` is
//! the glue that draws it. When the panel closes on a press or on focus
//! leaving is the disclosure menu's own rule
//! ([`crate::components::disclosure_menu::rules`]), so the bar's two
//! disclosures close alike.

use lepticons_data::LucideGlyph;

/// The `id` of the menu button, given the panel's `id`: focus returns to it
/// when Escape hides the panel.
pub(crate) fn button_id(panel_id: &str) -> String {
    format!("{panel_id}-button")
}

/// The `id` of the group's panel (the header's Settings menu), given the
/// bar's panel's `id`, so two bars on one page never share one.
pub(crate) fn group_id(panel_id: &str) -> String {
    format!("{panel_id}-group")
}

/// Whether the panel shows once the button is pressed, given whether it
/// showed before.
pub(crate) fn toggled(open: bool) -> bool {
    !open
}

/// The button's icon: the hamburger while the panel is hidden, an X while it
/// shows, the one that closes it. Its name never changes: `aria-expanded`
/// says which.
pub(crate) fn glyph(open: bool) -> LucideGlyph {
    if open {
        LucideGlyph::X
    } else {
        LucideGlyph::Menu
    }
}

/// The panel's classes: its own, and the one that says whether it is open,
/// which `style.css` reads below 54rem alone.
pub(crate) fn panel_class(open: bool) -> &'static str {
    if open {
        "navigation-bar-panel navigation-bar-open"
    } else {
        "navigation-bar-panel navigation-bar-closed"
    }
}

/// Whether a key pressed in the bar hides the panel and returns focus to
/// the button: Escape, only while the panel is shown and only while the
/// button is drawn, which is below 54rem alone. Where it isn't drawn there is
/// nothing to close and nowhere for focus to return to.
pub(crate) fn escape_closes(open: bool, key: &str, button_drawn: bool) -> bool {
    open && button_drawn && key == "Escape"
}

/// Whether the bar itself hides the panel: whenever it shows no pages, as
/// the sign-in page doesn't, whatever the button was pressed to do, so a
/// sign-in finds it hidden.
pub(crate) fn hidden_without_pages(shows_pages: bool) -> bool {
    !shows_pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_and_the_group_take_ids_of_their_own_from_the_panels() {
        assert_eq!(button_id("menu"), "menu-button");
        assert_eq!(group_id("menu"), "menu-group");
        assert_eq!(button_id("demo-menu"), "demo-menu-button");
        assert_eq!(group_id("demo-menu"), "demo-menu-group");
    }

    #[test]
    fn the_button_shows_what_was_hidden_and_hides_what_was_shown() {
        assert!(toggled(false));
        assert!(!toggled(true));
    }

    #[test]
    fn the_icon_is_the_hamburger_while_the_panel_is_hidden_and_an_x_while_it_shows() {
        assert_eq!(glyph(false), LucideGlyph::Menu);
        assert_eq!(glyph(true), LucideGlyph::X);
    }

    #[test]
    fn the_panel_is_marked_closed_until_the_button_shows_it() {
        for open in [true, false] {
            let classes = panel_class(open);
            assert!(
                classes
                    .split(' ')
                    .any(|class| class == "navigation-bar-panel"),
                "{classes}"
            );
            assert_eq!(
                classes
                    .split(' ')
                    .any(|class| class == "navigation-bar-closed"),
                !open,
                "{classes}"
            );
        }
    }

    #[test]
    fn escape_hides_the_panel_only_while_it_is_shown_and_the_button_is_drawn() {
        assert!(escape_closes(true, "Escape", true));
        for (open, key, drawn) in [
            (false, "Escape", true),
            (true, "Enter", true),
            (true, "Tab", true),
            (true, "escape", true),
            // Links opened on a narrow screen, the window widened past
            // 54rem: the button is undrawn, so Escape has nothing to do.
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
    fn the_panel_is_hidden_whenever_the_page_shows_none() {
        assert!(hidden_without_pages(false));
        assert!(!hidden_without_pages(true));
    }
}
