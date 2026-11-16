//! Which languages a build offers (technical design §8.4, decision 101). A
//! release, built without the default features, offers only the languages
//! `offered-locales.txt` lists, which its checks keep to the complete and
//! current ones; a development build offers every catalog it carries, so a
//! translation can be reviewed in place, as the pseudolocales are. German
//! here is a translation that lacks a message and that the list doesn't name.

use l10n::catalog::{Catalog, Source};
use l10n::{Localizer, MessageId};

use crate::support::tag;

const EN: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "main.ftl",
        text: "greeting = Hello\nfarewell = Goodbye\n",
    }],
};

const DE: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "main.ftl",
        text: "greeting = Hallo\n",
    }],
};

const GREETING: MessageId = MessageId::new("greeting");

fn chain(localizer: &Localizer) -> Vec<String> {
    localizer
        .locales()
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[cfg(feature = "pseudo")]
#[test]
fn a_development_build_offers_a_language_the_list_does_not_name_for_review() {
    let localizer = Localizer::offered(&[tag("de")], &[EN, DE]);
    assert_eq!(chain(&localizer), ["de", "en-US"]);
    assert_eq!(localizer.message(GREETING, &[]).text(), "Hallo");
}

#[cfg(not(feature = "pseudo"))]
#[test]
fn a_release_build_serves_a_language_the_list_does_not_name_in_american_english() {
    let localizer = Localizer::offered(&[tag("de")], &[EN, DE]);
    assert_eq!(chain(&localizer), ["en-US"]);
    assert_eq!(localizer.language(), tag("en-US"));
    assert_eq!(localizer.message(GREETING, &[]).text(), "Hello");
}

#[test]
fn every_build_offers_american_english() {
    let localizer = Localizer::offered(&[tag("en-US")], &[EN, DE]);
    assert_eq!(chain(&localizer), ["en-US"]);
    assert_eq!(localizer.message(GREETING, &[]).text(), "Hello");
}
