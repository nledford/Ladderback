//! Settings' "Page width" section (technical design §4.5): how wide each
//! page's content is, kept in this browser. The choices, what a kept value
//! stands for, what a choice keeps and how each width lays out `<main>` are
//! `page_width.rs`'s, host-tested; keeping it is `page.rs`'s. This is the DOM
//! glue, which the `@ui` scenarios of `features/ui/page_width.feature` run in
//! Chromium, Firefox and WebKit.

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use crate::announcer::Politeness;
use crate::components::alert::rules::AlertTone;
use crate::components::{Alert, RadioGroup};
use crate::live::reply;
use crate::page;
use crate::page_width::{GROUP, HEADING_ID, PageWidth, options};
use crate::text;

/// The section: a radio group of Narrow and Full width, the one in effect
/// chosen. A choice applies at once, with no button and no reload, to
/// `width`, the page's own (`App`), once this browser has kept it. A browser
/// that refuses to keep it is said so in an error alert, and assertively,
/// as the choice's answer, since the alert is no live region; the page and
/// the group stay as they were.
#[component]
pub(super) fn PageWidthSection(
    localizer: Arc<Localizer>,
    width: RwSignal<PageWidth>,
) -> impl IntoView {
    let chosen = RwSignal::new(width.get_untracked().value());
    let refused = RwSignal::new(false);
    let not_kept = text(&localizer, msg::UI_PAGE_WIDTH_NOT_KEPT);
    let said = not_kept.clone();
    // Arrowing through the group sets `chosen` at each step: each choice is
    // kept and applied as it is made.
    Effect::new(move |_| {
        let wanted = PageWidth::from_kept(Some(chosen.get()));
        let current = width.get_untracked();
        if wanted == current {
            return;
        }
        if page::keep_page_width(wanted.to_keep()) {
            refused.set(false);
            width.set(wanted);
        } else {
            refused.set(true);
            reply(Politeness::Assertive, &said);
            chosen.set(current.value());
        }
    });
    let choices = options(
        text(&localizer, msg::UI_PAGE_WIDTH_NARROW),
        text(&localizer, msg::UI_PAGE_WIDTH_FULL),
    );
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID>{text(&localizer, msg::UI_PAGE_WIDTH_HEADING)}</h2>
            <RadioGroup
                name=GROUP
                legend=text(&localizer, msg::UI_PAGE_WIDTH_LEGEND)
                description=text(&localizer, msg::UI_PAGE_WIDTH_HINT)
                options=choices
                chosen=chosen
            />
            {move || {
                refused
                    .get()
                    .then(|| {
                        let words = not_kept.clone();
                        view! {
                            <Alert localizer=Arc::clone(&alert_localizer) tone=AlertTone::Error>
                                {words}
                            </Alert>
                        }
                    })
            }}
        </section>
    }
}
