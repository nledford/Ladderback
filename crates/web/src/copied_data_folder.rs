//! The DOM glue of the start page's notice of a copied data folder (§4.4,
//! §7.1, §16 decision 144): it reads where the machine stands, and, while
//! its data folder came from another computer, says that backups run and
//! maintenance is stopped, and offers "This is a new computer: take a new
//! ID", which a review dialog confirms. Once the new ID is taken, it says to
//! restart Ladderback and links to the restart. The `@ui` scenarios of
//! `features/ui/copied_data_folder.feature` run it in the browsers.

use std::sync::Arc;

use api::{MachineDto, MachineStandingDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::announcer::Politeness;
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, Matching};
use crate::components::{
    ACTIONS, Button, Confirmation, Link, ReviewDialog, Working, dialog_refusal_alert, focus_element,
};
use crate::live::reply;
use crate::repositories::Sentence;
use crate::routes::refused_sentence;
use crate::text;
use crate::variants::Tone;
use crate::ws::{self, Got, Posted};

/// The notice's heading, which names its region, and where focus goes once
/// the new ID is taken and the button that asked for it is gone.
const HEADING_ID: &str = "copied-data-folder-heading";

/// The button that asks for a new machine ID, which the review returns
/// focus to.
const NEW_ID_BUTTON_ID: &str = "copied-data-folder-new-id";

/// What the review says it is about.
const REVIEW_BODY_ID: &str = "copied-data-folder-review-body";

/// The routes the notice reads and posts to (§4.2).
const READ_PATH: &str = "/api/v1/machine";
const NEW_ID_PATH: &str = "/api/v1/machine/new-id";

/// Where the restart is offered (decision 94).
const RESTART_HREF: &str = "/settings#restart";

/// The start page's notice of a copied data folder: nothing while the
/// machine stands as usual, or before the read is answered.
#[component]
pub(crate) fn CopiedDataFolder(localizer: Arc<Localizer>) -> impl IntoView {
    let standing = RwSignal::<Option<MachineStandingDto>>::new(None);
    spawn_local(async move {
        let url = format!("{}{READ_PATH}", ws::origin());
        if let Some(Got::Read(read)) = ws::get::<MachineDto>(&url).await {
            // The reader may have left the page before the answer came.
            standing.try_set(Some(read.standing));
        }
    });
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    // A request the daemon refused or didn't answer, said in the review.
    let problem = RwSignal::<Option<Sentence>>::new(None);

    let take_localizer = Arc::clone(&localizer);
    let take = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        problem.set(None);
        let localizer = Arc::clone(&take_localizer);
        spawn_local(async move {
            let url = format!("{}{NEW_ID_PATH}", ws::origin());
            let posted = ws::post(&url, &serde_json::Map::new()).await;
            busy.try_set(false);
            let said = match posted {
                Some(Posted::Done(answer)) => match serde_json::from_str::<MachineDto>(&answer) {
                    Ok(read) => {
                        // Closed first, then redrawn: the button the review
                        // returns focus to goes with the redraw, so focus
                        // moves to the notice's heading once it is drawn
                        // (WCAG 2.4.3).
                        open.try_set(false);
                        standing.try_set(Some(read.standing));
                        request_animation_frame(|| {
                            focus_element(HEADING_ID);
                        });
                        reply(
                            Politeness::Polite,
                            &text(&localizer, msg::UI_COPIED_DATA_FOLDER_TAKEN),
                        );
                        return;
                    }
                    Err(_) => unanswered(&localizer),
                },
                Some(Posted::Refused(code, args, detail)) => {
                    refused_sentence(&localizer, code, &args, detail.as_deref())
                }
                None => unanswered(&localizer),
            };
            problem.try_set(Some(said));
        });
    });

    let heading = text(&localizer, msg::UI_COPIED_DATA_FOLDER_HEADING);
    let body = text(&localizer, msg::UI_COPIED_DATA_FOLDER_BODY);
    let button_label = text(&localizer, msg::UI_COPIED_DATA_FOLDER_NEW_ID);
    let taken = text(&localizer, msg::UI_COPIED_DATA_FOLDER_TAKEN);
    let maintenance_off = text(&localizer, msg::UI_COPIED_DATA_FOLDER_MAINTENANCE_OFF);
    let restart_label = text(&localizer, msg::UI_COPIED_DATA_FOLDER_RESTART);
    (move || {
        let shown = standing.get()?;
        let copied = match shown {
            MachineStandingDto::Copied => true,
            MachineStandingDto::NewIdTaken => false,
            MachineStandingDto::Same
            | MachineStandingDto::Unread
            | MachineStandingDto::RefusedByHub => return None,
        };
        let content = if copied {
            let review_localizer = Arc::clone(&localizer);
            view! {
                <p>{body.clone()}</p>
                <div class=ACTIONS>
                    <Button
                        content=Content::Text(button_label.clone().into())
                        id=NEW_ID_BUTTON_ID
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        on_press=Callback::new(move |()| {
                            problem.set(None);
                            open.set(true);
                        })
                    />
                </div>
                {review(&review_localizer, open, busy, problem, take)}
            }
            .into_any()
        } else {
            let restart_localizer = Arc::clone(&localizer);
            let restart_label = restart_label.clone();
            view! {
                <p>{taken.clone()}</p>
                <p>{maintenance_off.clone()}</p>
                <p>
                    {Destination::route(RESTART_HREF, Matching::Exact)
                        .map(|to| {
                            view! {
                                <Link to=to localizer=restart_localizer>
                                    {restart_label}
                                </Link>
                            }
                        })}
                </p>
            }
            .into_any()
        };
        let heading = heading.clone();
        Some(view! {
            // The notice's region, named by its heading. The heading takes
            // focus once the new ID is taken (WCAG 2.4.3).
            <section aria-labelledby=HEADING_ID>
                <h2 id=HEADING_ID tabindex="-1" class="my-1 text-[1.125rem]">
                    {heading}
                </h2>
                {content}
            </section>
        })
    })
    .into_any()
}

/// The review of taking a new machine ID: what happens to this computer's
/// plans, its maintenance and the snapshots under the old ID.
fn review(
    localizer: &Arc<Localizer>,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    problem: RwSignal<Option<Sentence>>,
    take: Callback<()>,
) -> impl IntoView + use<> {
    let problem_localizer = Arc::clone(localizer);
    let body = text(localizer, msg::UI_COPIED_DATA_FOLDER_REVIEW_BODY);
    view! {
        <ReviewDialog
            open=open
            title=text(localizer, msg::UI_COPIED_DATA_FOLDER_REVIEW_TITLE)
            dismiss_label=text(localizer, msg::UI_REVIEW_CANCEL)
            confirm=Some(Confirmation {
                label: text(localizer, msg::UI_COPIED_DATA_FOLDER_REVIEW_CONFIRM),
                tone: Tone::Error,
                action: take,
            })
            returns_to=NEW_ID_BUTTON_ID.to_owned()
            described_by=REVIEW_BODY_ID.to_owned()
            working=Working {
                active: busy.into(),
                status: text(localizer, msg::UI_COPIED_DATA_FOLDER_REVIEW_TAKING),
            }
            fallback=HEADING_ID.to_owned()
        >
            <p id=REVIEW_BODY_ID>{body}</p>
            {move || {
                problem.get().map(|sentence| dialog_refusal_alert(&problem_localizer, sentence))
            }}
        </ReviewDialog>
    }
}

/// A request the daemon didn't answer, or answered with what isn't where
/// the machine stands.
fn unanswered(localizer: &Localizer) -> Sentence {
    Sentence::new(
        &text(localizer, msg::UI_COPIED_DATA_FOLDER_UNREACHABLE),
        None,
    )
}
