//! The gallery's shared vocabulary demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::text;
use crate::variants::{Placement, Size, StatusTone, Tone};

/// The shared vocabulary's demo (`#shared-vocabulary-demo`).
#[component]
pub(super) fn VocabularyDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="shared-vocabulary-demo">
            <DemoHeading text=t(msg::UI_GALLERY_VOCABULARY_HEADING) />
            // Every tone, its name and its swatch drawn in it, so the axe
            // row reads each tone's text against the page.
            <h3 id="gallery-tones">{t(msg::UI_GALLERY_TONES_HEADING)}</h3>
            // `role="list"`: Safari drops a list's role once its markers
            // are styled away.
            <ul class="vocabulary-demo" role="list" aria-labelledby="gallery-tones">
                {[
                    (Tone::Primary, msg::UI_GALLERY_TONE_PRIMARY),
                    (Tone::Secondary, msg::UI_GALLERY_TONE_SECONDARY),
                    (Tone::Success, msg::UI_GALLERY_TONE_SUCCESS),
                    (Tone::Warning, msg::UI_GALLERY_TONE_WARNING),
                    (Tone::Error, msg::UI_GALLERY_TONE_ERROR),
                    (Tone::Inherit, msg::UI_GALLERY_TONE_INHERIT),
                ]
                    .into_iter()
                    .map(|(tone, name)| {
                        let class = format!("vocabulary-tone {}", tone.class());
                        view! {
                            <li class=class>
                                {swatch("vocabulary-swatch".to_owned())} <span>{t(name)}</span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
            // Every status tone's pair, its name in its words on its fill,
            // so the axe row reads each pair's words on its fill.
            <h3 id="gallery-status-pairs">{t(msg::UI_GALLERY_STATUS_PAIRS_HEADING)}</h3>
            <ul
                class="vocabulary-demo flex flex-wrap gap-2"
                role="list"
                aria-labelledby="gallery-status-pairs"
            >
                {[
                    (StatusTone::Success, msg::UI_GALLERY_TONE_SUCCESS),
                    (StatusTone::Warning, msg::UI_GALLERY_TONE_WARNING),
                    (StatusTone::Error, msg::UI_GALLERY_TONE_ERROR),
                ]
                    .into_iter()
                    .map(|(tone, name)| {
                        let class = format!("px-2 rounded-(--radius-small) {}", tone.pair());
                        view! { <li class=class>{t(name)}</li> }
                    })
                    .collect_view()}
            </ul>
            <h3 id="gallery-sizes">{t(msg::UI_GALLERY_SIZES_HEADING)}</h3>
            <ul class="vocabulary-demo" role="list" aria-labelledby="gallery-sizes">
                {[
                    (Size::Small, msg::UI_GALLERY_SIZE_SMALL),
                    (Size::Medium, msg::UI_GALLERY_SIZE_MEDIUM),
                    (Size::Large, msg::UI_GALLERY_SIZE_LARGE),
                    (Size::XLarge, msg::UI_GALLERY_SIZE_XLARGE),
                ]
                    .into_iter()
                    .map(|(size, name)| {
                        view! {
                            <li class="vocabulary-size">
                                {swatch(format!("vocabulary-swatch {}", size.class()))}
                                <span>{t(name)}</span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
            <h3 id="gallery-placements">{t(msg::UI_GALLERY_PLACEMENTS_HEADING)}</h3>
            <ul
                class="vocabulary-demo vocabulary-placements"
                role="list"
                aria-labelledby="gallery-placements"
            >
                {[
                    (Placement::Inline, msg::UI_GALLERY_PLACEMENT_INLINE),
                    (Placement::Start, msg::UI_GALLERY_PLACEMENT_START),
                    (Placement::Center, msg::UI_GALLERY_PLACEMENT_CENTER),
                    (Placement::End, msg::UI_GALLERY_PLACEMENT_END),
                ]
                    .into_iter()
                    .map(|(placement, name)| {
                        let class = placement.class();
                        view! {
                            <li class=class>
                                {swatch("vocabulary-swatch".to_owned())} <span>{t(name)}</span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        </section>
    }
}

/// A swatch of the vocabulary demo: a circle stroked in the color of the
/// text around it, decoration beside the words that name what it shows, so
/// a screen reader skips it.
fn swatch(class: String) -> impl IntoView {
    view! {
        <svg
            class=class
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            aria-hidden="true"
            focusable="false"
        >
            <circle cx="12" cy="12" r="9" />
        </svg>
    }
}
