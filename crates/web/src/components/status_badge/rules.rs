//! What a status badge shows for each state (components §4.5): its pill, a
//! pair of Tailwind utilities, its words on its fill, whose tokens the
//! contrast gate checks, a status tone's pair for an operation that ended
//! (decision 108), and its icon, a shape of its own for every state, so a
//! reader who can't tell the colors apart scans a table of them without
//! reading each label (A2). `components/status_badge.rs`'s `StatusBadge` is
//! the glue that draws them.

use crate::variants::StatusTone;
use lepticons_data::LucideGlyph;

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
    Cancelled,
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
        Self::Cancelled,
        Self::Empty,
    ];

    /// The pill the badge is drawn on, its words' utility and its fill's:
    /// Success's pair for an operation that succeeded, with warnings too
    /// (its icon and its words tell the warnings), Error's for one that
    /// failed, and the badge's own Loading and Empty pairs for the rest.
    pub fn pill(self) -> &'static str {
        match self {
            Self::Running => "text-badge-loading-fg bg-badge-loading-bg",
            Self::Succeeded | Self::SucceededWithWarnings => StatusTone::Success.pair(),
            Self::Failed => StatusTone::Error.pair(),
            Self::Queued | Self::Cancelled | Self::Empty => "text-badge-empty-fg bg-badge-empty-bg",
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
            Self::Cancelled => LucideGlyph::CircleSlash,
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
            (BadgeState::Cancelled, LucideGlyph::CircleSlash),
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
    fn each_state_is_drawn_on_its_pill_a_succeeded_or_failed_one_in_its_status_tones_pair() {
        let pills = [
            (BadgeState::Queued, "text-badge-empty-fg bg-badge-empty-bg"),
            (
                BadgeState::Running,
                "text-badge-loading-fg bg-badge-loading-bg",
            ),
            (BadgeState::Succeeded, StatusTone::Success.pair()),
            (
                BadgeState::SucceededWithWarnings,
                StatusTone::Success.pair(),
            ),
            (BadgeState::Failed, StatusTone::Error.pair()),
            (
                BadgeState::Cancelled,
                "text-badge-empty-fg bg-badge-empty-bg",
            ),
            (BadgeState::Empty, "text-badge-empty-fg bg-badge-empty-bg"),
        ];
        for (state, pill) in pills {
            assert_eq!(state.pill(), pill, "{state:?}");
        }
    }
}
