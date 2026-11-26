//! The gallery's status badge demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::StatusBadge;
use crate::components::status_badge::rules::BadgeState;
use crate::text;

/// The status badge's demo (`#status-badge-demo`).
#[component]
pub(super) fn StatusBadgeDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="status-badge-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BADGE_HEADING) />
            <p class="flex flex-wrap gap-2">
                // Every state a badge can show, each with its own icon.
                <StatusBadge state=BadgeState::Queued label=t(msg::UI_STATUS_QUEUED) />
                <StatusBadge state=BadgeState::Running label=t(msg::UI_STATUS_RUNNING) />
                <StatusBadge state=BadgeState::Succeeded label=t(msg::UI_STATUS_SUCCEEDED) />
                <StatusBadge
                    state=BadgeState::SucceededWithWarnings
                    label=t(msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS)
                />
                <StatusBadge state=BadgeState::Failed label=t(msg::UI_STATUS_FAILED) />
                <StatusBadge state=BadgeState::Canceled label=t(msg::UI_STATUS_CANCELED) />
                <StatusBadge state=BadgeState::Empty label=t(msg::UI_GALLERY_BADGE_EMPTY) />
            </p>
        </section>
    }
}
