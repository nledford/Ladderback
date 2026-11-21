//! The gallery's text field demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::ACTIONS;
use crate::components::button::Content;
use crate::components::{Button, TextField, text_field};
use crate::live::reply;
use crate::text;
use crate::variants::Size;

/// The text field's demo (`#text-field-demo`).
#[component]
pub(super) fn TextFieldDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The one-line field's demo: its error, what is typed and the error's words.
    let form_error = RwSignal::<Option<String>>::new(None);
    let form_value = RwSignal::new(String::new());
    let required_error = t(msg::UI_GALLERY_REPOSITORY_REQUIRED);
    // The multi-line field's demo: its entries and its error, which its own
    // button shows and clears as the one-line field's does.
    let lines_error = RwSignal::<Option<String>>::new(None);
    let lines_value = RwSignal::new(String::new());
    let folders_error = t(msg::UI_GALLERY_FOLDERS_REQUIRED);
    view! {
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
                        <div class=ACTIONS>
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
    }
}
