//! The gallery's step-up field demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::{Button, Confirmation, ReviewDialog, StepUp, StepUpField, show_answer};
use crate::text;
use crate::variants::Tone;

/// The step-up field's demo (`#step-up-field-demo`).
#[component]
pub(super) fn StepUpFieldDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The step-up demos, one per thing the field can ask for: whether each
    // dialog is open, what is typed and the error its confirm button shows,
    // whatever was typed, through `show_answer` as a page's answer is, so a
    // second wrong try is heard again.
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
    view! {
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
                    action: Callback::new(move |()| {
                        spawn_local(show_answer(step_up_error, Some(step_up_wrong.clone())));
                    }),
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
                        spawn_local(show_answer(step_up_id_error, Some(step_up_id_wrong.clone())));
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
    }
}
