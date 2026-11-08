//! Steading's web UI (technical design §4.4), a Leptos client-side app
//! built with trunk and embedded in the `steading` binary.
//!
//! M0 ships only the page skeleton every page will share: a skip link,
//! landmarks and a single `<h1>` (§4.5). Its text moves into the `l10n`
//! catalog with the first real screen in M1b (§8.4).

use leptos::prelude::*;

#[component]
fn App() -> impl IntoView {
    view! {
        <a class="skip-link" href="#main">"Skip to content"</a>
        <header class="site-header">
            <p class="brand">"Steading"</p>
        </header>
        <main id="main" tabindex="-1">
            <h1>"Welcome to Steading"</h1>
            <p>
                "Nothing is set up yet. Repositories, plans and backups arrive in later releases."
            </p>
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
