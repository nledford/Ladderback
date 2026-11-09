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
use leptos_router::components::{Route, Router, Routes};
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
#[cfg(all(test, target_arch = "wasm32"))]
mod formatting;
#[cfg(feature = "gallery")]
mod gallery;
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod live;
mod page;
mod routes;

use routes::{NotFound, Start};

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
    let arrival = Arc::clone(&localizer);
    let main = NodeRef::<leptos::html::Main>::new();
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
        <header class="site-header">
            <p class="brand">{text(&localizer, msg::UI_BRAND)}</p>
        </header>
        <main id="main" tabindex="-1" node_ref=main>
            <Router>
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
                </Routes>
            </Router>
        </main>
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
