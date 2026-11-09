//! Steading's web UI (technical design §4.4), a Leptos client-side app
//! built with trunk and embedded in the `steading` binary.
//!
//! The page skeleton every page will share: a skip link, landmarks and a
//! single `<h1>` (§4.5). Every visible text comes out of the `l10n` catalog, in
//! the language [`page`] negotiates (§8.4); none is written here.

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

mod page;

/// The text of message `id` in the page's language.
fn text(localizer: &Localizer, id: MessageId) -> String {
    localizer.message(id, &[]).into_text()
}

#[component]
fn App(localizer: Arc<Localizer>) -> impl IntoView {
    view! {
        <a class="skip-link" href="#main">{text(&localizer, msg::UI_SKIP_LINK)}</a>
        <header class="site-header">
            <p class="brand">{text(&localizer, msg::UI_BRAND)}</p>
        </header>
        <main id="main" tabindex="-1">
            <h1>{text(&localizer, msg::UI_START_HEADING)}</h1>
            <p>{text(&localizer, msg::UI_START_BODY)}</p>
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    let localizer = Arc::new(page::localizer());
    page::declare(&localizer);
    leptos::mount::mount_to_body(move || view! { <App localizer=localizer /> });
}
