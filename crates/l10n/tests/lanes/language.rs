//! The language and direction a page declares (technical design §8.4): the
//! `lang` and `dir` of `<html>` follow the first locale of the chain, so a
//! screen reader speaks the page in the right voice and a right-to-left page
//! lays out right to left. The pseudolocales' own rows are in `pseudo`.

use l10n::catalog::{Catalog, Source};
use l10n::{Direction, Localizer};

use crate::support::tag;

const HELLO: &[Source] = &[Source {
    name: "hello.ftl",
    text: "hello = Hello\n",
}];

/// A catalog of one message for the locale `tag`.
fn catalog(tag: &'static str) -> Catalog {
    Catalog {
        tag,
        sources: HELLO,
    }
}

#[test]
fn a_page_in_american_english_declares_en_us_and_runs_left_to_right() {
    let localizer = Localizer::new(&[]);
    assert_eq!(localizer.language(), tag("en-US"));
    assert_eq!(localizer.direction(), Direction::Ltr);
}

#[test]
fn a_page_declares_the_first_locale_it_is_shown_in() {
    let catalogs = [catalog("en-US"), catalog("de")];
    let localizer = Localizer::from_catalogs(&[tag("de-DE")], &catalogs);
    assert_eq!(localizer.language(), tag("de"));
    assert_eq!(localizer.direction(), Direction::Ltr);
}

#[test]
fn a_page_with_no_catalog_at_all_declares_en_us() {
    let localizer = Localizer::from_catalogs(&[tag("ar")], &[]);
    assert!(localizer.locales().is_empty());
    assert_eq!(localizer.language(), tag("en-US"));
    assert_eq!(localizer.direction(), Direction::Ltr);
}

#[test]
fn a_page_in_a_language_written_right_to_left_runs_right_to_left() {
    for language in ["ar", "he", "fa", "ur"] {
        let catalogs = [catalog("en-US"), catalog(language)];
        let localizer = Localizer::from_catalogs(&[tag(language)], &catalogs);
        assert_eq!(localizer.language(), tag(language));
        assert_eq!(localizer.direction(), Direction::Rtl, "{language}");
    }
}

#[test]
fn the_direction_follows_the_first_locale_and_not_one_further_down_the_chain() {
    // Arabic is asked for second, and served, but German comes first.
    let catalogs = [catalog("en-US"), catalog("de"), catalog("ar")];
    let localizer = Localizer::from_catalogs(&[tag("de"), tag("ar")], &catalogs);
    assert_eq!(localizer.language(), tag("de"));
    assert_eq!(localizer.direction(), Direction::Ltr);
}
