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
use crate::components::{
    Checkbox, Confirmation, FormField, ModalDialog, Progress, ReviewDialog, SortableTable,
    StatusBadge, Toast, show_toast,
};
use crate::live::{modal_gone, open_modal, say};
use crate::routes::PageHeading;
use crate::text;

#[component]
pub(crate) fn Gallery(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
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
    // without a `close` event, so leaving the page ends the count too.
    let counted = RwSignal::new(false);
    on_cleanup(move || modal_gone(counted));
    // The demos' state: a form-field error, the progress bar's percent and
    // the toast text.
    let form_error = RwSignal::<Option<String>>::new(None);
    let form_value = RwSignal::new(String::new());
    let required_error = t(msg::UI_GALLERY_REPOSITORY_REQUIRED);
    let progress = RwSignal::new(40u8);
    let toast = RwSignal::<Option<String>>::new(None);
    let settings_saved = t(msg::UI_GALLERY_SETTINGS_SAVED);
    // The review demos: whether each is open, and what confirming the first
    // announces.
    let review_open = RwSignal::new(false);
    let empty_review_open = RwSignal::new(false);
    let review_confirmed = t(msg::UI_GALLERY_DEMO_REVIEW_CONFIRMED);
    let review_body = t(msg::UI_GALLERY_DEMO_REVIEW_BODY);
    let empty_review_body = t(msg::UI_GALLERY_DEMO_EMPTY_REVIEW_BODY);
    let dialog_body = t(msg::UI_GALLERY_DEMO_DIALOG_BODY);
    let dialog_polite_text = backup_finished.clone();
    let dialog_polite_label = polite_button.clone();
    view! {
        <PageHeading>{heading}</PageHeading>
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
                <button
                    type="button"
                    on:click=move |_| {
                        if let Some(dialog) = dialog.get() {
                            open_modal(&dialog, counted);
                        }
                    }
                >
                    {open_modal_button}
                </button>
            </p>
            <dialog
                node_ref=dialog
                aria-labelledby="gallery-modal-heading"
                on:close=move |_| modal_gone(counted)
            >
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
                    <button
                        type="button"
                        on:click=move |_| say(Politeness::Assertive, &restore_failed_in_modal)
                    >
                        {modal_assertive_button}
                    </button>
                </p>
                <p>
                    <button
                        type="button"
                        on:click=move |_| {
                            if let Some(dialog) = dialog.get() {
                                dialog.close();
                            }
                        }
                    >
                        {close_modal_button}
                    </button>
                </p>
            </dialog>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_DIALOG_HEADING)}</h2>
            <ModalDialog
                label=t(msg::UI_GALLERY_DEMO_DIALOG)
                open_label=t(msg::UI_GALLERY_OPEN_DEMO_DIALOG)
                close_label=t(msg::UI_GALLERY_CLOSE_DEMO_DIALOG)
            >
                <p>{dialog_body.clone()}</p>
                <p>
                    <button
                        type="button"
                        on:click=move |_| say(Politeness::Polite, &dialog_polite_text)
                    >
                        {dialog_polite_label.clone()}
                    </button>
                </p>
            </ModalDialog>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_TABLE_HEADING)}</h2>
            <SortableTable
                caption=t(msg::UI_GALLERY_DEMO_REPOSITORIES)
                column=t(msg::UI_GALLERY_REPOSITORY_COLUMN)
                sort_label=t(msg::UI_GALLERY_SORT_BY_REPOSITORY)
                sorted_message=t(msg::UI_GALLERY_SORTED_ASCENDING)
                rows=vec![
                    t(msg::UI_GALLERY_DEMO_REPO_MOOR),
                    t(msg::UI_GALLERY_DEMO_REPO_HARBOR),
                    t(msg::UI_GALLERY_DEMO_REPO_ANCHOR),
                ]
            />
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_FORM_HEADING)}</h2>
            <FormField
                id="form-field-input"
                value=form_value
                label=t(msg::UI_GALLERY_REPOSITORY_NAME)
                description=t(msg::UI_GALLERY_REPOSITORY_HINT)
                error=form_error
            />
            <p>
                <button
                    type="button"
                    on:click=move |_| {
                        // A second check of a corrected field clears the
                        // error, rewiring the input's description; only the
                        // error's arrival is announced.
                        if form_error.get_untracked().is_some() {
                            form_error.set(None);
                        } else {
                            form_error.set(Some(required_error.clone()));
                            say(Politeness::Polite, &required_error);
                        }
                    }
                >
                    {t(msg::UI_GALLERY_CHECK_REPOSITORY)}
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_CHECKBOX_HEADING)}</h2>
            // Its two states: off with a description, and on without one.
            <Checkbox
                id="gallery-notification"
                label=t(msg::UI_GALLERY_DEMO_NOTIFICATION)
                description=t(msg::UI_GALLERY_DEMO_NOTIFICATION_HINT)
                checked=RwSignal::new(false)
            />
            <Checkbox id="gallery-log" label=t(msg::UI_GALLERY_DEMO_LOG) checked=RwSignal::new(true) />
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_BADGE_HEADING)}</h2>
            <p class="flex flex-wrap gap-2">
                <StatusBadge state="loading" label=t(msg::UI_GALLERY_BADGE_LOADING) />
                <StatusBadge state="empty" label=t(msg::UI_GALLERY_BADGE_EMPTY) />
                <StatusBadge state="error" label=t(msg::UI_GALLERY_BADGE_ERROR) />
                <StatusBadge state="success" label=t(msg::UI_GALLERY_BADGE_SUCCESS) />
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_PROGRESS_HEADING)}</h2>
            <Progress label=t(msg::UI_GALLERY_DEMO_BACKUP_PROGRESS) value=progress />
            <p>
                <button type="button" on:click=move |_| {
                    if progress.get_untracked() < 100 {
                        progress.set((progress.get_untracked() + 40).min(100));
                    }
                }>
                    {t(msg::UI_GALLERY_ADVANCE_PROGRESS)}
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_TOAST_HEADING)}</h2>
            <Toast text=toast />
            <p>
                <button
                    type="button"
                    on:click=move |_| show_toast(toast, settings_saved.clone())
                >
                    {t(msg::UI_GALLERY_SAVE_SETTINGS)}
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_REVIEW_HEADING)}</h2>
            <p>
                <button type="button" id="gallery-review" on:click=move |_| review_open.set(true)>
                    {t(msg::UI_GALLERY_OPEN_REVIEW)}
                </button>
            </p>
            <p>
                <button
                    type="button"
                    id="gallery-empty-review"
                    on:click=move |_| empty_review_open.set(true)
                >
                    {t(msg::UI_GALLERY_OPEN_EMPTY_REVIEW)}
                </button>
            </p>
            <ReviewDialog
                open=review_open
                title=t(msg::UI_GALLERY_DEMO_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CANCEL)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CONFIRM_DEMO_REVIEW),
                    action: Callback::new(move |()| {
                        review_open.set(false);
                        say(Politeness::Polite, &review_confirmed);
                    }),
                })
                returns_to="gallery-review".to_owned()
                described_by="gallery-review-body".to_owned()
            >
                <p id="gallery-review-body">{review_body}</p>
            </ReviewDialog>
            <ReviewDialog
                open=empty_review_open
                title=t(msg::UI_GALLERY_DEMO_EMPTY_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=None
                returns_to="gallery-empty-review".to_owned()
                described_by="gallery-empty-review-body".to_owned()
            >
                <p id="gallery-empty-review-body">{empty_review_body}</p>
            </ReviewDialog>
        </section>
    }
}
