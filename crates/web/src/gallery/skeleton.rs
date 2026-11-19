//! The gallery's skeleton demo (technical design §4.5, "Tests").

use std::num::NonZeroU8;
use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::Content;
use crate::components::skeleton::rules::{Shape, Share};
use crate::components::{Button, Skeleton};
use crate::text;
use crate::variants::Rem;

/// The skeleton's demo (`#skeleton-demo`).
#[component]
pub(super) fn SkeletonDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The skeleton demo: whether its plan has come, which its toggle sets,
    // the words that say the wait, and the plan's own lines.
    let plan_shown = RwSignal::new(false);
    let (plan_loading, plan_name) = (
        t(msg::UI_GALLERY_SKELETON_LOADING),
        t(msg::UI_GALLERY_SKELETON_PLAN),
    );
    let (plan_folder, plan_schedule, plan_keeps) = (
        t(msg::UI_GALLERY_SKELETON_FOLDER),
        t(msg::UI_GALLERY_SKELETON_SCHEDULE),
        t(msg::UI_GALLERY_SKELETON_KEEPS),
    );
    // Its shapes: three lines standing for a paragraph (the plan's lines
    // too), the last one shorter, a block standing for a field, and two
    // lines standing for a short note.
    let (paragraph, field, note) = (
        Shape::Lines {
            // Three lines: one, and two more.
            count: NonZeroU8::MIN.saturating_add(2),
            last: Share::percent(60),
        },
        Shape::Block {
            inline: Rem::tenths(200),
            block: Rem::tenths(25),
        },
        Shape::Lines {
            // Two lines: one, and one more.
            count: NonZeroU8::MIN.saturating_add(1),
            last: Share::percent(40),
        },
    );
    view! {
        <section id="skeleton-demo">
            <DemoHeading text=t(msg::UI_GALLERY_SKELETON_HEADING) />
            // Three lines standing for a paragraph, the last one shorter,
            // and a block standing for a field, each in the room its line
            // of the page gives it.
            <div class="my-4">
                <Skeleton shape=paragraph />
            </div>
            <div class="my-4">
                <Skeleton shape=field />
            </div>
            // On Ledger Gray, as in a toast or the pause banner, where the
            // bars stand apart from it as they do from the page.
            <div class="bg-surface my-4 p-4">
                <Skeleton shape=note />
            </div>
            // A region whose content is on its way: busy, with its skeleton,
            // until the toggle below brings the content, three lines in the
            // place of the skeleton's three, so the region keeps its height
            // and focus stays on the toggle. The wait is said in words on
            // the line just before the region, outside it, since a screen
            // reader may leave a busy region unread; the plan's name takes
            // that line when the plan comes. Nothing announces the arrival.
            <p>
                {move || {
                    if plan_shown.get() { plan_name.clone() } else { plan_loading.clone() }
                }}
            </p>
            <div aria-busy=move || {
                if plan_shown.get() { "false" } else { "true" }
            }>
                {move || {
                    if plan_shown.get() {
                        view! {
                            <p class="my-0">
                                {plan_folder.clone()} <br /> {plan_schedule.clone()} <br />
                                {plan_keeps.clone()}
                            </p>
                        }
                            .into_any()
                    } else {
                        view! { <Skeleton shape=paragraph /> }.into_any()
                    }
                }}
            </div>
            <p>
                <Button
                    content=Content::Text(t(msg::UI_GALLERY_SKELETON_SHOW).into())
                    pressed=plan_shown
                    on_press=Callback::new(move |()| plan_shown.update(|shown| *shown = !*shown))
                />
            </p>
        </section>
    }
}
