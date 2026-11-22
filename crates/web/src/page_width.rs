//! The decisions behind Settings' "Page width" section (technical design
//! §4.5): the choices it offers, the one a browser's kept value stands for,
//! what saving a choice keeps, and how each width lays out `<main>`.
//! Host-tested here; the DOM glue is `settings/page_width.rs`, and `page.rs`
//! keeps and reads the choice.

/// The `id` of the section's heading.
pub(crate) const HEADING_ID: &str = "page-width-heading";

/// The `name` of the radio group of the widths, and the start of each of its
/// radio buttons' `id`.
pub(crate) const GROUP: &str = "page-width";

/// How wide the page's content is: the one choice of Settings' "Page width"
/// section, kept in this browser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PageWidth {
    /// At most 60rem, centered: the choice until another is made.
    #[default]
    Narrow,
    /// The window's whole width, with paragraphs and forms kept to a
    /// readable line length.
    Full,
}

impl PageWidth {
    /// The width `kept` stands for: a choice's value, as its radio button
    /// gives it or as the browser kept it. What the browser kept is outside
    /// input, so anything but Full width's own value is Narrow.
    pub(crate) fn from_kept(kept: Option<&str>) -> Self {
        if kept == Some(FULL) {
            Self::Full
        } else {
            Self::Narrow
        }
    }

    /// The value of its radio button, which is also what saving it keeps
    /// when it is kept.
    pub(crate) const fn value(self) -> &'static str {
        match self {
            Self::Narrow => NARROW,
            Self::Full => FULL,
        }
    }

    /// What saving it keeps in the browser: Full width's value, or nothing
    /// for Narrow, which forgets the one kept.
    pub(crate) const fn to_keep(self) -> Option<&'static str> {
        match self {
            Self::Narrow => None,
            Self::Full => Some(FULL),
        }
    }

    /// The Tailwind utilities `<main>` takes, and the footer's frame
    /// (`components::Footer`), so the three line up: the page's frame
    /// ([`frame_class`](Self::frame_class)) and, in Full width, a bound that
    /// holds each paragraph and form in it to 70 characters of its own font,
    /// a readable line however wide the window is; tables and the rest take
    /// the whole width.
    pub(crate) const fn main_class(self) -> &'static str {
        match self {
            Self::Narrow => self.frame_class(),
            Self::Full => "[&_p]:max-w-[70ch] [&_form]:max-w-[70ch]",
        }
    }

    /// The Tailwind utilities of the page's frame, which `<main>`, the
    /// header's content and the footer's take: as wide as the choice lays the
    /// page. Narrow centers it at 60rem (240 steps of Tailwind's 0.25rem
    /// spacing), today's frame, and fills the line up to that
    /// (`inline-full`): in the app's column (`main.rs`) a box with auto
    /// inline margins would otherwise shrink to what it holds. Full width
    /// leaves it to the window.
    pub(crate) const fn frame_class(self) -> &'static str {
        match self {
            Self::Narrow => "mx-auto max-w-240 inline-full",
            Self::Full => "",
        }
    }
}

/// Narrow's value.
const NARROW: &str = "narrow";

/// Full width's value, which saving it keeps.
const FULL: &str = "full";

/// The choices, in the order the section offers them: Narrow, worded
/// `narrow`, then Full width, worded `full`.
pub(crate) fn options(narrow: String, full: String) -> Vec<(&'static str, String)> {
    vec![
        (PageWidth::Narrow.value(), narrow),
        (PageWidth::Full.value(), full),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_comes_first_then_full_width_each_under_its_value() {
        assert_eq!(
            options("Narrow".to_owned(), "Full width".to_owned()),
            [
                (PageWidth::Narrow.value(), "Narrow".to_owned()),
                (PageWidth::Full.value(), "Full width".to_owned()),
            ]
        );
        assert_ne!(PageWidth::Narrow.value(), PageWidth::Full.value());
    }

    #[test]
    fn each_choices_value_stands_for_it() {
        for width in [PageWidth::Narrow, PageWidth::Full] {
            assert_eq!(PageWidth::from_kept(Some(width.value())), width);
        }
    }

    #[test]
    fn full_width_is_kept_as_full_and_narrow_as_nothing() {
        assert_eq!(PageWidth::Full.to_keep(), Some("full"));
        assert_eq!(PageWidth::Narrow.to_keep(), None);
        for width in [PageWidth::Narrow, PageWidth::Full] {
            assert_eq!(PageWidth::from_kept(width.to_keep()), width);
        }
    }

    #[test]
    fn with_nothing_kept_or_a_value_settings_doesnt_keep_the_page_is_narrow() {
        for kept in [None, Some(""), Some("FULL"), Some(" full"), Some("wide")] {
            assert_eq!(
                PageWidth::from_kept(kept),
                PageWidth::Narrow,
                "kept {kept:?}"
            );
        }
    }

    #[test]
    fn narrow_centers_the_page_at_60rem_and_full_width_bounds_only_paragraphs_and_forms() {
        let narrow = PageWidth::Narrow.main_class();
        let full = PageWidth::Full.main_class();
        // 240 steps of Tailwind's 0.25rem spacing: 60rem.
        assert!(
            narrow.split(' ').any(|class| class == "max-w-240"),
            "{narrow}"
        );
        assert!(
            narrow.split(' ').any(|class| class == "mx-auto"),
            "{narrow}"
        );
        assert!(!full.contains("max-w-240"), "{full}");
        for measure in ["[&_p]:max-w-[70ch]", "[&_form]:max-w-[70ch]"] {
            assert!(full.split(' ').any(|class| class == measure), "{full}");
        }
    }

    #[test]
    fn the_frame_narrows_to_60rem_and_centers_and_full_width_bounds_nothing() {
        let narrow = PageWidth::Narrow.frame_class();
        // 240 steps of Tailwind's 0.25rem spacing: 60rem.
        assert!(
            narrow.split(' ').any(|class| class == "max-w-240"),
            "{narrow}"
        );
        assert!(
            narrow.split(' ').any(|class| class == "mx-auto"),
            "{narrow}"
        );
        assert_eq!(
            PageWidth::Full.frame_class(),
            "",
            "Full width's frame is the window"
        );
    }

    #[test]
    fn main_takes_every_class_of_the_frame_in_both_widths() {
        for width in [PageWidth::Narrow, PageWidth::Full] {
            let main = width.main_class();
            for class in width
                .frame_class()
                .split(' ')
                .filter(|class| !class.is_empty())
            {
                assert!(
                    main.split(' ').any(|taken| taken == class),
                    "main's classes `{main}` lack the frame's `{class}`"
                );
            }
        }
    }
}
