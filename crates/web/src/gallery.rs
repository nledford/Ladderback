//! The component gallery (technical design §4.5, "Tests"): the dev-only route
//! `/_gallery`, where Playwright drives the shared components with real input.
//! Built only with the `gallery` feature. Today it shows the announcer: buttons
//! that queue a polite or an assertive message, and a dialog standing in for a
//! modal.

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos_router::components::A;

use crate::announcer::Politeness;
use crate::text;

/// Sends `text` to the live region of `politeness`, through the page's
/// announcer.
fn say(politeness: Politeness, text: &str) {
    let _ = (politeness, text);
}

/// The dialog that stands in for a modal: opening and closing it tells the
/// announcer, so polite messages wait while it is open.
fn open_modal() {}

fn close_modal() {}

#[component]
pub(crate) fn Gallery(localizer: Arc<Localizer>) -> impl IntoView {
    let t = |id| text(&localizer, id);
    let (heading, announcer_heading) = (
        t(msg::UI_GALLERY_HEADING),
        t(msg::UI_GALLERY_ANNOUNCER_HEADING),
    );
    let (start_link, missing_link) = (
        t(msg::UI_GALLERY_START_LINK),
        t(msg::UI_GALLERY_MISSING_LINK),
    );
    let (polite_button, polite_other_button) = (
        t(msg::UI_GALLERY_POLITE_BUTTON),
        t(msg::UI_GALLERY_POLITE_OTHER_BUTTON),
    );
    let (assertive_button, open_modal_button) = (
        t(msg::UI_GALLERY_ASSERTIVE_BUTTON),
        t(msg::UI_GALLERY_OPEN_MODAL),
    );
    let (modal_heading, modal_polite_button, close_modal_button) = (
        t(msg::UI_GALLERY_MODAL_HEADING),
        t(msg::UI_GALLERY_MODAL_POLITE_BUTTON),
        t(msg::UI_GALLERY_CLOSE_MODAL),
    );
    let backup_finished = t(msg::UI_GALLERY_BACKUP_FINISHED);
    let agent_online = t(msg::UI_GALLERY_AGENT_ONLINE);
    let restore_failed = t(msg::UI_GALLERY_RESTORE_FAILED);
    let backup_finished_in_modal = backup_finished.clone();
    view! {
        <h1 tabindex="-1">{heading}</h1>
        <p><A href="/">{start_link}</A></p>
        <p><A href="/_missing">{missing_link}</A></p>
        <section>
            <h2>{announcer_heading}</h2>
            <p>
                <button type="button" on:click=move |_| say(Politeness::Polite, &backup_finished)>
                    {polite_button}
                </button>
            </p>
            <p>
                <button type="button" on:click=move |_| say(Politeness::Polite, &agent_online)>
                    {polite_other_button}
                </button>
            </p>
            <p>
                <button type="button" on:click=move |_| say(Politeness::Assertive, &restore_failed)>
                    {assertive_button}
                </button>
            </p>
            <p>
                <button type="button" on:click=move |_| open_modal()>
                    {open_modal_button}
                </button>
            </p>
            <dialog aria-labelledby="gallery-modal-heading">
                <h2 id="gallery-modal-heading">{modal_heading}</h2>
                <p>
                    <button
                        type="button"
                        on:click=move |_| say(Politeness::Polite, &backup_finished_in_modal)
                    >
                        {modal_polite_button}
                    </button>
                </p>
                <p>
                    <button type="button" on:click=move |_| close_modal()>
                        {close_modal_button}
                    </button>
                </p>
            </dialog>
        </section>
    }
}
