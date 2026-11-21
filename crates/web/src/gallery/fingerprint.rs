//! The gallery's fingerprint demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::Fingerprint;
use crate::text;

/// The fingerprint a demo shows: the shape of a hub key's, of no real key.
const DEMO_FINGERPRINT: &str = "7K3Q-M9XD-2RTV-8HFA-QW5N";

/// The fingerprint's demo (`#fingerprint-demo`): one fingerprint beside the
/// words that name it, which narrow screens and larger text wrap.
#[component]
pub(super) fn FingerprintDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let name = text(&localizer, msg::UI_GALLERY_FINGERPRINT_NAME);
    view! {
        <section id="fingerprint-demo">
            <DemoHeading text=text(&localizer, msg::UI_GALLERY_FINGERPRINT_HEADING) />
            <p class="flex flex-wrap gap-x-2">
                <span>{name.clone()}</span>
                <Fingerprint value=DEMO_FINGERPRINT name=name />
            </p>
        </section>
    }
}
