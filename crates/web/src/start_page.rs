//! The start page, the route `/` (§4.4): on a daemon serving as a hub, its
//! dashboard (`dashboard_page.rs`; §16, decision 173); elsewhere the
//! configured plans and when each runs next, the ways to the Repositories
//! and Plans pages while there are none, the way to the Operations page, and
//! where the format of a plan in `config.toml` is described. Both say the
//! start page's notices first. A plan's "Next run" cell is
//! `routes::next_run_cell`'s, host-tested; this is the DOM glue, which the
//! `@ui` scenarios of `features/ui/start_page.feature` and
//! `features/ui/hub_dashboard.feature` run.

use std::sync::Arc;

use api::{PlanSummaryDto, PlansDto};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::skeleton::rules::Awaited;
use crate::components::{
    Column, Link, Name as TableName, Sentences, State, Table, refusal_alert, say_refusals,
};
use crate::components::{first_load, paced};
use crate::copied_data_folder::CopiedDataFolder;
use crate::full_disk_access_warning::FullDiskAccessSteps;
use crate::repositories::{Sentence, repository_notes};
use crate::routes::{PageHeading, next_run_cell, refused_sentence, time_view};
use crate::settings::agents::hub_known;
use crate::sign_in::SignInOff;
use crate::text;
use crate::variants::Rem;
use crate::ws::{self, Got, Phase};

/// Where the format of a plan in `config.toml` is described: §6.1 of the
/// technical design, at the tag of the release this build is, so a reader
/// gets the format their build reads.
pub(crate) const PLAN_FORMAT_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/v",
    env!("CARGO_PKG_VERSION"),
    "/docs/design/scheduling-and-config.md#61-source-of-truth"
);

/// The route `/`: the dashboard on a daemon serving as a hub (§16, decision
/// 173), the start page on one that isn't; nothing until the page knows
/// which, so a hub's start page never shows the welcome first.
#[component]
pub(crate) fn StartRoute(localizer: Arc<Localizer>) -> impl IntoView {
    let hub = hub_known();
    move || match hub() {
        None => ().into_any(),
        Some(true) => {
            view! { <crate::dashboard_page::Dashboard localizer=Arc::clone(&localizer) /> }
                .into_any()
        }
        Some(false) => view! { <Start localizer=Arc::clone(&localizer) /> }.into_any(),
    }
}

/// What the start page says first, under its heading, on every daemon: that
/// sign-in is off, with how to turn it on (security §10.1), the Full Disk
/// Access steps (decision 115), and a copied data folder or a machine its
/// hub refused or removed (decision 144).
pub(crate) fn notices(localizer: &Arc<Localizer>) -> impl IntoView + use<> {
    view! {
        <SignInOff localizer=Arc::clone(localizer) />
        <FullDiskAccessSteps localizer=Arc::clone(localizer) />
        <CopiedDataFolder localizer=Arc::clone(localizer) />
    }
}

/// The start page of a daemon that isn't a hub: its notices, then the
/// configured plans, each with when it runs next on its schedule (§5), the
/// way to the operations page, where a backup is started by hand, and,
/// last, for whoever writes `config.toml` by hand, where the format of a
/// plan is described. Without plans it says to connect a repository first
/// and add the plan on the Plans page, with the way to each (#477, #1373).
#[component]
fn Start(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_START_HEADING);
    let unreachable_heading = text(&localizer, msg::UI_UNREACHABLE_HEADING);
    let body = text(&localizer, msg::UI_START_PLANS_BODY);
    let loading = text(&localizer, msg::UI_LOADING);
    let unreachable_body = text(&localizer, msg::UI_UNREACHABLE_BODY);
    let empty = text(&localizer, msg::UI_START_NO_PLANS);
    let operations_link = text(&localizer, msg::UI_START_OPERATIONS_LINK);
    let repositories_link = text(&localizer, msg::UI_START_REPOSITORIES_LINK);
    let plans_link = text(&localizer, msg::UI_START_PLANS_LINK);
    let plan_format_link = text(&localizer, msg::UI_START_PLAN_FORMAT_LINK);
    let phase = RwSignal::new(Phase::Loading);
    // The configured plans; `None` until the read answers.
    let plans = RwSignal::<Option<Vec<PlanSummaryDto>>>::new(None);
    // A read the daemon refused, a session that ended and was not signed in
    // again among the reasons: said in the list's place, as the daemon said
    // it. Only no answer at all is a daemon that can't be reached.
    let refused = RwSignal::<Option<Sentence>>::new(None);
    say_refusals(refused);
    let url = format!("{}/api/v1/plans", ws::origin());
    let read_localizer = Arc::clone(&localizer);
    spawn_local(async move {
        let read = ws::get::<PlansDto>(&url).await;
        // The reader may have followed a link before the answer came: the
        // page's signals went with it.
        if phase.try_get_untracked().is_none() {
            return;
        }
        match read {
            Some(Got::Read(read)) => {
                plans.set(Some(read.plans));
                phase.set(Phase::Ready);
            }
            Some(Got::Refused(code, args, detail)) => {
                refused.set(Some(refused_sentence(
                    &read_localizer,
                    code,
                    &args,
                    detail.as_deref(),
                )));
                phase.set(Phase::Ready);
            }
            None => phase.set(Phase::Unreachable),
        }
    });
    // Each next run is written in the viewer's own zone, as every instant the
    // page shows is (§8.4).
    let viewer = l10n::viewer_zone();
    let notices = notices(&localizer);
    let links = Arc::clone(&localizer);
    let operations_localizer = Arc::clone(&localizer);
    let (phase, pace) = paced(move || phase.get(), |phase| *phase == Phase::Loading);
    view! {
        <PageHeading>
            {move || {
                if phase.get() == Phase::Unreachable {
                    unreachable_heading.clone()
                } else {
                    heading.clone()
                }
            }}
        </PageHeading>
        {notices}
        <p>{body}</p>
        {move || match phase.get() {
            Phase::Loading => first_load(loading.clone(), Awaited::table(2), pace).into_any(),
            Phase::Unreachable => view! { <p>{unreachable_body.clone()}</p> }.into_any(),
            Phase::Ready => {
                match (refused.get(), plans.get()) {
                    (Some(sentence), _) => refusal_alert(&localizer, sentence).into_any(),
                    (None, Some(plans)) if !plans.is_empty() => {
                        start_plans_table(&localizer, plans, viewer.clone()).into_any()
                    }
                    _ => {
                        let repositories_link = repositories_link.clone();
                        let plans_link = plans_link.clone();
                        let to = Destination::route("/repositories", Matching::Exact);
                        let to_plans = Destination::route("/plans", Matching::Exact);
                        let localizer = Arc::clone(&localizer);
                        let plans_localizer = Arc::clone(&localizer);
                        view! {
                            <p>{empty.clone()}</p>
                            <p>
                                {to
                                    .map(|to| {
                                        view! {
                                            <Link
                                                to=to
                                                localizer=localizer
                                                placement=LinkPlacement::InText
                                            >
                                                {repositories_link}
                                            </Link>
                                        }
                                    })}
                            </p>
                            <p>
                                {to_plans
                                    .map(|to| {
                                        view! {
                                            <Link
                                                to=to
                                                localizer=plans_localizer
                                                placement=LinkPlacement::InText
                                            >
                                                {plans_link}
                                            </Link>
                                        }
                                    })}
                            </p>
                        }
                            .into_any()
                    }
                }
            }
        }}
        <p>
            {Destination::route("/operations", Matching::Exact)
                .map(|to| {
                    view! {
                        <Link to=to localizer=operations_localizer placement=LinkPlacement::InText>
                            {operations_link}
                        </Link>
                    }
                })}
        </p>
        // The format of a plan in `config.toml` comes last: the Plans page
        // adds and changes plans, and the file is for an advanced user.
        <p>
            {Destination::outside(PLAN_FORMAT_URL)
                .map(|to| {
                    view! {
                        <Link to=to localizer=links placement=LinkPlacement::InText>
                            {plan_format_link}
                        </Link>
                    }
                })}
        </p>
    }
}

/// The start page's table of plans, named by its caption: each plan's ID as
/// its row's header and its next run, and, only when a plan has some, its
/// notes, such as a glob that can never match (#510).
fn start_plans_table(
    localizer: &Arc<Localizer>,
    plans: Vec<PlanSummaryDto>,
    viewer: Option<api::IanaZone>,
) -> impl IntoView + use<> {
    let noted = plans.iter().any(|plan| !plan.warnings.is_empty());
    let plan = Column::new(
        "plan",
        text(localizer, msg::UI_COLUMN_PLAN),
        Rem::tenths(120),
        |row: Signal<PlanSummaryDto>| (move || row.with(|plan| plan.id.clone())).into_any(),
    );
    let next_localizer = Arc::clone(localizer);
    let mut columns = vec![Column::new(
        "next-run",
        text(localizer, msg::UI_COLUMN_NEXT_RUN),
        Rem::tenths(120),
        move |row: Signal<PlanSummaryDto>| {
            let (shown, raw) =
                row.with_untracked(|plan| next_run_cell(&next_localizer, plan, viewer.as_ref()));
            time_view(shown, raw)
        },
    )];
    if noted {
        let notes_localizer = Arc::clone(localizer);
        columns.push(Column::new(
            "notes",
            text(localizer, msg::UI_COLUMN_NOTES),
            Rem::tenths(240),
            move |row: Signal<PlanSummaryDto>| {
                row.with_untracked(|plan| repository_notes(&notes_localizer, &plan.warnings))
                    .into_iter()
                    .map(|note| view! { <div class="text-muted">{note}</div> })
                    .collect_view()
                    .into_any()
            },
        ));
    }
    let sentences = Sentences {
        loading: text(localizer, msg::UI_LOADING),
        empty: text(localizer, msg::UI_START_NO_PLANS),
        unreachable: text(localizer, msg::UI_UNREACHABLE_BODY),
    };
    view! {
        <Table
            name=TableName::Caption(text(localizer, msg::UI_START_CAPTION))
            row_header=plan
            columns=columns
            rows=Signal::stored(plans)
            key=|plan: &PlanSummaryDto| plan.id.clone()
            state=State::Ready
            sentences=sentences
        />
    }
}
