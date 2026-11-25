//! The gallery's navigation bar demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, LinkPlacement, LinkTone, Matching};
use crate::components::{Button, Link, NavigationBar, NavigationPage};
use crate::text;

/// The navigation bar's demo (`#navigation-bar-demo`): a bar as the
/// header's, its brand a link to the start page, its pages a link to this
/// page, so marked as the current one, and a link to the Plans page, its
/// group a link to the Log and a button under the hairline; then a button
/// after it, where focus goes when it leaves the bar. From 54rem up it is
/// one line; below, its rows sit behind its hamburger, "Demo menu". The
/// button keeps 14rem of room under it, so the open panel at 390 px covers
/// it alone and never part of the next demo's controls, whose targets would
/// shrink under it (2.5.8).
#[component]
pub(super) fn NavigationBarDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let t = |id| text(&localizer, id);
    let page = |path: &str, words: String| {
        Destination::route(path, Matching::Section).map(|to| NavigationPage { words, to })
    };
    let pages = [
        page("/_gallery", t(msg::UI_GALLERY_NAVIGATION_BAR_HERE)),
        page("/plans", t(msg::UI_GALLERY_NAVIGATION_BAR_PLANS)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let group_pages = page("/log", t(msg::UI_GALLERY_NAVIGATION_BAR_LOG))
        .into_iter()
        .collect();
    let action = t(msg::UI_GALLERY_NAVIGATION_BAR_ACTION);
    let brand = t(msg::UI_GALLERY_NAVIGATION_BAR_BRAND);
    let heading = t(msg::UI_GALLERY_NAVIGATION_BAR_HEADING);
    let label = t(msg::UI_GALLERY_NAVIGATION_BAR_LABEL);
    let menu_label = t(msg::UI_GALLERY_NAVIGATION_BAR_MENU);
    let group_label = t(msg::UI_GALLERY_NAVIGATION_BAR_GROUP);
    let after = t(msg::UI_GALLERY_NAVIGATION_BAR_AFTER);
    let brand_localizer = Arc::clone(&localizer);
    view! {
        <section id="navigation-bar-demo">
            <DemoHeading text=heading />
            <NavigationBar
                localizer=Arc::clone(&localizer)
                panel_id="demo-navigation".to_owned()
                label=label
                menu_label=menu_label
                shows_pages=true
                pages=pages
                group_label=group_label
                group_pages=group_pages
                group_end=move || {
                    view! {
                        <Button
                            content=Content::Text(action.clone().into())
                            appearance=Appearance::Ghost
                        />
                    }
                }
            >
                <p class="m-0 font-semibold">
                    {Destination::route("/", Matching::Exact)
                        .map(|to| {
                            view! {
                                <Link
                                    to=to
                                    localizer=Arc::clone(&brand_localizer)
                                    tone=LinkTone::Inherit
                                    placement=LinkPlacement::Alone
                                >
                                    {brand}
                                </Link>
                            }
                        })}
                </p>
            </NavigationBar>
            <div class="mt-2 mb-56">
                <Button content=Content::Text(after.into()) />
            </div>
        </section>
    }
}
