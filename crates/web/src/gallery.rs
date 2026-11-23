//! The component gallery (technical design §4.5, "Tests"): the dev-only route
//! `/_gallery`, where Playwright drives the shared components with real input.
//! Built only with the `gallery` feature. Each demo is a module of its own,
//! named after the component it shows (`tooltip` for the tooltip's demo).

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;

use crate::components::Link;
use crate::components::link::rules::{Destination, LinkPlacement, LinkTone, Matching};
use crate::page;
use crate::repositories::Sentence;
use crate::routes::{PageHeading, text_with};
use crate::text;

mod alert;
mod announcer;
mod badge;
mod banner;
mod button;
mod checkbox;
mod fingerprint;
mod footer;
mod icon;
mod link;
mod modal_dialog;
mod pause_updates;
mod progress;
mod radio_group;
mod review_dialog;
mod skeleton;
mod spinner;
mod status_badge;
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
    let heading = t(msg::UI_GALLERY_HEADING);
    let (start_link, missing_link) = (
        t(msg::UI_GALLERY_START_LINK),
        t(msg::UI_GALLERY_MISSING_LINK),
    );
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{demo_link(&localizer, Destination::route("/", Matching::Exact), start_link)}</p>
        <p>
            {demo_link(&localizer, Destination::route("/_missing", Matching::Exact), missing_link)}
        </p>
        <announcer::AnnouncerDemo localizer=Arc::clone(&localizer) />
        <modal_dialog::ModalDialogDemo localizer=Arc::clone(&localizer) />
        <checkbox::CheckboxDemo localizer=Arc::clone(&localizer) />
        <radio_group::RadioGroupDemo localizer=Arc::clone(&localizer) />
        <status_badge::StatusBadgeDemo localizer=Arc::clone(&localizer) />
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
        // After the steps' demo, past every demo a journey walks to from the
        // page's top, and before the table's, which journeys reach by
        // Shift+Tab from the page's start, so its stops lengthen none of
        // those walks; its scenarios start at its anchor.
        <table::LiveTableDemo localizer=Arc::clone(&localizer) />
        // After the live table's demo: it has no Tab stop, so it lengthens no walk.
        <fingerprint::FingerprintDemo localizer=Arc::clone(&localizer) />
        // After the fingerprint's demo: no part of a footer is a Tab stop,
        // so it lengthens no walk either.
        <footer::FooterDemo localizer=Arc::clone(&localizer) />
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
/// a script focuses it (`tabindex="-1"`), so it is no stop for Tab. An `id`,
/// where given, lets a control in the demo hand focus to it.
#[component]
fn DemoHeading(text: String, #[prop(optional)] id: Option<&'static str>) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading.get()
            && let Some(section) = heading.parent_element()
        {
            page::focus_when_named(&section.id(), &heading);
        }
    });
    view! {
        <h2 node_ref=heading id=id tabindex="-1">
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
            <Link to=to localizer=localizer placement=LinkPlacement::InText>
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
                <Link to=to localizer=localizer tone=tone placement=LinkPlacement::InText>
                    {words}
                </Link>
                {after}
            }
            .into_any()
        }
        _ => sentence.plain().into_any(),
    }
}
