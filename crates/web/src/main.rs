//! Ladderback's web UI (technical design §4.4), a Leptos client-side app
//! built with trunk and embedded in the `ladderback` binary.
//!
//! The page skeleton every page shares: a skip link, landmarks and a single
//! `<h1>` per route (§4.5). Every visible text comes out of the `l10n`
//! catalog, in the language [`page`] negotiates (§8.4); none is written here.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::sync::Arc;

use l10n::{Localizer, MessageId, msg};
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;

// Nothing but the gallery announces yet: the first page that does is a later
// slice's, and until then a build without the gallery leaves these unused.
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod announcer;
// The shared component set, exercised by the gallery in a `gallery` build; a
// build without the gallery has no user for it yet.
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod components;
// The focus-wrap decision is host-testable; components.rs (the DOM glue
// around it) runs in a browser via the gallery's @ui scenarios.
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod focus;
// The wasm32-only test modules (`just test-wasm` runs them under Node) are
// declared with `cfg(test)` and `cfg(target_arch)` as two attributes, not as
// `cfg(all(test, ...))`: cargo-mutants skips a module under a plain
// `cfg(test)` and does not look inside `all(...)`, so it would mutate tests
// no host can run, and every one of those mutants would "survive".
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod formatting;
#[cfg(feature = "gallery")]
mod gallery;
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod live;
// The live pages' decisions (host-tested) and their DOM glue; see
// `backend.rs` for why the fetch-and-socket glue is only covered by the
// browser journeys.
mod backend;
mod maintenance;
// What a row of the Operations page says, decided on the host.
mod operations;
mod page;
mod repositories;
mod routes;
// The Snapshots page's DOM glue; the decisions it sends and shows are
// `maintenance.rs` and `wording.rs`.
mod snapshots;
// Declared as `formatting` is, for the same reason.
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod time_zones;
// The live pages' fetch and WebSocket glue; see `backend.rs` for why this
// one is only covered by the browser journeys.
mod ws;
// What the Snapshots page says, decided on the host.
mod wording;

// The toast's arrival and lifetime decisions are host-testable; the DOM
// glue around them runs in a browser via the gallery's @ui scenarios.
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod toast;

use routes::{NotFound, Operations, Repositories, Start};
use snapshots::Snapshots;

/// The text of message `id` in the page's language.
fn text(localizer: &Localizer, id: MessageId) -> String {
    localizer.message(id, &[]).into_text()
}

/// The page of `/_gallery`: the component gallery in a build that has it, and
/// a path nothing serves in one that doesn't.
fn gallery_page(localizer: Arc<Localizer>) -> AnyView {
    #[cfg(feature = "gallery")]
    {
        view! { <gallery::Gallery localizer=localizer /> }.into_any()
    }
    #[cfg(not(feature = "gallery"))]
    {
        view! { <NotFound localizer=localizer /> }.into_any()
    }
}

/// Watches the router: each path the reader navigates to after the first is
/// titled (`page::retitle`). The first path is the page load, which
/// `page::declare` titled. Focus is the page heading's (`routes::PageHeading`).
#[component]
fn Arrival(localizer: Arc<Localizer>) -> impl IntoView {
    let pathname = use_location().pathname;
    Effect::new(move |previous: Option<()>| {
        let path = pathname.get();
        if previous.is_some() {
            page::retitle(&localizer, &path);
        }
    });
}

#[component]
fn App(localizer: Arc<Localizer>) -> impl IntoView {
    let not_found = Arc::clone(&localizer);
    let start = Arc::clone(&localizer);
    let gallery = Arc::clone(&localizer);
    let operations = Arc::clone(&localizer);
    let snapshots = Arc::clone(&localizer);
    let repositories = Arc::clone(&localizer);
    let arrival = Arc::clone(&localizer);
    let main = NodeRef::<leptos::html::Main>::new();
    let (nav_label, operations_link, snapshots_link, repositories_link) = (
        text(&localizer, msg::UI_NAV_LABEL),
        text(&localizer, msg::UI_NAV_OPERATIONS),
        text(&localizer, msg::UI_NAV_SNAPSHOTS),
        text(&localizer, msg::UI_NAV_REPOSITORIES),
    );
    view! {
        // The skip link moves focus into `<main>` itself and stops the click
        // there. The router takes any click on a same-origin link it was not
        // told to leave alone, and only scrolls to the target, so focus would
        // stay where it was. A handler that has already stopped the click is
        // left alone by the router, and the address stays as it was.
        <a
            class="skip-link"
            href="#main"
            on:click=move |event| {
                event.prevent_default();
                if let Some(main) = main.get() {
                    let _ = main.focus();
                }
            }
        >
            {text(&localizer, msg::UI_SKIP_LINK)}
        </a>
        <Router>
            <header class="site-header">
                <p class="brand">{text(&localizer, msg::UI_BRAND)}</p>
                <nav class="site-nav" aria-label=nav_label>
                    <ul>
                        <li><A href="/operations">{operations_link}</A></li>
                        <li><A href="/snapshots">{snapshots_link}</A></li>
                        <li><A href="/repositories">{repositories_link}</A></li>
                    </ul>
                </nav>
            </header>
            <main id="main" tabindex="-1" node_ref=main>
                <Arrival localizer=arrival />
                <Routes fallback=move || view! { <NotFound localizer=Arc::clone(&not_found) /> }>
                    <Route
                        path=path!("/")
                        view=move || view! { <Start localizer=Arc::clone(&start) /> }
                    />
                    <Route
                        path=path!("/_gallery")
                        view=move || gallery_page(Arc::clone(&gallery))
                    />
                    <Route
                        path=path!("/operations")
                        view=move || view! { <Operations localizer=Arc::clone(&operations) /> }
                    />
                    <Route
                        path=path!("/snapshots")
                        view=move || view! { <Snapshots localizer=Arc::clone(&snapshots) /> }
                    />
                    <Route
                        path=path!("/repositories")
                        view=move || view! { <Repositories localizer=Arc::clone(&repositories) /> }
                    />
                </Routes>
            </main>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    let localizer = Arc::new(page::localizer());
    page::declare(&localizer);
    // The app mounts into `#app`, beside the live regions of `index.html`, so
    // that Leptos never renders over them.
    let Some(app) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("app"))
        .and_then(|app| app.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        leptos::logging::error!("index.html has no #app to mount the app into");
        return;
    };
    leptos::mount::mount_to(app, move || view! { <App localizer=localizer /> }).forget();
}
