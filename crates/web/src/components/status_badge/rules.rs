//! What a status badge shows for each state (components §4.5): its badge's
//! tone, one of the badge's tones (`badge/rules.rs`), a status tone's pair
//! for an operation that ended (decision 108), whose words on its fill the
//! contrast gate checks, and its icon, a shape of its own for every state,
//! so a reader who can't tell the colors apart scans a table of them
//! without reading each label (A2). `components/status_badge.rs`'s
//! `StatusBadge` is the glue that draws them, as a `Badge`.

use lepticons_data::LucideGlyph;

use crate::components::badge::rules::BadgeTone;

/// A state a status badge shows: one an operation is in, or none to tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeState {
    /// The operation waits for its turn.
    Queued,
    /// The operation runs.
    Running,
    /// The operation ended well.
    Succeeded,
    /// The operation ended well, with warnings.
    SucceededWithWarnings,
    /// The operation ended badly.
    Failed,
    /// The operation was cancelled.
    Canceled,
    /// Nothing to tell: no result, or a status this build doesn't know.
    Empty,
}

impl BadgeState {
    /// Every state, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 7] = [
        Self::Queued,
        Self::Running,
        Self::Succeeded,
        Self::SucceededWithWarnings,
        Self::Failed,
        Self::Canceled,
        Self::Empty,
    ];

    /// The badge's tone, whose pair of tokens its pill is drawn in: Ladder
    /// Blue's while it runs, green once it ended well, amber once it ended
    /// well with warnings, a success with something to read (#942), red once
    /// it failed, and gray while there is nothing to tell yet, or any longer.
    pub fn tone(self) -> BadgeTone {
        match self {
            Self::Running => BadgeTone::Primary,
            Self::Succeeded => BadgeTone::Success,
            Self::SucceededWithWarnings => BadgeTone::Warning,
            Self::Failed => BadgeTone::Error,
            Self::Queued | Self::Canceled | Self::Empty => BadgeTone::Secondary,
        }
    }

    /// The icon beside the label, its shape the state's own (the
    /// maintainer's choice on #384).
    pub fn glyph(self) -> LucideGlyph {
        match self {
            Self::Queued => LucideGlyph::Clock,
            Self::Running => LucideGlyph::LoaderCircle,
            Self::Succeeded => LucideGlyph::CircleCheck,
            // Lucide's `triangle-exclamation-point`, by the earlier name the
            // crate's Lucide release gives it.
            Self::SucceededWithWarnings => LucideGlyph::TriangleAlert,
            Self::Failed => LucideGlyph::CircleX,
            Self::Canceled => LucideGlyph::CircleSlash,
            Self::Empty => LucideGlyph::CircleDashed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_has_an_icon_of_its_own() {
        let icons = [
            (BadgeState::Queued, LucideGlyph::Clock),
            (BadgeState::Running, LucideGlyph::LoaderCircle),
            (BadgeState::Succeeded, LucideGlyph::CircleCheck),
            // Lucide's `triangle-exclamation-point`, by its earlier name.
            (
                BadgeState::SucceededWithWarnings,
                LucideGlyph::TriangleAlert,
            ),
            (BadgeState::Failed, LucideGlyph::CircleX),
            (BadgeState::Canceled, LucideGlyph::CircleSlash),
            (BadgeState::Empty, LucideGlyph::CircleDashed),
        ];
        for (state, icon) in icons {
            assert_eq!(state.glyph(), icon, "{state:?}");
        }
        let mut shapes: Vec<_> = BadgeState::ALL.iter().map(|state| state.glyph()).collect();
        shapes.sort();
        shapes.dedup();
        assert_eq!(
            shapes.len(),
            BadgeState::ALL.len(),
            "no two states share an icon"
        );
    }

    #[test]
    fn each_state_is_drawn_in_the_badge_tone_of_its_kind() {
        let tones = [
            (BadgeState::Queued, BadgeTone::Secondary),
            (BadgeState::Running, BadgeTone::Primary),
            (BadgeState::Succeeded, BadgeTone::Success),
            (BadgeState::SucceededWithWarnings, BadgeTone::Warning),
            (BadgeState::Failed, BadgeTone::Error),
            (BadgeState::Canceled, BadgeTone::Secondary),
            (BadgeState::Empty, BadgeTone::Secondary),
        ];
        for (state, tone) in tones {
            assert_eq!(state.tone(), tone, "{state:?}");
        }
    }
}
