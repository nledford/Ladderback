//! The component gallery (technical design §4.5, "Tests"): the dev-only route
//! `/_gallery`, where Playwright drives the shared components with real input.
//! Built only with the `gallery` feature. Today it shows the announcer: buttons
//! that queue a polite or an assertive message, and a dialog standing in for a
//! modal.

use std::rc::Rc;
use std::sync::Arc;

use api::{EntryDto, EntryKindDto, FolderDto, MsgArg};
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::announcer::Politeness;
use crate::backend::Progress as Reading;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::icon::Meaning;
use crate::components::spinner::rules::LabelShown;
use crate::components::status_badge::rules::BadgeState;
use crate::components::{
    Button, Checkbox, Confirmation, FolderLoader, FolderPage, FormField, Icon, ModalDialog,
    PauseUpdates, Progress, RadioGroup, ReviewDialog, SortableTable, Spinner, StatusBadge, StepUp,
    StepUpField, Toast, Tree, show_toast,
};
use crate::live::{close_modal_when_gone, modal_gone, open_modal, reply, say};
use crate::pause::{self, Ends};
use crate::repositories::Sentence;
use crate::routes::{PageHeading, text_with};
use crate::text;
use crate::variants::{Placement, Size, Tone};

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
    // without a `close` event, so leaving the page closes it and ends the count.
    let counted = RwSignal::new(false);
    close_modal_when_gone(dialog, counted);
    // The demos' state: a form-field error, the progress bar's percent and
    // the toast text.
    let form_error = RwSignal::<Option<String>>::new(None);
    let form_value = RwSignal::new(String::new());
    let required_error = t(msg::UI_GALLERY_REPOSITORY_REQUIRED);
    // The multi-line field's demo: its entries and its error, which its own
    // button shows and clears as the one-line field's does.
    let lines_error = RwSignal::<Option<String>>::new(None);
    let lines_value = RwSignal::new(String::new());
    let folders_error = t(msg::UI_GALLERY_FOLDERS_REQUIRED);
    const GIB: u64 = 1 << 30;
    let progress = RwSignal::new(Reading {
        step: Some(api::OperationStep::BackingUp),
        unit: api::ProgressUnit::Bytes,
        current: 4 * GIB,
        total: Some(10 * GIB),
    });
    let toast = RwSignal::<Option<String>>::new(None);
    let settings_saved = t(msg::UI_GALLERY_SETTINGS_SAVED);
    // Whether the icon demo's notifications are on, which its icon-only
    // toggle button shows and changes.
    let notifications = RwSignal::new(false);
    // The review demos: whether each is open, and what confirming the first
    // announces, either way it goes ahead. The third starts on a button of
    // its own (`starts_on`).
    let review_open = RwSignal::new(false);
    let empty_review_open = RwSignal::new(false);
    let secret_review_open = RwSignal::new(false);
    let secret_copy = NodeRef::<leptos::html::Button>::new();
    let review_confirmed = t(msg::UI_GALLERY_DEMO_REVIEW_CONFIRMED);
    let review_body = t(msg::UI_GALLERY_DEMO_REVIEW_BODY);
    let also_confirmed = review_confirmed.clone();
    let empty_review_body = t(msg::UI_GALLERY_DEMO_EMPTY_REVIEW_BODY);
    let secret_review_body = t(msg::UI_GALLERY_DEMO_SECRET_REVIEW_BODY);
    let copy_demo_secret = t(msg::UI_GALLERY_COPY_DEMO_SECRET);
    // The fourth is held: it says why it can't be confirmed, and never is.
    let held_review_open = RwSignal::new(false);
    let held_review_body = t(msg::UI_GALLERY_DEMO_HELD_REVIEW_BODY);
    let held_review_reason = t(msg::UI_GALLERY_DEMO_HELD_REVIEW_REASON);
    let dialog_body = t(msg::UI_GALLERY_DEMO_DIALOG_BODY);
    let dialog_polite_text = backup_finished.clone();
    let dialog_polite_label = polite_button.clone();
    // The pause control's demo: whether it is paused, and the demo runs that
    // finished meanwhile, each a change the banner counts.
    let demo_paused = RwSignal::new(false);
    let demo_waiting = RwSignal::new(0usize);
    let demo_ends = StoredValue::new(Ends::default());
    let demo_finished = backup_finished.clone();
    let banner_localizer = Arc::clone(&localizer);
    let resumed_localizer = Arc::clone(&localizer);
    // The step-up demos, one per thing the field can ask for: whether each
    // dialog is open, what is typed and the error its confirm button shows,
    // whatever was typed.
    let step_up_open = RwSignal::new(false);
    let step_up_value = RwSignal::new(String::new());
    let step_up_error = RwSignal::<Option<String>>::new(None);
    let step_up_wrong = t(msg::UI_GALLERY_DEMO_STEP_UP_WRONG);
    let step_up_body = t(msg::UI_GALLERY_DEMO_STEP_UP_BODY);
    let step_up_localizer = Arc::clone(&localizer);
    let step_up_id_open = RwSignal::new(false);
    let step_up_id_value = RwSignal::new(String::new());
    let step_up_id_error = RwSignal::<Option<String>>::new(None);
    let step_up_id_wrong = t(msg::UI_GALLERY_DEMO_STEP_UP_ID_WRONG);
    let step_up_id_body = t(msg::UI_GALLERY_DEMO_STEP_UP_BODY);
    let step_up_id_localizer = Arc::clone(&localizer);
    let step_up_id = StepUp::ShortId {
        label: t(msg::UI_GALLERY_DEMO_STEP_UP_ID_LABEL),
        hint: t(msg::UI_GALLERY_DEMO_STEP_UP_ID_HINT),
        short: "1a2b3c4d".to_owned(),
    };
    // The button demo: how many times its buttons were pressed (its toggles
    // not counted), whether its busy buttons (its submit button among them)
    // are busy, which its toggle sets, and whether its other toggle is
    // pressed.
    let presses = RwSignal::new(0u64);
    let count = Callback::new(move |()| presses.update(|n| *n += 1));
    let working = RwSignal::new(false);
    let hidden_files = RwSignal::new(false);
    let presses_localizer = Arc::clone(&localizer);
    let form_sent = t(msg::UI_GALLERY_BUTTON_SENT);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>
            <A href="/">{start_link}</A>
        </p>
        <p>
            <A href="/_missing">{missing_link}</A>
        </p>
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
                // A second check of a corrected field clears the
                // error, rewiring the input's description; only the
                // error's arrival is announced.
                <button
                    type="button"
                    on:click=move |_| {
                        if form_error.get_untracked().is_some() {
                            form_error.set(None);
                        } else {
                            form_error.set(Some(required_error.clone()));
                            reply(Politeness::Polite, &required_error);
                        }
                    }
                >
                    {t(msg::UI_GALLERY_CHECK_REPOSITORY)}
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_LINES_HEADING)}</h2>
            <FormField
                id="form-field-lines"
                value=lines_value
                label=t(msg::UI_GALLERY_DEMO_FOLDERS)
                description=t(msg::UI_GALLERY_DEMO_FOLDERS_HINT)
                error=lines_error
                lines=3
            />
            <p>
                <button
                    type="button"
                    on:click=move |_| {
                        if lines_error.get_untracked().is_some() {
                            lines_error.set(None);
                        } else {
                            lines_error.set(Some(folders_error.clone()));
                            reply(Politeness::Polite, &folders_error);
                        }
                    }
                >
                    {t(msg::UI_GALLERY_CHECK_FOLDERS)}
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
            <Checkbox
                id="gallery-log"
                label=t(msg::UI_GALLERY_DEMO_LOG)
                checked=RwSignal::new(true)
            />
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_RADIO_HEADING)}</h2>
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
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_BADGE_HEADING)}</h2>
            <p class="flex flex-wrap gap-2">
                // Every state a badge can show, each with its own icon.
                <StatusBadge state=BadgeState::Queued label=t(msg::UI_STATUS_QUEUED) />
                <StatusBadge state=BadgeState::Running label=t(msg::UI_STATUS_RUNNING) />
                <StatusBadge state=BadgeState::Succeeded label=t(msg::UI_STATUS_SUCCEEDED) />
                <StatusBadge
                    state=BadgeState::SucceededWithWarnings
                    label=t(msg::UI_STATUS_SUCCEEDED_WITH_WARNINGS)
                />
                <StatusBadge state=BadgeState::Failed label=t(msg::UI_STATUS_FAILED) />
                <StatusBadge state=BadgeState::Cancelled label=t(msg::UI_STATUS_CANCELLED) />
                <StatusBadge state=BadgeState::Empty label=t(msg::UI_GALLERY_BADGE_EMPTY) />
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_VOCABULARY_HEADING)}</h2>
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
        <section>
            <h2>{t(msg::UI_GALLERY_PROGRESS_HEADING)}</h2>
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_BACKUP_PROGRESS)
                reading=progress
            />
            <p>
                <button
                    type="button"
                    on:click=move |_| {
                        progress
                            .update(|demo| demo.current = (demo.current + 4 * GIB).min(10 * GIB));
                    }
                >
                    {t(msg::UI_GALLERY_ADVANCE_PROGRESS)}
                </button>
            </p>
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_UNKNOWN_PROGRESS)
                reading=Signal::stored(Reading::default())
            />
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_STEP_PROGRESS)
                reading=Signal::stored(Reading {
                    step: Some(api::OperationStep::ReadingIndex),
                    ..Reading::default()
                })
            />
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_TOAST_HEADING)}</h2>
            <Toast text=toast />
            <p>
                <button type="button" on:click=move |_| show_toast(toast, settings_saved.clone())>
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
            <p>
                <button
                    type="button"
                    id="gallery-secret-review"
                    on:click=move |_| secret_review_open.set(true)
                >
                    {t(msg::UI_GALLERY_OPEN_SECRET_REVIEW)}
                </button>
            </p>
            <p>
                <button
                    type="button"
                    id="gallery-held-review"
                    on:click=move |_| held_review_open.set(true)
                >
                    {t(msg::UI_GALLERY_OPEN_HELD_REVIEW)}
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
                        reply(Politeness::Polite, &review_confirmed);
                    }),
                })
                also=Some(Confirmation {
                    label: t(msg::UI_GALLERY_ALSO_DEMO_REVIEW),
                    action: Callback::new(move |()| {
                        review_open.set(false);
                        reply(Politeness::Polite, &also_confirmed);
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
            <ReviewDialog
                open=secret_review_open
                title=t(msg::UI_GALLERY_DEMO_SECRET_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=None
                returns_to="gallery-secret-review".to_owned()
                described_by="gallery-secret-review-body".to_owned()
                starts_on=secret_copy
            >
                <p id="gallery-secret-review-body">{secret_review_body}</p>
                <p>
                    <button type="button" node_ref=secret_copy>
                        {copy_demo_secret}
                    </button>
                </p>
            </ReviewDialog>
            <ReviewDialog
                open=held_review_open
                title=t(msg::UI_GALLERY_DEMO_HELD_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CANCEL)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CONFIRM_HELD_REVIEW),
                    action: Callback::new(move |()| held_review_open.set(false)),
                })
                returns_to="gallery-held-review".to_owned()
                described_by="gallery-held-review-body".to_owned()
                held=Signal::stored(Some(held_review_reason))
            >
                <p id="gallery-held-review-body">{held_review_body}</p>
            </ReviewDialog>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_TREE_HEADING)}</h2>
            <Tree
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_TREE)
                id="gallery-tree".to_owned()
                loader=demo_tree(Arc::clone(&localizer))
                empty=t(msg::UI_SNAPSHOT_EMPTY)
            />
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_STEP_UP_HEADING)}</h2>
            <p>
                <button type="button" id="gallery-step-up" on:click=move |_| step_up_open.set(true)>
                    {t(msg::UI_GALLERY_OPEN_STEP_UP)}
                </button>
            </p>
            <ReviewDialog
                open=step_up_open
                title=t(msg::UI_GALLERY_DEMO_STEP_UP)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CHECK_DEMO_STEP_UP),
                    action: Callback::new(move |()| step_up_error.set(Some(step_up_wrong.clone()))),
                })
                returns_to="gallery-step-up".to_owned()
                described_by="gallery-step-up-body".to_owned()
            >
                <p id="gallery-step-up-body">{step_up_body}</p>
                <StepUpField
                    id="gallery-step-up-field"
                    localizer=step_up_localizer
                    step_up=StepUp::Password
                    value=step_up_value
                    error=step_up_error
                />
            </ReviewDialog>
            <p>
                <button
                    type="button"
                    id="gallery-step-up-id"
                    on:click=move |_| step_up_id_open.set(true)
                >
                    {t(msg::UI_GALLERY_OPEN_STEP_UP_ID)}
                </button>
            </p>
            <ReviewDialog
                open=step_up_id_open
                title=t(msg::UI_GALLERY_DEMO_STEP_UP_ID)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CHECK_DEMO_STEP_UP_ID),
                    action: Callback::new(move |()| {
                        step_up_id_error.set(Some(step_up_id_wrong.clone()));
                    }),
                })
                returns_to="gallery-step-up-id".to_owned()
                described_by="gallery-step-up-id-body".to_owned()
            >
                <p id="gallery-step-up-id-body">{step_up_id_body}</p>
                <StepUpField
                    id="gallery-step-up-id-field"
                    localizer=step_up_id_localizer
                    step_up=step_up_id
                    value=step_up_id_value
                    error=step_up_id_error
                />
            </ReviewDialog>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_PAUSE_HEADING)}</h2>
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
                <button
                    type="button"
                    on:click=move |_| {
                        if demo_paused.get_untracked() {
                            demo_ends.update_value(|ends| ends.count("succeeded"));
                            demo_waiting.update(|waiting| *waiting = waiting.saturating_add(1));
                        } else {
                            say(Politeness::Polite, &demo_finished);
                        }
                    }
                >
                    {t(msg::UI_GALLERY_FINISH_DEMO_RUN)}
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_SPINNER_HEADING)}</h2>
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
            // reachable, named by the spinner's label, which inherits the
            // button's text color.
            <p>
                <button type="button" aria-disabled="true">
                    <Spinner tone=Tone::Inherit label=t(msg::UI_GALLERY_SPINNER_SAVING) />
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_ICON_HEADING)}</h2>
            // The defaults: decoration, Inherit and Small, before the words
            // that say what it shows. A clock reads the same in either
            // direction, so a right-to-left page shows it unmirrored.
            <p>
                <Icon glyph=LucideGlyph::Clock />
                " "
                {t(msg::UI_GALLERY_ICON_LINE)}
            </p>
            // Every size, each icon named by its label, so each is an image.
            // The Small one names no size, so it shows the default.
            <p class="flex flex-wrap items-center gap-4">
                <Icon
                    glyph=LucideGlyph::Bell
                    meaning=Meaning::Labelled(t(msg::UI_GALLERY_ICON_SMALL))
                />
                {[
                    (Size::Medium, msg::UI_GALLERY_ICON_MEDIUM),
                    (Size::Large, msg::UI_GALLERY_ICON_LARGE),
                    (Size::XLarge, msg::UI_GALLERY_ICON_XLARGE),
                ]
                    .into_iter()
                    .map(|(size, name)| {
                        view! {
                            <Icon
                                glyph=LucideGlyph::Bell
                                size=size
                                meaning=Meaning::Labelled(t(name))
                            />
                        }
                    })
                    .collect_view()}
            </p>
            // Every tone, named in words beside its icon, which is
            // decoration. The line above shows Inherit as the default.
            <p class="flex flex-wrap items-center gap-4">
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
                        view! {
                            <span class="inline-flex items-center gap-1">
                                <Icon glyph=LucideGlyph::Info tone=tone />
                                {t(name)}
                            </span>
                        }
                    })
                    .collect_view()}
            </p>
            // An icon-only toggle button: named by its own label, which
            // stays the same pressed or not, the icon inside it decoration,
            // so a screen reader hears the name and the state once. The bell
            // is crossed out while it is off, so the shape shows the state
            // too. Pressing it changes nothing but the demo's own state.
            <p class="actions">
                <button
                    type="button"
                    aria-label=t(msg::UI_GALLERY_ICON_NOTIFICATIONS)
                    aria-pressed=move || if notifications.get() { "true" } else { "false" }
                    on:click=move |_| notifications.update(|on| *on = !*on)
                >
                    <Icon glyph=Signal::derive(move || {
                        if notifications.get() { LucideGlyph::Bell } else { LucideGlyph::BellOff }
                    }) />
                </button>
            </p>
        </section>
        <section>
            <h2>{t(msg::UI_GALLERY_BUTTON_HEADING)}</h2>
            // Every appearance in every tone, each named by the two in
            // words, a row an appearance. A Filled button has no Inherit.
            {[
                (
                    Appearance::Filled,
                    &[
                        (Tone::Primary, msg::UI_GALLERY_BUTTON_FILLED_PRIMARY),
                        (Tone::Secondary, msg::UI_GALLERY_BUTTON_FILLED_SECONDARY),
                        (Tone::Success, msg::UI_GALLERY_BUTTON_FILLED_SUCCESS),
                        (Tone::Warning, msg::UI_GALLERY_BUTTON_FILLED_WARNING),
                        (Tone::Error, msg::UI_GALLERY_BUTTON_FILLED_ERROR),
                    ][..],
                ),
                (
                    Appearance::Outlined,
                    &[
                        (Tone::Primary, msg::UI_GALLERY_BUTTON_OUTLINED_PRIMARY),
                        (Tone::Secondary, msg::UI_GALLERY_BUTTON_OUTLINED_SECONDARY),
                        (Tone::Success, msg::UI_GALLERY_BUTTON_OUTLINED_SUCCESS),
                        (Tone::Warning, msg::UI_GALLERY_BUTTON_OUTLINED_WARNING),
                        (Tone::Error, msg::UI_GALLERY_BUTTON_OUTLINED_ERROR),
                        (Tone::Inherit, msg::UI_GALLERY_BUTTON_OUTLINED_INHERIT),
                    ][..],
                ),
                (
                    Appearance::Ghost,
                    &[
                        (Tone::Primary, msg::UI_GALLERY_BUTTON_GHOST_PRIMARY),
                        (Tone::Secondary, msg::UI_GALLERY_BUTTON_GHOST_SECONDARY),
                        (Tone::Success, msg::UI_GALLERY_BUTTON_GHOST_SUCCESS),
                        (Tone::Warning, msg::UI_GALLERY_BUTTON_GHOST_WARNING),
                        (Tone::Error, msg::UI_GALLERY_BUTTON_GHOST_ERROR),
                        (Tone::Inherit, msg::UI_GALLERY_BUTTON_GHOST_INHERIT),
                    ][..],
                ),
            ]
                .into_iter()
                .map(|(appearance, tones): (Appearance, &[(Tone, MessageId)])| {
                    view! {
                        <p class="actions">
                            {tones
                                .iter()
                                .map(|&(tone, name)| {
                                    view! {
                                        <Button
                                            content=Content::Text(t(name).into())
                                            appearance=appearance
                                            tone=tone
                                            on_press=count
                                        />
                                    }
                                })
                                .collect_view()}
                        </p>
                    }
                })
                .collect_view()}
            // Every size, in the defaults otherwise (Outlined, Secondary).
            // The Medium one names no size, so it shows the default.
            <p class="actions">
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_SMALL).into())
                    size=Size::Small
                    on_press=count
                />
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_MEDIUM).into())
                    on_press=count
                />
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_LARGE).into())
                    size=Size::Large
                    on_press=count
                />
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_XLARGE).into())
                    size=Size::XLarge
                    on_press=count
                />
            </p>
            // An icon before its label, then an icon-only button at every
            // size, named by its label, the icon inside it decoration.
            <p class="actions">
                <Button
                    content=Content::IconAndText {
                        glyph: LucideGlyph::CalendarPlus.into(),
                        label: t(msg::UI_GALLERY_BUTTON_SCHEDULE).into(),
                    }
                    appearance=Appearance::Filled
                    tone=Tone::Primary
                    on_press=count
                />
                {[
                    (Size::Small, msg::UI_GALLERY_BUTTON_DETAILS_SMALL),
                    (Size::Medium, msg::UI_GALLERY_BUTTON_DETAILS_MEDIUM),
                    (Size::Large, msg::UI_GALLERY_BUTTON_DETAILS_LARGE),
                    (Size::XLarge, msg::UI_GALLERY_BUTTON_DETAILS_XLARGE),
                ]
                    .into_iter()
                    .map(|(size, name)| {
                        view! {
                            <Button
                                content=Content::IconOnly {
                                    glyph: LucideGlyph::Info.into(),
                                    label: t(name).into(),
                                }
                                size=size
                                on_press=count
                            />
                        }
                    })
                    .collect_view()}
            </p>
            // A toggle, then three buttons that turn busy while it is
            // pressed: one with only its label, whose spinner stands before
            // it, one whose spinner takes its icon's place and one with only
            // an icon. Busy, each keeps its name and its place in the Tab
            // order, and ignores presses. The toggle comes first, so the
            // label that grows moves nothing the reader is about to press.
            <p class="actions">
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_WORKING).into())
                    appearance=Appearance::Ghost
                    pressed=working
                    on_press=Callback::new(move |()| working.update(|on| *on = !*on))
                />
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_SAVE).into())
                    appearance=Appearance::Filled
                    tone=Tone::Primary
                    busy=working
                    on_press=count
                />
                <Button
                    content=Content::IconAndText {
                        glyph: LucideGlyph::CircleCheck.into(),
                        label: t(msg::UI_GALLERY_BUTTON_CHECK).into(),
                    }
                    busy=working
                    on_press=count
                />
                <Button
                    content=Content::IconOnly {
                        glyph: LucideGlyph::RotateCwClock.into(),
                        label: t(msg::UI_GALLERY_BUTTON_REFRESH).into(),
                    }
                    busy=working
                    on_press=count
                />
            </p>
            // A button that can't be used, still in the Tab order, and a
            // toggle, bolder with an inset edge while pressed.
            <p class="actions">
                <Button
                    content=Content::IconAndText {
                        glyph: LucideGlyph::X.into(),
                        label: t(msg::UI_GALLERY_BUTTON_DELETE).into(),
                    }
                    tone=Tone::Error
                    disabled=true
                    on_press=count
                />
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_BUTTON_HIDDEN_FILES).into())
                    pressed=hidden_files
                    on_press=Callback::new(move |()| hidden_files.update(|on| *on = !*on))
                />
            </p>
            // A submit button sends its small form, which says so politely,
            // unless the toggle above has made it busy: then it sends none.
            <form on:submit=move |event| {
                event.prevent_default();
                reply(Politeness::Polite, &form_sent);
            }>
                <p class="actions">
                    <Button
                        content=Content::Text(t(msg::UI_GALLERY_BUTTON_SEND).into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        kind=Kind::Submit
                        busy=working
                    />
                </p>
            </form>
            <p>
                {move || {
                    text_with(
                        &presses_localizer,
                        msg::UI_GALLERY_BUTTON_PRESSES,
                        &[("count", MsgArg::Count(presses.get()))],
                    )
                }}
            </p>
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

/// The demo tree's folders, read at once: `documents` holds two files,
/// `empty` nothing, `many` five files in pages of two, `slow` is never
/// answered (loading) and `unreadable` is refused (error). One file's key
/// isn't the name it shows, and one name is written right to left.
fn demo_tree(localizer: Arc<Localizer>) -> FolderLoader {
    Rc::new(move |path: String, from: u64| -> FolderPage {
        let name = |id| text(&localizer, id);
        let entry = |id, kind, size: Option<u64>, modified: Option<&str>| EntryDto {
            name: name(id),
            display: name(id),
            kind,
            size,
            modified: modified.map(str::to_owned),
        };
        let folder = |id| entry(id, EntryKindDto::Folder, None, None);
        let file = |id, size| entry(id, EntryKindDto::File, Some(size), None);
        let entries = if path.is_empty() {
            vec![
                folder(msg::UI_GALLERY_TREE_DOCUMENTS),
                folder(msg::UI_GALLERY_TREE_EMPTY),
                folder(msg::UI_GALLERY_TREE_MANY),
                folder(msg::UI_GALLERY_TREE_SLOW),
                folder(msg::UI_GALLERY_TREE_UNREADABLE),
                entry(msg::UI_GALLERY_TREE_LATEST, EntryKindDto::Link, None, None),
                entry(msg::UI_GALLERY_TREE_PIPE, EntryKindDto::Other, None, None),
                // rustic keeps U+202E as it is in the key; the daemon shows
                // it visibly.
                EntryDto {
                    name: "abc\u{202e}txt.exe".to_owned(),
                    ..file(msg::UI_GALLERY_TREE_SHOWN, 300)
                },
                file(msg::UI_GALLERY_TREE_WRITTEN_RTL, 64),
                file(msg::UI_GALLERY_TREE_README, 1_200),
            ]
        } else if path == name(msg::UI_GALLERY_TREE_DOCUMENTS) {
            vec![
                entry(
                    msg::UI_GALLERY_TREE_NOTES,
                    EntryKindDto::File,
                    Some(5),
                    Some("2026-11-01T12:00:00Z"),
                ),
                file(msg::UI_GALLERY_TREE_REPORT, 2_048),
            ]
        } else if path == name(msg::UI_GALLERY_TREE_EMPTY) {
            Vec::new()
        } else if path == name(msg::UI_GALLERY_TREE_MANY) {
            [
                msg::UI_GALLERY_TREE_ALPHA,
                msg::UI_GALLERY_TREE_BRAVO,
                msg::UI_GALLERY_TREE_CHARLIE,
                msg::UI_GALLERY_TREE_DELTA,
                msg::UI_GALLERY_TREE_ECHO,
            ]
            .into_iter()
            .map(|id| file(id, 100))
            .collect()
        } else if path == name(msg::UI_GALLERY_TREE_SLOW) {
            return Box::pin(std::future::pending());
        } else {
            let refused = Sentence::new(&name(msg::UI_GALLERY_TREE_UNREADABLE_SAID), None);
            return Box::pin(std::future::ready(Err(refused)));
        };
        // A page of two of `many`; every other folder in one page.
        let (total, size) = (
            entries.len() as u64,
            if entries.len() == 5 { 2 } else { 1_000 },
        );
        let page: Vec<EntryDto> = entries
            .into_iter()
            .skip(usize::try_from(from).unwrap_or(usize::MAX))
            .take(size)
            .collect();
        let next = from.saturating_add(page.len() as u64);
        Box::pin(std::future::ready(Ok(FolderDto {
            path,
            entries: page,
            total,
            next: (next < total).then_some(next),
        })))
    })
}
