//! The decisions behind Settings' "Language" section (technical design §8.4,
//! decision 101): the choices it offers, the one it shows chosen and the
//! language saving a choice keeps in the browser. Host-tested here; the DOM
//! glue is `settings/language.rs`, and `page.rs` keeps and reads the choice.

use l10n::Language;

/// The `id` of the section, which the page's address names by its fragment
/// when the page is shown again after a save, so focus comes back to it.
pub(crate) const SECTION_ID: &str = "language";

/// The `id` of the section's heading.
pub(crate) const HEADING_ID: &str = "language-heading";

/// The `name` of the radio group of the languages, and the start of each of
/// its radio buttons' `id`.
pub(crate) const GROUP: &str = "language";

/// The value of the choice that leaves the language to the browser's
/// languages. No language tag is spelled so: a catalog's tag names a folder
/// of `locales/`, and none is named this.
pub(crate) const BROWSER: &str = "browser";

/// The choices, in the order the section offers them: the browser's
/// languages, worded `browser`, then each language `languages` holds, by its
/// own name.
pub(crate) fn options(browser: String, languages: &[Language]) -> Vec<(&'static str, String)> {
    std::iter::once((BROWSER, browser))
        .chain(
            languages
                .iter()
                .map(|language| (language.tag, language.name.clone())),
        )
        .collect()
}

/// The choice shown chosen: the language `kept` names when it is one of
/// `languages`, else the browser's languages. What the browser kept is
/// outside input, and a choice it no longer offers is no choice.
pub(crate) fn chosen(kept: Option<&str>, languages: &[Language]) -> &'static str {
    kept.and_then(|kept| to_keep(kept, languages))
        .unwrap_or(BROWSER)
}

/// The tag saving the choice `value` keeps in the browser: the language's
/// own, or none for the browser's languages, which forgets the one kept.
pub(crate) fn to_keep(value: &str, languages: &[Language]) -> Option<&'static str> {
    languages
        .iter()
        .find(|language| language.tag == value)
        .map(|language| language.tag)
}

#[cfg(test)]
mod tests {
    use l10n::Direction;

    use super::*;

    fn language(tag: &'static str, name: &str) -> Language {
        Language {
            tag,
            name: name.to_owned(),
            language: tag.parse().unwrap(),
            direction: Direction::Ltr,
        }
    }

    fn offered() -> Vec<Language> {
        vec![language("en-US", "English"), language("ja", "日本語")]
    }

    #[test]
    fn the_browsers_languages_come_first_then_each_language_by_its_own_name() {
        assert_eq!(
            options("The browser's languages".to_owned(), &offered()),
            [
                (BROWSER, "The browser's languages".to_owned()),
                ("en-US", "English".to_owned()),
                ("ja", "日本語".to_owned()),
            ]
        );
    }

    #[test]
    fn the_language_kept_is_shown_chosen_when_it_is_offered() {
        assert_eq!(chosen(Some("ja"), &offered()), "ja");
        assert_eq!(chosen(Some("en-US"), &offered()), "en-US");
    }

    #[test]
    fn with_no_language_kept_or_one_not_offered_the_browsers_languages_are_chosen() {
        for kept in [None, Some("fr"), Some(""), Some("JA"), Some(BROWSER)] {
            assert_eq!(chosen(kept, &offered()), BROWSER, "kept {kept:?}");
        }
    }

    #[test]
    fn saving_a_language_keeps_its_tag_and_saving_the_browsers_languages_keeps_none() {
        assert_eq!(to_keep("ja", &offered()), Some("ja"));
        assert_eq!(to_keep(BROWSER, &offered()), None);
        assert_eq!(to_keep("fr", &offered()), None);
    }
}
