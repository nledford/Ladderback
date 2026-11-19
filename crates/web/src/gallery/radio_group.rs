//! The gallery's radio group demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Direction, Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::{LabelLanguage, RadioGroup};
use crate::text;

/// The radio group's demo (`#radio-group-demo`).
#[component]
pub(super) fn RadioGroupDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="radio-group-demo">
            <DemoHeading text=t(msg::UI_GALLERY_RADIO_HEADING) />
            // A group with a hint, the middle choice made as it opens.
            <RadioGroup
                name="gallery-frequency"
                legend=t(msg::UI_GALLERY_DEMO_FREQUENCY)
                description=t(msg::UI_GALLERY_DEMO_FREQUENCY_HINT)
                options=vec![
                    ("daily", t(msg::UI_GALLERY_DEMO_DAILY)),
                    ("weekly", t(msg::UI_GALLERY_DEMO_WEEKLY)),
                    ("monthly", t(msg::UI_GALLERY_DEMO_MONTHLY)),
                ]
                chosen=RwSignal::new("weekly")
            />
            // A group whose choices are named in their own languages: each
            // label declares its language and the way it runs.
            <RadioGroup
                name="gallery-language"
                legend=t(msg::UI_GALLERY_DEMO_LANGUAGE)
                options=vec![
                    ("en", t(msg::UI_GALLERY_DEMO_ENGLISH)),
                    ("ja", t(msg::UI_GALLERY_DEMO_JAPANESE)),
                    ("ar", t(msg::UI_GALLERY_DEMO_ARABIC)),
                ]
                label_languages=vec![
                    (
                        "en",
                        LabelLanguage {
                            lang: "en-US".to_owned(),
                            dir: Direction::Ltr,
                        },
                    ),
                    (
                        "ja",
                        LabelLanguage {
                            lang: "ja".to_owned(),
                            dir: Direction::Ltr,
                        },
                    ),
                    (
                        "ar",
                        LabelLanguage {
                            lang: "ar".to_owned(),
                            dir: Direction::Rtl,
                        },
                    ),
                ]
                chosen=RwSignal::new("en")
            />
            // A group with an error, as a form shows once it is checked: no
            // choice made, so Tab lands on its first.
            <RadioGroup
                name="gallery-window"
                legend=t(msg::UI_GALLERY_DEMO_WINDOW)
                options=vec![
                    ("mornings", t(msg::UI_GALLERY_DEMO_MORNINGS)),
                    ("evenings", t(msg::UI_GALLERY_DEMO_EVENINGS)),
                ]
                error=Some(t(msg::UI_GALLERY_DEMO_WINDOW_ERROR))
                chosen=RwSignal::new("")
            />
        </section>
    }
}
