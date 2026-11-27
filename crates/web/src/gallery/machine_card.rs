//! The gallery's machine card demo (technical design §4.5, "Tests").

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::backend::Progress as Reading;
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::machine_card::CARDS;
use crate::components::status_badge::rules::BadgeState;
use crate::components::{Button, CardFact, DisclosureMenu, Link, MachineCard, TitleLevel};
use crate::dashboard_page::shown_instant;
use crate::routes::text_with;
use crate::text;

/// The instants the demo's cards tell, fixed so the demo never changes.
const HUB_BACKUP: &str = "2026-11-09T09:00:00Z";
const HUB_NEXT: &str = "2026-11-10T09:00:00Z";
const LAPTOP_SEEN: &str = "2026-11-08T18:30:00Z";
const LAPTOP_BACKUP: &str = "2026-11-08T18:00:00Z";

/// The machine card's demo (`#machine-card-demo`): three cards in the list
/// the dashboard lays them out in. The hub's own, marked "This hub", with
/// its Back up menu; one whose last run failed, which needs attention; and
/// one that runs an operation, its progress bar at 25 percent.
#[component]
pub(super) fn MachineCardDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let t = |id| text(&localizer, id);
    let at = |raw: &str| shown_instant(&localizer, raw, None, None);
    let fact = |term, words: String, time: Option<&str>| CardFact {
        term: t(term),
        words,
        time: time.map(str::to_owned),
    };
    let count = |count: u64| {
        text_with(
            &localizer,
            msg::UI_MACHINE_CARD_PLAN_COUNT,
            &[("count", MsgArg::Count(count))],
        )
    };
    let named = |id, hostname: &str| {
        text_with(
            &localizer,
            id,
            &[("hostname", MsgArg::Text(hostname.to_owned()))],
        )
    };
    let open = |hostname: &str| {
        let words = named(msg::UI_MACHINE_CARD_OPEN, hostname);
        let localizer = Arc::clone(&localizer);
        Destination::route(&format!("/machines/{hostname}"), Matching::Exact).map(move |to| {
            view! {
                <Link to=to localizer=localizer placement=LinkPlacement::Alone>
                    {words}
                </Link>
            }
        })
    };
    let (open_hub, open_laptop, open_desktop) =
        (open("demo-hub"), open("demo-laptop"), open("demo-desktop"));
    let (heading, back_up) = (
        t(msg::UI_GALLERY_MACHINE_CARD_HEADING),
        t(msg::UI_MACHINE_CARD_BACK_UP),
    );
    let (succeeded, failed_badge, running_badge) = (
        t(msg::UI_STATUS_SUCCEEDED),
        t(msg::UI_STATUS_FAILED),
        t(msg::UI_STATUS_RUNNING),
    );
    let labels = ["demo-hub", "demo-laptop", "demo-desktop"]
        .map(|hostname| named(msg::UI_DASHBOARD_PROGRESS, hostname));
    let [hub_label, laptop_label, desktop_label] = labels;
    let online = t(msg::UI_DASHBOARD_ONLINE_NOW);
    let hub_facts = vec![
        fact(
            msg::UI_MACHINE_CARD_LAST_BACKUP,
            at(HUB_BACKUP),
            Some(HUB_BACKUP),
        ),
        fact(msg::UI_MACHINE_CARD_NEXT_RUN, at(HUB_NEXT), Some(HUB_NEXT)),
        fact(msg::UI_MACHINE_CARD_PLANS, count(2), None),
    ];
    let laptop_facts = vec![
        fact(
            msg::UI_MACHINE_CARD_LAST_BACKUP,
            at(LAPTOP_BACKUP),
            Some(LAPTOP_BACKUP),
        ),
        fact(
            msg::UI_MACHINE_CARD_NEXT_RUN,
            t(msg::UI_DASHBOARD_NOT_PLANNED),
            None,
        ),
        fact(msg::UI_MACHINE_CARD_PLANS, count(3), None),
    ];
    let desktop_facts = vec![
        fact(
            msg::UI_MACHINE_CARD_LAST_BACKUP,
            t(msg::UI_MACHINE_CARD_NO_BACKUP),
            None,
        ),
        fact(
            msg::UI_MACHINE_CARD_NEXT_RUN,
            t(msg::UI_DASHBOARD_NOT_PLANNED),
            None,
        ),
        fact(
            msg::UI_MACHINE_CARD_PLANS,
            t(msg::UI_MACHINE_CARD_PLANS_UNREPORTED),
            None,
        ),
    ];
    let laptop_seen = text_with(
        &localizer,
        msg::UI_DASHBOARD_LAST_CONNECTED,
        &[("when", MsgArg::Text(at(LAPTOP_SEEN)))],
    );
    let failed = text_with(
        &localizer,
        msg::UI_MACHINE_CARD_FAILED,
        &[("plan", MsgArg::Text("photos".to_owned()))],
    );
    let start = |plan: &str| {
        text_with(
            &localizer,
            msg::UI_START_PLAN,
            &[("plan", MsgArg::Text(plan.to_owned()))],
        )
    };
    let rows = [start("documents"), start("music")].map(|words| {
        view! {
            <li class="px-2">
                <Button content=Content::Text(words.into()) appearance=Appearance::Ghost />
            </li>
        }
    });
    let running = Reading {
        step: Some(api::OperationStep::BackingUp),
        unit: api::ProgressUnit::Bytes,
        current: 512,
        total: Some(2048),
        reported_at: None,
        since: None,
    };
    let none = Signal::stored(None::<String>);
    let no_progress = Signal::stored(None::<Reading>);
    let (hub_localizer, laptop_localizer, desktop_localizer) = (
        Arc::clone(&localizer),
        Arc::clone(&localizer),
        Arc::clone(&localizer),
    );
    view! {
        <section id="machine-card-demo">
            <DemoHeading text=heading />
            <ul class=CARDS role="list">
                <li class="m-0">
                    <MachineCard
                        localizer=hub_localizer
                        hostname="demo-hub".to_owned()
                        heading_id="machine-card-demo-hub".to_owned()
                        level=TitleLevel::Three
                        this_hub=true
                        connection=Signal::stored(online.clone())
                        attention=none
                        status=Signal::stored((BadgeState::Succeeded, succeeded))
                        facts=Signal::stored(hub_facts)
                        progress_label=hub_label
                        progress=no_progress
                    >
                        <DisclosureMenu
                            label=back_up
                            panel_id="machine-card-demo-menu".to_owned()
                            current=false
                        >
                            <ul>{rows}</ul>
                        </DisclosureMenu>
                        {open_hub}
                    </MachineCard>
                </li>
                <li class="m-0">
                    <MachineCard
                        localizer=laptop_localizer
                        hostname="demo-laptop".to_owned()
                        heading_id="machine-card-demo-laptop".to_owned()
                        level=TitleLevel::Three
                        connection=Signal::stored(laptop_seen)
                        attention=Signal::stored(Some(failed))
                        status=Signal::stored((BadgeState::Failed, failed_badge))
                        facts=Signal::stored(laptop_facts)
                        progress_label=laptop_label
                        progress=no_progress
                    >
                        {open_laptop}
                    </MachineCard>
                </li>
                <li class="m-0">
                    <MachineCard
                        localizer=desktop_localizer
                        hostname="demo-desktop".to_owned()
                        heading_id="machine-card-demo-desktop".to_owned()
                        level=TitleLevel::Three
                        connection=Signal::stored(online)
                        attention=none
                        status=Signal::stored((BadgeState::Running, running_badge))
                        facts=Signal::stored(desktop_facts)
                        progress_label=desktop_label
                        progress=Signal::stored(Some(running))
                    >
                        {open_desktop}
                    </MachineCard>
                </li>
            </ul>
        </section>
    }
}
