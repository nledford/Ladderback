//! The gallery's card demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::card::CARDS;
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::{Button, Card, Link, TitleLevel};
use crate::text;

/// The card's demo (`#card-demo`): two cards in the list cards stand in.
/// One of words alone, with no actions, so Tab passes it by; and one whose
/// actions, a button and a link, are the card's only stops for Tab.
#[component]
pub(super) fn CardDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let t = |id| text(&localizer, id);
    let heading = t(msg::UI_GALLERY_CARD_HEADING);
    let (plain, plain_words) = (
        t(msg::UI_GALLERY_CARD_PLAIN),
        t(msg::UI_GALLERY_CARD_PLAIN_WORDS),
    );
    let (controls, controls_words) = (
        t(msg::UI_GALLERY_CARD_CONTROLS),
        t(msg::UI_GALLERY_CARD_CONTROLS_WORDS),
    );
    let (action, open) = (t(msg::UI_GALLERY_CARD_ACTION), t(msg::UI_GALLERY_CARD_OPEN));
    let link_localizer = Arc::clone(&localizer);
    let link = Destination::route("/", Matching::Exact).map(move |to| {
        view! {
            <Link to=to localizer=link_localizer placement=LinkPlacement::Alone>
                {open}
            </Link>
        }
    });
    view! {
        <section id="card-demo">
            <DemoHeading text=heading />
            <ul class=CARDS role="list">
                <li class="m-0">
                    <Card
                        heading_id="card-demo-plain".to_owned()
                        level=TitleLevel::Three
                        heading=move || plain
                    >
                        <p class="m-0">{plain_words}</p>
                    </Card>
                </li>
                <li class="m-0">
                    <Card
                        heading_id="card-demo-controls".to_owned()
                        level=TitleLevel::Three
                        heading=move || controls
                        actions=move || {
                            view! {
                                <Button content=Content::Text(action.into()) />
                                {link}
                            }
                        }
                    >
                        <p class="m-0">{controls_words}</p>
                    </Card>
                </li>
            </ul>
        </section>
    }
}
