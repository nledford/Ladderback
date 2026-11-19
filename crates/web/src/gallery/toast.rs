//! The gallery's toast demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::alert::rules::AlertTone;
use crate::components::button::Content;
use crate::components::{Alert, Button, Toast, ToastTone, Toasts};
use crate::text;

/// The toast's demo (`#toast-demo`).
#[component]
pub(super) fn ToastDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
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
    view! {
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
    }
}

/// The ID of the toast demo's Save button, where focus goes when a toast is
/// dismissed.
const TOAST_DEMO_SAVE: &str = "toast-demo-save";
