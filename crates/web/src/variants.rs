//! The shared vocabulary of the component set (components §4.5; DESIGN.md,
//! "Shared vocabulary"): the tones, sizes and placements every shared
//! component takes, so a component names one of these values rather than a
//! color, a length or a side of its own. Each value is drawn by a class of
//! `style.css`:
//!
//! - a tone's class sets one custom property, `--tone`, which the component
//!   draws with (`var(--tone)`);
//! - a size's class sets one custom property, `--size`, its length on the
//!   scale every component shares, in `em`, so it grows with the reader's
//!   text;
//! - a placement's class goes on the line around a component's part and
//!   places the part in it, in logical terms, so it follows the page's
//!   direction.

/// The color a component draws with, one token of `style.css` each. None
/// carries meaning alone: a component that shows a tone says it in words or
/// an icon too (1.4.1). A component's §4.5 contract names the tones it
/// supports where one makes no sense.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Ladder Blue (`--accent`), the one accent for action. The default: a
    /// component draws in it unless its caller names another tone.
    #[default]
    Primary,
    /// Slate Note (`--muted`), neutral.
    Secondary,
    /// Tick Green (`--success`): something ended well.
    Success,
    /// Flag Amber (`--warning`): something needs a look.
    Warning,
    /// Red Ink (`--error`): something failed.
    Error,
    /// The color of the text around the component (`currentColor`), for a
    /// part inside another component, such as a spinner in a button.
    Inherit,
}

impl Tone {
    /// Every tone, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 6] = [
        Self::Primary,
        Self::Secondary,
        Self::Success,
        Self::Warning,
        Self::Error,
        Self::Inherit,
    ];

    /// The class of `style.css` that sets `--tone` to this tone's color.
    pub fn class(self) -> &'static str {
        match self {
            Self::Primary => "tone-primary",
            Self::Secondary => "tone-secondary",
            Self::Success => "tone-success",
            Self::Warning => "tone-warning",
            Self::Error => "tone-error",
            Self::Inherit => "tone-inherit",
        }
    }
}

/// How large a component is drawn, on one scale every component shares:
/// each size's class sets `--size` in `style.css` (Small 1em, Medium 1.5em,
/// Large 2em, XLarge 3em), which the component draws its length with, so a
/// spinner and an icon of one size line up. A component that needs other
/// lengths may deviate, saying why in its §4.5 row. There is no default
/// size: a component picks its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
    XLarge,
}

impl Size {
    /// Every size, smallest first.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::XLarge];

    /// The class of `style.css` that sets `--size` to this size's length.
    pub fn class(self) -> &'static str {
        match self {
            Self::Small => "size-small",
            Self::Medium => "size-medium",
            Self::Large => "size-large",
            Self::XLarge => "size-xlarge",
        }
    }
}

/// Where a component sits in its line, in the page's direction: `Start` is
/// the left in English and the right in Arabic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// In the line of text, as wide as what it holds. The default: a
    /// component placed nowhere in particular stays where its caller puts
    /// it.
    #[default]
    Inline,
    /// On a line of its own, at the line's start.
    Start,
    /// On a line of its own, in its middle.
    Center,
    /// On a line of its own, at the line's end.
    End,
}

impl Placement {
    /// Every placement, in the order the gallery shows them.
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Inline, Self::Start, Self::Center, Self::End];

    /// The class of `style.css` that places a component so. It goes on the
    /// line, an element of the component's own around its part, never on the
    /// part's root: it sets the line's `display` and where the line puts what
    /// it holds, which on a root with a `display` and a box of its own (a
    /// badge, a button) would be overridden or would stretch the box.
    pub fn class(self) -> &'static str {
        match self {
            Self::Inline => "placement-inline",
            Self::Start => "placement-start",
            Self::Center => "placement-center",
            Self::End => "placement-end",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No two values of an axis share a class, and no class is empty.
    fn distinct(classes: &[&str]) {
        let mut seen: Vec<_> = classes.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), classes.len(), "{classes:?}");
        assert!(classes.iter().all(|class| !class.is_empty()), "{classes:?}");
    }

    #[test]
    fn each_tone_sets_the_tone_hook_of_its_own() {
        let classes = [
            (Tone::Primary, "tone-primary"),
            (Tone::Secondary, "tone-secondary"),
            (Tone::Success, "tone-success"),
            (Tone::Warning, "tone-warning"),
            (Tone::Error, "tone-error"),
            (Tone::Inherit, "tone-inherit"),
        ];
        for (tone, class) in classes {
            assert_eq!(tone.class(), class, "{tone:?}");
        }
        distinct(&Tone::ALL.map(Tone::class));
        assert_eq!(
            Tone::default(),
            Tone::Primary,
            "a component draws in the accent"
        );
    }

    #[test]
    fn each_size_has_a_class_of_its_own() {
        let classes = [
            (Size::Small, "size-small"),
            (Size::Medium, "size-medium"),
            (Size::Large, "size-large"),
            (Size::XLarge, "size-xlarge"),
        ];
        for (size, class) in classes {
            assert_eq!(size.class(), class, "{size:?}");
        }
        distinct(&Size::ALL.map(Size::class));
    }

    #[test]
    fn each_placement_has_a_class_of_its_own() {
        let classes = [
            (Placement::Inline, "placement-inline"),
            (Placement::Start, "placement-start"),
            (Placement::Center, "placement-center"),
            (Placement::End, "placement-end"),
        ];
        for (placement, class) in classes {
            assert_eq!(placement.class(), class, "{placement:?}");
        }
        distinct(&Placement::ALL.map(Placement::class));
        assert_eq!(
            Placement::default(),
            Placement::Inline,
            "a component stays in its line"
        );
    }
}
