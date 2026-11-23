//! The gallery's disclosure menu demo (technical design §4.5, "Tests").

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos_router::hooks::use_location;

use super::DemoHeading;
use crate::components::button::{Appearance, Content};
use crate::components::link::rules::{Destination, LinkPlacement, Matching};
use crate::components::{Button, DisclosureMenu, Link};
use crate::text;

/// The disclosure menu's demo (`#disclosure-menu-demo`): two menus at the
/// end of a row, the last of them where the header's Settings menu stands,
/// then a button after them, where focus goes when it leaves the last. The
/// first menu's row is a link to the Log, so its button isn't marked; the
/// last's rows are a link to this page, so its button is marked as the
/// current page's group, a link to the start page and a button.
#[component]
pub(super) fn DisclosureMenuDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let t = |id| text(&localizer, id);
    let row = |path: &str, words: String| {
        Destination::route(path, Matching::Exact).map(|to| {
            view! {
                <li>
                    <Link to=to localizer=Arc::clone(&localizer) placement=LinkPlacement::Alone>
                        {words}
                    </Link>
                </li>
            }
        })
    };
    let location = use_location();
    let here = Destination::route("/_gallery", Matching::Exact);
    let holds_this_page = Signal::derive(move || {
        here.as_ref().is_some_and(|here| {
            here.current(&location.pathname.get(), &location.search.get())
                .is_some()
        })
    });
    let here_row = row("/_gallery", t(msg::UI_GALLERY_DISCLOSURE_MENU_HERE));
    let start_row = row("/", t(msg::UI_GALLERY_DISCLOSURE_MENU_START));
    let action = t(msg::UI_GALLERY_DISCLOSURE_MENU_ACTION);
    let log_row = row("/log", t(msg::UI_GALLERY_DISCLOSURE_MENU_LOG));
    view! {
        <section id="disclosure-menu-demo">
            <DemoHeading text=t(msg::UI_GALLERY_DISCLOSURE_MENU_HEADING) />
            <div class="flex flex-wrap justify-end gap-2">
                <DisclosureMenu
                    label=t(msg::UI_GALLERY_DISCLOSURE_MENU_OTHER)
                    panel_id="demo-other-pages-menu".to_owned()
                    current=false
                >
                    <ul>{log_row}</ul>
                </DisclosureMenu>
                <DisclosureMenu
                    label=t(msg::UI_GALLERY_DISCLOSURE_MENU_PAGES)
                    panel_id="demo-pages-menu".to_owned()
                    current=holds_this_page
                >
                    <ul>{here_row}{start_row}</ul>
                    <div class="border-bs border-field-edge mt-1 pt-1 px-2">
                        <Button content=Content::Text(action.into()) appearance=Appearance::Ghost />
                    </div>
                </DisclosureMenu>
            </div>
            <div class="mt-2">
                <Button content=Content::Text(t(msg::UI_GALLERY_DISCLOSURE_MENU_AFTER).into()) />
            </div>
        </section>
    }
}
