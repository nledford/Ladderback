//! The languages a person can choose to see the web UI in, in Settings
//! (technical design §8.4, decision 101): the ones this build offers, each
//! named in itself, as its readers write it.

use unic_langid::LanguageIdentifier;

use crate::catalog::{Catalog, Offer};
use crate::locale::Direction;
use crate::localizer::Localizer;
use crate::pseudo::Pseudo;
use crate::{format, msg};

/// A language this build offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Language {
    /// Its tag, as the choice of it is kept: `en-US`, `ja`, `ar-XB`.
    pub tag: &'static str,
    /// Its name written in itself: `English`, `日本語`.
    pub name: String,
    /// The language the name is written in, which a page marks it with
    /// (`lang`, WCAG 3.1.2): the one [`Localizer::language`] declares for
    /// the tag, so `ar` for `ar-XB`.
    ///
    /// [`Localizer::language`]: crate::Localizer::language
    pub language: LanguageIdentifier,
    /// The direction the name runs.
    pub direction: Direction,
}

/// The languages this build offers, each named in itself: the catalogs it
/// offers, in their order, then, in a development build, the pseudolocales.
pub fn languages() -> Vec<Language> {
    languages_of(crate::catalog::catalogs(), Offer::BUILD, Pseudo::ALL)
}

/// Whether this build offers `tag`, written exactly as [`languages`] gives
/// it: the one rule a kept choice meets, both for the page, which asks for it
/// then, and for Settings, which shows it chosen then. It reads no catalog.
pub(crate) fn offers(tag: &str) -> bool {
    tags(crate::catalog::catalogs(), Offer::BUILD, Pseudo::ALL).any(|offered| offered == tag)
}

/// The tags of the catalogs among `catalogs` that `offer` includes, in their
/// order, then those of `pseudos`.
fn tags<'a>(
    catalogs: &'a [Catalog],
    offer: Offer,
    pseudos: &'a [Pseudo],
) -> impl Iterator<Item = &'static str> + 'a {
    catalogs
        .iter()
        .map(|catalog| catalog.tag)
        .filter(move |tag| offer.includes(tag))
        .chain(pseudos.iter().map(|pseudo| pseudo.code()))
}

/// The languages among `catalogs` that `offer` includes, then `pseudos`.
/// Each is named by its own catalog's `ui-language-name`, read through a
/// localizer of that language alone, which a pseudolocale transforms.
fn languages_of(catalogs: &[Catalog], offer: Offer, pseudos: &[Pseudo]) -> Vec<Language> {
    tags(catalogs, offer, pseudos)
        .filter_map(|tag| {
            let own = Localizer::on::<format::Active>(&[tag.parse().ok()?], catalogs, offer);
            Some(Language {
                tag,
                name: own.message(msg::UI_LANGUAGE_NAME, &[]).into_text(),
                language: own.language(),
                direction: own.direction(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Source;

    const CATALOGS: [Catalog; 3] = [
        Catalog {
            tag: "en-US",
            sources: &[Source {
                name: "ui.ftl",
                text: "ui-language-name = English\n",
            }],
        },
        Catalog {
            tag: "ja",
            sources: &[Source {
                name: "ui.ftl",
                text: "ui-language-name = 日本語\n",
            }],
        },
        Catalog {
            tag: "fa",
            sources: &[Source {
                name: "ui.ftl",
                text: "ui-language-name = فارسی\n",
            }],
        },
    ];

    fn tag(raw: &str) -> LanguageIdentifier {
        raw.parse().unwrap()
    }

    #[test]
    fn each_language_offered_is_named_in_itself_in_the_order_of_the_catalogs() {
        let languages = languages_of(&CATALOGS, Offer::Every, &[]);
        assert_eq!(
            languages,
            [
                Language {
                    tag: "en-US",
                    name: "English".to_owned(),
                    language: tag("en-US"),
                    direction: Direction::Ltr,
                },
                Language {
                    tag: "ja",
                    name: "日本語".to_owned(),
                    language: tag("ja"),
                    direction: Direction::Ltr,
                },
                Language {
                    tag: "fa",
                    name: "فارسی".to_owned(),
                    language: tag("fa"),
                    direction: Direction::Rtl,
                },
            ]
        );
    }

    #[test]
    fn a_catalog_the_build_does_not_offer_is_not_a_language_to_choose() {
        let languages = languages_of(&CATALOGS, Offer::Listed(&["en-US", "fa"]), &[]);
        let tags: Vec<&str> = languages.iter().map(|language| language.tag).collect();
        assert_eq!(tags, ["en-US", "fa"]);
    }

    #[cfg(feature = "pseudo")]
    #[test]
    fn a_development_build_offers_the_pseudolocales_last_each_named_in_itself() {
        let languages = languages();
        let named: Vec<(&str, &str, String, Direction)> = languages
            .iter()
            .map(|language| {
                (
                    language.tag,
                    language.name.as_str(),
                    language.language.to_string(),
                    language.direction,
                )
            })
            .collect();
        let pseudolocales = [
            ("en-XA", "Eƞɠŀişħ", "en-XA".to_owned(), Direction::Ltr),
            ("ar-XB", "Ǝuƃʅısɥ", "ar".to_owned(), Direction::Rtl),
        ];
        assert_eq!(named.get(named.len() - 2..), Some(&pseudolocales[..]));
        assert_eq!(
            named.first(),
            Some(&("en-US", "English", "en-US".to_owned(), Direction::Ltr))
        );
    }
}
