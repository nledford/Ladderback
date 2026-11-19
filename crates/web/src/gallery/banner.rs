//! The gallery's banner demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::banner::rules::BannerTone;
use crate::components::button::Content;
use crate::components::{Banner, BannerDismissal, Button};
use crate::text;

/// The banner's demo (`#banner-demo`).
#[component]
pub(super) fn BannerDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The banner demo: its words in each tone it takes, and the edition of
    // its dismissible notice, which the button after it makes newer, so the
    // dismissal of the one before doesn't keep it away.
    let (banner_information, banner_warning, banner_notice) = (
        t(msg::UI_GALLERY_BANNER_INFORMATION),
        t(msg::UI_GALLERY_BANNER_WARNING),
        t(msg::UI_GALLERY_BANNER_DISMISSIBLE),
    );
    let banner_edition = RwSignal::new(0_u32);
    let banner_demo_localizer = Arc::clone(&localizer);
    view! {
        <section id="banner-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BANNER_HEADING) />
            // Each tone it takes, information first, which is the default.
            <Banner localizer=Arc::clone(&localizer)>{banner_information}</Banner>
            // A long path, which wraps inside the banner on a small screen.
            <Banner localizer=Arc::clone(&localizer) tone=BannerTone::Warning>
                {banner_warning}
            </Banner>
            // A notice the reader dismisses: it goes, focus goes to the
            // button that brings a newer one, and this browser keeps this
            // edition away on every later load.
            {move || {
                let edition = banner_edition.get().to_string();
                let notice = banner_notice.clone();
                view! {
                    <Banner
                        localizer=Arc::clone(&banner_demo_localizer)
                        dismissal=BannerDismissal {
                            remember_as: BANNER_DEMO,
                            edition,
                            then_focus: BANNER_DEMO_NEWER.to_owned(),
                        }
                    >
                        {notice}
                    </Banner>
                }
            }}
            <p>
                <Button
                    id=BANNER_DEMO_NEWER
                    content=Content::Text(t(msg::UI_GALLERY_BANNER_NEWER).into())
                    on_press=Callback::new(move |()| {
                        banner_edition.update(|edition| *edition = edition.saturating_add(1));
                    })
                />
            </p>
        </section>
    }
}

/// The name the gallery's dismissible demo banner is remembered under in
/// the browser's local storage.
const BANNER_DEMO: &str = "gallery-demo";

/// The ID of the banner demo's button that brings a newer edition of its
/// notice, where focus goes when the notice is dismissed.
const BANNER_DEMO_NEWER: &str = "banner-demo-newer";
