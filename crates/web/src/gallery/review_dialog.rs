//! The gallery's review dialog demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::announcer::Politeness;
use crate::components::button::Content;
use crate::components::{Button, Confirmation, ReviewDialog};
use crate::live::reply;
use crate::text;
use crate::variants::Tone;

/// The review dialog's demo (`#review-dialog-demo`).
#[component]
pub(super) fn ReviewDialogDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
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
    view! {
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
    }
}
