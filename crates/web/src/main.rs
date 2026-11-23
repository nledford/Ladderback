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
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::use_location;
use leptos_router::path;

// What a screen reader is told about the page's own changes, and when.
mod announcer;
// The shared component set, exercised by the gallery in a `gallery` build; a
// build without the gallery has no user for it yet, nor for the re-exports
// of its root.
#[cfg_attr(
    not(feature = "gallery"),
    expect(
        dead_code,
        unused_imports,
        reason = "only the gallery uses the shared components yet"
    )
)]
mod components;
// The start page's notice of a data folder copied from another computer.
mod copied_data_folder;
// What the Log page lists and copies, decided on the host;
// `daemon_log_page.rs` is its glue.
mod daemon_log;
// The Log page's DOM glue; `daemon_log.rs` decides for it.
mod daemon_log_page;
// What downloading an entry of a snapshot sends and says, decided on the
// host; `snapshot_page.rs` is its glue.
mod download;
// The focus-wrap decision is host-testable; components.rs (the DOM glue
// around it) runs in a browser via the gallery's @ui scenarios.
mod focus;
// The wasm32-only test modules (`just test-wasm` runs them under Node) are
// declared with `cfg(test)` and `cfg(target_arch)` as two attributes, not as
// `cfg(all(test, ...))`: cargo-mutants skips a module under a plain
// `cfg(test)` and does not look inside `all(...)`, so it would mutate tests
// no host can run, and every one of those mutants would "survive".
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod formatting;
// What the Full Disk Access warning, a banner on every page with the steps
// on the start page, shows and says, decided on the host;
// `full_disk_access_warning.rs` is its glue.
mod full_disk_access;
// The Full Disk Access warning's DOM glue: the banner and the steps.
mod full_disk_access_warning;
#[cfg(feature = "gallery")]
mod gallery;
// What Settings' Language section offers, shows chosen and keeps, decided on
// the host; `settings/language.rs` is its glue.
mod language_choice;
// The live regions' glue around the announcer.
mod live;
// What the admin password's, the API tokens' and the dry-run allowlist's
// sections of Settings decide, host-tested; `settings.rs` is their glue.
mod admin_password;
mod api_tokens;
mod dry_run_allowlist;
// The live pages' decisions (host-tested) and their DOM glue; see
// `backend.rs` for why the fetch-and-socket glue is only covered by the
// browser journeys.
mod backend;
mod maintenance;
// What the header's menu decides on a narrow screen, host-tested;
// `MenuButton` and `Navigation` below are its glue.
mod navigation;
// What a row of the Operations page says, decided on the host.
mod operations;
// The Operations page: its DOM glue, and the decisions not yet moved out of
// it (#739), host-tested there; `operations.rs`, `backend.rs` and `pause.rs`
// decide the rest.
mod operations_page;
mod page;
// What Settings' Page width section offers, keeps and lays the page out
// with, decided on the host; `settings/page_width.rs` is its glue.
mod page_width;
mod repositories;
// The Repositories page: its DOM glue, and its form's state, host-tested
// there; `repositories.rs` decides the rest.
mod repositories_page;
// What a paused live page shows, counts and says, decided on the host.
mod pause;
// What the Machines tab's lists show while paused, count and say, decided on
// the host; `settings/machines.rs` is their glue. Only a build with the hub
// has it (decision 122).
#[cfg(feature = "hub")]
mod machines_list;
// What the hub's dashboard shows while paused, counts, says, reads again
// for and how its times read, decided on the host; `dashboard_page.rs` is
// its glue. Only a build with the hub has it (decision 122).
#[cfg(feature = "hub")]
mod dashboard;
// What the hub's pairing dialog decides, on the host; `settings/agents.rs`
// is its glue. Only a build with the hub has it (decision 122).
#[cfg(feature = "hub")]
mod pairing;
// What a panic writes to the console: its place, never its message (#1411).
mod panic_report;
// What the Plans page shows, sends and says, decided on the host;
// `plans_page.rs` is its glue.
mod plans;
// The Plans page's DOM glue; `plans.rs` decides for it.
mod plans_page;
// What the "Pending changes" page shows, sends and says, decided on the
// host; `proposals_page.rs` is its glue.
mod proposals;
// The "Pending changes" page's DOM glue; `proposals.rs` decides for it.
mod proposals_page;
// What the restart section of Settings offers, says and waits for, decided
// on the host; `settings.rs` is its glue.
mod restart;
// What restoring an entry of a snapshot sends, waits for and says, decided
// on the host; `snapshot_page.rs` is its glue.
mod restore;
mod routes;
// The Settings page's DOM glue; `api_tokens.rs` decides for it.
mod settings;
// Settings' tabs, the address that opens each and what each panel holds,
// decided on the host; `settings.rs` is its glue.
mod settings_tabs;
// What the page's standing with sign-in is, decided on the host.
mod session;
// The sign-in page and dialog's DOM glue; `session.rs` decides for them.
mod sign_in;
// The choices of Settings' Signing in section, decided on the host;
// `settings/sign_in.rs` is its glue.
mod sign_in_settings;
// The Snapshots page's DOM glue; the decisions it sends and shows are
// `maintenance.rs`, `stats.rs` and `wording.rs`.
mod snapshots;
// What a repository's stats section shows and says, decided on the host;
// `snapshots.rs` is its glue.
mod stats;
// Declared as `formatting` is, for the same reason.
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod time_zones;
// A snapshot's page's DOM glue; `components/tree/rules.rs` decides for its
// tree.
mod snapshot_page;
// The start page's DOM glue; `routes::next_run_cell` words a plan's next run.
mod start_page;
// The live pages' fetch and WebSocket glue; see `backend.rs` for why this
// one is only covered by the browser journeys.
mod ws;
// A reader's visit to a live page, which the page's background work lasts
// for: host-tested, as `ws.rs`'s glue is not.
mod visit;
// What the Snapshots page says, decided on the host.
mod wording;

// The shared vocabulary of tones, sizes and placements every shared
// component takes, mapped to `style.css`'s hooks on the host. Only the
// gallery draws with it yet, and the host tests check it, so only a build of
// neither leaves it unused.
#[cfg_attr(
    all(not(feature = "gallery"), not(test)),
    expect(dead_code, reason = "only the gallery draws with the vocabulary yet")
)]
mod variants;

use components::banner::rules::BannerTone;
use components::button::{Appearance, Content};
use components::link::rules::{Destination, LinkTone, Matching};
use components::{Banner, Button, Footer, Link, Tip, Tooltip, focus_element};
use daemon_log_page::DaemonLog;
use full_disk_access_warning::{FullDiskAccessBanner, FullDiskAccessFound};
use operations_page::Operations;
use page_width::PageWidth;
use plans_page::Plans;
use proposals_page::Proposals;
use repositories_page::Repositories;
use routes::NotFound;
use session::{SignInOffNotice, Standing, Title};
use settings::Settings;
use sign_in::{
    IdleWarning, SessionEnded, SignInPage, SignOut, SignOutFailure, admin_password_link,
};
use snapshot_page::Snapshot;
use snapshots::Snapshots;
use start_page::Start;
use variants::{Side, Size};

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
    let plans = Arc::clone(&localizer);
    let proposals = Arc::clone(&localizer);
    let snapshots = Arc::clone(&localizer);
    let snapshot = Arc::clone(&localizer);
    let repositories = Arc::clone(&localizer);
    let log = Arc::clone(&localizer);
    let settings = Arc::clone(&localizer);
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
                path=path!("/plans")
                view=move || view! { <Plans localizer=Arc::clone(&plans) /> }
            />
            <Route
                path=path!("/proposals")
                view=move || view! { <Proposals localizer=Arc::clone(&proposals) /> }
            />
            <Route
                path=path!("/snapshots")
                view=move || view! { <Snapshots localizer=Arc::clone(&snapshots) /> }
            />
            <Route
                path=path!("/snapshots/:alias/:snapshot")
                view=move || view! { <Snapshot localizer=Arc::clone(&snapshot) /> }
            />
            <Route
                path=path!("/repositories")
                view=move || view! { <Repositories localizer=Arc::clone(&repositories) /> }
            />
            <Route
                path=path!("/log")
                view=move || view! { <DaemonLog localizer=Arc::clone(&log) /> }
            />
            <Route
                path=path!("/settings")
                view=move || view! { <Settings localizer=Arc::clone(&settings) /> }
            />
        </Routes>
    }
}

/// The navigation's pages, in its order: each one's path and the message
/// that names it. A page's link is current on its path and on every page
/// below it (`Matching::Section`), as a snapshot's page is below Snapshots.
const PAGES: [(&str, MessageId); 7] = [
    ("/operations", msg::UI_NAV_OPERATIONS),
    ("/plans", msg::UI_NAV_PLANS),
    ("/proposals", msg::UI_NAV_PROPOSALS),
    ("/snapshots", msg::UI_NAV_SNAPSHOTS),
    ("/repositories", msg::UI_NAV_REPOSITORIES),
    ("/log", msg::UI_NAV_LOG),
    ("/settings", msg::UI_NAV_SETTINGS),
];

/// The pages' links, shown wherever the pages are, in the color of the text
/// around them: they leave Ladder Blue to the focus ring, the brand's mark
/// and an update's notice, so a page's own action is the first blue of its
/// own, and the current page is told by its weight (decision 141). Below
/// 41rem they sit in the menu's panel, behind the menu button, a disclosure
/// `navigation` decides for (decision 143). `open` is the button's state: a
/// page chosen from the links shows the panel closed, so the header is as
/// tall again as it was.
#[component]
fn Navigation(localizer: Arc<Localizer>, open: RwSignal<bool>) -> impl IntoView {
    let label = text(&localizer, msg::UI_NAV_LABEL);
    let pathname = use_location().pathname;
    Effect::new(move |previous: Option<()>| {
        pathname.track();
        if previous.is_some() {
            open.set(false);
        }
    });
    let links = PAGES
        .into_iter()
        .filter_map(|(path, name)| {
            let words = text(&localizer, name);
            Destination::route(path, Matching::Section).map(|to| {
                view! {
                    <li>
                        <Link to=to localizer=Arc::clone(&localizer) tone=LinkTone::Inherit>
                            {words}
                        </Link>
                    </li>
                }
            })
        })
        .collect_view();
    view! {
        <nav class="site-nav" aria-label=label>
            <ul>{links}</ul>
        </nav>
    }
}

/// The header's menu button, the hamburger that below 41rem ends the brand's
/// line and shows the menu's panel, and is drawn only there (`style.css`).
/// Icon-only, so its name ("Menu", the same in both states) is its label and
/// its tooltip, as every icon-only button's is; the icon is a hamburger
/// while the panel is hidden and an X while it shows. A disclosure:
/// `aria-expanded` says whether the panel shows, and `aria-controls` names
/// it. XLarge, 3em, so it is a finger's target (44 px) at the page's own text
/// size. The tooltip stands at the button's inline start, where it covers
/// neither the panel nor the page's content under the header.
#[component]
fn MenuButton(localizer: Arc<Localizer>, open: RwSignal<bool>) -> impl IntoView {
    let menu = text(&localizer, msg::UI_MENU_BUTTON);
    view! {
        <div class="site-menu-button">
            <Tooltip tip=Tip::Names(menu.clone()) side=Side::Start>
                <Button
                    id=navigation::MENU_BUTTON_ID.to_owned()
                    content=Content::IconOnly {
                        glyph: Signal::derive(move || navigation::glyph(open.get())),
                        label: Signal::stored(menu),
                    }
                    appearance=Appearance::Ghost
                    size=Size::XLarge
                    expanded=open
                    controls=navigation::MENU_ID.to_owned()
                    on_press=Callback::new(move |()| {
                        open.update(|open| *open = navigation::toggled(*open))
                    })
                />
            </Tooltip>
        </div>
    }
}

/// Whether the menu button is drawn now: below 41rem, where `style.css`
/// draws it, and only there (a `display: none` box has no size). Read from
/// the box, not from the breakpoint, so the glue never repeats the CSS.
fn menu_button_is_drawn() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(navigation::MENU_BUTTON_ID))
        .is_some_and(|button| {
            let drawn = button.get_bounding_client_rect();
            navigation::is_drawn(drawn.width(), drawn.height())
        })
}

/// The end of the header's line, after the pages' links: while the page has a
/// session, Sign out, which is not a page and so stands apart from their
/// list, and below 41rem under them in the menu's panel. With no session
/// there is nothing at the line's end, not an empty box that still takes the
/// line's gap. The running version is the footer's (decision 142).
#[component]
fn Account(
    localizer: Arc<Localizer>,
    standing: RwSignal<Standing>,
    sign_out_failed: RwSignal<bool>,
) -> impl IntoView {
    move || {
        (standing.get() == Standing::SignedIn).then(|| {
            view! {
                <div class="site-account">
                    <SignOut
                        localizer=Arc::clone(&localizer)
                        standing=standing
                        failed=sign_out_failed
                    />
                </div>
            }
        })
    }
}

/// The product's mark, beside its name in the header: the master icon's
/// shapes (`assets/brand/icon.svg`), filled from the theme's tokens by
/// `style.css`, so it follows the light and dark colors. It is decoration:
/// the link it sits in is named by the product's name alone.
#[component]
fn BrandMark() -> impl IntoView {
    view! {
        <svg
            class="brand-mark"
            xmlns="http://www.w3.org/2000/svg"
            width="1em"
            height="1em"
            viewBox="0 0 512 512"
            aria-hidden="true"
            focusable="false"
        >
            <rect class="brand-mark-tile" width="512" height="512" rx="112" />
            <g class="brand-mark-slats">
                <rect x="156.8" y="96" width="44" height="320" rx="22" />
                <rect x="311.2" y="96" width="44" height="320" rx="22" />
                <rect x="178.8" y="140.8" width="154.4" height="40" rx="20" />
                <rect x="178.8" y="236.8" width="154.4" height="40" rx="20" />
                <rect x="178.8" y="332.8" width="154.4" height="40" rx="20" />
            </g>
        </svg>
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
    // Only a change shows or takes away the dialog that asks to sign in
    // again: a request refused while it is open leaves it as it is.
    let asking = Memo::new(move |_| ended.get());
    // What the daemon runs, and the update it found, once it has told the
    // page (decision 90, decision 107); the Log page heads a bug report with
    // the build.
    let told = RwSignal::new(None::<session::Told>);
    provide_context(told);
    provide_context(standing);
    // The width chosen in this browser, in effect from the first render;
    // Settings' Page width section changes it.
    let width = RwSignal::new(PageWidth::from_kept(page::stored_page_width().as_deref()));
    provide_context(width);
    provide_context(notice);
    provide_context(sign_in::PasswordChanged(RwSignal::new(false)));
    // Offered until a signed-out answer says otherwise, as a daemon from
    // before the choice reads it.
    let keep_offered = RwSignal::new(true);
    provide_context(sign_in::KeepOffered(keep_offered));
    // What the agent's Full Disk Access probe found, which the banner reads
    // on each page and the start page's steps check again (decision 115).
    provide_context(FullDiskAccessFound(RwSignal::new(None)));
    ws::watch_session(warning, ended, told, keep_offered);
    // The hub's key, on a daemon serving as a hub, with the standing it was
    // read in: Settings shows its Machines tab once it is read (decision
    // 121).
    #[cfg(feature = "hub")]
    let hub = RwSignal::new(None::<(Standing, Option<api::HubDto>)>);
    #[cfg(feature = "hub")]
    provide_context(settings::agents::HubKey(hub));
    spawn_local(async move {
        // A live session's CSRF token and lease are the page's once read.
        let read = ws::session().await;
        // The notice first, so the pages it shows on are drawn with it.
        notice.set(SignInOffNotice::of(read.as_ref()));
        standing.set(Standing::of(read.as_ref()));
    });
    // The page reads the hub's key once for each standing it is in, with
    // sign-in off or in a session, as the hub answers no one else; in any
    // other it has none. A read answered once the standing has changed
    // again is dropped, so an old answer never stands for the new one.
    #[cfg(feature = "hub")]
    {
        let reads = StoredValue::new(0_u64);
        Effect::new(move |_| {
            let now = shown.get();
            let read = reads.get_value().wrapping_add(1);
            reads.set_value(read);
            match now {
                Standing::Asking => {}
                Standing::Off | Standing::SignedIn => spawn_local(async move {
                    let key = settings::agents::read_hub().await;
                    if reads.try_get_value() == Some(read) {
                        hub.try_set(Some((now, key)));
                    }
                }),
                Standing::SignedOut | Standing::Unread => hub.set(Some((now, None))),
            }
        });
    }
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
    let account = Arc::clone(&localizer);
    let menu_button = Arc::clone(&localizer);
    // Whether the menu button shows the menu's panel (the pages' links and
    // Sign out) below 41rem (decision 143). Any key pressed in the header
    // can hide it: Escape, which then returns focus to the button, while the
    // button is drawn.
    let menu_open = RwSignal::new(false);
    let hide_menu = move |event: web_sys::KeyboardEvent| {
        if !navigation::escape_closes(
            menu_open.get_untracked(),
            &event.key(),
            menu_button_is_drawn(),
        ) {
            return;
        }
        // An Escape that hides a tooltip has done its job (the tooltip's own
        // rule, WCAG 1.4.13): it leaves the panel, and focus, where they are.
        let tooltip_shown = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| {
                document
                    .query_selector(".tooltip:popover-open")
                    .ok()
                    .flatten()
            })
            .is_some();
        if !tooltip_shown {
            menu_open.set(false);
            focus_element(navigation::MENU_BUTTON_ID);
        }
    };
    // A link chosen in the panel hides it, the current page's too, which
    // changes no route for the router to close it on.
    let hide_menu_on_link = move |event: web_sys::MouseEvent| {
        let on_link = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|target| target.closest("a").ok().flatten().is_some());
        if on_link {
            menu_open.set(false);
        }
    };
    // The header's pages go with a sign-out, and the button's state with
    // them: the next sign-in finds the panel hidden, whatever it was.
    Effect::new(move |_| {
        if navigation::hidden_without_pages(shown.with(|shown| shown.shows_pages())) {
            menu_open.set(false);
        }
    });
    // Sign out is in the header and says in the page when it fails, so the
    // header keeps its height (`sign_in::SignOutFailure`). The failure is
    // the session's: once the page has none, from Sign out everywhere or a
    // new password, it is not said again on the next sign-in.
    let sign_out_failed = RwSignal::new(false);
    Effect::new(move |_| {
        let standing = shown.get();
        sign_out_failed.update(|failed| *failed = standing.keeps_sign_out_failure(*failed));
    });
    let failure = Arc::clone(&localizer);
    // The update the daemon's release check found, an information banner on
    // a line of its own under the pages' links and Sign out, whose whole
    // sentence links to the release's notes without telling GitHub the page
    // it came from. It asks the reader to act, so it stays in the header
    // while the running version is in the footer (decision 142). Not a live
    // region: only the announcer talks to screen readers (A3), and the
    // header is read in order.
    let updates = Arc::clone(&localizer);
    let update = move || {
        told.get().and_then(|told| told.update).map(|update| {
            let sentence = routes::text_with(
                &updates,
                msg::UI_UPDATE_AVAILABLE,
                &[("version", api::MsgArg::Text(update.version))],
            );
            let link_localizer = Arc::clone(&updates);
            view! {
                <div class="site-update basis-full [min-inline-size:0]">
                    <Banner localizer=Arc::clone(&updates)>
                        <Link to=Destination::Outside(update.notes) localizer=link_localizer>
                            {sentence}
                        </Link>
                    </Banner>
                </div>
            }
        })
    };
    let pages = Arc::clone(&localizer);
    let dialog = Arc::clone(&localizer);
    let idle = Arc::clone(&localizer);
    let main = NodeRef::<leptos::html::Main>::new();
    let on_the_network = text(&localizer, msg::UI_SIGN_IN_OFF_NETWORK);
    let full_disk_access = Arc::clone(&localizer);
    let network_banner = Arc::clone(&localizer);
    let footer = Arc::clone(&localizer);
    // The build the daemon runs, as people read it, wherever the header shows
    // the pages; the web UI is built into the daemon, so its build is the
    // page's too. Until the daemon has said which it is, and on the sign-in
    // page, there is none (decision 107).
    let version = Signal::derive(move || {
        if shown.get().shows_pages() {
            told.get().map(|told| told.build.display())
        } else {
            None
        }
    });
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
            // A column at least as tall as the window, so the footer, whose
            // top margin takes the room a short page leaves, stands at its
            // foot (`Footer`).
            <div class="flex flex-col min-block-dvh">
                // The band spans the window, and the header's content stands
                // in the page's frame, as wide as Settings' Page width lays
                // `<main>` and 1rem from its inline edges, so the product's
                // name starts where each page's heading does (decision 141).
                <div class="py-3 bg-surface">
                    <header
                        class=move || format!("site-header {}", width.get().frame_class())
                        on:keydown=hide_menu
                    >
                        // The brand's line: the product's mark and name lead
                        // back to the start page, marked as the current page
                        // there and only there, in the color of the text
                        // around it, so it still looks like the brand. Below
                        // 41rem the menu button ends it, level with it
                        // (decision 143).
                        <div class="flex flex-wrap items-center gap-x-4 basis-full">
                            <p class="brand">
                                {Destination::route("/", Matching::Exact)
                                    .map(|to| {
                                        view! {
                                            <Link
                                                to=to
                                                localizer=Arc::clone(&localizer)
                                                tone=LinkTone::Inherit
                                            >
                                                <BrandMark />
                                                {text(&localizer, msg::UI_BRAND)}
                                            </Link>
                                        }
                                    })}
                            </p>
                            {move || {
                                shown
                                    .get()
                                    .shows_pages()
                                    .then(|| {
                                        view! {
                                            <MenuButton
                                                localizer=Arc::clone(&menu_button)
                                                open=menu_open
                                            />
                                        }
                                    })
                            }}
                        </div>
                        {move || {
                            shown
                                .get()
                                .shows_pages()
                                .then(|| {
                                    view! {
                                        // The pages' links and Sign out: in the
                                        // header's line wherever they fit, and
                                        // below 41rem the menu's panel, which
                                        // the menu button shows and hides.
                                        <div
                                            id=navigation::MENU_ID
                                            class=move || navigation::menu_class(menu_open.get())
                                            on:click=hide_menu_on_link
                                        >
                                            <Navigation
                                                localizer=Arc::clone(&navigation)
                                                open=menu_open
                                            />
                                            <Account
                                                localizer=Arc::clone(&account)
                                                standing=standing
                                                sign_out_failed=sign_out_failed
                                            />
                                        </div>
                                        {update.clone()}
                                    }
                                })
                        }}
                    </header>
                </div>
                <main id="main" tabindex="-1" node_ref=main class=move || width.get().main_class()>
                    {move || {
                        (notice.get() == SignInOffNotice::EveryPage)
                            .then(|| {
                                let words = on_the_network.clone();
                                let link = admin_password_link(&network_banner);
                                view! {
                                    <Banner
                                        localizer=Arc::clone(&network_banner)
                                        id=routes::SIGN_IN_OFF_NETWORK_ID
                                        tone=BannerTone::Warning
                                    >
                                        {words}
                                        " "
                                        {link}
                                    </Banner>
                                }
                            })
                    }}
                    // On every page while the agent found the grant missing,
                    // after the network's notice: neither can be dismissed.
                    <FullDiskAccessBanner localizer=full_disk_access shown=shown />
                    {move || {
                        (shown.get() == Standing::SignedIn)
                            .then(|| {
                                view! {
                                    <SignOutFailure
                                        localizer=Arc::clone(&failure)
                                        failed=sign_out_failed
                                    />
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
                <Footer localizer=footer version=version width=width />
            </div>
            {move || {
                asking
                    .get()
                    .then(|| view! { <SessionEnded localizer=Arc::clone(&dialog) ended=ended /> })
            }}
            <IdleWarning localizer=idle warning=warning />
        </Router>
    }
}

fn main() {
    panic_report::install();
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
