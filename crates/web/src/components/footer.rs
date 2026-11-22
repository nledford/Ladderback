//! The footer (components §4.5): the foot of every page that shows the pages,
//! holding the running version, quiet, its content in the page's frame.

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use crate::page_width::PageWidth;
use crate::routes::text_with;

/// The footer (§4.5, decision 142): the build the daemon runs, `version` as
/// people read it (`0.4.0+3f937cf`), worded "Version 0.4.0+3f937cf", in a
/// `<footer>`.
///
/// Placed as `<main>`'s next sibling, never inside it or inside a section,
/// it is the page's `contentinfo` landmark. Its content stands in `<main>`'s
/// frame, `width`'s own classes ([`PageWidth::main_class`]), and its version
/// 1rem in from it, where the page's text starts. It is quiet: the version in
/// Slate Note under a 1px Slate Note rule across the window, no fill, no
/// shadow. In the column the app lays out (`main.rs`), its top margin takes
/// the room a short page leaves, so it stands at the window's foot.
///
/// With no `version` it draws nothing, no empty landmark: the sign-in page
/// tells no one which build runs (decision 107). The version is plain text,
/// never a stop for Tab and not a live region (A3): a page that loads with it
/// has nothing to announce, and a screen reader reads it where it stands.
#[component]
pub fn Footer(
    localizer: Arc<Localizer>,
    #[prop(into)] version: Signal<Option<String>>,
    #[prop(into)] width: Signal<PageWidth>,
) -> impl IntoView {
    move || {
        version.get().map(|version| {
            let words = text_with(
                &localizer,
                msg::UI_VERSION,
                &[("version", MsgArg::Text(version))],
            );
            view! {
                <footer class="mbs-auto border-bs border-bs-muted text-muted">
                    <div class=move || width.get().main_class()>
                        <p class="m-0 px-4 py-3">{words}</p>
                    </div>
                </footer>
            }
        })
    }
}
