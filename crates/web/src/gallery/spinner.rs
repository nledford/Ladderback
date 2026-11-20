//! The gallery's spinner demo (technical design §4.5, "Tests").
//!
//! Its spinners turn as long as the gallery is open, beside one another,
//! with nothing to pause them, which 2.2.2 (Pause, Stop, Hide) would refuse
//! on a page a reader uses. They stay: each is a case the spinner's
//! scenarios measure (its sizes, tones and label placements), a still one
//! would not show that it turns, the gallery is built only with the
//! `gallery` feature, which `just architecture` keeps out of every release
//! build, and the reduced-motion rule stops them all for a reader who asks
//! for less motion.

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::spinner::rules::LabelShown;
use crate::components::{Button, Spinner};
use crate::text;
use crate::variants::{Placement, Size, Tone};

/// The spinner's demo (`#spinner-demo`).
#[component]
pub(super) fn SpinnerDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="spinner-demo">
            <DemoHeading text=t(msg::UI_GALLERY_SPINNER_HEADING) />
            // The defaults: Small (as tall as the text), Primary and Inline,
            // its label beside it, as a step of a longer task shows it.
            <p>
                {t(msg::UI_GALLERY_SPINNER_STEP)} " "
                <Spinner label=t(msg::UI_GALLERY_SPINNER_LOADING) />
            </p>
            // Every size, then every tone, each with its label hidden, so
            // each is an image named by it. The Small one and the Primary
            // one name no size or tone, so they show the defaults.
            <p class="flex flex-wrap items-center gap-4">
                <Spinner shown=LabelShown::Hidden label=t(msg::UI_GALLERY_SPINNER_SMALL) />
                {[
                    (Size::Medium, msg::UI_GALLERY_SPINNER_MEDIUM),
                    (Size::Large, msg::UI_GALLERY_SPINNER_LARGE),
                    (Size::XLarge, msg::UI_GALLERY_SPINNER_XLARGE),
                ]
                    .into_iter()
                    .map(|(size, name)| {
                        view! { <Spinner shown=LabelShown::Hidden size=size label=t(name) /> }
                    })
                    .collect_view()}
            </p>
            <p class="flex flex-wrap items-center gap-4">
                <Spinner
                    shown=LabelShown::Hidden
                    size=Size::Medium
                    label=t(msg::UI_GALLERY_SPINNER_PRIMARY)
                />
                {[
                    (Tone::Secondary, msg::UI_GALLERY_SPINNER_SECONDARY),
                    (Tone::Success, msg::UI_GALLERY_SPINNER_SUCCESS),
                    (Tone::Warning, msg::UI_GALLERY_SPINNER_WARNING),
                    (Tone::Error, msg::UI_GALLERY_SPINNER_ERROR),
                    (Tone::Inherit, msg::UI_GALLERY_SPINNER_INHERIT),
                ]
                    .into_iter()
                    .map(|(tone, name)| {
                        view! {
                            <Spinner
                                shown=LabelShown::Hidden
                                size=Size::Medium
                                tone=tone
                                label=t(name)
                            />
                        }
                    })
                    .collect_view()}
            </p>
            // Start, Center and End: each on a line of its own, whose start
            // and end follow the page's direction, each a size of its own:
            // a drawing shorter than the line, one as tall and one taller,
            // so a long label's first line shows centered on each.
            {[
                (Placement::Start, Size::Small, msg::UI_GALLERY_SPINNER_START),
                (Placement::Center, Size::Medium, msg::UI_GALLERY_SPINNER_CENTER),
                (Placement::End, Size::XLarge, msg::UI_GALLERY_SPINNER_END),
            ]
                .into_iter()
                .map(|(placement, size, name)| {
                    view! { <Spinner placement=placement size=size label=t(name) /> }
                })
                .collect_view()}
            // A button busy with what it started: unavailable but still
            // reachable, a spinner a screen reader skips before its label,
            // which names it.
            <p>
                <Button content=Content::Text(t(msg::UI_GALLERY_SPINNER_SAVE).into()) busy=true />
            </p>
        </section>
    }
}
