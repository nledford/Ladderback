//! The badge (components §4.5): a count or a tag on a pill of its tone's
//! colors, said in words. [`rules`] decides its tones and how it is drawn,
//! host-tested; this module is the glue that draws it.

use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::Icon;
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
/// tokens ([`BadgeTone`]: Secondary, the gray pair, unless its page names
/// another), with an optional `icon` before the words. The status badge is
/// one use of it.
///
/// It is plain text: no role, not a live region, never a stop for Tab, so a
/// screen reader reads its words where they stand. The icon is an [`Icon`]
/// in its defaults, decoration beside the words (`aria-hidden`), stroked in
/// their color (Inherit), so the tone's pair covers it too, and as tall as
/// the text (Small). The tone, the label and the icon may be signals: the
/// pill, its words and its icon follow them in place, as a live row's
/// status does. It has no size and no placement: its words are the text
/// around it, and it stands in their line.
///
/// What the caller owes WCAG 2.2 AA, since the badge does none of it:
///
/// - **Words that say it all (1.4.1).** The label says what the badge counts
///   or names, the count included ("Pending changes 3", never "3" alone):
///   the tone and the icon only repeat it.
/// - **Telling a change.** A badge isn't a live region: a count that changes
///   while the reader is on the page is said through the announcer where
///   the reader needs to hear it (4.1.3), never by the badge.
#[component]
pub fn Badge(
    #[prop(into)] label: Signal<String>,
    #[prop(optional, into)] tone: Signal<BadgeTone>,
    #[prop(optional, into)] icon: Option<Signal<LucideGlyph>>,
) -> impl IntoView {
    view! {
        <span class=move || rules::badge_class(
            tone.get(),
        )>
            {icon
                .map(|glyph| {
                    view! {
                        <span class=rules::ICON>
                            <Icon glyph=glyph />
                        </span>
                    }
                })} <span class=rules::WORDS>{move || label.get()}</span>
        </span>
    }
}
