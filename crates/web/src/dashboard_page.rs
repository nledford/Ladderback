//! The hub's dashboard, its start page `/` (technical design §4.3, §4.4;
//! #569, #1440; §16, decisions 173 and 175): the start page's notices, then
//! a sentence that counts the machines and links to those that need
//! attention, then a machine card for each, the hub's own first, with its
//! Back up menu, then each approved machine in the order it was approved.
//! Each card links to its machine's page (`machine_page.rs`). Its old
//! address, `/dashboard`, leads there. What it shows while paused, counts,
//! says, reads again for, how its times and progress read and what each
//! card tells is `dashboard.rs`'s, decided on the host; this is the DOM
//! glue, which the `@ui` scenarios of `features/ui/hub_dashboard.feature`
//! run.
//!
//! The machines are read when the page opens, again each time the events
//! stream opens, the first time and after a reconnect, and when an event of
//! the hub's says a machine was approved, came online or went offline,
//! reported an operation, its plans or its progress. The hub's own machine
//! is read from the hub itself: who it is (`/api/v1/machine`), its
//! operations as the operations page follows them, and its plans' status
//! (`/api/v1/status`), read again as its operations change. A pause holds
//! what the page shows, not the reads.

use std::sync::Arc;
use std::time::Duration;

use api::{
    HUB_DASHBOARD_PATH, HubMachineDto, HubMachinesDto, IanaZone, MachineDto, MsgArg, StatusDto,
};
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::components::Redirect;

use crate::announcer::Politeness;
use crate::backend::{OperationsState, Rereads};
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::machine_card::CARDS;
use crate::components::skeleton::rules::Awaited;
use crate::components::status_badge::rules::BadgeState;
use crate::components::{
    Button, CardFact, DisclosureMenu, Link, MachineCard, PauseUpdates, State, TitleLevel,
    first_load, paced, refusal_alert, say_refusals,
};
use crate::dashboard::{
    CardFacts, CardStatus, Fleet, NextRun, Own, OwnStory, attention_link, attention_words,
    card_facts, connection_line, instant, own_machine, progress_reading, reads_again, summary,
};
use crate::live::reply;
use crate::operations_page::{StartBackup, badge_of};
use crate::pause;
use crate::repositories::Sentence;
use crate::routes::{NotFound, PageHeading, refused_sentence, text_with};
use crate::settings::agents::hub_known;
use crate::start_page::notices;
use crate::text;
use crate::visit::Visit;
use crate::wording::Words;
use crate::ws::{self, Phase, Posted};

/// The banner beside the pause button while the page is paused.
const PAUSED_ID: &str = "dashboard-paused";
/// How long one read of the machines may take before it counts as
/// unanswered.
const READ_PATIENCE: Duration = Duration::from_secs(5);
/// How long the page waits before it asks again after a read that went
/// unanswered while none had answered: the "few seconds" it says.
const ASK_AGAIN: Duration = Duration::from_secs(3);
/// How often the cards read the time again, so that a planned run falls
/// due on them while nothing else changes (decision 175).
const TICK: Duration = Duration::from_secs(60);

/// The route `/dashboard`, the dashboard's old address: on a daemon serving
/// as a hub, it leads to the start page, which is the dashboard, in place of
/// itself in the browser's history; on one that isn't, what any path nothing
/// serves shows; nothing until the page knows which (§16, decision 173).
#[component]
pub(crate) fn DashboardRoute(localizer: Arc<Localizer>) -> impl IntoView {
    let hub = hub_known();
    move || match hub() {
        None => ().into_any(),
        Some(true) => {
            let options = NavigateOptions {
                replace: true,
                ..NavigateOptions::default()
            };
            view! { <Redirect path="/" options=options /> }.into_any()
        }
        Some(false) => view! { <NotFound localizer=Arc::clone(&localizer) /> }.into_any(),
    }
}

/// Lands one read's answer in `fleet`, or its lack of one: an answer
/// replaces the machines shown, and no answer keeps them, or says the hub
/// didn't answer while none has. Whether the page still waits for its
/// first answer.
fn land(fleet: Fleet, read: Option<HubMachinesDto>) -> bool {
    if let Some(read) = read {
        let _ = fleet.machines.try_set(read.machines);
        let _ = fleet.state.try_set(State::Ready);
        return false;
    }
    let waiting = matches!(
        fleet.state.try_get_untracked(),
        Some(State::Loading | State::Unreachable)
    );
    if waiting {
        let _ = fleet.state.try_set(State::Unreachable);
    }
    waiting
}

/// Reads the machines into `fleet`, paced by `rereads` (`ws::reread_hub`).
fn read(fleet: Fleet, rereads: StoredValue<Rereads>, visit: &Visit) {
    ws::reread_hub(
        format!("{}{HUB_DASHBOARD_PATH}", ws::origin()),
        rereads,
        visit,
        (READ_PATIENCE, ASK_AGAIN),
        move |read| land(fleet, read),
    );
}

/// Reads the machines into `fleet` now, when the events stream opens and
/// when an event asks for it, and the hub's own machine, as long as the
/// page shows; the time the cards are read at, every minute.
pub(crate) fn follow(fleet: Fleet, localizer: &Arc<Localizer>) {
    let rereads = StoredValue::new(Rereads::default());
    let (visit, departure) = Visit::begin();
    on_cleanup(move || drop(departure));
    fleet.now.set(ws::now_ms());
    read(fleet, rereads, &visit);
    let (opening, hearing) = (visit.clone(), visit.clone());
    ws::listen(
        &ws::origin(),
        &visit,
        move || read(fleet, rereads, &opening),
        move |event| {
            if reads_again(&event) {
                read(fleet, rereads, &hearing);
            }
        },
    );
    follow_own(fleet, localizer, &visit);
    let ticking = visit.clone();
    spawn_local(async move {
        let _ = ticking
            .during(async move {
                loop {
                    gloo_timers::future::sleep(TICK).await;
                    if fleet.now.try_set(ws::now_ms()).is_some() {
                        break;
                    }
                }
            })
            .await;
    });
}

/// Reads the hub's own machine into `fleet` (decision 175): who it is,
/// once; its operations, as the operations page follows them; and its
/// plans' status, read again each time an operation comes, goes or
/// changes its status.
fn follow_own(fleet: Fleet, localizer: &Arc<Localizer>, visit: &Visit) {
    let base = ws::origin();
    let identity = RwSignal::<Option<MachineDto>>::new(None);
    let status = RwSignal::<Option<StatusDto>>::new(None);
    let operations = RwSignal::new(OperationsState::default());
    // The history's read refused or unanswered leaves the card without
    // operations; the dashboard's own read says the hub didn't answer.
    let phase = RwSignal::new(Phase::Loading);
    let refused = RwSignal::<Option<Sentence>>::new(None);
    let say = Arc::clone(localizer);
    ws::watch(
        &base,
        operations,
        phase,
        refused,
        move |code, args, detail| refused_sentence(&say, code, args, detail),
        visit,
    );
    let url = format!("{base}/api/v1/machine");
    let asking = visit.clone();
    spawn_local(async move {
        let _ = asking
            .during(async move {
                loop {
                    if let Some(read) = ws::poll::<MachineDto>(&url, READ_PATIENCE).await {
                        let _ = identity.try_set(Some(read));
                        break;
                    }
                    gloo_timers::future::sleep(ASK_AGAIN).await;
                }
            })
            .await;
    });
    // Each operation and its status: the first, and each change of either,
    // reads the plans' status again. The effect reads `operations` itself,
    // so each change wakes it: a memo tracked and never read is never
    // computed, so it never hears a change.
    let story = StoredValue::new(OwnStory::default());
    let rereads = StoredValue::new(Rereads::default());
    let status_url = format!("{base}/api/v1/status");
    let reading = visit.clone();
    Effect::new(move |_| {
        let moved = operations
            .with(|operations| story.try_update_value(|story| story.moves_on(operations)));
        if moved != Some(true) {
            return;
        }
        ws::reread_hub(
            status_url.clone(),
            rereads,
            &reading,
            (READ_PATIENCE, ASK_AGAIN),
            move |read: Option<StatusDto>| match read {
                Some(read) => {
                    let _ = status.try_set(Some(read));
                    false
                }
                None => status.try_with_untracked(Option::is_none).unwrap_or(false),
            },
        );
    });
    Effect::new(move |_| {
        let Some(identity) = identity.get() else {
            return;
        };
        let own = status.with(|status| {
            operations.with(|operations| own_machine(&identity, status.as_ref(), operations))
        });
        fleet.own.set(Own::Read(own.map(Box::new)));
    });
}

/// The pause button of a page that shows `fleet`, and its banner while
/// paused (A3; §16, decision 65).
pub(crate) fn pause_control(localizer: &Arc<Localizer>, fleet: Fleet) -> impl IntoView + use<> {
    let (pause_label, paused_said) = (
        text(localizer, msg::UI_LIVE_PAUSE),
        text(localizer, msg::UI_LIVE_PAUSED),
    );
    let banner_localizer = Arc::clone(localizer);
    let resume_localizer = Arc::clone(localizer);
    let on_resume = Callback::new(move |()| {
        let words = fleet.resume();
        text_with(&resume_localizer, words.id, &words.args)
    });
    view! {
        <PauseUpdates
            label=pause_label
            paused_said=paused_said
            paused=Signal::derive(move || fleet.paused())
            banner=Signal::derive(move || {
                let words = pause::banner(fleet.pending());
                text_with(&banner_localizer, words.id, &words.args)
            })
            banner_id=PAUSED_ID
            on_pause=Callback::new(move |()| fleet.pause())
            on_resume=on_resume
        />
    }
}

/// One machine that needs attention, as the sentence's link names it: its
/// ID, its hostname, its zone and why.
type Needing = (String, String, Option<String>, crate::dashboard::Attention);

/// The dashboard itself, a hub's start page: its heading, the start page's
/// notices, then the sentence and the machines' cards.
#[component]
pub(crate) fn Dashboard(localizer: Arc<Localizer>) -> impl IntoView {
    let fleet = Fleet::new();
    follow(fleet, &localizer);
    let viewer = l10n::viewer_zone();
    let loading = text(&localizer, msg::UI_DASHBOARD_LOADING);
    let unreachable = text(&localizer, msg::UI_DASHBOARD_UNREACHABLE);
    let none = text(&localizer, msg::UI_DASHBOARD_NONE);
    let shown = fleet.shown();
    let now = fleet.shown_now();
    let own = fleet.own_id();
    // How many machines show, and those that need attention: the sentence
    // is built again only when these change.
    let counted = Memo::new(move |_| {
        let now = now.get();
        shown.with(|machines| {
            let needing = machines
                .iter()
                .filter_map(|machine| {
                    card_facts(machine, now).attention.map(|attention| {
                        (
                            machine.machine_id.clone(),
                            machine.hostname.clone(),
                            machine.zone.clone(),
                            attention,
                        )
                    })
                })
                .collect::<Vec<Needing>>();
            (machines.len(), needing)
        })
    });
    let none_approved = Memo::new(move |_| {
        let own = own.get();
        shown.with(|machines| {
            machines
                .iter()
                .all(|machine| Some(&machine.machine_id) == own.as_ref())
        })
    });
    // A start the hub refused, said and shown above the cards.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let (state, pace) = paced(
        move || fleet.shown_state().get(),
        |state| *state == State::Loading,
    );
    let heading = text(&localizer, msg::UI_DASHBOARD_HEADING);
    let intro = text(&localizer, msg::UI_DASHBOARD_INTRO);
    let notices = notices(&localizer);
    let pause = pause_control(&localizer, fleet);
    view! {
        <PageHeading>{heading}</PageHeading>
        {notices}
        <p>{intro}</p>
        {pause}
        {move || match state.get() {
            State::Loading => first_load(loading.clone(), Awaited::table(2), pace).into_any(),
            State::Unreachable => view! { <p>{unreachable.clone()}</p> }.into_any(),
            State::Ready => {
                let card_localizer = Arc::clone(&localizer);
                let card_viewer = viewer.clone();
                let none = none.clone();
                let alert_localizer = Arc::clone(&localizer);
                view! {
                    {summary_view(&localizer, counted, viewer.clone())}
                    {move || {
                        refused.get().map(|sentence| refusal_alert(&alert_localizer, sentence))
                    }}
                    <ul class=CARDS role="list">
                        <For
                            each=move || shown.get()
                            key=|machine: &HubMachineDto| machine.machine_id.clone()
                            children=move |machine: HubMachineDto| {
                                view! {
                                    <li class="m-0">
                                        {card(
                                            &card_localizer,
                                            fleet,
                                            machine,
                                            card_viewer.clone(),
                                            refused,
                                        )}
                                    </li>
                                }
                            }
                        />
                    </ul>
                    {move || none_approved.get().then(|| view! { <p>{none.clone()}</p> })}
                }
                    .into_any()
            }
        }}
    }
}

/// The sentence above the cards (decision 175): how many machines show,
/// and a link to the page of each that needs attention, which says why.
fn summary_view(
    localizer: &Arc<Localizer>,
    counted: Memo<(usize, Vec<Needing>)>,
    viewer: Option<IanaZone>,
) -> impl IntoView + use<> {
    let localizer = Arc::clone(localizer);
    move || {
        counted.with(|(count, needing)| {
            let words = summary(*count, needing.len());
            let sentence = text_with(&localizer, words.id, &words.args);
            let links = needing
                .iter()
                .filter_map(|(agent_id, hostname, zone, attention)| {
                    let to = Destination::route(&format!("/machines/{agent_id}"), Matching::Exact)?;
                    let words = attention_link(hostname, attention, |at| {
                        shown_instant(&localizer, at, viewer.as_ref(), zone.as_deref())
                    });
                    let words = text_with(&localizer, words.id, &words.args);
                    Some(view! {
                        <li>
                            <Link
                                to=to
                                localizer=Arc::clone(&localizer)
                                placement=LinkPlacement::InText
                            >
                                {words}
                            </Link>
                        </li>
                    })
                })
                .collect::<Vec<_>>();
            view! {
                <p>{sentence}</p>
                {(!links.is_empty()).then(|| view! { <ul>{links}</ul> })}
            }
        })
    }
}

/// What a card's status badge shows of `status`.
fn status_badge(localizer: &Localizer, status: &CardStatus) -> (BadgeState, String) {
    match status {
        CardStatus::Running => (BadgeState::Running, text(localizer, msg::UI_STATUS_RUNNING)),
        CardStatus::Last(status) => badge_of(localizer, status),
        CardStatus::Never => (
            BadgeState::Empty,
            text(localizer, msg::UI_DASHBOARD_NEVER_RUN),
        ),
    }
}

/// A card's facts, in words, each time as `shown` writes it.
fn card_fact_list(
    localizer: &Localizer,
    facts: &CardFacts,
    shown: impl Fn(&str) -> String,
) -> Vec<CardFact> {
    let t = |id| text(localizer, id);
    let at = |raw: &str| (shown(raw), Some(raw.to_owned()));
    let (backup, backup_at) = facts
        .last_backup
        .as_deref()
        .map_or_else(|| (t(msg::UI_MACHINE_CARD_NO_BACKUP), None), at);
    let (next, next_at) = match &facts.next_run {
        NextRun::NotPlanned => (t(msg::UI_DASHBOARD_NOT_PLANNED), None),
        NextRun::At(raw) => at(raw),
        NextRun::WasDue(raw) => (
            text_with(
                localizer,
                msg::UI_MACHINE_CARD_WAS_DUE,
                &[("when", MsgArg::Text(shown(raw)))],
            ),
            None,
        ),
    };
    let plans = facts.plans.map_or_else(
        || t(msg::UI_MACHINE_CARD_PLANS_UNREPORTED),
        |count| {
            text_with(
                localizer,
                msg::UI_MACHINE_CARD_PLAN_COUNT,
                &[("count", MsgArg::Count(count))],
            )
        },
    );
    vec![
        CardFact {
            term: t(msg::UI_MACHINE_CARD_LAST_BACKUP),
            words: backup,
            time: backup_at,
        },
        CardFact {
            term: t(msg::UI_MACHINE_CARD_NEXT_RUN),
            words: next,
            time: next_at,
        },
        CardFact {
            term: t(msg::UI_MACHINE_CARD_PLANS),
            words: plans,
            time: None,
        },
    ]
}

/// The card of `first`, a machine as the page first showed it, kept as the
/// page shows it since: the hub's own marked "This hub" and holding the
/// Back up menu, any other a link to its page alone. Its hostname is the
/// one it was approved under, which a later read doesn't change.
fn card(
    localizer: &Arc<Localizer>,
    fleet: Fleet,
    first: HubMachineDto,
    viewer: Option<IanaZone>,
    refused: RwSignal<Option<Sentence>>,
) -> impl IntoView + use<> {
    let HubMachineDto {
        machine_id: agent_id,
        hostname,
        ..
    } = first;
    let this_hub = fleet.own_id().get_untracked().as_deref() == Some(agent_id.as_str());
    let shown = fleet.shown();
    let now = fleet.shown_now();
    let page = format!("/machines/{agent_id}");
    let menu_id = format!("backup-menu-{agent_id}");
    let heading_id = format!("dashboard-machine-{agent_id}");
    let machine = Signal::derive(move || {
        shown.with(|machines| {
            machines
                .iter()
                .find(|machine| machine.machine_id == agent_id)
                .cloned()
        })
    });
    let zone = Memo::new(move |_| machine.with(|machine| machine.as_ref()?.zone.clone()));
    let facts = Memo::new(move |_| {
        let now = now.get();
        machine.with(|machine| machine.as_ref().map(|machine| card_facts(machine, now)))
    });
    let shown_at = {
        let localizer = Arc::clone(localizer);
        let viewer = viewer.clone();
        move |at: &str| shown_instant(&localizer, at, viewer.as_ref(), zone.get().as_deref())
    };
    let connection = {
        let localizer = Arc::clone(localizer);
        let shown_at = shown_at.clone();
        Signal::derive(move || {
            let (online, at) = machine.with(|machine| {
                machine
                    .as_ref()
                    .map(|machine| (machine.online, machine.last_seen.clone()))
                    .unwrap_or_default()
            });
            text_with(
                &localizer,
                connection_line(online),
                &[("when", MsgArg::Text(shown_at(&at)))],
            )
        })
    };
    let attention = {
        let localizer = Arc::clone(localizer);
        let shown_at = shown_at.clone();
        Signal::derive(move || {
            facts.with(|facts| {
                let attention = facts.as_ref()?.attention.as_ref()?;
                let words = attention_words(attention, &shown_at);
                Some(text_with(&localizer, words.id, &words.args))
            })
        })
    };
    let status = {
        let localizer = Arc::clone(localizer);
        Signal::derive(move || {
            facts.with(|facts| {
                let status = facts
                    .as_ref()
                    .map_or(CardStatus::Never, |facts| facts.status.clone());
                status_badge(&localizer, &status)
            })
        })
    };
    let fact_list = {
        let localizer = Arc::clone(localizer);
        Signal::derive(move || {
            facts.with(|facts| {
                facts
                    .as_ref()
                    .map(|facts| card_fact_list(&localizer, facts, &shown_at))
                    .unwrap_or_default()
            })
        })
    };
    let progress = Signal::derive(move || {
        machine.with(|machine| machine.as_ref()?.progress.as_ref().map(progress_reading))
    });
    let progress_label = text_with(
        localizer,
        msg::UI_DASHBOARD_PROGRESS,
        &[("hostname", MsgArg::Text(hostname.clone()))],
    );
    let open_words = text_with(
        localizer,
        msg::UI_MACHINE_CARD_OPEN,
        &[("hostname", MsgArg::Text(hostname.clone()))],
    );
    let open = Destination::route(&page, Matching::Exact).map(|to| {
        view! {
            <Link to=to localizer=Arc::clone(localizer) placement=LinkPlacement::Alone>
                {open_words}
            </Link>
        }
    });
    let menu = this_hub.then(|| {
        let plans = Memo::new(move |_| {
            machine.with(|machine| {
                machine
                    .as_ref()
                    .and_then(|machine| machine.plans.as_ref())
                    .map(|plans| plans.plans.iter().map(|plan| plan.plan.clone()).collect())
                    .unwrap_or_default()
            })
        });
        back_up_menu(localizer, menu_id, plans, refused)
    });
    view! {
        <MachineCard
            localizer=Arc::clone(localizer)
            hostname=hostname
            heading_id=heading_id
            level=TitleLevel::Two
            this_hub=this_hub
            connection=connection
            attention=attention
            status=status
            facts=fact_list
            progress_label=progress_label
            progress=progress
            zone=viewer
        >
            {menu}
            {open}
        </MachineCard>
    }
}

/// The hub's own Back up menu (decision 175): a disclosure menu whose rows
/// start a backup of each of its `plans`, as the operations page's buttons
/// do, through the same request. The start is said politely; a refusal is
/// said and shown in `refused`. A row's press closes the menu and returns
/// focus to its button, as the disclosure menu does for any row's button.
fn back_up_menu(
    localizer: &Arc<Localizer>,
    panel_id: String,
    plans: Memo<Vec<String>>,
    refused: RwSignal<Option<Sentence>>,
) -> impl IntoView + use<> {
    let label = text(localizer, msg::UI_MACHINE_CARD_BACK_UP);
    let started = text(localizer, msg::UI_BACKUP_STARTED);
    let localizer = Arc::clone(localizer);
    let rows = move || {
        plans
            .get()
            .into_iter()
            .map(|plan| {
                let words = text_with(
                    &localizer,
                    msg::UI_START_PLAN,
                    &[("plan", MsgArg::Text(plan.clone()))],
                );
                let starting = RwSignal::new(false);
                let started = started.clone();
                let localizer = Arc::clone(&localizer);
                let on_press = Callback::new(move |()| {
                    let request = StartBackup { plan: plan.clone() };
                    let url = format!("{}/api/v1/operations", ws::origin());
                    let started = started.clone();
                    let localizer = Arc::clone(&localizer);
                    refused.set(None);
                    starting.set(true);
                    spawn_local(async move {
                        let answer = ws::post(&url, &request).await;
                        let _ = starting.try_set(false);
                        match answer {
                            Some(Posted::Done(_)) => reply(Politeness::Polite, &started),
                            Some(Posted::Refused(code, args, detail)) => {
                                let _ = refused.try_set(Some(refused_sentence(
                                    &localizer,
                                    code,
                                    &args,
                                    detail.as_deref(),
                                )));
                            }
                            None => {}
                        }
                    });
                });
                view! {
                    <li class="px-2">
                        <Button
                            content=Content::IconAndText {
                                glyph: LucideGlyph::Play.into(),
                                label: words.into(),
                            }
                            appearance=Appearance::Ghost
                            busy=starting
                            on_press=on_press
                        />
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <DisclosureMenu label=label panel_id=panel_id current=false>
            <ul>{rows}</ul>
        </DisclosureMenu>
    }
}

/// The instant `at` as the dashboard writes it, in the viewer's zone and the
/// machine's `zone` beside it when they differ; as it came when it can't be
/// read, never a blank.
pub(crate) fn shown_instant(
    localizer: &Localizer,
    at: &str,
    viewer: Option<&IanaZone>,
    zone: Option<&str>,
) -> String {
    instant(at, viewer, zone).map_or_else(
        || at.to_owned(),
        |Words { id, args }| text_with(localizer, id, &args),
    )
}
