//! The gallery's pause control demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::button::Content;
use crate::components::{Button, PauseUpdates};
use crate::live::say;
use crate::pause::{self, Ends};
use crate::routes::text_with;
use crate::text;

/// The pause control's demo (`#pause-control-demo`).
#[component]
pub(super) fn PauseUpdatesDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The pause control's demo: whether it is paused, and the demo runs that
    // finished meanwhile, each a change the banner counts.
    let demo_paused = RwSignal::new(false);
    let demo_waiting = RwSignal::new(0usize);
    let demo_ends = StoredValue::new(Ends::default());
    let demo_finished = t(msg::UI_GALLERY_BACKUP_FINISHED);
    let banner_localizer = Arc::clone(&localizer);
    let resumed_localizer = Arc::clone(&localizer);
    view! {
        <section id="pause-control-demo">
            <DemoHeading text=t(msg::UI_GALLERY_PAUSE_HEADING) />
            <PauseUpdates
                label=t(msg::UI_GALLERY_PAUSE_DEMO)
                paused_said=t(msg::UI_LIVE_PAUSED)
                paused=demo_paused
                banner=Signal::derive(move || {
                    let words = pause::banner(demo_waiting.get());
                    text_with(&banner_localizer, words.id, &words.args)
                })
                banner_id="gallery-paused"
                on_pause=Callback::new(move |()| {
                    demo_ends.set_value(Ends::default());
                    demo_waiting.set(0);
                    demo_paused.set(true);
                })
                on_resume=Callback::new(move |()| {
                    demo_paused.set(false);
                    demo_waiting.set(0);
                    let words = pause::resumed(demo_ends.get_value());
                    demo_ends.set_value(Ends::default());
                    text_with(&resumed_localizer, words.id, &words.args)
                })
            />
            <p>
                // A run that finishes by itself: said at once, or
                // counted while the demo is paused.
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_FINISH_DEMO_RUN).into())
                    on_press=Callback::new(move |()| {
                        if demo_paused.get_untracked() {
                            demo_ends.update_value(|ends| ends.count("succeeded"));
                            demo_waiting.update(|waiting| *waiting = waiting.saturating_add(1));
                        } else {
                            say(Politeness::Polite, &demo_finished);
                        }
                    })
                />
            </p>
        </section>
    }
}
