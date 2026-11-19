//! The gallery's alert demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::{DemoHeading, sentence_link};
use crate::components::alert::rules::AlertTone;
use crate::components::button::Content;
use crate::components::link::rules::{Destination, LinkTone, Matching};
use crate::components::{Alert, Button, Dismissal};
use crate::text;

/// The alert's demo (`#alert-demo`).
#[component]
pub(super) fn AlertDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The alert demo: its words in each tone, the warning's with a link
    // inside it in the alert's text color (Inherit), and whether its
    // dismissible notice shows, which its Dismiss button clears and the
    // button after it sets again.
    let (alert_information, alert_success, alert_error) = (
        t(msg::UI_GALLERY_ALERT_INFORMATION),
        t(msg::UI_GALLERY_ALERT_SUCCESS),
        t(msg::UI_GALLERY_ALERT_ERROR),
    );
    let alert_warning = sentence_link(
        &localizer,
        Destination::route("/operations", Matching::Section),
        msg::UI_GALLERY_ALERT_WARNING,
        t(msg::UI_GALLERY_ALERT_WARNING_LINK),
        LinkTone::Inherit,
    );
    let notice_shown = RwSignal::new(true);
    let (notice, notice_show) = (
        t(msg::UI_GALLERY_ALERT_DISMISSIBLE),
        t(msg::UI_GALLERY_ALERT_SHOW),
    );
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <section id="alert-demo">
            <DemoHeading text=t(msg::UI_GALLERY_ALERT_HEADING) />
            // Each tone, information first, which is the default.
            <Alert localizer=Arc::clone(&localizer)>{alert_information}</Alert>
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Success>
                {alert_success}
            </Alert>
            // A link inside, in the alert's text color.
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Warning>
                {alert_warning}
            </Alert>
            // A long path, which wraps inside the alert on a small screen.
            <Alert localizer=Arc::clone(&localizer) tone=AlertTone::Error>
                {alert_error}
            </Alert>
            // A notice the reader dismisses: the page takes it away, and
            // focus goes to the button that brings it back.
            {move || {
                notice_shown
                    .get()
                    .then(|| {
                        let notice = notice.clone();
                        view! {
                            <Alert
                                localizer=Arc::clone(&alert_localizer)
                                dismissal=Dismissal {
                                    on_dismiss: Callback::new(move |()| notice_shown.set(false)),
                                    then_focus: ALERT_DEMO_SHOW.to_owned(),
                                }
                            >
                                {notice}
                            </Alert>
                        }
                    })
            }}
            <p>
                <Button
                    id=ALERT_DEMO_SHOW
                    content=Content::Text(notice_show.into())
                    disabled=notice_shown
                    on_press=Callback::new(move |()| notice_shown.set(true))
                />
            </p>
        </section>
    }
}

/// The ID of the alert demo's button that brings its dismissed notice back,
/// where focus goes when the notice is dismissed.
const ALERT_DEMO_SHOW: &str = "alert-demo-show";
