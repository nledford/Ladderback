//! The badge (components §4.5): a count or a tag on a pill of its tone's
//! colors, said in words. [`rules`] decides its tones and how it is drawn,
//! host-tested; this module is the glue that draws it.

use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use rules::BadgeTone;

// What a badge shows and how it is drawn, decided on the host; `Badge` is its
// glue.
pub(crate) mod rules;
// The badge as the browser build draws it, run by `just test-wasm`; declared
// as `main.rs` declares `formatting`, for the same reason.
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests;

/// A badge (§4.5): a count or a tag ("Pending changes 3"), its `label` the
/// words that say what it counts or names, on a pill of its `tone`'s pair of
/// tokens ([`BadgeTone`], Secondary unless its page names another), with an
/// optional `icon` before the words.
#[component]
pub fn Badge(
    #[prop(into)] label: Signal<String>,
    #[prop(optional, into)] tone: Signal<BadgeTone>,
    #[prop(optional, into)] icon: Option<Signal<LucideGlyph>>,
) -> impl IntoView {
    let _ = (label, tone, icon);
    let stub: () = todo!();
    stub
}
