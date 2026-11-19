//! The gallery's badge demo (technical design §4.5, "Tests").

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::Badge;
use crate::components::badge::rules::BadgeTone;
use crate::routes::text_with;
use crate::text;

/// The badge's demo (`#badge-demo`).
#[component]
pub(super) fn BadgeDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The badge demo's count, a number in its words.
    let pending_changes = text_with(
        &localizer,
        msg::UI_GALLERY_BADGES_PENDING,
        &[("count", MsgArg::Count(3))],
    );
    view! {
        <section id="badge-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BADGES_HEADING) />
            // A count and two tags, each saying in words what it counts or
            // names, the long one wrapping in a narrow room.
            <p class="flex flex-wrap gap-2">
                <Badge label=pending_changes icon=LucideGlyph::Pencil />
                <Badge label=t(msg::UI_GALLERY_BADGES_TAG) />
                <Badge label=t(msg::UI_GALLERY_BADGES_LONG) icon=LucideGlyph::CalendarClock />
            </p>
            // Every tone, named in words, so no tone is told by its color.
            <p class="flex flex-wrap gap-2">
                <Badge
                    label=t(msg::UI_GALLERY_TONE_PRIMARY)
                    tone=BadgeTone::Primary
                    icon=LucideGlyph::Info
                />
                <Badge label=t(msg::UI_GALLERY_TONE_SECONDARY) tone=BadgeTone::Secondary />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_SUCCESS)
                    tone=BadgeTone::Success
                    icon=LucideGlyph::CircleCheck
                />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_WARNING)
                    tone=BadgeTone::Warning
                    icon=LucideGlyph::TriangleAlert
                />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_ERROR)
                    tone=BadgeTone::Error
                    icon=LucideGlyph::CircleX
                />
            </p>
        </section>
    }
}
