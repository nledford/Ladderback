//! The gallery's steps demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::{Button, Reached, Steps, StepsName};
use crate::text;

/// The steps' demo (`#steps-demo`).
#[component]
pub(super) fn StepsDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // Where the steps demo's hub move stands: at its third step, its long
    // one, until its button moves it on.
    let hub_move = RwSignal::new(Reached::At(2));
    view! {
        <section id="steps-demo">
            <DemoHeading text=t(msg::UI_GALLERY_STEPS_HEADING) />
            // Two done, the current one's long name wrapping, two to do;
            // the button moves it on, each change said.
            <Steps
                localizer=Arc::clone(&localizer)
                name=StepsName(t(msg::UI_GALLERY_STEPS_MOVE))
                steps=vec![
                    t(msg::UI_GALLERY_STEPS_CHECK),
                    t(msg::UI_GALLERY_STEPS_COPY),
                    t(msg::UI_GALLERY_STEPS_COMMIT),
                    t(msg::UI_GALLERY_STEPS_SWITCH),
                    t(msg::UI_GALLERY_STEPS_CLEAN_UP),
                ]
                reached=hub_move
            />
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_STEPS_ADVANCE).into())
                    on_press=Callback::new(move |()| {
                        hub_move
                            .update(|reached| {
                                *reached = match *reached {
                                    Reached::At(place) if place < 4 => Reached::At(place + 1),
                                    Reached::At(_) => Reached::Finished,
                                    Reached::FailedAt(_) | Reached::Finished => Reached::At(0),
                                };
                            });
                    })
                />
            </p>
            // One done, one failed, one to do.
            <Steps
                localizer=Arc::clone(&localizer)
                name=StepsName(t(msg::UI_GALLERY_STEPS_PAIRING))
                steps=vec![
                    t(msg::UI_GALLERY_STEPS_TOKEN),
                    t(msg::UI_GALLERY_STEPS_CONNECT),
                    t(msg::UI_GALLERY_STEPS_APPROVAL),
                ]
                reached=Reached::FailedAt(1)
            />
        </section>
    }
}
