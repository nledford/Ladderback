//! The pseudolocales (technical design §8.4): `en-XA` and `ar-XB`, the `en-US`
//! catalog put through a transform. Text that comes out of a catalog changes,
//! and text that doesn't, such as an argument, an address or the operating
//! system's own words, stays as it was, so a screen of untouched English shows
//! what is written in the code instead of the catalog.
//!
//! A development tool. A release build is made without the `pseudo` feature,
//! which leaves `fluent-pseudo` out of it: [`Pseudo`] then has no value, so
//! nothing can ask for one, and `en-XA` and `ar-XB` are languages like any
//! other that no catalog serves.

use std::borrow::Cow;

use unic_langid::LanguageIdentifier;

/// A pseudolocale. It is asked for by its exact tag, and a localizer looks
/// messages up in it where it would look them up in `en-US`: it is that
/// catalog, transformed, in English's plural rules.
///
/// The `pseudo` feature gates the variants, not a twin of each function. With
/// it off the type has no value, so nothing is left that could ask for one,
/// and there is no second copy of the code for `cargo mutants` to mutate and
/// report missed: it mutates code a build leaves out.
#[derive(Clone, Copy)]
pub(crate) enum Pseudo {
    /// `en-XA`: every letter accented and the vowels doubled, so that text
    /// grows by about a third, as translations do.
    #[cfg(feature = "pseudo")]
    Accented,
    /// `ar-XB`: every letter upside down. A page in it declares Arabic, and
    /// so runs right to left; it tests the layout, not Arabic's shaping.
    #[cfg(feature = "pseudo")]
    Flipped,
}

impl Pseudo {
    /// Every pseudolocale this build has: none without the `pseudo` feature.
    const ALL: &[Self] = &[
        #[cfg(feature = "pseudo")]
        Self::Accented,
        #[cfg(feature = "pseudo")]
        Self::Flipped,
    ];

    /// The pseudolocale `tag` names, if it names one. Only the exact tag
    /// does: `en`, `en-GB`, `ar` and `ar-XA` ask for none.
    pub(crate) fn from_tag(tag: &LanguageIdentifier) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|pseudo| pseudo.tag() == *tag)
    }

    /// The tag it is asked for by, and is listed by in a chain of locales.
    pub(crate) fn tag(self) -> LanguageIdentifier {
        match self {
            #[cfg(feature = "pseudo")]
            Self::Accented => unic_langid::langid!("en-XA"),
            #[cfg(feature = "pseudo")]
            Self::Flipped => unic_langid::langid!("ar-XB"),
        }
    }

    /// The language a page in it declares in its `lang` attribute: its own tag
    /// for `en-XA`, and Arabic for `ar-XB`, which is why it runs right to left.
    pub(crate) fn language(self) -> LanguageIdentifier {
        match self {
            #[cfg(feature = "pseudo")]
            Self::Accented => unic_langid::langid!("en-XA"),
            #[cfg(feature = "pseudo")]
            Self::Flipped => unic_langid::langid!("ar"),
        }
    }

    /// What a bundle runs each text of a message through. The bundle reads
    /// it for the text of a message and its terms only, not for a value put
    /// into it, a number or a string literal.
    pub(crate) fn transform(self) -> fn(&str) -> Cow<'_, str> {
        match self {
            #[cfg(feature = "pseudo")]
            Self::Accented => |text| fluent_pseudo::transform(text, false, true),
            #[cfg(feature = "pseudo")]
            Self::Flipped => |text| fluent_pseudo::transform(text, true, false),
        }
    }
}

/// What `preferences` ask for in the way of a pseudolocale: the preferences
/// before the first pseudolocale in them, and that pseudolocale, when there
/// is one. The first one wins, and what comes after it is left out: it stands
/// in for `en-US`, which ends a chain of locales, so nothing after it would
/// be reached.
pub(crate) fn split(preferences: &[LanguageIdentifier]) -> (&[LanguageIdentifier], Option<Pseudo>) {
    let at = preferences
        .iter()
        .position(|tag| Pseudo::from_tag(tag).is_some())
        .unwrap_or(preferences.len());
    let (before, rest) = preferences
        .split_at_checked(at)
        .unwrap_or((preferences, &[]));
    (before, rest.first().and_then(Pseudo::from_tag))
}
