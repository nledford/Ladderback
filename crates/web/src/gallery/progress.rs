//! The gallery's progress bar demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::backend::Progress as Reading;
use crate::components::button::Content;
use crate::components::{Button, Progress};
use crate::text;
use crate::variants::{Size, Tone};

/// The progress bar's demo (`#progress-demo`).
#[component]
pub(super) fn ProgressDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The demo's state: the progress bar's percent.
    const GIB: u64 = 1 << 30;
    let progress = RwSignal::new(Reading {
        step: Some(api::OperationStep::BackingUp),
        unit: api::ProgressUnit::Bytes,
        current: 4 * GIB,
        total: Some(10 * GIB),
        reported_at: None,
    });
    // The reading of the progress demo's bars at each size and tone.
    let sixty = Reading {
        current: 6 * GIB,
        ..progress.get_untracked()
    };
    view! {
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
    }
}
