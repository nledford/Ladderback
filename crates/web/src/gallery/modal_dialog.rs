//! The gallery's modal dialog demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::either::Either;
use leptos::prelude::*;
use leptos::task::{spawn_local, tick};

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::button::{Appearance, Content};
use crate::components::{Button, ModalDialog, TitleLevel};
use crate::live::say;
use crate::text;
use crate::variants::Tone;

/// The modal dialog's demo (`#dialog-demo`).
#[component]
pub(super) fn ModalDialogDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    let dialog_body = t(msg::UI_GALLERY_DEMO_DIALOG_BODY);
    let dialog_polite_text = t(msg::UI_GALLERY_BACKUP_FINISHED);
    let dialog_polite_label = t(msg::UI_GALLERY_POLITE_BUTTON);
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
    }
}
