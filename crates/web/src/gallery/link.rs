//! The gallery's link demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::{DemoHeading, demo_link, sentence_link};
use crate::components::Link;
use crate::components::link::rules::{Destination, LinkPlacement, LinkTone, Matching};
use crate::text;

/// The link's demo (`#link-demo`).
#[component]
pub(super) fn LinkDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // A link standing alone, a row of a list of links.
    let standing = |path: &str, words: String| {
        Destination::route(path, Matching::Exact).map(|to| {
            view! {
                <li>
                    <Link to=to localizer=Arc::clone(&localizer) placement=LinkPlacement::Alone>
                        {words}
                    </Link>
                </li>
            }
        })
    };
    view! {
        <section id="link-demo">
            <DemoHeading text=t(msg::UI_GALLERY_LINK_HEADING) />
            // A page of the app, and the page the reader is on, which is the
            // current page and bold.
            <p>
                {demo_link(
                    &localizer,
                    Destination::route("/", Matching::Exact),
                    t(msg::UI_GALLERY_LINK_START),
                )}
            </p>
            <p>
                {demo_link(
                    &localizer,
                    Destination::route("/_gallery", Matching::Exact),
                    t(msg::UI_GALLERY_LINK_HERE),
                )}
            </p>
            // Another site: its icon after its words, and words for a
            // screen reader after them. Long enough to wrap at 320 px with
            // 200 % text.
            <p>
                {demo_link(
                    &localizer,
                    Destination::outside(RUSTIC_DOCS),
                    t(msg::UI_GALLERY_LINK_OUTSIDE),
                )}
            </p>
            // A link inside a sentence, and one inside a status sentence in
            // the Error tone, drawn in the sentence's color (Inherit).
            <p>
                {sentence_link(
                    &localizer,
                    Destination::route("/plans", Matching::Section),
                    msg::UI_GALLERY_LINK_IN_SENTENCE,
                    t(msg::UI_GALLERY_LINK_IN_SENTENCE_LINK),
                    LinkTone::Primary,
                )}
            </p>
            <p class="text-error">
                {sentence_link(
                    &localizer,
                    Destination::route("/log", Matching::Section),
                    msg::UI_GALLERY_LINK_STATUS,
                    t(msg::UI_GALLERY_LINK_STATUS_LINK),
                    LinkTone::Inherit,
                )}
            </p>
            // Another site in a line set right to left: its icon on the
            // words' left.
            <p dir="rtl">
                {demo_link(
                    &localizer,
                    Destination::outside(RUSTIC_DOCS),
                    t(msg::UI_GALLERY_LINK_OUTSIDE_RTL),
                )}
            </p>
            // Links standing alone, in a list of links: no underline at rest,
            // the page the reader is on bold with a bar at its start.
            <ul>
                {standing("/_gallery", t(msg::UI_GALLERY_LINK_STANDING_HERE))}
                {standing("/", t(msg::UI_GALLERY_LINK_STANDING_START))}
            </ul>
        </section>
    }
}

/// rustic's documentation, the demo's other site.
pub(super) const RUSTIC_DOCS: &str = "https://rustic.cli.rs/";
