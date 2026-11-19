//! The DOM glue of the Full Disk Access warning (§4.4, §16 decision 115): a
//! warning banner on every page, which reads what the agent's probe found on
//! each page the reader goes to and links to the steps, and the start page's
//! steps, which send Check again and the request to open System Settings,
//! answer a check that finds the grant still missing in a toast in the steps,
//! and hand focus to the page's heading when the warning goes from under them.
//! Whether it warns, whether it offers the settings button, its steps and
//! what each answer says are `full_disk_access.rs`'s, on the host; the `@ui`
//! scenarios of `features/ui/full_disk_access.feature` run this in the
//! browsers.

use std::sync::Arc;

use api::FullDiskAccessDto;
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_location;

use crate::announcer::Politeness;
use crate::components::banner::rules::BannerTone;
use crate::components::button::{Appearance, Button, Content};
use crate::components::link::rules::{Destination, Matching};
use crate::components::{
    Banner, Link, Toast, ToastTone, Toasts, hand_focus_when_gone, sentence_view,
};
use crate::full_disk_access::{BANNER_ID, STEPS, opens_settings, said_after_check, warns};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::{PAGE_HEADING_ID, refused_sentence};
use crate::session::Standing;
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Posted};

/// What the agent's probe found last, as the page last read it: the banner
/// and the start page's steps show the same answer, so Check again on the
/// steps takes the banner away too. `None` until a read is answered, and
/// after one refused or unanswered.
#[derive(Clone, Copy)]
pub(crate) struct FullDiskAccessFound(pub(crate) RwSignal<Option<FullDiskAccessDto>>);

/// The warning's heading, which names its region.
const HEADING_ID: &str = "full-disk-access-heading";

/// The Check again button, which a dismissed toast hands focus back to.
const CHECK_ID: &str = "full-disk-access-check";

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

/// The warning banner every page shows while the agent found the grant
/// missing: what that costs, in one sentence, and a link to the start page,
/// where the steps are. It has no Dismiss: hiding it would bring back the
/// warning a reader misses (#948). It reads the probe's answer again on each
/// page the reader goes to, once the pages show (`shown`), as the start page
/// did before the banner, so a backup's probe is seen without a reload.
#[component]
pub(crate) fn FullDiskAccessBanner(
    localizer: Arc<Localizer>,
    shown: Memo<Standing>,
) -> impl IntoView {
    let found = use_context::<FullDiskAccessFound>().map(|FullDiskAccessFound(found)| found);
    let pathname = use_location().pathname;
    Effect::new(move |_| {
        pathname.track();
        let Some(found) = found else {
            return;
        };
        if !shown.get().shows_pages() {
            return;
        }
        let url = format!("{}{READ_PATH}", ws::origin());
        spawn_local(async move {
            // A refusal shows nothing: the page warns only of a grant the
            // agent found missing, and a page refused for want of a session
            // knows nothing. No answer, as while the daemon restarts, keeps
            // what the page last read.
            match ws::get::<FullDiskAccessDto>(&url).await {
                Some(Got::Read(read)) => {
                    found.try_set(Some(read));
                }
                Some(Got::Refused(..)) => {
                    found.try_set(None);
                }
                None => {}
            }
        });
    });
    let sentence = text(&localizer, msg::UI_FULL_DISK_ACCESS_BANNER);
    let link = text(&localizer, msg::UI_FULL_DISK_ACCESS_BANNER_LINK);
    move || {
        let missing = found.is_some_and(|found| warns(found.get().as_ref()));
        (missing && shown.get().shows_pages()).then(|| {
            let (sentence, link) = (sentence.clone(), link.clone());
            let to = Destination::route("/", Matching::Exact);
            let link_localizer = Arc::clone(&localizer);
            view! {
                // Its `id` lets each page's heading name it in
                // `aria-describedby` (`PageHeading`).
                <Banner localizer=Arc::clone(&localizer) id=BANNER_ID tone=BannerTone::Warning>
                    {sentence}
                    " "
                    {to
                        .map(|to| {
                            view! {
                                <Link to=to localizer=link_localizer>
                                    {link}
                                </Link>
                            }
                        })}
                </Banner>
            }
        })
    }
}

/// The start page's steps to give the agent Full Disk Access, under the
/// banner while the grant is missing: why it matters, the steps, the button
/// that opens System Settings for a browser on the agent's own Mac, and
/// Check again. Nothing shows while the grant isn't found missing; the
/// banner reads what the probe found.
#[component]
pub(crate) fn FullDiskAccessSteps(localizer: Arc<Localizer>) -> impl IntoView {
    let Some(FullDiskAccessFound(found)) = use_context::<FullDiskAccessFound>() else {
        return ().into_any();
    };
    // The request a button sent, while the daemon answers it: that button
    // is busy, the other unavailable, so neither takes a press.
    let pending = RwSignal::<Option<Request>>::new(None);
    // A button's answer that isn't a new reading: a refusal, or none at all.
    let problem = RwSignal::<Option<Sentence>>::new(None);
    // The answer to a check that leaves the warning as it was.
    let toasts = Toasts::new();
    // The read is the banner's, on every page. Apart, so a check that finds
    // the grant still missing doesn't draw the warning again under the
    // reader's focus.
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
                            let said = text(&localizer, said_after_check(read.state));
                            // A grant still missing leaves the warning as it
                            // was, so a toast in it answers the press; one
                            // found or unknown takes the warning away, which
                            // shows the answer, and the announcer says it.
                            if warns(Some(&read)) {
                                toasts.show(ToastTone::Primary, said);
                            } else {
                                reply(Politeness::Polite, &said);
                            }
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
    (move || {
        shown.get().then(|| {
            let check_button = NodeRef::<leptos::html::Button>::new();
            // Check again is what makes the warning go: focus then moves to
            // the page's heading, never to the page itself (WCAG 2.4.3).
            hand_focus_when_gone(check_button, PAGE_HEADING_ID.to_owned());
            let (open_label, check_label) = (open_label.clone(), check_label.clone());
            let (heading, body, steps) = (heading.clone(), body.clone(), steps.clone());
            let toast_localizer = Arc::clone(&localizer);
            view! {
                // The steps' region, named by its heading, under the banner
                // that warns of the same grant.
                <section aria-labelledby=HEADING_ID>
                    <h2 id=HEADING_ID class="my-1 text-[1.125rem]">
                        {heading}
                    </h2>
                    <p>{body}</p>
                    <ol>
                        {steps.iter().map(|step| view! { <li>{step.clone()}</li> }).collect_view()}
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
                            id=CHECK_ID
                            node_ref=check_button
                            busy=Signal::derive(move || pending.get() == Some(Request::Check))
                            disabled=Signal::derive(move || {
                                pending.get() == Some(Request::Open)
                            })
                            on_press=check
                        />
                    </div>
                    <Toast localizer=toast_localizer toasts=toasts then_focus=CHECK_ID />
                    <p role="alert">{move || problem.get().map(sentence_view)}</p>
                </section>
            }
        })
    })
    .into_any()
}

/// A button's request the daemon didn't answer, or answered with what isn't
/// a reading.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(&text(localizer, msg::UI_FULL_DISK_ACCESS_UNREACHABLE), None)
}
