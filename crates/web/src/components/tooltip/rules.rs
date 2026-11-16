//! What a tooltip tells a screen reader, when it shows and where it stands
//! (components §4.5), decided on the host. `components/tooltip.rs`'s
//! `Tooltip` is the glue that measures the page and draws it; a popover
//! (#784) places itself with the same [`place`].
//!
//! Where it stands is worked out in the viewport's own coordinates, the ones
//! the browser measures in (`getBoundingClientRect`): `left` and `top` from
//! the viewport's top-left corner in every direction of writing. The sides
//! it is given are logical ([`Side`]), and the page's [`Direction`] turns
//! `Start` and `End` into the left or the right.

use l10n::Direction;

use crate::variants::Side;

/// What a tooltip says, and what it is to a screen reader. The two can't be
/// mixed up: a description is read after the control's own name, and a name
/// shown for an icon-only control is the control's name, read once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tip {
    /// A description of the control, which already has a name of its own:
    /// the tooltip is a `role="tooltip"` the control's `aria-describedby`
    /// points at, so a screen reader reads it after the name, whether it
    /// shows or not. Never the only place the control's name lives.
    Describes(String),
    /// The name of an icon-only control, shown to a sighted reader: the
    /// control's `aria-label` is set to it, and the tooltip is hidden from a
    /// screen reader (`aria-hidden`), so the name is heard once, from the
    /// control, and comes from this one string.
    Names(String),
}

impl Tip {
    /// The words the tooltip shows.
    pub fn text(&self) -> &str {
        todo!()
    }

    /// The tooltip's `role`: a description is a tooltip; a name has none,
    /// since a screen reader never meets it.
    pub fn role(&self) -> Option<&'static str> {
        todo!()
    }

    /// The tooltip's `aria-hidden`: hidden when it shows the control's name,
    /// which the control itself carries.
    pub fn hidden(&self) -> Option<&'static str> {
        todo!()
    }

    /// The control's `aria-describedby`: the tooltip's `id`, for a
    /// description.
    pub fn described_by<'a>(&self, id: &'a str) -> Option<&'a str> {
        let _ = id;
        todo!()
    }

    /// The control's `aria-label`: the name the tooltip shows, for an
    /// icon-only control.
    pub fn label(&self) -> Option<&str> {
        todo!()
    }
}

/// How long the pointer rests on a control before its tooltip shows, so a
/// pointer passing over the page doesn't flash every tooltip on its way.
pub const HOVER_DELAY_MS: u32 = 300;

/// How long a tooltip stays once the pointer has left the control and the
/// tooltip, enough to cross the gap between the two (1.4.13, Hoverable).
pub const GRACE_MS: u32 = 150;

/// The gap between a tooltip and its control, in the control's `em`.
pub const GAP_EM: f64 = 0.375;

/// The least room kept between a tooltip and the viewport's edge, in the
/// control's `em`.
pub const EDGE_EM: f64 = 0.5;

/// Whether a pointer of this `pointerType` hovers. A finger doesn't: it
/// touches what it presses, so a touch shows a tooltip only as the focus it
/// gives does.
pub fn hovers(pointer_type: &str) -> bool {
    let _ = pointer_type;
    todo!()
}

/// The page's direction, from the computed `direction` of a control.
pub fn direction_of(computed: &str) -> Direction {
    let _ = computed;
    todo!()
}

/// What happens to a tooltip: its control gains or loses keyboard focus,
/// the pointer comes onto or leaves the control or the tooltip, the reader
/// presses Escape, or a wait the tooltip asked for ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The control gained focus that shows (`:focus-visible`): from the
    /// keyboard, not from a pointer's press, which hovering already serves.
    Focus,
    /// The control lost focus.
    Blur,
    /// The pointer came onto the control or the tooltip.
    PointerIn,
    /// The pointer left them both.
    PointerOut,
    /// The reader pressed Escape.
    Escape,
    /// The pointer has rested long enough ([`HOVER_DELAY_MS`]).
    ShowDue,
    /// The grace after the pointer left has passed ([`GRACE_MS`]).
    HideDue,
}

/// What the tooltip's one wait does after an event: stays as it is, is
/// called off, or starts anew (calling off one already waiting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Keep,
    CallOff,
    /// Wait this many milliseconds, then send the event.
    Start(u32, Event),
}

/// Whether a tooltip shows, and why: focus, the pointer, or neither, and
/// whether Escape dismissed it. Escape's dismissal lasts until focus and the
/// pointer have both left, or the control gains focus anew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    focused: bool,
    hovered: bool,
    dismissed: bool,
    open: bool,
}

impl Shown {
    /// Whether the tooltip shows now.
    pub fn open(&self) -> bool {
        todo!()
    }

    /// Takes in `event` and says what becomes of the tooltip's wait.
    pub fn on(&mut self, event: Event) -> Wait {
        let _ = event;
        todo!()
    }
}

/// A box in the viewport, in CSS pixels from its top-left corner, as the
/// browser measures it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// What a tooltip is placed in: the viewport's size, the page's direction,
/// and the gap to its control and the least room to the viewport's edge, in
/// CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub width: f64,
    pub height: f64,
    pub direction: Direction,
    pub gap: f64,
    pub edge: f64,
}

/// Where a tooltip stands: the side of its control it took, and its
/// top-left corner in the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub side: Side,
    pub left: f64,
    pub top: f64,
}

/// The sides a tooltip that wants `wanted` tries, in order: its own, the
/// opposite one, then the two of the other axis (the end before the start,
/// where reading goes on; above before below, the vocabulary's default).
pub fn sides_to_try(wanted: Side) -> [Side; 4] {
    let _ = wanted;
    todo!()
}

/// Where a tooltip `width` by `height` stands beside `anchor`, its control:
/// on the first side [`sides_to_try`] gives where it fits whole, inside the
/// viewport with `room.edge` to spare; where none does, on the side with
/// the most room. Along its side it is centered on the control and moved
/// inside the viewport; across, it is moved only away from the control, so
/// it never covers it (2.4.11) and stays inside the viewport where it can.
pub fn place(anchor: Rect, width: f64, height: f64, room: Room, wanted: Side) -> Spot {
    let _ = (anchor, width, height, room, wanted);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describes() -> Tip {
        Tip::Describes("Frees the space no snapshot uses.".to_owned())
    }

    #[test]
    fn a_description_is_a_tooltip_the_control_is_described_by() {
        let tip = describes();
        assert_eq!(tip.text(), "Frees the space no snapshot uses.");
        assert_eq!(tip.role(), Some("tooltip"));
        assert_eq!(tip.hidden(), None);
        assert_eq!(tip.described_by("tooltip-3"), Some("tooltip-3"));
        assert_eq!(tip.label(), None, "the control keeps its own name");
    }

    #[test]
    fn a_name_is_the_controls_label_and_is_hidden_from_a_screen_reader() {
        let tip = Tip::Names("Demo schedule".to_owned());
        assert_eq!(tip.text(), "Demo schedule");
        assert_eq!(tip.label(), Some("Demo schedule"));
        assert_eq!(tip.hidden(), Some("true"));
        assert_eq!(tip.role(), None);
        assert_eq!(tip.described_by("tooltip-3"), None, "never read twice");
    }

    #[test]
    fn a_mouse_or_a_pen_hovers_and_a_finger_does_not() {
        assert!(hovers("mouse"));
        assert!(hovers("pen"));
        assert!(!hovers("touch"));
    }

    #[test]
    fn the_direction_is_the_controls_computed_one() {
        assert_eq!(direction_of("rtl"), Direction::Rtl);
        assert_eq!(direction_of("ltr"), Direction::Ltr);
    }

    /// A tooltip after `events`, from the start.
    fn after(events: &[Event]) -> Shown {
        let mut shown = Shown::default();
        for event in events {
            shown.on(*event);
        }
        shown
    }

    use Event::{Blur, Escape, Focus, HideDue, PointerIn, PointerOut, ShowDue};

    #[test]
    fn keyboard_focus_shows_it_at_once_and_blur_hides_it() {
        let mut shown = Shown::default();
        assert!(!shown.open());
        assert_eq!(shown.on(Focus), Wait::CallOff);
        assert!(shown.open());
        assert_eq!(shown.on(Blur), Wait::CallOff);
        assert!(!shown.open());
    }

    #[test]
    fn the_pointer_shows_it_after_resting_and_never_before() {
        let mut shown = Shown::default();
        assert_eq!(shown.on(PointerIn), Wait::Start(HOVER_DELAY_MS, ShowDue));
        assert!(!shown.open(), "not before the wait");
        assert_eq!(shown.on(ShowDue), Wait::Keep);
        assert!(shown.open());
    }

    #[test]
    fn a_pointer_that_leaves_before_the_wait_shows_nothing() {
        let mut shown = after(&[PointerIn]);
        assert_eq!(shown.on(PointerOut), Wait::CallOff);
        assert!(!shown.open());
        // A wait that ends anyway (it was called off too late) shows nothing.
        shown.on(ShowDue);
        assert!(!shown.open());
    }

    #[test]
    fn it_stays_while_the_pointer_crosses_to_it_and_goes_once_the_grace_has_passed() {
        let mut shown = after(&[PointerIn, ShowDue]);
        assert_eq!(shown.on(PointerOut), Wait::Start(GRACE_MS, HideDue));
        assert!(shown.open(), "the grace to cross the gap");
        assert_eq!(shown.on(PointerIn), Wait::CallOff, "on the tooltip now");
        assert!(shown.open());
        shown.on(PointerOut);
        assert_eq!(shown.on(HideDue), Wait::Keep);
        assert!(!shown.open());
    }

    #[test]
    fn a_grace_that_ends_while_the_pointer_is_back_hides_nothing() {
        let mut shown = after(&[PointerIn, ShowDue, PointerOut, PointerIn]);
        shown.on(HideDue);
        assert!(shown.open());
    }

    #[test]
    fn escape_under_focus_alone_lasts_until_focus_leaves() {
        let mut shown = after(&[Focus, Escape]);
        assert!(!shown.open());
        assert_eq!(shown.on(PointerIn), Wait::CallOff);
        shown.on(ShowDue);
        assert!(!shown.open(), "dismissed while focus stays");
    }

    #[test]
    fn focus_keeps_it_after_the_pointer_leaves_and_the_pointer_after_focus_leaves() {
        let mut shown = after(&[PointerIn, ShowDue, Focus]);
        assert_eq!(shown.on(PointerOut), Wait::CallOff, "focus keeps it");
        assert!(shown.open());
        shown.on(HideDue);
        assert!(shown.open(), "a late grace leaves it to focus");

        let mut shown = after(&[Focus, PointerIn]);
        assert_eq!(shown.on(Blur), Wait::Keep, "the pointer keeps it");
        assert!(shown.open());
        assert_eq!(shown.on(PointerOut), Wait::Start(GRACE_MS, HideDue));
        shown.on(HideDue);
        assert!(!shown.open());
    }

    #[test]
    fn focus_shows_it_at_once_over_a_pointer_still_waiting() {
        let mut shown = after(&[PointerIn]);
        assert_eq!(shown.on(Focus), Wait::CallOff);
        assert!(shown.open());
    }

    #[test]
    fn blur_while_the_pointer_waits_leaves_the_wait() {
        let mut shown = after(&[Focus, Blur, PointerIn]);
        assert_eq!(shown.on(Blur), Wait::Keep);
        shown.on(ShowDue);
        assert!(shown.open());
    }

    #[test]
    fn escape_hides_it_where_it_is_and_it_stays_hidden_until_both_have_left() {
        let mut shown = after(&[Focus, PointerIn]);
        assert_eq!(shown.on(Escape), Wait::CallOff);
        assert!(!shown.open());
        // Neither the pointer resting again nor a late wait brings it back.
        assert_eq!(shown.on(PointerOut), Wait::CallOff);
        shown.on(PointerIn);
        shown.on(ShowDue);
        assert!(!shown.open(), "dismissed while focus stays");
        shown.on(PointerOut);
        shown.on(Blur);
        // Both have left: the next rest shows it again.
        shown.on(PointerIn);
        shown.on(ShowDue);
        assert!(shown.open());
    }

    #[test]
    fn escape_under_the_pointer_alone_lasts_until_the_pointer_leaves() {
        let mut shown = after(&[PointerIn, ShowDue, Escape]);
        assert!(!shown.open());
        assert_eq!(shown.on(PointerIn), Wait::CallOff);
        shown.on(ShowDue);
        assert!(!shown.open());
        shown.on(PointerOut);
        shown.on(PointerIn);
        shown.on(ShowDue);
        assert!(shown.open());
    }

    #[test]
    fn where_no_side_fits_a_tooltip_takes_the_side_it_overflows_least() {
        let ltr = room(Direction::Ltr);
        // Too tall for above (278) or below (278 - 100 = 178), too wide for
        // either end (438): above lacks the least.
        let lower = Rect {
            top: 290.0,
            height: 120.0,
            ..MIDDLE
        };
        assert_eq!(
            place(lower, 900.0, 400.0, ltr, Side::Below).side,
            Side::Above
        );
        // What it lacks, not the share of it that fits: above lacks 322 and
        // holds less than half, the end lacks 462 and holds more.
        assert_eq!(
            place(lower, 900.0, 600.0, ltr, Side::Below).side,
            Side::Above
        );
        let higher = Rect {
            top: 190.0,
            height: 120.0,
            ..MIDDLE
        };
        assert_eq!(
            place(higher, 900.0, 400.0, ltr, Side::Above).side,
            Side::Below
        );
        // The end lacks less than above or below.
        let flat = Rect {
            left: 10.0,
            top: 100.0,
            width: 50.0,
            height: 400.0,
        };
        assert_eq!(place(flat, 1000.0, 700.0, ltr, Side::Above).side, Side::End);
        let flat_end = Rect {
            left: 940.0,
            ..flat
        };
        assert_eq!(
            place(flat_end, 1000.0, 700.0, ltr, Side::Above).side,
            Side::Start
        );
        // Lacking as much: the earlier side in the order.
        assert_eq!(
            place(MIDDLE, 2000.0, 700.0, ltr, Side::Below).side,
            Side::Below
        );
        assert_eq!(
            place(MIDDLE, 2000.0, 700.0, ltr, Side::Above).side,
            Side::Above
        );
    }

    #[test]
    fn escape_with_nothing_to_dismiss_changes_nothing() {
        let mut shown = Shown::default();
        assert_eq!(shown.on(Escape), Wait::Keep);
        shown.on(Focus);
        assert!(shown.open(), "nothing was dismissed");
        let mut leaving = after(&[PointerIn, ShowDue, PointerOut]);
        assert_eq!(
            leaving.on(Escape),
            Wait::CallOff,
            "a tooltip in its grace goes"
        );
        assert!(!leaving.open());
    }

    #[test]
    fn blur_with_the_pointer_gone_ends_a_dismissal() {
        let mut shown = after(&[Focus, Escape, Blur]);
        assert!(!shown.open());
        shown.on(PointerIn);
        shown.on(ShowDue);
        assert!(shown.open());
    }

    #[test]
    fn a_tooltip_tries_its_side_the_opposite_one_then_the_other_axis() {
        use Side::{Above, Below, End, Start};
        assert_eq!(sides_to_try(Above), [Above, Below, End, Start]);
        assert_eq!(sides_to_try(Below), [Below, Above, End, Start]);
        assert_eq!(sides_to_try(Start), [Start, End, Above, Below]);
        assert_eq!(sides_to_try(End), [End, Start, Above, Below]);
    }

    /// A 1000 by 600 viewport, a 4 px gap and 8 px kept from its edges.
    fn room(direction: Direction) -> Room {
        Room {
            width: 1000.0,
            height: 600.0,
            direction,
            gap: 4.0,
            edge: 8.0,
        }
    }

    /// A control 100 by 20 in the middle of the viewport.
    const MIDDLE: Rect = Rect {
        left: 450.0,
        top: 290.0,
        width: 100.0,
        height: 20.0,
    };

    #[test]
    fn with_room_all_round_a_tooltip_stands_on_its_side_centered_on_the_control() {
        let ltr = room(Direction::Ltr);
        let spot = |side| place(MIDDLE, 60.0, 30.0, ltr, side);
        assert_eq!(
            spot(Side::Above),
            Spot {
                side: Side::Above,
                left: 470.0,
                top: 256.0
            }
        );
        assert_eq!(
            spot(Side::Below),
            Spot {
                side: Side::Below,
                left: 470.0,
                top: 314.0
            }
        );
        assert_eq!(
            spot(Side::Start),
            Spot {
                side: Side::Start,
                left: 386.0,
                top: 285.0
            }
        );
        assert_eq!(
            spot(Side::End),
            Spot {
                side: Side::End,
                left: 554.0,
                top: 285.0
            }
        );
    }

    #[test]
    fn right_to_left_the_start_is_the_right_and_the_end_the_left() {
        let rtl = room(Direction::Rtl);
        let spot = |side| place(MIDDLE, 60.0, 30.0, rtl, side);
        assert_eq!(
            spot(Side::Start),
            Spot {
                side: Side::Start,
                left: 554.0,
                top: 285.0
            }
        );
        assert_eq!(
            spot(Side::End),
            Spot {
                side: Side::End,
                left: 386.0,
                top: 285.0
            }
        );
        assert_eq!(spot(Side::Above).top, 256.0, "above and below stay");
    }

    #[test]
    fn a_tooltip_that_just_fits_takes_its_side() {
        let ltr = room(Direction::Ltr);
        // 290 above the control, less the gap and the edge, is 278, and
        // 600 - 310 below it is 278 too: half a pixel more fits on neither,
        // and the tooltip takes the end.
        assert_eq!(
            place(MIDDLE, 60.0, 278.0, ltr, Side::Above).side,
            Side::Above
        );
        assert_eq!(place(MIDDLE, 60.0, 278.5, ltr, Side::Above).side, Side::End);
        assert_eq!(
            place(MIDDLE, 60.0, 278.0, ltr, Side::Below).side,
            Side::Below
        );
        assert_eq!(place(MIDDLE, 60.0, 278.5, ltr, Side::Below).side, Side::End);
        // 450 before it and 1000 - 550 after it, less both, are 438.
        assert_eq!(
            place(MIDDLE, 438.0, 30.0, ltr, Side::Start).side,
            Side::Start
        );
        assert_eq!(
            place(MIDDLE, 438.5, 30.0, ltr, Side::Start).side,
            Side::Above
        );
        assert_eq!(place(MIDDLE, 438.0, 30.0, ltr, Side::End).side, Side::End);
        assert_eq!(place(MIDDLE, 438.5, 30.0, ltr, Side::End).side, Side::Above);
    }

    #[test]
    fn with_no_room_on_its_side_a_tooltip_flips_to_the_opposite_one() {
        let ltr = room(Direction::Ltr);
        let top = Rect {
            top: 10.0,
            ..MIDDLE
        };
        assert_eq!(
            place(top, 60.0, 30.0, ltr, Side::Above),
            Spot {
                side: Side::Below,
                left: 470.0,
                top: 34.0
            }
        );
        let bottom = Rect {
            top: 570.0,
            ..MIDDLE
        };
        assert_eq!(
            place(bottom, 60.0, 30.0, ltr, Side::Below),
            Spot {
                side: Side::Above,
                left: 470.0,
                top: 536.0
            }
        );
        let start = Rect {
            left: 10.0,
            ..MIDDLE
        };
        assert_eq!(
            place(start, 60.0, 30.0, ltr, Side::Start),
            Spot {
                side: Side::End,
                left: 114.0,
                top: 285.0
            }
        );
        let end = Rect {
            left: 890.0,
            ..MIDDLE
        };
        assert_eq!(
            place(end, 60.0, 30.0, ltr, Side::End),
            Spot {
                side: Side::Start,
                left: 826.0,
                top: 285.0
            }
        );
    }

    #[test]
    fn with_no_room_on_either_side_of_its_axis_a_tooltip_takes_the_other_axis() {
        let ltr = room(Direction::Ltr);
        // A control as wide as the viewport, less a little.
        let wide = Rect {
            left: 20.0,
            top: 290.0,
            width: 960.0,
            height: 20.0,
        };
        assert_eq!(
            place(wide, 60.0, 30.0, ltr, Side::End),
            Spot {
                side: Side::Above,
                left: 470.0,
                top: 256.0
            }
        );
        let high = Rect {
            left: 450.0,
            top: 20.0,
            width: 100.0,
            height: 560.0,
        };
        assert_eq!(
            place(high, 60.0, 30.0, ltr, Side::Above),
            Spot {
                side: Side::End,
                left: 554.0,
                top: 285.0
            }
        );
        // The end has no room either: the start.
        let high_end = Rect {
            left: 900.0,
            ..high
        };
        assert_eq!(
            place(high_end, 60.0, 30.0, ltr, Side::Below).side,
            Side::Start
        );
        // Above has no room either: below.
        let wide_top = Rect { top: 10.0, ..wide };
        assert_eq!(
            place(wide_top, 60.0, 30.0, ltr, Side::Start).side,
            Side::Below
        );
    }

    #[test]
    fn along_its_side_a_tooltip_stays_inside_the_viewport() {
        let ltr = room(Direction::Ltr);
        let near_left = Rect {
            left: 0.0,
            ..MIDDLE
        };
        assert_eq!(place(near_left, 300.0, 30.0, ltr, Side::Above).left, 8.0);
        let near_right = Rect {
            left: 950.0,
            width: 50.0,
            ..MIDDLE
        };
        assert_eq!(place(near_right, 300.0, 30.0, ltr, Side::Below).left, 692.0);
        let near_top = Rect {
            left: 450.0,
            top: 0.0,
            width: 100.0,
            height: 20.0,
        };
        assert_eq!(place(near_top, 60.0, 100.0, ltr, Side::End).top, 8.0);
        let near_bottom = Rect {
            top: 580.0,
            ..near_top
        };
        assert_eq!(place(near_bottom, 60.0, 100.0, ltr, Side::Start).top, 492.0);
        // Longer than the viewport less its edges: from the start edge.
        assert_eq!(place(MIDDLE, 990.0, 30.0, ltr, Side::Above).left, 8.0);
        assert_eq!(
            place(
                Rect {
                    left: 0.0,
                    ..MIDDLE
                },
                60.0,
                590.0,
                ltr,
                Side::End
            )
            .top,
            8.0
        );
    }

    #[test]
    fn across_its_side_a_tooltip_moves_into_the_viewport_only_away_from_its_control() {
        let ltr = room(Direction::Ltr);
        // A control scrolled mostly out of the top: below it, at the edge.
        let gone_up = Rect {
            top: -18.0,
            ..MIDDLE
        };
        assert_eq!(place(gone_up, 60.0, 30.0, ltr, Side::Below).top, 8.0);
        // Out of the bottom: above it, at the edge.
        let gone_down = Rect {
            top: 598.0,
            ..MIDDLE
        };
        assert_eq!(place(gone_down, 60.0, 30.0, ltr, Side::Above).top, 562.0);
        // Out of the left: after it, at the edge; out of the right: before.
        let gone_left = Rect {
            left: -98.0,
            ..MIDDLE
        };
        assert_eq!(place(gone_left, 60.0, 30.0, ltr, Side::End).left, 8.0);
        let gone_right = Rect {
            left: 998.0,
            ..MIDDLE
        };
        assert_eq!(place(gone_right, 60.0, 30.0, ltr, Side::Start).left, 932.0);
    }

    #[test]
    fn a_tooltip_never_covers_its_control() {
        let overlaps = |a: Rect, spot: Spot, width: f64, height: f64| {
            spot.left < a.left + a.width
                && a.left < spot.left + width
                && spot.top < a.top + a.height
                && a.top < spot.top + height
        };
        let anchors = [-200.0, 0.0, 10.0, 450.0, 940.0, 1100.0]
            .into_iter()
            .flat_map(|left| {
                [-100.0, 0.0, 10.0, 290.0, 570.0, 700.0]
                    .into_iter()
                    .map(move |top| Rect {
                        left,
                        top,
                        ..MIDDLE
                    })
            });
        let sizes = [(60.0, 30.0), (300.0, 300.0), (2000.0, 700.0)];
        for direction in [Direction::Ltr, Direction::Rtl] {
            let room = room(direction);
            for (anchor, (width, height)) in anchors
                .clone()
                .flat_map(|anchor| sizes.into_iter().map(move |size| (anchor, size)))
            {
                for side in Side::ALL {
                    let spot = place(anchor, width, height, room, side);
                    assert!(
                        !overlaps(anchor, spot, width, height),
                        "{anchor:?} {width}x{height} {side:?}: {spot:?}"
                    );
                }
            }
        }
    }
}
