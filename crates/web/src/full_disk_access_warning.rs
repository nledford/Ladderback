//! The DOM glue of the start page's Full Disk Access warning (§4.4, §16
//! decision 115), a warning banner: it reads what the agent's probe found,
//! draws the warning, sends Check again and the request to open System
//! Settings, and hands focus to the page's heading when the warning goes
//! from under it. Whether it warns, whether it offers the settings button,
//! its steps and what each answer says are `full_disk_access.rs`'s, on the
//! host; the `@ui` scenarios of `features/ui/full_disk_access.feature` run
//! this in Chromium.

use std::sync::Arc;

use api::FullDiskAccessDto;
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Appearance, Button, Content};
use crate::components::{Banner, hand_focus_when_gone, sentence_view};
use crate::full_disk_access::{STEPS, opens_settings, said_after_check, warns};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::{PAGE_HEADING_ID, refused_sentence};
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Posted};

/// The warning's heading, which names its region.
const HEADING_ID: &str = "full-disk-access-heading";

/// The routes the warning reads and posts to (§4.2).
const READ_PATH: &str = "/api/v1/full-disk-access";
const CHECK_PATH: &str = "/api/v1/full-disk-access/check";
const SETTINGS_PATH: &str = "/api/v1/full-disk-access/settings";

/// The warning's two requests, one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    Check,
    Open,
}

/// The start page's warning that macOS hasn't given the agent Full Disk
/// Access: why it matters, the steps, the button that opens System Settings
/// for a browser on the agent's own Mac, and Check again. Nothing shows
/// while the grant isn't found missing.
#[component]
pub(crate) fn FullDiskAccessWarning(localizer: Arc<Localizer>) -> impl IntoView {
    let found = RwSignal::<Option<FullDiskAccessDto>>::new(None);
    // The request a button sent, while the daemon answers it: that button
    // is busy, the other unavailable, so neither takes a press.
    let pending = RwSignal::<Option<Request>>::new(None);
    // A button's answer that isn't a new reading: a refusal, or none at all.
    let problem = RwSignal::<Option<Sentence>>::new(None);
    let url = format!("{}{READ_PATH}", ws::origin());
    spawn_local(async move {
        // A refusal or no answer shows nothing: the page warns only of a
        // grant the agent found missing.
        if let Some(Got::Read(read)) = ws::get::<FullDiskAccessDto>(&url).await {
            found.try_set(Some(read));
        }
    });
    // Apart, so a check that finds the grant still missing doesn't draw the
    // warning again under the reader's focus.
    let shown = Memo::new(move |_| warns(found.get().as_ref()));
    let offers_settings = Memo::new(move |_| opens_settings(found.get().as_ref()));

    let check_localizer = Arc::clone(&localizer);
    let check = Callback::new(move |()| {
        pending.set(Some(Request::Check));
        problem.set(None);
        let localizer = Arc::clone(&check_localizer);
        spawn_local(async move {
            let url = format!("{}{CHECK_PATH}", ws::origin());
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            pending.try_set(None);
            match posted {
                Some(Posted::Done(answer)) => {
                    match serde_json::from_str::<FullDiskAccessDto>(&answer) {
                        Ok(read) => {
                            reply(
                                Politeness::Polite,
                                &text(&localizer, said_after_check(read.state)),
                            );
                            found.try_set(Some(read));
                        }
                        Err(_) => {
                            problem.try_set(Some(unanswered(&localizer)));
                        }
                    }
                }
                Some(Posted::Refused(code, args, detail)) => {
                    problem.try_set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                }
                None => {
                    problem.try_set(Some(unanswered(&localizer)));
                }
            }
        });
    });

    let open_localizer = Arc::clone(&localizer);
    let open = Callback::new(move |()| {
        pending.set(Some(Request::Open));
        problem.set(None);
        let localizer = Arc::clone(&open_localizer);
        spawn_local(async move {
            let url = format!("{}{SETTINGS_PATH}", ws::origin());
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            pending.try_set(None);
            match posted {
                Some(Posted::Done(_)) => reply(
                    Politeness::Polite,
                    &text(&localizer, msg::UI_FULL_DISK_ACCESS_OPENED),
                ),
                Some(Posted::Refused(code, args, detail)) => {
                    problem.try_set(Some(refused_sentence(
                        &localizer,
                        code,
                        &args,
                        detail.as_deref(),
                    )));
                }
                None => {
                    problem.try_set(Some(unanswered(&localizer)));
                }
            }
        });
    });

    let heading = text(&localizer, msg::UI_FULL_DISK_ACCESS_HEADING);
    let body = text(&localizer, msg::UI_FULL_DISK_ACCESS_BODY);
    let steps = STEPS.map(|step| text(&localizer, step));
    let open_label = text(&localizer, msg::UI_FULL_DISK_ACCESS_OPEN);
    let check_label = text(&localizer, msg::UI_FULL_DISK_ACCESS_CHECK);
    move || {
        shown.get().then(|| {
            let check_button = NodeRef::<leptos::html::Button>::new();
            // Check again is what makes the warning go: focus then moves to
            // the page's heading, never to the page itself (WCAG 2.4.3).
            hand_focus_when_gone(check_button, PAGE_HEADING_ID.to_owned());
            let (open_label, check_label) = (open_label.clone(), check_label.clone());
            let (heading, body, steps) = (heading.clone(), body.clone(), steps.clone());
            view! {
                // A warning banner, its words the region its heading names,
                // which a screen reader lists by that name as before.
                <Banner localizer=Arc::clone(&localizer) tone=BannerTone::Warning>
                    <section aria-labelledby=HEADING_ID>
                        <h2 id=HEADING_ID class="my-1 text-[1.125rem]">
                            {heading}
                        </h2>
                        <p>{body}</p>
                        <ol>
                            {steps
                                .iter()
                                .map(|step| view! { <li>{step.clone()}</li> })
                                .collect_view()}
                        </ol>
                        <div class="actions">
                            {move || {
                                offers_settings
                                    .get()
                                    .then(|| {
                                        view! {
                                            <Button
                                                content=Content::Text(open_label.clone().into())
                                                appearance=Appearance::Filled
                                                tone=Tone::Primary
                                                busy=Signal::derive(move || {
                                                    pending.get() == Some(Request::Open)
                                                })
                                                disabled=Signal::derive(move || {
                                                    pending.get() == Some(Request::Check)
                                                })
                                                on_press=open
                                            />
                                        }
                                    })
                            }}
                            <Button
                                content=Content::Text(check_label.clone().into())
                                node_ref=check_button
                                busy=Signal::derive(move || pending.get() == Some(Request::Check))
                                disabled=Signal::derive(move || {
                                    pending.get() == Some(Request::Open)
                                })
                                on_press=check
                            />
                        </div>
                        <p role="alert">{move || problem.get().map(sentence_view)}</p>
                    </section>
                </Banner>
            }
        })
    }
}

/// A button's request the daemon didn't answer, or answered with what isn't
/// a reading.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_FULL_DISK_ACCESS_UNREACHABLE), None)
}
