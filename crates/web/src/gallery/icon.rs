//! The gallery's icon demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::icon::Meaning;
use crate::components::{Button, Icon};
use crate::text;
use crate::variants::{Size, Tone};

/// The icon's demo (`#icon-demo`).
#[component]
pub(super) fn IconDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // Whether the icon demo's notifications are on, which its icon-only
    // toggle button shows and changes.
    let notifications = RwSignal::new(false);
    view! {
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
    }
}
