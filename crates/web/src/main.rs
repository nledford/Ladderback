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
use leptos::task::spawn_local;
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
// What a paused live page shows, counts and says, decided on the host.
mod pause;
// What a progress bar tells a screen reader, and when, decided on the host;
// `components.rs`'s `Progress` is its glue.
mod progress;
mod routes;
// What the page's standing with sign-in is, decided on the host.
mod session;
// The sign-in page and dialog's DOM glue; `session.rs` decides for them.
mod sign_in;
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
// A reader's visit to a live page, which the page's background work lasts
// for: host-tested, as `ws.rs`'s glue is not.
mod visit;
// What the Snapshots page says, decided on the host.
mod wording;

// The toast's arrival and lifetime decisions are host-testable; the DOM
// glue around them runs in a browser via the gallery's @ui scenarios.
#[cfg_attr(not(feature = "gallery"), allow(dead_code))]
mod toast;

use routes::{NotFound, Operations, Repositories, Start};
use session::{SignInOffNotice, Standing, Title};
use sign_in::{IdleWarning, SessionEnded, SignInPage, SignOut};
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

/// The pages of the app, each at its path, shown with sign-in off or once
/// the page has a session.
#[component]
fn Pages(localizer: Arc<Localizer>) -> impl IntoView {
    let not_found = Arc::clone(&localizer);
    let start = Arc::clone(&localizer);
    let gallery = Arc::clone(&localizer);
    let operations = Arc::clone(&localizer);
    let snapshots = Arc::clone(&localizer);
    let repositories = Arc::clone(&localizer);
    view! {
        <Arrival localizer=localizer />
        <Routes fallback=move || view! { <NotFound localizer=Arc::clone(&not_found) /> }>
            <Route path=path!("/") view=move || view! { <Start localizer=Arc::clone(&start) /> } />
            <Route path=path!("/_gallery") view=move || gallery_page(Arc::clone(&gallery)) />
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
    }
}

/// The site's navigation, shown wherever the pages are; while the page has a
/// session, it ends with the Sign out control.
#[component]
fn Navigation(localizer: Arc<Localizer>, standing: RwSignal<Standing>) -> impl IntoView {
    let (label, operations, snapshots, repositories) = (
        text(&localizer, msg::UI_NAV_LABEL),
        text(&localizer, msg::UI_NAV_OPERATIONS),
        text(&localizer, msg::UI_NAV_SNAPSHOTS),
        text(&localizer, msg::UI_NAV_REPOSITORIES),
    );
    view! {
        <nav class="site-nav" aria-label=label>
            <ul>
                <li>
                    <A href="/operations">{operations}</A>
                </li>
                <li>
                    <A href="/snapshots">{snapshots}</A>
                </li>
                <li>
                    <A href="/repositories">{repositories}</A>
                </li>
                {move || {
                    (standing.get() == Standing::SignedIn)
                        .then(|| {
                            view! {
                                <li>
                                    <SignOut localizer=Arc::clone(&localizer) standing=standing />
                                </li>
                            }
                        })
                }}
            </ul>
        </nav>
    }
}

/// Asks the daemon where the page stands with sign-in (security §10.1), then
/// shows the pages, or the sign-in page in their place. A request refused
/// later for want of a session opens the dialog that asks to sign in again,
/// over the page as it is. With sign-in off on the network, every page says
/// that anyone who can reach Ladderback can use it.
#[component]
fn App(localizer: Arc<Localizer>) -> impl IntoView {
    let standing = RwSignal::new(Standing::Asking);
    // Only a change of standing redraws the page: a page shown stays as it is.
    let shown = Memo::new(move |_| standing.get());
    let notice = RwSignal::new(SignInOffNotice::None);
    let warning = RwSignal::new(false);
    let ended = RwSignal::new(false);
    // The build the daemon runs, once it has told the page (decision NN).
    let build = RwSignal::new(None::<api::BuildDto>);
    provide_context(standing);
    provide_context(notice);
    ws::watch_session(warning, ended, build);
    spawn_local(async move {
        // A live session's CSRF token and lease are the page's once read.
        let read = ws::session().await;
        // The notice first, so the pages it shows on are drawn with it.
        notice.set(SignInOffNotice::of(read.as_ref()));
        standing.set(Standing::of(read.as_ref()));
    });
    // The sign-in page has a title of its own; once signed in, the page asked
    // for has the title its address names.
    let titles = Arc::clone(&localizer);
    Effect::new(move |previous: Option<Standing>| {
        let now = shown.get();
        if let Some(title) = Title::on(previous, now) {
            page::retitle_as(&titles, title);
        }
        now
    });
    let navigation = Arc::clone(&localizer);
    // The build the daemon says it runs: the web UI is built into it, so it
    // is the page's too. Until the daemon has said, nothing is shown.
    let versions = Arc::clone(&localizer);
    let version = move || {
        build.get().map(|build| {
            let version = routes::text_with(
                &versions,
                msg::UI_VERSION,
                &[("version", api::MsgArg::Text(build.display()))],
            );
            view! { <p class="site-version">{version}</p> }
        })
    };
    let pages = Arc::clone(&localizer);
    let dialog = Arc::clone(&localizer);
    let idle = Arc::clone(&localizer);
    let main = NodeRef::<leptos::html::Main>::new();
    let on_the_network = text(&localizer, msg::UI_SIGN_IN_OFF_NETWORK);
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
                {move || {
                    shown
                        .get()
                        .shows_pages()
                        .then(|| {
                            view! {
                                <Navigation localizer=Arc::clone(&navigation) standing=standing />
                                {version.clone()}
                            }
                        })
                }}
            </header>
            <main id="main" tabindex="-1" node_ref=main>
                {move || {
                    (notice.get() == SignInOffNotice::EveryPage)
                        .then(|| {
                            view! {
                                <p id=routes::SIGN_IN_OFF_NETWORK_ID>{on_the_network.clone()}</p>
                            }
                        })
                }}
                {move || match shown.get() {
                    Standing::Asking => ().into_any(),
                    Standing::SignedOut => {
                        view! { <SignInPage localizer=Arc::clone(&pages) standing=standing /> }
                            .into_any()
                    }
                    Standing::Off | Standing::SignedIn | Standing::Unread => {
                        view! { <Pages localizer=Arc::clone(&pages) /> }.into_any()
                    }
                }}
            </main>
            {move || {
                ended
                    .get()
                    .then(|| view! { <SessionEnded localizer=Arc::clone(&dialog) ended=ended /> })
            }}
            <IdleWarning localizer=idle warning=warning />
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
