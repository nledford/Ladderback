//! The decisions behind Settings' "Page width" section (technical design
//! §4.5): the choices it offers, the one a browser's kept value stands for,
//! what saving a choice keeps, and how each width lays out `<main>`.
//! Host-tested here; the DOM glue is `settings/page_width.rs`, and `page.rs`
//! keeps and reads the choice.

/// The `id` of the section.
pub(crate) const SECTION_ID: &str = "page-width";

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
        let _ = kept;
        todo!()
    }

    /// The value of its radio button, which is also what saving it keeps
    /// when it is kept.
    pub(crate) const fn value(self) -> &'static str {
        todo!()
    }

    /// What saving it keeps in the browser: Full width's value, or nothing
    /// for Narrow, which forgets the one kept.
    pub(crate) const fn to_keep(self) -> Option<&'static str> {
        todo!()
    }

    /// The Tailwind utilities `<main>` takes.
    pub(crate) const fn main_class(self) -> &'static str {
        todo!()
    }
}

/// The choices, in the order the section offers them: Narrow, worded
/// `narrow`, then Full width, worded `full`.
pub(crate) fn options(narrow: String, full: String) -> Vec<(&'static str, String)> {
    let _ = (narrow, full);
    todo!()
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
}
