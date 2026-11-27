//! The machine card (components §4.5; §16, decision 175): one machine at a
//! glance on the hub's dashboard. What it tells, and why a machine needs
//! attention, is `dashboard.rs`'s, decided on the host; this is the glue
//! that draws it.

use std::sync::Arc;

use api::{IanaZone, MsgArg};
use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::status_badge::rules::BadgeState;
use super::{Card, Icon, Progress, StatusBadge, TitleLevel};
use crate::backend;
use crate::repositories::{Run, runs};
use crate::routes::{text_with, time_view};
use crate::text;
use crate::variants::{Size, Tone};

/// One fact a card tells: its term, and its words, with the instant
/// (RFC 3339) they write when they are a time, which a `<time>` carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardFact {
    pub term: String,
    pub words: String,
    pub time: Option<String>,
}

/// A machine card (§4.5, decision 175): the shared [`Card`], named by its heading,
/// the machine's `hostname`, which `this_hub` marks "This hub" in words
/// under it. Then, in this order:
///
/// - **The attention mark**, while `attention` holds the reason: a
///   paragraph, first after the heading, that says "Needs attention:" in
///   bold, then the reason's sentence, one message with the reason in it
///   (§8.4), after a Warning icon a screen reader doesn't hear. Never color
///   alone, never a colored edge or fill.
/// - **The connection's line** (`connection`), online now or when it last
///   connected.
/// - **The status**, in a status badge (`status`).
/// - **The progress** of the operation the machine runs, while `progress`
///   holds one, in the shared progress bar named `progress_label`, which
///   is never announced (decision 83) and hands focus to the heading when
///   it goes.
/// - **The facts**, a description list (`facts`).
/// - **The controls**, `children`: the caller's links and menus, the
///   card's actions.
///
/// The card itself is never a stop for Tab, nor a control: its controls
/// and its progress bar are. Its heading takes focus from a script
/// (`tabindex="-1"`), at `level`. It is flat, as every [`Card`] is. Cards
/// stand in a list the caller lays out with `CARDS`: one column on a
/// narrow page, several side by side on a wide one.
///
/// What the caller owes WCAG 2.2 AA: a `heading_id` unique on the page, the
/// heading's level in the page's outline, and controls named by the
/// machine they act on (2.4.6), as "Open demo-laptop" is.
#[component]
pub fn MachineCard(
    localizer: Arc<Localizer>,
    hostname: String,
    heading_id: String,
    #[prop(optional)] level: TitleLevel,
    #[prop(optional)] this_hub: bool,
    #[prop(into)] connection: Signal<String>,
    #[prop(into)] attention: Signal<Option<String>>,
    #[prop(into)] status: Signal<(BadgeState, String)>,
    #[prop(into)] facts: Signal<Vec<CardFact>>,
    progress_label: String,
    #[prop(into)] progress: Signal<Option<backend::Progress>>,
    #[prop(default = None)] zone: Option<IanaZone>,
    children: Children,
) -> impl IntoView {
    let mark_localizer = Arc::clone(&localizer);
    let hub_mark = this_hub.then(|| {
        let words = text(&localizer, msg::UI_MACHINE_CARD_THIS_HUB);
        view! {
            " "
            <span class="block text-base font-normal text-muted">{words}</span>
        }
    });
    let name = view! {
        <bdi>{hostname}</bdi>
        {hub_mark}
    };
    let then_focus = heading_id.clone();
    let mark = move || {
        attention.get().map(|reason| {
            // The catalog's words around the reason, the placeable it
            // isolates, are the mark's lead: in bold, whichever side of the
            // reason a language puts them.
            let words = text_with(
                &mark_localizer,
                msg::UI_MACHINE_CARD_ATTENTION,
                &[("reason", MsgArg::Text(reason))],
            );
            let lead = runs(&words)
                .into_iter()
                .map(|run| match run {
                    Run::Words(words) => view! { <strong>{words.to_owned()}</strong> }.into_any(),
                    Run::Placeable(reason) => reason.to_owned().into_any(),
                })
                .collect_view();
            view! {
                <p class="flex items-start gap-2 m-0">
                    <Icon glyph=LucideGlyph::TriangleAlert tone=Tone::Warning size=Size::Medium />
                    <span>{lead}</span>
                </p>
            }
        })
    };
    let running = Memo::new(move |_| progress.with(Option::is_some));
    let bar = move || {
        running.get().then(|| {
            view! {
                <Progress
                    localizer=Arc::clone(&localizer)
                    label=progress_label.clone()
                    then_focus=then_focus.clone()
                    reading=Signal::derive(move || progress.get().unwrap_or_default())
                    zone=zone.clone()
                />
            }
        })
    };
    let list = move || {
        facts
            .get()
            .into_iter()
            .map(|fact| {
                view! {
                    <div class="flex flex-wrap gap-x-2">
                        <dt class="text-muted">{fact.term}</dt>
                        <dd class="m-0 [min-inline-size:0] wrap-anywhere">
                            {time_view(fact.words, fact.time)}
                        </dd>
                    </div>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <Card heading_id=heading_id level=level heading=move || name actions=children>
            {mark}
            <p class="m-0">{connection}</p>
            <p class="m-0">
                <StatusBadge
                    state=Signal::derive(move || status.get().0)
                    label=Signal::derive(move || status.get().1)
                />
            </p>
            {bar}
            <dl class="flex flex-col gap-1 m-0">{list}</dl>
        </Card>
    }
}
