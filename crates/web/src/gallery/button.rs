//! The gallery's button demo (technical design §4.5, "Tests").

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, MessageId, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::Button;
use crate::components::button::{Appearance, Content, Kind};
use crate::live::reply;
use crate::routes::text_with;
use crate::text;
use crate::variants::{Size, Tone};

/// The button's demo (`#button-demo`).
#[component]
pub(super) fn ButtonDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
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
    }
}
