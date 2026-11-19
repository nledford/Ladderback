//! The gallery's checkbox demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::Checkbox;
use crate::text;

/// The checkbox's demo (`#checkbox-demo`).
#[component]
pub(super) fn CheckboxDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="checkbox-demo">
            <DemoHeading text=t(msg::UI_GALLERY_CHECKBOX_HEADING) />
            // Its two states: off with a description, and on without one.
            <Checkbox
                id="gallery-notification"
                label=t(msg::UI_GALLERY_DEMO_NOTIFICATION)
                description=t(msg::UI_GALLERY_DEMO_NOTIFICATION_HINT)
                checked=RwSignal::new(false)
            />
            <Checkbox
                id="gallery-log"
                label=t(msg::UI_GALLERY_DEMO_LOG)
                checked=RwSignal::new(true)
            />
            // Its error, as a form shows once it is checked, under its
            // description.
            <Checkbox
                id="gallery-terms"
                label=t(msg::UI_GALLERY_DEMO_TERMS)
                description=t(msg::UI_GALLERY_DEMO_TERMS_HINT)
                error=Some(t(msg::UI_GALLERY_DEMO_TERMS_ERROR))
                checked=RwSignal::new(false)
            />
        </section>
    }
}
