//! The component gallery (technical design §4.5, "Tests"): the dev-only route
//! `/_gallery`, where Playwright drives the shared components with real input.
//! Built only with the `gallery` feature. Today it shows the announcer: buttons
//! that queue a polite or an assertive message, and a dialog standing in for a
//! modal.

use std::num::NonZeroU8;
use std::rc::Rc;
use std::sync::Arc;

use api::{EntryDto, EntryKindDto, FolderDto, MsgArg};
use l10n::{Direction, Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::either::Either;
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use crate::announcer::Politeness;
use crate::backend::Progress as Reading;
use crate::components::alert::rules::AlertTone;
use crate::components::badge::rules::BadgeTone;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::icon::Meaning;
use crate::components::link::rules::{Destination, LinkTone, Matching};
use crate::components::skeleton::rules::{Shape, Share};
use crate::components::spinner::rules::LabelShown;
use crate::components::status_badge::rules::BadgeState;
use crate::components::tabs::{Activation, Holds, Orientation, TabSet, TabsName};
use crate::components::{
    Alert, Align, Badge, Banner, BannerDismissal, Button, Checkbox, Column, Confirmation,
    Dismissal, FolderLoader, FolderPage, Footer, Icon, LabelLanguage, Link, ModalDialog, Name,
    PauseUpdates, Progress, RadioGroup, ReviewDialog, Sentences, Skeleton, Sort, SortKey, Spinner,
    State, StatusBadge, StepUp, StepUpField, Tab, Table, Tabs, TextField, Tip, TitleLevel, Toast,
    ToastTone, Toasts, Tooltip, Tree, text_field,
};
use crate::live::{close_modal_when_gone, modal_gone, open_modal, reply, say};
use crate::page;
use crate::pause::{self, Ends};
use crate::repositories::Sentence;
use crate::routes::{PageHeading, text_with};
use crate::text;
use crate::variants::{Placement, Rem, Side, Size, StatusTone, Tone};

#[component]
pub(crate) fn Gallery(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    let table_localizer = Arc::clone(&localizer);
    // The tab the manual demo's set tells it is chosen.
    let manual_chosen = RwSignal::new(0_usize);
    // The badge demo's count, a number in its words.
    let pending_changes = text_with(
        &localizer,
        msg::UI_GALLERY_BADGES_PENDING,
        &[("count", MsgArg::Count(3))],
    );
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
    let address_value = RwSignal::new(String::new());
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
    // The reading of the progress demo's bars at each size and tone.
    let sixty = Reading {
        current: 6 * GIB,
        ..progress.get_untracked()
    };
    // The toast demo's toasts, and whether its restore's failure stands on
    // the page, as a failure must beside its toast.
    let toasts = Toasts::new();
    let restore_failure_shown = RwSignal::new(false);
    let toast_button = |id: &'static str, label, tone, words| {
        let words: String = t(words);
        view! {
            <p>
                <Button
                    content=Content::Text(t(label).into())
                    id=id
                    on_press=Callback::new(move |()| {
                        if tone == ToastTone::Error {
                            restore_failure_shown.set(true);
                        }
                        toasts.show(tone, words.clone());
                    })
                />
            </p>
        }
    };
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
    let dialog_open = RwSignal::new(false);
    // The demo warning, which hands Escape to the page as the idle warning
    // does: whether it is open, the button it starts on, and whether the page
    // has said it keeps the warning open, cleared first on each keep so a
    // second one is said again.
    let warning_open = RwSignal::new(false);
    let warning_keep = NodeRef::<leptos::html::Button>::new();
    let warning_kept = RwSignal::new(false);
    let keep_warning = move || {
        warning_kept.set(false);
        spawn_local(async move {
            tick().await;
            warning_kept.try_set(true);
        });
    };
    let (warning_body, warning_kept_text, warning_answer, warning_keep_label) = (
        t(msg::UI_GALLERY_DEMO_WARNING_BODY),
        t(msg::UI_GALLERY_DEMO_WARNING_KEPT),
        t(msg::UI_GALLERY_ANSWER_DEMO_WARNING),
        t(msg::UI_GALLERY_KEEP_DEMO_WARNING),
    );
    // The demo setting, whose button is drawn anew each time the setting
    // changes, as a page draws a control afresh once a run has changed what
    // it does: whether the setting is on, and whether its dialog is open.
    let setting_on = RwSignal::new(false);
    let setting_open = RwSignal::new(false);
    let (setting_turn_on, setting_turn_off, setting_body, setting_keep, setting_change) = (
        t(msg::UI_GALLERY_TURN_DEMO_SETTING_ON),
        t(msg::UI_GALLERY_TURN_DEMO_SETTING_OFF),
        t(msg::UI_GALLERY_DEMO_SETTING_BODY),
        t(msg::UI_GALLERY_KEEP_DEMO_SETTING),
        t(msg::UI_GALLERY_CHANGE_DEMO_SETTING),
    );
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
    let step_up_error = RwSignal::<Option<String>>::new(None);
    let step_up_wrong = t(msg::UI_GALLERY_DEMO_STEP_UP_WRONG);
    let step_up_body = t(msg::UI_GALLERY_DEMO_STEP_UP_BODY);
    let step_up_localizer = Arc::clone(&localizer);
    // The admin password's field is drawn afresh at each opening, as a
    // restore's is: the dialog opens again empty, a password shown there
    // hidden. Keyed on the opening, not on `open` turning false, which a
    // dialog closed with Escape and opened again at once may never see: its
    // late `close` finds it open again and is left alone (`ModalDialog`).
    let step_up_opening = RwSignal::new(0_u32);
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
    // The tooltip demo's icon-only button: its name, the tooltip's too.
    let schedule = t(msg::UI_GALLERY_TOOLTIP_SCHEDULE);
    // The alert demo: its words in each tone, the warning's with a link
    // inside it in the alert's text color (Inherit), and whether its
    // dismissible notice shows, which its Dismiss button clears and the
    // button after it sets again.
    let (alert_information, alert_success, alert_error) = (
        t(msg::UI_GALLERY_ALERT_INFORMATION),
        t(msg::UI_GALLERY_ALERT_SUCCESS),
        t(msg::UI_GALLERY_ALERT_ERROR),
    );
    let alert_warning = sentence_link(
        &localizer,
        Destination::route("/operations", Matching::Section),
        msg::UI_GALLERY_ALERT_WARNING,
        t(msg::UI_GALLERY_ALERT_WARNING_LINK),
        LinkTone::Inherit,
    );
    let notice_shown = RwSignal::new(true);
    let (notice, notice_show) = (
        t(msg::UI_GALLERY_ALERT_DISMISSIBLE),
        t(msg::UI_GALLERY_ALERT_SHOW),
    );
    let alert_localizer = Arc::clone(&localizer);
    // The skeleton demo: whether its plan has come, which its toggle sets,
    // the words that say the wait, and the plan's own lines.
    let plan_shown = RwSignal::new(false);
    let (plan_loading, plan_name) = (
        t(msg::UI_GALLERY_SKELETON_LOADING),
        t(msg::UI_GALLERY_SKELETON_PLAN),
    );
    let (plan_folder, plan_schedule, plan_keeps) = (
        t(msg::UI_GALLERY_SKELETON_FOLDER),
        t(msg::UI_GALLERY_SKELETON_SCHEDULE),
        t(msg::UI_GALLERY_SKELETON_KEEPS),
    );
    // Its shapes: three lines standing for a paragraph (the plan's lines
    // too), the last one shorter, a block standing for a field, and two
    // lines standing for a short note.
    let (paragraph, field, note) = (
        Shape::Lines {
            // Three lines: one, and two more.
            count: NonZeroU8::MIN.saturating_add(2),
            last: Share::percent(60),
        },
        Shape::Block {
            inline: Rem::tenths(200),
            block: Rem::tenths(25),
        },
        Shape::Lines {
            // Two lines: one, and one more.
            count: NonZeroU8::MIN.saturating_add(1),
            last: Share::percent(40),
        },
    );
    // The banner demo: its words in each tone it takes, and the edition of
    // its dismissible notice, which the button after it makes newer, so the
    // dismissal of the one before doesn't keep it away.
    let (banner_information, banner_warning, banner_notice) = (
        t(msg::UI_GALLERY_BANNER_INFORMATION),
        t(msg::UI_GALLERY_BANNER_WARNING),
        t(msg::UI_GALLERY_BANNER_DISMISSIBLE),
    );
    let banner_edition = RwSignal::new(0_u32);
    let banner_demo_localizer = Arc::clone(&localizer);
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{demo_link(&localizer, Destination::route("/", Matching::Exact), start_link)}</p>
        <p>
            {demo_link(&localizer, Destination::route("/_missing", Matching::Exact), missing_link)}
        </p>
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
        <section id="dialog-demo">
            <DemoHeading text=t(msg::UI_GALLERY_DIALOG_HEADING) />
            <Button
                content=Content::Text(t(msg::UI_GALLERY_OPEN_DEMO_DIALOG).into())
                on_press=Callback::new(move |()| dialog_open.set(true))
            />
            <ModalDialog
                open=dialog_open
                id="gallery-demo-dialog"
                title=t(msg::UI_GALLERY_DEMO_DIALOG)
                close_label=t(msg::UI_GALLERY_CLOSE_DEMO_DIALOG)
            >
                <p>{dialog_body.clone()}</p>
                <p>
                    <Button
                        content=Content::Text(dialog_polite_label.clone().into())
                        on_press=Callback::new(move |()| {
                            say(Politeness::Polite, &dialog_polite_text);
                        })
                    />
                </p>
            </ModalDialog>
            <Button
                content=Content::Text(t(msg::UI_GALLERY_OPEN_DEMO_WARNING).into())
                on_press=Callback::new(move |()| {
                    warning_kept.set(false);
                    warning_open.set(true);
                })
            />
            // Escape and Keep the demo warning keep it open; Answer the demo
            // warning closes it from outside, as the daemon's answer closes
            // the idle warning.
            <ModalDialog
                open=warning_open
                id="gallery-demo-warning"
                title=t(msg::UI_GALLERY_DEMO_WARNING)
                described_by="gallery-demo-warning-body"
                starts_on=warning_keep
                on_cancel=Callback::new(move |()| keep_warning())
            >
                <p id="gallery-demo-warning-body">{warning_body}</p>
                <p aria-live="polite" aria-atomic="true">
                    {move || warning_kept.get().then(|| warning_kept_text.clone())}
                </p>
                <p class="actions">
                    <Button
                        content=Content::Text(warning_answer.into())
                        on_press=Callback::new(move |()| warning_open.set(false))
                    />
                    <Button
                        content=Content::Text(warning_keep_label.into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        node_ref=warning_keep
                        on_press=Callback::new(move |()| keep_warning())
                    />
                </p>
            </ModalDialog>
            // Changing the setting draws its button anew, so the browser's own
            // return of focus, to the button that opened the dialog, finds it
            // gone: `returns_to` names the button by its id instead.
            {move || {
                let open_setting = Callback::new(move |()| setting_open.set(true));
                if setting_on.get() {
                    Either::Left(
                        view! {
                            <Button
                                content=Content::Text(setting_turn_off.clone().into())
                                id="gallery-demo-setting"
                                on_press=open_setting
                            />
                        },
                    )
                } else {
                    Either::Right(
                        view! {
                            <Button
                                content=Content::Text(setting_turn_on.clone().into())
                                id="gallery-demo-setting"
                                on_press=open_setting
                            />
                        },
                    )
                }
            }}
            <ModalDialog
                open=setting_open
                id="gallery-demo-setting-dialog"
                title=t(msg::UI_GALLERY_DEMO_SETTING)
                title_level=TitleLevel::Three
                described_by="gallery-demo-setting-body"
                returns_to="gallery-demo-setting"
            >
                <p id="gallery-demo-setting-body">{setting_body}</p>
                <p class="actions">
                    <Button
                        content=Content::Text(setting_keep.into())
                        on_press=Callback::new(move |()| setting_open.set(false))
                    />
                    <Button
                        content=Content::Text(setting_change.into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        on_press=Callback::new(move |()| {
                            setting_on.update(|on| *on = !*on);
                            setting_open.set(false);
                        })
                    />
                </p>
            </ModalDialog>
        </section>
        <section id="checkbox-demo">
            <DemoHeading text=t(msg::UI_GALLERY_CHECKBOX_HEADING) />
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
            // Its error, as a form shows once it is checked, under its
            // description.
            <Checkbox
                id="gallery-terms"
                label=t(msg::UI_GALLERY_DEMO_TERMS)
                description=t(msg::UI_GALLERY_DEMO_TERMS_HINT)
                error=Some(t(msg::UI_GALLERY_DEMO_TERMS_ERROR))
                checked=RwSignal::new(false)
            />
        </section>
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
        <section id="status-badge-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BADGE_HEADING) />
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
        <section id="badge-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BADGES_HEADING) />
            // A count and two tags, each saying in words what it counts or
            // names, the long one wrapping in a narrow room.
            <p class="flex flex-wrap gap-2">
                <Badge label=pending_changes icon=LucideGlyph::Pencil />
                <Badge label=t(msg::UI_GALLERY_BADGES_TAG) />
                <Badge label=t(msg::UI_GALLERY_BADGES_LONG) icon=LucideGlyph::CalendarClock />
            </p>
            // Every tone, named in words, so no tone is told by its color.
            <p class="flex flex-wrap gap-2">
                <Badge
                    label=t(msg::UI_GALLERY_TONE_PRIMARY)
                    tone=BadgeTone::Primary
                    icon=LucideGlyph::Info
                />
                <Badge label=t(msg::UI_GALLERY_TONE_SECONDARY) tone=BadgeTone::Secondary />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_SUCCESS)
                    tone=BadgeTone::Success
                    icon=LucideGlyph::CircleCheck
                />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_WARNING)
                    tone=BadgeTone::Warning
                    icon=LucideGlyph::TriangleAlert
                />
                <Badge
                    label=t(msg::UI_GALLERY_TONE_ERROR)
                    tone=BadgeTone::Error
                    icon=LucideGlyph::CircleX
                />
            </p>
        </section>
        <section id="shared-vocabulary-demo">
            <DemoHeading text=t(msg::UI_GALLERY_VOCABULARY_HEADING) />
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
            // Every status tone's pair, its name in its words on its fill,
            // so the axe row reads each pair's words on its fill.
            <h3 id="gallery-status-pairs">{t(msg::UI_GALLERY_STATUS_PAIRS_HEADING)}</h3>
            <ul
                class="vocabulary-demo flex flex-wrap gap-2"
                role="list"
                aria-labelledby="gallery-status-pairs"
            >
                {[
                    (StatusTone::Success, msg::UI_GALLERY_TONE_SUCCESS),
                    (StatusTone::Warning, msg::UI_GALLERY_TONE_WARNING),
                    (StatusTone::Error, msg::UI_GALLERY_TONE_ERROR),
                ]
                    .into_iter()
                    .map(|(tone, name)| {
                        let class = format!("px-2 rounded-(--radius-small) {}", tone.pair());
                        view! { <li class=class>{t(name)}</li> }
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
        <section id="toast-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TOAST_HEADING) />
            // A button for each tone, then the toast under them, so Tab
            // reaches its Dismiss button from each.
            {toast_button(
                TOAST_DEMO_SAVE,
                msg::UI_GALLERY_SAVE_SETTINGS,
                ToastTone::Success,
                msg::UI_GALLERY_SETTINGS_SAVED,
            )}
            {toast_button(
                "toast-demo-queue",
                msg::UI_GALLERY_QUEUE_CHECK,
                ToastTone::Primary,
                msg::UI_GALLERY_CHECK_QUEUED,
            )}
            {toast_button(
                "toast-demo-back-up",
                msg::UI_GALLERY_BACK_UP_PLAN,
                ToastTone::Warning,
                msg::UI_GALLERY_BACKUP_WARNED,
            )}
            {toast_button(
                "toast-demo-restore",
                msg::UI_GALLERY_RESTORE_SNAPSHOT,
                ToastTone::Error,
                msg::UI_GALLERY_DEMO_RESTORE_FAILED,
            )}
            // Dismissed, a toast hands focus to the first button.
            <Toast localizer=Arc::clone(&localizer) toasts=toasts then_focus=TOAST_DEMO_SAVE />
            // The failure stays on the page after its toast has gone.
            {
                let localizer = Arc::clone(&localizer);
                let detail = t(msg::UI_GALLERY_DEMO_RESTORE_FAILED_DETAIL);
                move || {
                    restore_failure_shown
                        .get()
                        .then(|| {
                            let detail = detail.clone();
                            view! {
                                <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Error>
                                    {detail}
                                </Alert>
                            }
                        })
                }
            }
        </section>
        <section id="review-dialog-demo">
            <DemoHeading text=t(msg::UI_GALLERY_REVIEW_HEADING) />
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_REVIEW).into())
                    id="gallery-review"
                    on_press=Callback::new(move |()| review_open.set(true))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_EMPTY_REVIEW).into())
                    id="gallery-empty-review"
                    on_press=Callback::new(move |()| empty_review_open.set(true))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_SECRET_REVIEW).into())
                    id="gallery-secret-review"
                    on_press=Callback::new(move |()| secret_review_open.set(true))
                />
            </p>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_HELD_REVIEW).into())
                    id="gallery-held-review"
                    on_press=Callback::new(move |()| held_review_open.set(true))
                />
            </p>
            <ReviewDialog
                open=review_open
                title=t(msg::UI_GALLERY_DEMO_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CANCEL)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CONFIRM_DEMO_REVIEW),
                    tone: Tone::Primary,
                    action: Callback::new(move |()| {
                        review_open.set(false);
                        reply(Politeness::Polite, &review_confirmed);
                    }),
                })
                also=Some(Confirmation {
                    label: t(msg::UI_GALLERY_ALSO_DEMO_REVIEW),
                    tone: Tone::Error,
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
                    <Button content=Content::Text(copy_demo_secret.into()) node_ref=secret_copy />
                </p>
            </ReviewDialog>
            <ReviewDialog
                open=held_review_open
                title=t(msg::UI_GALLERY_DEMO_HELD_REVIEW)
                dismiss_label=t(msg::UI_REVIEW_CANCEL)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CONFIRM_HELD_REVIEW),
                    tone: Tone::Primary,
                    action: Callback::new(move |()| held_review_open.set(false)),
                })
                returns_to="gallery-held-review".to_owned()
                described_by="gallery-held-review-body".to_owned()
                held=Signal::stored(Some(held_review_reason))
            >
                <p id="gallery-held-review-body">{held_review_body}</p>
            </ReviewDialog>
        </section>
        <section id="tree-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TREE_HEADING) />
            <Tree
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_TREE)
                id="gallery-tree".to_owned()
                loader=demo_tree(Arc::clone(&localizer))
                empty=t(msg::UI_SNAPSHOT_EMPTY)
            />
        </section>
        <section id="step-up-field-demo">
            <DemoHeading text=t(msg::UI_GALLERY_STEP_UP_HEADING) />
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_STEP_UP).into())
                    id="gallery-step-up"
                    on_press=Callback::new(move |()| {
                        step_up_opening.update(|opening| *opening = opening.wrapping_add(1));
                        step_up_open.set(true);
                    })
                />
            </p>
            <ReviewDialog
                open=step_up_open
                title=t(msg::UI_GALLERY_DEMO_STEP_UP)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CHECK_DEMO_STEP_UP),
                    tone: Tone::Primary,
                    action: Callback::new(move |()| step_up_error.set(Some(step_up_wrong.clone()))),
                })
                returns_to="gallery-step-up".to_owned()
                described_by="gallery-step-up-body".to_owned()
            >
                <p id="gallery-step-up-body">{step_up_body}</p>
                {move || {
                    step_up_opening.track();
                    view! {
                        <StepUpField
                            id="gallery-step-up-field"
                            localizer=Arc::clone(&step_up_localizer)
                            step_up=StepUp::Password
                            value=RwSignal::new(String::new())
                            error=step_up_error
                        />
                    }
                }}
            </ReviewDialog>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_OPEN_STEP_UP_ID).into())
                    id="gallery-step-up-id"
                    on_press=Callback::new(move |()| step_up_id_open.set(true))
                />
            </p>
            <ReviewDialog
                open=step_up_id_open
                title=t(msg::UI_GALLERY_DEMO_STEP_UP_ID)
                dismiss_label=t(msg::UI_REVIEW_CLOSE)
                confirm=Some(Confirmation {
                    label: t(msg::UI_GALLERY_CHECK_DEMO_STEP_UP_ID),
                    tone: Tone::Primary,
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
        <section id="icon-demo">
            <DemoHeading text=t(msg::UI_GALLERY_ICON_HEADING) />
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
                <Button
                    content=Content::IconOnly {
                        glyph: Signal::derive(move || {
                            if notifications.get() {
                                LucideGlyph::Bell
                            } else {
                                LucideGlyph::BellOff
                            }
                        }),
                        label: t(msg::UI_GALLERY_ICON_NOTIFICATIONS).into(),
                    }
                    pressed=notifications
                    on_press=Callback::new(move |()| notifications.update(|on| *on = !*on))
                />
            </p>
        </section>
        <section id="button-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BUTTON_HEADING) />
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
        <section id="skeleton-demo">
            <DemoHeading text=t(msg::UI_GALLERY_SKELETON_HEADING) />
            // Three lines standing for a paragraph, the last one shorter,
            // and a block standing for a field, each in the room its line
            // of the page gives it.
            <div class="my-4">
                <Skeleton shape=paragraph />
            </div>
            <div class="my-4">
                <Skeleton shape=field />
            </div>
            // On Ledger Gray, as in a toast or the pause banner, where the
            // bars stand apart from it as they do from the page.
            <div class="bg-surface my-4 p-4">
                <Skeleton shape=note />
            </div>
            // A region whose content is on its way: busy, with its skeleton,
            // until the toggle below brings the content, three lines in the
            // place of the skeleton's three, so the region keeps its height
            // and focus stays on the toggle. The wait is said in words on
            // the line just before the region, outside it, since a screen
            // reader may leave a busy region unread; the plan's name takes
            // that line when the plan comes. Nothing announces the arrival.
            <p>
                {move || {
                    if plan_shown.get() { plan_name.clone() } else { plan_loading.clone() }
                }}
            </p>
            <div aria-busy=move || {
                if plan_shown.get() { "false" } else { "true" }
            }>
                {move || {
                    if plan_shown.get() {
                        view! {
                            <p class="my-0">
                                {plan_folder.clone()} <br /> {plan_schedule.clone()} <br />
                                {plan_keeps.clone()}
                            </p>
                        }
                            .into_any()
                    } else {
                        view! { <Skeleton shape=paragraph /> }.into_any()
                    }
                }}
            </div>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_SKELETON_SHOW).into())
                    pressed=plan_shown
                    on_press=Callback::new(move |()| plan_shown.update(|shown| *shown = !*shown))
                />
            </p>
        </section>
        <section id="banner-demo">
            <DemoHeading text=t(msg::UI_GALLERY_BANNER_HEADING) />
            // Each tone it takes, information first, which is the default.
            <Banner localizer=Arc::clone(&localizer)>{banner_information}</Banner>
            // A long path, which wraps inside the banner on a small screen.
            <Banner localizer=Arc::clone(&localizer) tone=BannerTone::Warning>
                {banner_warning}
            </Banner>
            // A notice the reader dismisses: it goes, focus goes to the
            // button that brings a newer one, and this browser keeps this
            // edition away on every later load.
            {move || {
                let edition = banner_edition.get().to_string();
                let notice = banner_notice.clone();
                view! {
                    <Banner
                        localizer=Arc::clone(&banner_demo_localizer)
                        dismissal=BannerDismissal {
                            remember_as: BANNER_DEMO,
                            edition,
                            then_focus: BANNER_DEMO_NEWER.to_owned(),
                        }
                    >
                        {notice}
                    </Banner>
                }
            }}
            <p>
                <Button
                    id=BANNER_DEMO_NEWER
                    content=Content::Text(t(msg::UI_GALLERY_BANNER_NEWER).into())
                    on_press=Callback::new(move |()| {
                        banner_edition.update(|edition| *edition = edition.saturating_add(1));
                    })
                />
            </p>
        </section>
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
        </section>
        <section id="alert-demo">
            <DemoHeading text=t(msg::UI_GALLERY_ALERT_HEADING) />
            // Each tone, information first, which is the default.
            <Alert localizer=Arc::clone(&localizer)>{alert_information}</Alert>
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Success>
                {alert_success}
            </Alert>
            // A link inside, in the alert's text color.
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Warning>
                {alert_warning}
            </Alert>
            // A long path, which wraps inside the alert on a small screen.
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Error>
                {alert_error}
            </Alert>
            // A notice the reader dismisses: the page takes it away, and
            // focus goes to the button that brings it back.
            {move || {
                notice_shown
                    .get()
                    .then(|| {
                        let notice = notice.clone();
                        view! {
                            <Alert
                                localizer=Arc::clone(&alert_localizer)
                                dismissal=Dismissal {
                                    on_dismiss: Callback::new(move |()| notice_shown.set(false)),
                                    then_focus: ALERT_DEMO_SHOW.to_owned(),
                                }
                            >
                                {notice}
                            </Alert>
                        }
                    })
            }}
            <p>
                <Button
                    id=ALERT_DEMO_SHOW
                    content=Content::Text(notice_show.into())
                    disabled=notice_shown
                    on_press=Callback::new(move |()| notice_shown.set(true))
                />
            </p>
        </section>
        <section id="tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_HEADING) />
            // Each tab the arrow keys arrive at is selected, the set named by
            // the heading above it. The middle panel holds a field, so it is
            // no Tab stop of its own, and what is typed there stays while
            // another panel shows.
            <h3 id="gallery-tabs-repository">{t(msg::UI_GALLERY_TABS_REPOSITORY)}</h3>
            <Tabs
                id="gallery-tabs-automatic"
                name=TabsName::LabelledBy("gallery-tabs-repository".to_owned())
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_OVERVIEW),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_OVERVIEW_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_NOTE),
                            Holds::Controls,
                            view! {
                                <TextField
                                    id="gallery-tabs-note-text"
                                    localizer=Arc::clone(&localizer)
                                    value=RwSignal::new(String::new())
                                    label=t(msg::UI_GALLERY_TABS_NOTE_FIELD)
                                    description=t(msg::UI_GALLERY_TABS_NOTE_HINT)
                                />
                            },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_HISTORY),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_HISTORY_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // Chosen by hand, as tabs whose panels read from the daemon
            // would be: the arrow keys only move focus. The paragraph above
            // says so and describes the list; the one below shows the tab
            // the set tells its caller is chosen.
            <p id="gallery-tabs-manual-hint">{t(msg::UI_GALLERY_TABS_MANUAL_HINT)}</p>
            <Tabs
                id="gallery-tabs-manual"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_AGENT))
                activation=Activation::Manual
                described_by="gallery-tabs-manual-hint"
                selected=manual_chosen
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_LOG),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_LOG_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_STATS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_STATS_TEXT)}</p> },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_PLANS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_PLANS_TEXT)}</p> },
                        ),
                    ],
                )
            />
            <p>
                {
                    let localizer = Arc::clone(&localizer);
                    let labels = [
                        t(msg::UI_GALLERY_TABS_LOG),
                        t(msg::UI_GALLERY_TABS_STATS),
                        t(msg::UI_GALLERY_TABS_PLANS),
                    ];
                    move || {
                        let tab = labels.get(manual_chosen.get()).cloned().unwrap_or_default();
                        text_with(
                            &localizer,
                            msg::UI_GALLERY_TABS_CHOSEN,
                            &[("tab", MsgArg::Text(tab))],
                        )
                    }
                }
            </p>
            // In a part of the page that runs right to left: the first tab
            // stands at the right, and the left arrow goes to the next.
            <div dir="rtl">
                <Tabs
                    id="gallery-tabs-rtl"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_RTL))
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
        <section id="vertical-tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_VERTICAL_HEADING) />
            // A column at the start of the panel: the down arrow goes to
            // the next tab and the up arrow to the one before.
            <Tabs
                id="gallery-tabs-vertical"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_PLAN))
                orientation=Orientation::Vertical
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_SOURCES),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_SOURCES_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_TIMES),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_TIMES_TEXT)}</p> },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_HOOKS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_HOOKS_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // The same in a part of the page that runs right to left: the
            // column at the panel's start, on the right, and the down arrow
            // still going to the next tab.
            <div dir="rtl">
                <Tabs
                    id="gallery-tabs-vertical-rtl"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_VERTICAL_RTL))
                    orientation=Orientation::Vertical
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_VERTICAL_RTL_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_VERTICAL_RTL_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_VERTICAL_RTL_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
        <section id="responsive-tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_RESPONSIVE_HEADING) />
            // As wide as the page: a column where the page is wide, a row
            // in a narrow window or with larger text. What is typed in the
            // middle panel stays while the layout changes.
            <Tabs
                id="gallery-tabs-responsive"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_SETTINGS))
                orientation=Orientation::Responsive
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_GENERAL),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_GENERAL_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_ADDRESS),
                            Holds::Controls,
                            view! {
                                <TextField
                                    id="gallery-tabs-address-input"
                                    localizer=Arc::clone(&localizer)
                                    value=address_value
                                    label=t(msg::UI_GALLERY_TABS_ADDRESS_FIELD)
                                    description=t(msg::UI_GALLERY_TABS_ADDRESS_HINT)
                                />
                            },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_ABOUT),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_ABOUT_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // The same tabs in a narrow column of a part of the page that
            // runs right to left: too narrow for the list beside the panel,
            // so a row, the left arrow going to the next tab.
            <div dir="rtl" class="max-w-sm">
                <Tabs
                    id="gallery-tabs-column"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_COLUMN))
                    orientation=Orientation::Responsive
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_COLUMN_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_COLUMN_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_COLUMN_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
        <section id="text-field-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TEXT_FIELD_HEADING) />
            <TextField
                id="form-field-input"
                localizer=Arc::clone(&localizer)
                value=form_value
                label=t(msg::UI_GALLERY_REPOSITORY_NAME)
                description=t(msg::UI_GALLERY_REPOSITORY_HINT)
                error=form_error
            />
            <p>
                // A second check of a corrected field clears the
                // error, rewiring the input's description; only the
                // error's arrival is announced.
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_CHECK_REPOSITORY).into())
                    on_press=Callback::new(move |()| {
                        if form_error.get_untracked().is_some() {
                            form_error.set(None);
                        } else {
                            form_error.set(Some(required_error.clone()));
                            reply(Politeness::Polite, &required_error);
                        }
                    })
                />
            </p>
            <TextField
                id="form-field-lines"
                localizer=Arc::clone(&localizer)
                value=lines_value
                label=t(msg::UI_GALLERY_DEMO_FOLDERS)
                description=t(msg::UI_GALLERY_DEMO_FOLDERS_HINT)
                error=lines_error
                kind=text_field::Kind::Lines { rows: 3 }
            />
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_CHECK_FOLDERS).into())
                    on_press=Callback::new(move |()| {
                        if lines_error.get_untracked().is_some() {
                            lines_error.set(None);
                        } else {
                            lines_error.set(Some(folders_error.clone()));
                            reply(Politeness::Polite, &folders_error);
                        }
                    })
                />
            </p>
            // Each kind, then each state, then a field and a button of the
            // same size in a row of actions, at every size. Last on the
            // page, so its many stops for Tab come after every other demo's.
            <TextField
                id="gallery-text-field-password"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_PASSWORD)
                description=t(msg::UI_GALLERY_TEXT_FIELD_PASSWORD_HINT)
                kind=text_field::Kind::Password
                autocomplete="current-password"
            />
            <TextField
                id="gallery-text-field-copies"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_COPIES)
                description=t(msg::UI_GALLERY_TEXT_FIELD_COPIES_HINT)
                kind=text_field::Kind::Number
            />
            <TextField
                id="gallery-text-field-time"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_TIME)
                description=t(msg::UI_GALLERY_TEXT_FIELD_TIME_HINT)
                icon=LucideGlyph::Clock
            />
            <TextField
                id="gallery-text-field-plan"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_PLAN)
                description=t(msg::UI_GALLERY_TEXT_FIELD_PLAN_HINT)
                required=true
            />
            <TextField
                id="gallery-text-field-host"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_HOST)
                description=t(msg::UI_GALLERY_TEXT_FIELD_HOST_HINT)
                error=RwSignal::new(Some(t(msg::UI_GALLERY_TEXT_FIELD_HOST_ERROR)))
            />
            <TextField
                id="gallery-text-field-snapshot"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new("1a2b3c4d".to_owned())
                label=t(msg::UI_GALLERY_TEXT_FIELD_SNAPSHOT)
                description=t(msg::UI_GALLERY_TEXT_FIELD_SNAPSHOT_HINT)
                kind=text_field::Kind::Code
                access=text_field::Access::ReadOnly
            />
            <TextField
                id="gallery-text-field-location"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new("/srv/demo".to_owned())
                label=t(msg::UI_GALLERY_TEXT_FIELD_LOCATION)
                description=t(msg::UI_GALLERY_TEXT_FIELD_LOCATION_HINT)
                access=text_field::Access::Disabled
            />
            <TextField
                id="gallery-text-field-keyring"
                localizer=Arc::clone(&localizer)
                value=RwSignal::new(String::new())
                label=t(msg::UI_GALLERY_TEXT_FIELD_KEYRING)
                description=t(msg::UI_GALLERY_TEXT_FIELD_KEYRING_HINT)
                kind=text_field::Kind::Password
                access=text_field::Access::Disabled
            />
            {[
                (
                    "gallery-text-field-small",
                    Size::Small,
                    msg::UI_GALLERY_TEXT_FIELD_SMALL,
                    msg::UI_GALLERY_TEXT_FIELD_ADD_SMALL,
                ),
                (
                    "gallery-text-field-medium",
                    Size::Medium,
                    msg::UI_GALLERY_TEXT_FIELD_MEDIUM,
                    msg::UI_GALLERY_TEXT_FIELD_ADD_MEDIUM,
                ),
                (
                    "gallery-text-field-large",
                    Size::Large,
                    msg::UI_GALLERY_TEXT_FIELD_LARGE,
                    msg::UI_GALLERY_TEXT_FIELD_ADD_LARGE,
                ),
                (
                    "gallery-text-field-xlarge",
                    Size::XLarge,
                    msg::UI_GALLERY_TEXT_FIELD_XLARGE,
                    msg::UI_GALLERY_TEXT_FIELD_ADD_XLARGE,
                ),
            ]
                .into_iter()
                .map(|(id, size, label, add)| {
                    view! {
                        <div class="actions">
                            <TextField
                                id=id
                                localizer=Arc::clone(&localizer)
                                value=RwSignal::new(String::new())
                                label=t(label)
                                size=size
                            />
                            <Button content=Content::Text(t(add).into()) size=size />
                        </div>
                    }
                })
                .collect_view()}
        </section>
        // After the text field's, so the walks from the page's start to
        // the Toast demo and the sections after it pass none of its bars.
        <section id="progress-demo">
            <DemoHeading text=t(msg::UI_GALLERY_PROGRESS_HEADING) />
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_DEMO_BACKUP_PROGRESS)
                reading=progress
            />
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_ADVANCE_PROGRESS).into())
                    on_press=Callback::new(move |()| {
                        progress
                            .update(|demo| demo.current = (demo.current + 4 * GIB).min(10 * GIB));
                    })
                />
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
            // Every size, then every tone, each 60% done. The Small one and
            // the Primary one name no size or tone, so they show the
            // defaults.
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_PROGRESS_SMALL)
                reading=Signal::stored(sixty)
            />
            {[
                (Size::Medium, msg::UI_GALLERY_PROGRESS_MEDIUM),
                (Size::Large, msg::UI_GALLERY_PROGRESS_LARGE),
                (Size::XLarge, msg::UI_GALLERY_PROGRESS_XLARGE),
            ]
                .into_iter()
                .map(|(size, name)| {
                    view! {
                        <Progress
                            localizer=Arc::clone(&localizer)
                            label=t(name)
                            reading=Signal::stored(sixty)
                            size=size
                        />
                    }
                })
                .collect_view()}
            <Progress
                localizer=Arc::clone(&localizer)
                label=t(msg::UI_GALLERY_PROGRESS_PRIMARY)
                reading=Signal::stored(sixty)
            />
            {[
                (Tone::Secondary, msg::UI_GALLERY_PROGRESS_SECONDARY),
                (Tone::Success, msg::UI_GALLERY_PROGRESS_SUCCESS),
                (Tone::Warning, msg::UI_GALLERY_PROGRESS_WARNING),
                (Tone::Error, msg::UI_GALLERY_PROGRESS_ERROR),
                (Tone::Inherit, msg::UI_GALLERY_PROGRESS_INHERIT),
            ]
                .into_iter()
                .map(|(tone, name)| {
                    view! {
                        <Progress
                            localizer=Arc::clone(&localizer)
                            label=t(name)
                            reading=Signal::stored(sixty)
                            tone=tone
                        />
                    }
                })
                .collect_view()}
        </section>
        // After the progress demo, past every demo a journey walks to from
        // the page's top, so its two stops lengthen none of those walks; its
        // scenarios start at its anchor.
        <section id="tabs-starting-on-a-marked-tab-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_MARKED_HEADING) />
            // A set started on its middle tab, which the gallery marks as
            // Settings marks the tab its address names: that tab is
            // selected and is the one Tab stop into the set, and the arrow
            // keys, Home and End move on from it.
            <Tabs
                id="gallery-tabs-marked"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_MARKED))
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_MARKED_SUMMARY),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_SUMMARY_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                                t(msg::UI_GALLERY_TABS_MARKED_FILES),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_FILES_TEXT)}</p> },
                            )
                            .selected_at_start(true),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_MARKED_TAGS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_TAGS_TEXT)}</p> },
                        ),
                    ],
                )
            />
        </section>
        // Before the tooltip's, so a journey reaches its controls by
        // Shift+Tab from the page's start, past the tooltip's few.
        <section id="table-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABLE_HEADING) />
            {demo_table(table_localizer)}
        </section>
        // Last on the page, so a journey reaches its controls by Shift+Tab
        // from the page's start, and its last button sits at the foot.
        <section id="tooltip-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TOOLTIP_HEADING) />
            // A control its tooltip describes, standing above it.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_PRUNE),
                    t(msg::UI_GALLERY_TOOLTIP_PRUNE_NOTE),
                    Side::Above,
                )}
            </p>
            // An icon-only control, named by the tooltip's one string: the
            // button's label, which the tooltip shows a sighted reader. The
            // button names itself with the same string, as an icon-only
            // Button must.
            <p>
                <Tooltip tip=Tip::Names(schedule.clone())>
                    <Button content=Content::IconOnly {
                        glyph: LucideGlyph::CalendarClock.into(),
                        label: schedule.into(),
                    } />
                </Tooltip>
            </p>
            // Each side, with room on it.
            <p class="flex flex-wrap items-center gap-4">
                {[
                    (
                        Side::Above,
                        msg::UI_GALLERY_TOOLTIP_ABOVE,
                        msg::UI_GALLERY_TOOLTIP_ABOVE_NOTE,
                    ),
                    (
                        Side::Below,
                        msg::UI_GALLERY_TOOLTIP_BELOW,
                        msg::UI_GALLERY_TOOLTIP_BELOW_NOTE,
                    ),
                    (
                        Side::Start,
                        msg::UI_GALLERY_TOOLTIP_START,
                        msg::UI_GALLERY_TOOLTIP_START_NOTE,
                    ),
                    (Side::End, msg::UI_GALLERY_TOOLTIP_END, msg::UI_GALLERY_TOOLTIP_END_NOTE),
                ]
                    .into_iter()
                    .map(|(side, name, note)| demo_tooltip(t(name), t(note), side))
                    .collect_view()}
            </p>
            // Start and End in a line set right to left: the start is the
            // right.
            <p dir="rtl" class="flex flex-wrap items-center gap-4">
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_START_RTL),
                    t(msg::UI_GALLERY_TOOLTIP_START_NOTE),
                    Side::Start,
                )}
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_END_RTL),
                    t(msg::UI_GALLERY_TOOLTIP_END_NOTE),
                    Side::End,
                )}
            </p>
            // A long note, which wraps.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_LONG),
                    t(msg::UI_GALLERY_TOOLTIP_LONG_NOTE),
                    Side::Above,
                )}
            </p>
            // Below, at the foot of the page, where there is no room: it
            // stands above.
            <p>
                {demo_tooltip(
                    t(msg::UI_GALLERY_TOOLTIP_FOOT),
                    t(msg::UI_GALLERY_TOOLTIP_FOOT_NOTE),
                    Side::Below,
                )}
            </p>
        </section>
    }
}

/// A demo's heading, first in its section. The section's `id` is the demo's
/// anchor: its heading in English, in lower case with hyphens for spaces,
/// then `-demo` (`step-up-field-demo`), so it finds the demo under a
/// pseudolocale too, which renames the heading. The address
/// `/_gallery#<id>` puts focus on the heading as the page loads
/// ([`page::focus_when_named`]), so a scenario can start at its own demo and
/// a demo added above it lengthens no walk (technical design §13.7.5). Only
/// a script focuses it (`tabindex="-1"`), so it is no stop for Tab.
#[component]
fn DemoHeading(text: String) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading.get()
            && let Some(section) = heading.parent_element()
        {
            page::focus_when_named(&section.id(), &heading);
        }
    });
    view! {
        <h2 node_ref=heading tabindex="-1">
            {text}
        </h2>
    }
}

/// rustic's documentation, the demo's other site.
const RUSTIC_DOCS: &str = "https://rustic.cli.rs/";

/// The ID of the alert demo's button that brings its dismissed notice back,
/// where focus goes when the notice is dismissed.
const ALERT_DEMO_SHOW: &str = "alert-demo-show";

/// The name the gallery's dismissible demo banner is remembered under in
/// the browser's local storage.
const BANNER_DEMO: &str = "gallery-demo";

/// The ID of the banner demo's button that brings a newer edition of its
/// notice, where focus goes when the notice is dismissed.
const BANNER_DEMO_NEWER: &str = "banner-demo-newer";

/// The ID of the toast demo's Save button, where focus goes when a toast is
/// dismissed.
const TOAST_DEMO_SAVE: &str = "toast-demo-save";

/// A demo link of its own, in its default tone.
fn demo_link(
    localizer: &Arc<Localizer>,
    to: Option<Destination>,
    words: String,
) -> impl IntoView + use<> {
    let localizer = Arc::clone(localizer);
    to.map(|to| {
        view! {
            <Link to=to localizer=localizer>
                {words}
            </Link>
        }
    })
}

/// The sentence `message` with a link inside it, its `words` the catalog's
/// `$link`, found between the marks the catalog puts around a placeable
/// (`Sentence::new`), so no sentence is built from pieces.
fn sentence_link(
    localizer: &Arc<Localizer>,
    to: Option<Destination>,
    message: MessageId,
    words: String,
    tone: LinkTone,
) -> impl IntoView + use<> {
    let text = text_with(localizer, message, &[("link", MsgArg::Text(words.clone()))]);
    let sentence = Sentence::new(&text, Some(&words));
    match (to, sentence.detail.clone()) {
        (Some(to), Some((words, after))) => {
            let localizer = Arc::clone(localizer);
            view! {
                {sentence.before}
                <Link to=to localizer=localizer tone=tone>
                    {words}
                </Link>
                {after}
            }
            .into_any()
        }
        _ => sentence.plain().into_any(),
    }
}

/// A demo button whose tooltip, `note`, describes it from `side`. Pressing
/// it changes nothing.
fn demo_tooltip(name: String, note: String, side: Side) -> impl IntoView {
    view! {
        <Tooltip tip=Tip::Describes(note) side=side>
            <Button content=Content::Text(name.into()) />
        </Tooltip>
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

/// A folder of the table demo: neutral demo data.
#[derive(Clone, Debug, PartialEq)]
struct DemoFolder {
    name: String,
    size: u64,
    changed_ms: i64,
    files: u64,
}

/// The table demo: a choice of what the table shows, then a table of demo
/// folders sortable by each column, with a total and a row action that
/// changes its row, then the same table in a narrow frame, which scrolls
/// with each row's header pinned at its start, and right to left.
fn demo_table(localizer: Arc<Localizer>) -> impl IntoView {
    let t = {
        let localizer = Arc::clone(&localizer);
        move |id| text(&localizer, id)
    };
    let folder = |name, size, changed_ms, files| DemoFolder {
        name: t(name),
        size,
        changed_ms,
        files,
    };
    let data = RwSignal::new(vec![
        folder(
            msg::UI_GALLERY_TABLE_PHOTOS,
            4_509_715_660,
            1_791_072_000_000,
            1204,
        ),
        folder(
            msg::UI_GALLERY_TABLE_MUSIC,
            13_529_146_982,
            1_787_270_400_000,
            12,
        ),
        folder(
            msg::UI_GALLERY_TABLE_DOCUMENTS,
            859_832_320,
            1_793_577_600_000,
            312,
        ),
        folder(
            msg::UI_GALLERY_TABLE_NOTES,
            1_572_864,
            1_794_441_600_000,
            12,
        ),
    ]);
    let chosen = RwSignal::new("rows");
    let state = Signal::derive(move || match chosen.get() {
        "loading" => State::Loading,
        "unreachable" => State::Unreachable,
        _ => State::Ready,
    });
    let rows = Signal::derive(move || {
        if chosen.get() == "empty" {
            Vec::new()
        } else {
            data.get()
        }
    });
    let sentences = || Sentences {
        loading: t(msg::UI_GALLERY_TABLE_LOADING),
        empty: t(msg::UI_GALLERY_TABLE_EMPTY),
        unreachable: t(msg::UI_GALLERY_TABLE_UNREACHABLE),
    };
    let total = |localizer: &Arc<Localizer>| {
        let (sizes, counts) = (Arc::clone(localizer), Arc::clone(localizer));
        Footer {
            header: Signal::stored(text(localizer, msg::UI_GALLERY_TABLE_TOTAL)),
            cells: vec![
                Signal::derive(move || {
                    let size = data.with(|rows| rows.iter().map(|row| row.size).sum());
                    text_with(
                        &sizes,
                        msg::UI_GALLERY_TABLE_SIZE,
                        &[("size", MsgArg::Bytes(size))],
                    )
                }),
                Signal::stored(String::new()),
                Signal::derive(move || {
                    let files = data.with(|rows| rows.iter().map(|row| row.files).sum());
                    text_with(
                        &counts,
                        msg::UI_GALLERY_TABLE_FILES,
                        &[("count", MsgArg::Count(files))],
                    )
                }),
            ],
        }
    };
    let (row_header, columns) = folder_columns(&localizer, Some(data));
    let (narrow_header, narrow_columns) = folder_columns(&localizer, None);
    let (rtl_header, rtl_columns) = folder_columns(&localizer, None);
    view! {
        // Before the table it changes, so what it changes follows it.
        <RadioGroup
            name="gallery-table-shows"
            legend=t(msg::UI_GALLERY_TABLE_SHOWS)
            options=vec![
                ("rows", t(msg::UI_GALLERY_TABLE_SHOWS_ROWS)),
                ("loading", t(msg::UI_GALLERY_TABLE_SHOWS_LOADING)),
                ("empty", t(msg::UI_GALLERY_TABLE_SHOWS_EMPTY)),
                ("unreachable", t(msg::UI_GALLERY_TABLE_SHOWS_UNREACHABLE)),
            ]
            chosen=chosen
        />
        <Table
            name=Name::Caption(t(msg::UI_GALLERY_TABLE_CAPTION))
            row_header=row_header
            columns=columns
            rows=rows
            key=|row: &DemoFolder| row.name.clone()
            state=state
            sentences=sentences()
            footer=total(&localizer)
        />
        <div class="gallery-narrow">
            <Table
                name=Name::Caption(t(msg::UI_GALLERY_TABLE_NARROW_CAPTION))
                row_header=narrow_header
                columns=narrow_columns
                rows=data
                key=|row: &DemoFolder| row.name.clone()
                state=State::Ready
                sentences=sentences()
            />
        </div>
        <div class="gallery-narrow" dir="rtl">
            <Table
                name=Name::Caption(t(msg::UI_GALLERY_TABLE_RTL_CAPTION))
                row_header=rtl_header
                columns=rtl_columns
                rows=data
                key=|row: &DemoFolder| row.name.clone()
                state=State::Ready
                sentences=sentences()
            />
        </div>
    }
}

/// The demo folders' columns: the name, the row's header, its size and the
/// number of its files, numbers at the end, and when it changed. Given the
/// folders' `data`, each is sortable and a last column holds the row's
/// action, which adds a file to its folder. Each value is isolated
/// (`<bdi>`): the demo's words are English in every frame, and in the
/// right-to-left one an English size would otherwise read "GiB 4.2" (a page
/// in a right-to-left language gets Fluent's isolation marks instead).
fn folder_columns(
    localizer: &Arc<Localizer>,
    data: Option<RwSignal<Vec<DemoFolder>>>,
) -> (Column<DemoFolder>, Vec<Column<DemoFolder>>) {
    let t = |id| text(localizer, id);
    let sortable = data.is_some();
    let sort = |column: Column<DemoFolder>,
                label,
                ascending,
                descending,
                key: fn(&DemoFolder) -> SortKey| {
        if sortable {
            column.sorted_by(Sort::new(t(label), t(ascending), t(descending), key))
        } else {
            column
        }
    };
    let name = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_NAME),
            Rem::tenths(80),
            |row: Signal<DemoFolder>| (move || row.with(|row| row.name.clone())).into_any(),
        ),
        msg::UI_GALLERY_TABLE_SORT_NAME,
        msg::UI_GALLERY_TABLE_SORTED_NAME_UP,
        msg::UI_GALLERY_TABLE_SORTED_NAME_DOWN,
        |row: &DemoFolder| SortKey::Text(row.name.to_lowercase()),
    );
    let sizes = Arc::clone(localizer);
    let size = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_SIZE_COLUMN),
            Rem::tenths(70),
            move |row: Signal<DemoFolder>| {
                let sizes = Arc::clone(&sizes);
                view! {
                    <bdi>
                        {move || {
                            let size = row.with(|row| row.size);
                            text_with(
                                &sizes,
                                msg::UI_GALLERY_TABLE_SIZE,
                                &[("size", MsgArg::Bytes(size))],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        )
        .align(Align::End),
        msg::UI_GALLERY_TABLE_SORT_SIZE,
        msg::UI_GALLERY_TABLE_SORTED_SIZE_UP,
        msg::UI_GALLERY_TABLE_SORTED_SIZE_DOWN,
        |row: &DemoFolder| SortKey::Number(i64::try_from(row.size).unwrap_or(i64::MAX)),
    );
    let dates = Arc::clone(localizer);
    let changed = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_CHANGED),
            Rem::tenths(90),
            move |row: Signal<DemoFolder>| {
                let dates = Arc::clone(&dates);
                view! {
                    <bdi>
                        {move || {
                            let utc_ms = row.with(|row| row.changed_ms);
                            text_with(
                                &dates,
                                msg::UI_GALLERY_TABLE_CHANGED_ON,
                                &[
                                    (
                                        "when",
                                        MsgArg::Timestamp {
                                            utc_ms,
                                            zone: None,
                                        },
                                    ),
                                ],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        ),
        msg::UI_GALLERY_TABLE_SORT_CHANGED,
        msg::UI_GALLERY_TABLE_SORTED_CHANGED_UP,
        msg::UI_GALLERY_TABLE_SORTED_CHANGED_DOWN,
        |row: &DemoFolder| SortKey::Number(row.changed_ms),
    );
    let counts = Arc::clone(localizer);
    let files = sort(
        Column::new(
            t(msg::UI_GALLERY_TABLE_FILES_COLUMN),
            // Wide enough for "Files" and its sort button on one line at
            // 1280 px; at 6rem the button wrapped under the word.
            Rem::tenths(80),
            move |row: Signal<DemoFolder>| {
                let counts = Arc::clone(&counts);
                view! {
                    <bdi>
                        {move || {
                            let files = row.with(|row| row.files);
                            text_with(
                                &counts,
                                msg::UI_GALLERY_TABLE_FILES,
                                &[("count", MsgArg::Count(files))],
                            )
                        }}
                    </bdi>
                }
                .into_any()
            },
        )
        .align(Align::End),
        msg::UI_GALLERY_TABLE_SORT_FILES,
        msg::UI_GALLERY_TABLE_SORTED_FILES_UP,
        msg::UI_GALLERY_TABLE_SORTED_FILES_DOWN,
        |row: &DemoFolder| SortKey::Number(i64::try_from(row.files).unwrap_or(i64::MAX)),
    );
    let mut columns = vec![size, changed, files];
    if let Some(data) = data {
        // The row's action, named with its row, adds a file to the folder,
        // which changes the row and, sorted by files, moves it.
        let labels = Arc::clone(localizer);
        columns.push(Column::new(
            t(msg::UI_GALLERY_TABLE_ACTIONS),
            Rem::tenths(90),
            move |row: Signal<DemoFolder>| {
                let name = row.with_untracked(|row| row.name.clone());
                let label = text_with(
                    &labels,
                    msg::UI_GALLERY_TABLE_ADD_FILE,
                    &[("folder", MsgArg::Text(name.clone()))],
                );
                let press = Callback::new(move |()| {
                    data.update(|rows| {
                        if let Some(row) = rows.iter_mut().find(|row| row.name == name) {
                            row.files = row.files.saturating_add(1);
                        }
                    });
                });
                view! { <Button content=Content::Text(label.into()) on_press=press /> }.into_any()
            },
        ));
    }
    (name, columns)
}
