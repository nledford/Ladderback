//! What a banner shows, how it is drawn and whether a dismissal keeps it
//! away (components §4.5): the tones it takes from the shared vocabulary
//! (`variants.rs`), the icon each tone shows and the word that names that
//! icon, so a tone is never told by its color alone, and the browser's memory
//! of a dismissal. `components/banner.rs`'s `Banner` is the glue that draws
//! it and reads and writes that memory.

use l10n::{MessageId, msg};
use lepticons_data::LucideGlyph;

use crate::variants::Tone;

/// The tones a banner is drawn in, each with an icon of its own and a word
/// that names it. A banner tells the reader of something that lasts across
/// the pages, so it is information or a warning: no Success, which a toast
/// says once and lets go; no Error, since a failure is stated once where it
/// happened and never as a red banner (DESIGN.md); no Secondary, which says
/// no tone and has no icon of its own; and no Inherit, which would draw its
/// icon and edge in the text's color, leaving nothing but its words to tell
/// its tone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BannerTone {
    /// Information (Ladder Blue, `--accent`): the vocabulary has no Info
    /// tone, since Primary covers it. The default.
    #[default]
    Primary,
    /// Something needs a look (Flag Amber).
    Warning,
}

impl BannerTone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 2] = [Self::Primary, Self::Warning];

    /// The shared vocabulary's tone it is.
    pub fn tone(self) -> Tone {
        todo!()
    }

    /// The Lucide glyph its icon shows, one shape per tone, the status
    /// badge's for warnings.
    pub fn glyph(self) -> LucideGlyph {
        todo!()
    }

    /// The word that names its icon for a screen reader ("Warning"), so the
    /// tone is said before the banner's words.
    pub fn label(self) -> MessageId {
        todo!()
    }
}

/// The classes of the banner: the banner's own, which names it, its tone's,
/// which its icon and its edge take, and its look.
pub fn banner_class(tone: BannerTone) -> String {
    let _ = tone;
    todo!()
}

/// The name of a banner's Dismiss button, which its tooltip shows too.
pub fn dismiss_name() -> MessageId {
    todo!()
}

/// The key of the browser's local storage that remembers the dismissal of
/// the banner named `banner`, one per banner, so that dismissing one keeps
/// no other away.
pub fn storage_key(banner: &str) -> String {
    let _ = banner;
    todo!()
}

/// Whether a banner stays away: `kept` is what the browser remembers of the
/// banner's last dismissal, the edition it was dismissed in, and `edition`
/// is the one the page would show now. Only a dismissal of this very
/// edition keeps it away, so a newer one (the next pending update) shows
/// again. What the browser keeps is outside input, and anything but this
/// edition, or nothing, shows the banner.
pub fn stays_dismissed(kept: Option<&str>, edition: &str) -> bool {
    let _ = (kept, edition);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_banner_is_information_in_ladder_blue_unless_its_page_names_a_warning() {
        assert_eq!(BannerTone::default(), BannerTone::Primary);
        assert_eq!(BannerTone::Primary.tone(), Tone::Primary);
        assert_eq!(BannerTone::Warning.tone(), Tone::Warning);
    }

    #[test]
    fn each_tone_shows_an_icon_of_its_own_named_by_a_word_of_its_own() {
        let drawn = BannerTone::ALL.map(|tone| (tone.glyph(), tone.label()));
        assert_eq!(
            drawn,
            [
                (LucideGlyph::Info, msg::UI_BANNER_INFORMATION),
                (LucideGlyph::TriangleAlert, msg::UI_BANNER_WARNING),
            ]
        );
    }

    #[test]
    fn a_banner_is_drawn_by_its_own_class_and_its_tone() {
        for tone in BannerTone::ALL {
            let class = banner_class(tone);
            let classes: Vec<&str> = class.split_whitespace().collect();
            assert_eq!(classes.first(), Some(&"banner"), "{class}");
            assert!(classes.contains(&tone.tone().class()), "{class}");
        }
    }

    #[test]
    fn its_dismiss_button_is_named_dismiss() {
        assert_eq!(dismiss_name(), msg::UI_BANNER_DISMISS);
    }

    #[test]
    fn each_banner_is_remembered_under_a_key_of_its_own() {
        assert_eq!(storage_key("update"), "ladderback-banner-update");
        assert_ne!(storage_key("update"), storage_key("sign-in-off"));
    }

    #[test]
    fn only_a_dismissal_of_the_edition_shown_keeps_a_banner_away() {
        assert!(stays_dismissed(Some("0.6.0"), "0.6.0"));
        assert!(!stays_dismissed(None, "0.6.0"));
        assert!(!stays_dismissed(Some("0.5.0"), "0.6.0"));
        assert!(!stays_dismissed(Some(""), "0.6.0"));
        assert!(!stays_dismissed(Some("0.6.0 "), "0.6.0"));
    }
}
