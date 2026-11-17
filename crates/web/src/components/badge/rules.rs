//! What a badge shows and how it is drawn (components §4.5): the tones it
//! takes from the shared vocabulary (`variants.rs`), each a text token on a
//! background token the contrast gate checks, and its look, in Tailwind
//! utilities. `components/badge.rs`'s `Badge` is the glue that draws it.

/// The tones a badge is drawn in, named as the shared vocabulary names them,
/// each a pill of its own pair of tokens: its words in the text token on the
/// background token, which `scripts/check-contrast.ts` holds to 4.5:1 in
/// both color schemes. A tone never says anything alone (1.4.1): the badge's
/// words do. No Inherit, which would leave a pill with no fill of its own,
/// its words the text around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeTone {
    /// Ladder Blue's pale pair (`--badge-loading-*`): something under way.
    Primary,
    /// The gray pair (`--badge-empty-*`): a count or a tag that says
    /// nothing went well or badly. The default: Ladder Blue is the accent
    /// for action, and a badge is none.
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
}

impl Default for BadgeTone {
    fn default() -> Self {
        todo!()
    }
}

/// The classes of a badge in `tone`: the badge's own, which names it, its
/// tone's pair of tokens and its look.
pub fn badge_class(tone: BadgeTone) -> String {
    let _ = tone;
    todo!()
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
