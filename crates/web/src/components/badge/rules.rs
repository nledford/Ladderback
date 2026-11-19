//! What a badge shows and how it is drawn (components §4.5): its tones,
//! named as the shared vocabulary (`variants.rs`) names them, each words on
//! a fill the contrast gate checks, the status tones' pairs for Success,
//! Warning and Error (decision 108), and its look, in Tailwind utilities.
//! `components/badge.rs`'s `Badge` is the glue that draws it.

use crate::variants::StatusTone;

/// The tones a badge is drawn in, named as the shared vocabulary names them,
/// each a pill of its own pair of tokens: its words in the text token on the
/// background token, which `scripts/check-contrast.ts` holds to 4.5:1 in
/// both color schemes. A tone never says anything alone (1.4.1): the badge's
/// words do. No Inherit, which would leave a pill with no fill of its own,
/// its words the text around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeTone {
    /// Ladder Blue's pale pair (`--badge-loading-*`): something under way.
    Primary,
    /// The gray pair (`--badge-empty-*`): a count or a tag that says
    /// nothing went well or badly. The default: Ladder Blue is the accent
    /// for action, and a badge is none.
    #[default]
    Secondary,
    /// Tick Green's status pair (`--success-*`, the vocabulary's
    /// `StatusTone::Success`): something ended well.
    Success,
    /// Flag Amber's status pair (`--warning-*`, `StatusTone::Warning`):
    /// something needs a look.
    Warning,
    /// Red Ink's status pair (`--error-*`, `StatusTone::Error`): something
    /// failed.
    Error,
}

impl BadgeTone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 5] = [
        Self::Primary,
        Self::Secondary,
        Self::Success,
        Self::Warning,
        Self::Error,
    ];

    /// Its words' token and its pill's, by their Tailwind names, written out
    /// whole so Tailwind finds them in the source: the badge's own Loading
    /// and Empty pairs for Primary and Secondary, and the status tone's pair
    /// for Success, Warning and Error, the one every component tinted in a
    /// status shares.
    fn pair(self) -> &'static str {
        match self {
            Self::Primary => "text-badge-loading-fg bg-badge-loading-bg",
            Self::Secondary => "text-badge-empty-fg bg-badge-empty-bg",
            Self::Success => StatusTone::Success.pair(),
            Self::Warning => StatusTone::Warning.pair(),
            Self::Error => StatusTone::Error.pair(),
        }
    }
}

/// How a badge is drawn, in Tailwind utilities (decision 117), logical ones
/// only, the same in every tone: a pill 0.125rem by 0.5rem around its words,
/// in rem, so it grows with the reader's text, and never wider than its room
/// (`max-inline-full`), so its words wrap inside it rather than run past a
/// narrow column. Its corner radius is half a line plus its block padding
/// (`0.5lh + 0.125rem`, which no utility names): a pill on one line, a
/// rounded box on two, where a full radius would cut into the words. Its
/// icon ([`ICON`]) and its words ([`WORDS`]) are two flex items, 0.25em
/// apart where they share a line. The badge sits on its words' baseline, as
/// plain text does, while they share the first line with the icon, whose
/// box takes no part in baseline alignment; words wrapped under the icon
/// leave the badge on the icon's box. Under forced colors the system drops
/// the fill, so the words take the color of the text around them, as the
/// vocabulary's tones do (the system's text color), whether or not the
/// engine forces it, and a 1px edge in that color, drawn inside the box (an
/// outline with a negative offset), so a frame that clips its content or
/// the badge beside it never cuts or covers the edge, keeps each badge apart
/// from the text and the badge beside it.
const LOOK: &str = "inline-flex max-inline-full flex-wrap items-baseline gap-x-[0.25em] \
    rounded-[calc(0.5lh_+_0.125rem)] py-0.5 px-2 \
    forced-colors:text-inherit forced-colors:outline-1 forced-colors:outline-solid \
    forced-colors:-outline-offset-1 forced-colors:outline-current";

/// The box around a badge's icon: one line of its words tall (`1lh`, which
/// no utility names), the icon centered in it, at the start of the badge's
/// line, so the icon stands centered on the words' first line however many
/// lines they take.
pub const ICON: &str = "flex flex-none self-start items-center [min-block-size:1lh]";

/// A badge's words (#1070): they take the room the icon leaves and wrap
/// there between words. They start as wide as their longest word
/// (`min-content`, which no utility names), so where that doesn't fit beside
/// the icon (a narrow column, larger text) they go under it, a line of the
/// badge's own, rather than break a word beside it; an alert's words, which
/// may hold a long path, never go under its icon. There a word longer than
/// the whole line breaks inside (`break-word`); `anywhere`, the page's,
/// would count a break between any two letters as their narrowest and keep
/// them beside the icon a letter or two a line. `min-inline-size: 0` lets
/// them narrow to that line.
pub const WORDS: &str = "flex-[1_1_min-content] [min-inline-size:0] wrap-break-word";

/// The classes of a badge in `tone`: the badge's own, which names it, its
/// tone's pair of tokens and its look.
pub fn badge_class(tone: BadgeTone) -> String {
    format!("badge {} {LOOK}", tone.pair())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_badge_is_gray_unless_its_page_names_another_tone() {
        assert_eq!(BadgeTone::default(), BadgeTone::Secondary);
    }

    /// Whether `class` is one of a pair's two utilities: a words token
    /// (`text-…-fg`) or a fill token (`bg-…-bg`).
    fn is_pair_color(class: &str) -> bool {
        (class.starts_with("text-") && class.ends_with("-fg"))
            || (class.starts_with("bg-") && class.ends_with("-bg"))
    }

    #[test]
    fn each_tone_draws_its_words_and_its_pill_in_a_pair_of_tokens_of_its_own() {
        // Primary and Secondary are the badge's own pairs; Success, Warning
        // and Error are the status tones' pairs every component shares
        // (decision 108).
        let pairs = [
            (
                BadgeTone::Primary,
                ["text-badge-loading-fg", "bg-badge-loading-bg"],
            ),
            (
                BadgeTone::Secondary,
                ["text-badge-empty-fg", "bg-badge-empty-bg"],
            ),
            (BadgeTone::Success, ["text-success-fg", "bg-success-bg"]),
            (BadgeTone::Warning, ["text-warning-fg", "bg-warning-bg"]),
            (BadgeTone::Error, ["text-error-fg", "bg-error-bg"]),
        ];
        for (tone, pair) in pairs {
            let class = badge_class(tone);
            let classes: Vec<&str> = class.split_whitespace().collect();
            for token in pair {
                assert!(classes.contains(&token), "{tone:?}: {class}");
            }
            // No other tone's pair: one color for the words, one for the pill.
            let colors = classes.iter().filter(|class| is_pair_color(class)).count();
            assert_eq!(colors, 2, "{tone:?}: {class}");
        }
    }

    #[test]
    fn a_badge_is_named_by_its_own_class_and_its_tone_changes_only_its_colors() {
        let look = |tone| {
            badge_class(tone)
                .split_whitespace()
                .filter(|class| !is_pair_color(class))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let secondary = look(BadgeTone::Secondary);
        assert_eq!(secondary.first().map(String::as_str), Some("badge"));
        for tone in BadgeTone::ALL {
            assert_eq!(look(tone), secondary, "{tone:?}");
        }
    }
}
