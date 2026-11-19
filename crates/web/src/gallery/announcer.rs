//! The gallery's announcer demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::Button;
use crate::components::button::Content;
use crate::live::{close_modal_when_gone, modal_gone, open_modal, say};
use crate::text;

/// The announcer's demo (`#announcer-demo`): a polite and an assertive message, and a modal.
#[component]
pub(super) fn AnnouncerDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    let announcer_heading = t(msg::UI_GALLERY_ANNOUNCER_HEADING);
    let (polite_button, polite_other_button) = (
        t(msg::UI_GALLERY_POLITE_BUTTON),
        t(msg::UI_GALLERY_POLITE_OTHER_BUTTON),
    );
    let (assertive_button, open_modal_button) = (
        t(msg::UI_GALLERY_ASSERTIVE_BUTTON),
        t(msg::UI_GALLERY_OPEN_MODAL),
    );
    let (modal_heading, modal_polite_button, modal_assertive_button, close_modal_button) = (
        t(msg::UI_GALLERY_MODAL_HEADING),
        t(msg::UI_GALLERY_MODAL_POLITE_BUTTON),
        t(msg::UI_GALLERY_MODAL_ASSERTIVE_BUTTON),
        t(msg::UI_GALLERY_CLOSE_MODAL),
    );
    let backup_finished = t(msg::UI_GALLERY_BACKUP_FINISHED);
    let agent_online = t(msg::UI_GALLERY_AGENT_ONLINE);
    let restore_failed = t(msg::UI_GALLERY_RESTORE_FAILED);
    let backup_finished_in_modal = backup_finished.clone();
    let restore_failed_in_modal = restore_failed.clone();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    // Whether the announcer counts the dialog as an open modal. Taking the page
    // away while it is open, with the back button say, removes the dialog
    // without a `close` event, so leaving the page closes it and ends the count.
    let counted = RwSignal::new(false);
    close_modal_when_gone(dialog, counted);
    view! {
        <section id="announcer-demo">
            <DemoHeading text=announcer_heading />
            <p>
                <Button
                    content=Content::Text(polite_button.into())
                    on_press=Callback::new(move |()| say(Politeness::Polite, &backup_finished))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(polite_other_button.into())
                    on_press=Callback::new(move |()| say(Politeness::Polite, &agent_online))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(assertive_button.into())
                    on_press=Callback::new(move |()| say(Politeness::Assertive, &restore_failed))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(open_modal_button.into())
                    on_press=Callback::new(move |()| {
                        if let Some(dialog) = dialog.get() {
                            open_modal(&dialog, counted);
                        }
                    })
                />
            </p>
            <dialog
                node_ref=dialog
                aria-labelledby="gallery-modal-heading"
                on:close=move |_| modal_gone(counted)
            >
                <h2 id="gallery-modal-heading">{modal_heading}</h2>
                <p>
                    <Button
                        content=Content::Text(modal_polite_button.into())
                        on_press=Callback::new(move |()| {
                            say(Politeness::Polite, &backup_finished_in_modal);
                        })
                    />
                </p>
                <p>
                    <Button
                        content=Content::Text(modal_assertive_button.into())
                        on_press=Callback::new(move |()| {
                            say(Politeness::Assertive, &restore_failed_in_modal);
                        })
                    />
                </p>
                <p>
                    <Button
                        content=Content::Text(close_modal_button.into())
                        on_press=Callback::new(move |()| {
                            if let Some(dialog) = dialog.get() {
                                dialog.close();
                            }
                        })
                    />
                </p>
            </dialog>
        </section>
    }
}
