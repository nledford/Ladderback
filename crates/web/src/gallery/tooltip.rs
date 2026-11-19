//! The gallery's tooltip demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::{Button, Tip, Tooltip};
use crate::text;
use crate::variants::Side;

/// The tooltip's demo (`#tooltip-demo`).
#[component]
pub(super) fn TooltipDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The tooltip demo's icon-only button: its name, the tooltip's too.
    let schedule = t(msg::UI_GALLERY_TOOLTIP_SCHEDULE);
    view! {
        <section id="tooltip-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TOOLTIP_HEADING) />
            // A control its tooltip describes, standing above it.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_PRUNE),
                    t(msg::UI_GALLERY_TOOLTIP_PRUNE_NOTE),
                    Side::Above,
                )}
            </p>
            // An icon-only control, named by the tooltip's one string: the
            // button's label, which the tooltip shows a sighted reader. The
            // button names itself with the same string, as an icon-only
            // Button must.
            <p>
                <Tooltip tip=Tip::Names(schedule.clone())>
                    <Button content=Content::IconOnly {
                        glyph: LucideGlyph::CalendarClock.into(),
                        label: schedule.into(),
                    } />
                </Tooltip>
            </p>
            // Each side, with room on it.
            <p class="flex flex-wrap items-center gap-4">
                {[
                    (
                        Side::Above,
                        msg::UI_GALLERY_TOOLTIP_ABOVE,
                        msg::UI_GALLERY_TOOLTIP_ABOVE_NOTE,
                    ),
                    (
                        Side::Below,
                        msg::UI_GALLERY_TOOLTIP_BELOW,
                        msg::UI_GALLERY_TOOLTIP_BELOW_NOTE,
                    ),
                    (
                        Side::Start,
                        msg::UI_GALLERY_TOOLTIP_START,
                        msg::UI_GALLERY_TOOLTIP_START_NOTE,
                    ),
                    (Side::End, msg::UI_GALLERY_TOOLTIP_END, msg::UI_GALLERY_TOOLTIP_END_NOTE),
                ]
                    .into_iter()
                    .map(|(side, name, note)| demo_tooltip(t(name), t(note), side))
                    .collect_view()}
            </p>
            // Start and End in a line set right to left: the start is the
            // right.
            <p dir="rtl" class="flex flex-wrap items-center gap-4">
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_START_RTL),
                    t(msg::UI_GALLERY_TOOLTIP_START_NOTE),
                    Side::Start,
                )}
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_END_RTL),
                    t(msg::UI_GALLERY_TOOLTIP_END_NOTE),
                    Side::End,
                )}
            </p>
            // A long note, which wraps.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_LONG),
                    t(msg::UI_GALLERY_TOOLTIP_LONG_NOTE),
                    Side::Above,
                )}
            </p>
            // Below, at the foot of the page, where there is no room: it
            // stands above.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_FOOT),
                    t(msg::UI_GALLERY_TOOLTIP_FOOT_NOTE),
                    Side::Below,
                )}
            </p>
        </section>
    }
}

/// A demo button whose tooltip, `note`, describes it from `side`. Pressing
/// it changes nothing.
fn demo_tooltip(name: String, note: String, side: Side) -> impl IntoView {
    view! {
        <Tooltip tip=Tip::Describes(note) side=side>
            <Button content=Content::Text(name.into()) />
        </Tooltip>
    }
}
