//! What the header's menu decides on a screen narrower than 54rem (decision
//! 143): its pages' links and Sign out sit in a panel behind a hamburger
//! button, which shows and hides it. Decided here and tested on the host;
//! `main.rs`'s `Navigation` and `MenuButton` are the glue that draws it,
//! which the `@ui` scenarios of `features/ui/header.feature` run in the
//! browser.

use lepticons_data::LucideGlyph;

/// The `id` of the menu's panel, which the menu button controls.
pub(crate) const MENU_ID: &str = "menu";

/// The `id` of the menu button, which focus returns to when it hides the
/// panel.
pub(crate) const MENU_BUTTON_ID: &str = "menu-button";

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
pub(crate) fn menu_class(open: bool) -> &'static str {
    if open {
        "site-menu site-menu-open"
    } else {
        "site-menu site-menu-closed"
    }
}

/// Whether a key pressed in the header hides the panel and returns focus to
/// the button: Escape, only while the panel is shown and only while the
/// button is drawn, which is below 54rem alone. Where it isn't drawn there is
/// nothing to close and nowhere for focus to return to.
pub(crate) fn escape_closes(open: bool, key: &str, button_drawn: bool) -> bool {
    open && button_drawn && key == "Escape"
}

/// Whether the page itself hides the panel: whenever it shows no pages, as
/// the sign-in page doesn't, whatever the button was pressed to do, so a
/// sign-in finds it hidden.
pub(crate) fn hidden_without_pages(shows_pages: bool) -> bool {
    !shows_pages
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
    fn the_icon_is_the_hamburger_while_the_panel_is_hidden_and_an_x_while_it_shows() {
        assert_eq!(glyph(false), LucideGlyph::Menu);
        assert_eq!(glyph(true), LucideGlyph::X);
    }

    #[test]
    fn the_panel_is_marked_closed_until_the_button_shows_it() {
        for open in [true, false] {
            let classes = menu_class(open);
            assert!(
                classes.split(' ').any(|class| class == "site-menu"),
                "{classes}"
            );
            assert_eq!(
                classes.split(' ').any(|class| class == "site-menu-closed"),
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
