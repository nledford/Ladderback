//! The catalogs compiled into the binary. `build.rs` embeds every `.ftl`
//! file under `locales/<tag>/`; nothing is loaded at run time, and a locale
//! tag never reaches a file path.

/// One embedded `.ftl` file.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    /// The file's name, such as `common.ftl`.
    pub name: &'static str,
    pub text: &'static str,
}

/// All the files of one locale.
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    /// The locale's tag, the name of its directory: `en-US`.
    pub tag: &'static str,
    pub sources: &'static [Source],
}

/// The tag of the source locale: the one every message ID comes from and
/// every other locale falls back to.
pub const SOURCE_TAG: &str = "en-US";

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

/// Every embedded catalog, the source locale included.
pub fn catalogs() -> &'static [Catalog] {
    CATALOGS
}

/// Which of the catalogs it carries a build offers: the ones a language
/// can be negotiated into (technical design §8.4, decision 101).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Every catalog: a development build's offer, so that a translation
    /// can be reviewed in place.
    Every,
    /// Only the catalogs of these tags.
    Listed(&'static [&'static str]),
}

impl Offer {
    /// Whether the catalog of `tag` is offered.
    pub(crate) fn includes(self, _tag: &str) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::Offer;

    #[test]
    fn an_offer_of_listed_tags_includes_those_tags_alone() {
        let offer = Offer::Listed(&["en-US", "ja"]);
        assert!(offer.includes("en-US"));
        assert!(offer.includes("ja"));
        assert!(!offer.includes("de"));
        assert!(!offer.includes("en"));
    }

    #[test]
    fn an_offer_of_every_catalog_includes_any_tag() {
        assert!(Offer::Every.includes("de"));
        assert!(Offer::Every.includes("en-US"));
    }
}
