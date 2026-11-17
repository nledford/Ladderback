//! What an alert shows and how it is drawn (components §4.5): the tones it
//! takes from the shared vocabulary (`variants.rs`), the icon each tone shows
//! and the word that names that icon, so a tone is never told by its color
//! alone. `components/alert.rs`'s `Alert` is the glue that draws it.

use l10n::MessageId;
use lepticons_data::LucideGlyph;

use crate::variants::Tone;

/// The tones an alert is drawn in, each with an icon of its own and a word
/// that names it. An alert says how something went or tells the reader
/// something, so it has no Secondary, which would say neither and has no
/// icon of its own, and no Inherit, which would draw its icon and edge in
/// the text's color, leaving nothing but its words to tell its tone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertTone {
    /// Information (Ladder Blue, `--accent`): the vocabulary has no Info
    /// tone, since Primary covers it. The default.
    #[default]
    Primary,
    /// Something ended well (Tick Green).
    Success,
    /// Something needs a look (Flag Amber).
    Warning,
    /// Something failed (Red Ink).
    Error,
}

impl AlertTone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Primary, Self::Success, Self::Warning, Self::Error];

    /// The shared vocabulary's tone it is.
    pub fn tone(self) -> Tone {
        todo!()
    }

    /// The Lucide glyph its icon shows, one shape per tone, the status
    /// badge's for success, warnings and failure.
    pub fn glyph(self) -> LucideGlyph {
        todo!()
    }

    /// The word that names its icon for a screen reader ("Error"), so the
    /// tone is said before the alert's words.
    pub fn label(self) -> MessageId {
        todo!()
    }
}

/// The classes of the alert: the alert's own, which draws its box, and its
/// tone's, which its icon and its edge take.
pub fn alert_class(tone: AlertTone) -> String {
    let _ = tone;
    todo!()
}

/// The name of an alert's Dismiss button, which its tooltip shows too.
pub fn dismiss_name() -> MessageId {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use l10n::msg;

    #[test]
    fn an_alert_is_information_in_ladder_blue_unless_its_page_names_another_tone() {
        assert_eq!(AlertTone::default(), AlertTone::Primary);
        assert_eq!(AlertTone::Primary.tone(), Tone::Primary);
        assert_eq!(AlertTone::Success.tone(), Tone::Success);
        assert_eq!(AlertTone::Warning.tone(), Tone::Warning);
        assert_eq!(AlertTone::Error.tone(), Tone::Error);
    }

    #[test]
    fn each_tone_shows_an_icon_of_its_own_named_by_a_word_of_its_own() {
        let drawn = AlertTone::ALL.map(|tone| (tone.glyph(), tone.label()));
        assert_eq!(
            drawn,
            [
                (LucideGlyph::Info, msg::UI_ALERT_INFORMATION),
                (LucideGlyph::CircleCheck, msg::UI_ALERT_SUCCESS),
                (LucideGlyph::TriangleAlert, msg::UI_ALERT_WARNING),
                (LucideGlyph::CircleX, msg::UI_ALERT_ERROR),
            ]
        );
    }

    #[test]
    fn an_alert_is_drawn_by_its_own_class_and_its_tone() {
        for tone in AlertTone::ALL {
            let class = alert_class(tone);
            let classes: Vec<&str> = class.split_whitespace().collect();
            assert_eq!(classes.first(), Some(&"alert"), "{class}");
            assert!(classes.contains(&tone.tone().class()), "{class}");
        }
    }

    #[test]
    fn its_dismiss_button_is_named_dismiss() {
        assert_eq!(dismiss_name(), msg::UI_ALERT_DISMISS);
    }
}
