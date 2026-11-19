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
    PauseUpdates, Progress, RadioGroup, Reached, ReviewDialog, Sentences, Skeleton, Sort, SortKey,
    Spinner, State, StatusBadge, StepUp, StepUpField, Steps, StepsName, Tab, Table, Tabs,
    TextField, Tip, TitleLevel, Toast, ToastTone, Toasts, Tooltip, Tree,
};
use crate::live::{close_modal_when_gone, modal_gone, open_modal, reply, say};
use crate::page;
use crate::pause::{self, Ends};
use crate::repositories::Sentence;
use crate::routes::{PageHeading, text_with};
use crate::text;
use crate::variants::{Placement, Rem, Side, Size, StatusTone, Tone};

mod alert;
mod badge;
mod banner;
mod button;
mod icon;
mod link;
mod pause_updates;
mod progress;
mod review_dialog;
mod skeleton;
mod spinner;
mod step_up_field;
mod steps;
mod table;
mod tabs;
mod text_field;
mod toast;
mod tooltip;
mod tree;
mod vocabulary;

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
        <badge::BadgeDemo localizer=Arc::clone(&localizer) />
        <vocabulary::VocabularyDemo localizer=Arc::clone(&localizer) />
        <toast::ToastDemo localizer=Arc::clone(&localizer) />
        <review_dialog::ReviewDialogDemo localizer=Arc::clone(&localizer) />
        <tree::TreeDemo localizer=Arc::clone(&localizer) />
        <step_up_field::StepUpFieldDemo localizer=Arc::clone(&localizer) />
        <pause_updates::PauseUpdatesDemo localizer=Arc::clone(&localizer) />
        <spinner::SpinnerDemo localizer=Arc::clone(&localizer) />
        <icon::IconDemo localizer=Arc::clone(&localizer) />
        <button::ButtonDemo localizer=Arc::clone(&localizer) />
        <skeleton::SkeletonDemo localizer=Arc::clone(&localizer) />
        <banner::BannerDemo localizer=Arc::clone(&localizer) />
        <link::LinkDemo localizer=Arc::clone(&localizer) />
        <alert::AlertDemo localizer=Arc::clone(&localizer) />
        <tabs::TabsDemo localizer=Arc::clone(&localizer) />
        <tabs::VerticalTabsDemo localizer=Arc::clone(&localizer) />
        <tabs::ResponsiveTabsDemo localizer=Arc::clone(&localizer) />
        <text_field::TextFieldDemo localizer=Arc::clone(&localizer) />
        // After the text field's, so the walks from the page's start to
        // the Toast demo and the sections after it pass none of its bars.
        <progress::ProgressDemo localizer=Arc::clone(&localizer) />
        // After the progress demo, past every demo a journey walks to from
        // the page's top, so its two stops lengthen none of those walks; its
        // scenarios start at its anchor.
        <tabs::MarkedTabsDemo localizer=Arc::clone(&localizer) />
        // After the marked tabs' demo, past every demo a journey walks to from
        // the page's top, and before the table's, which journeys reach by
        // Shift+Tab from the page's end, so its one stop lengthens none of
        // those walks; its scenarios start at its anchor.
        <steps::StepsDemo localizer=Arc::clone(&localizer) />
        // Before the tooltip's, so a journey reaches its controls by
        // Shift+Tab from the page's start, past the tooltip's few.
        <table::TableDemo localizer=Arc::clone(&localizer) />
        // Last on the page, so a journey reaches its controls by Shift+Tab
        // from the page's start, and its last button sits at the foot.
        <tooltip::TooltipDemo localizer=Arc::clone(&localizer) />
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
