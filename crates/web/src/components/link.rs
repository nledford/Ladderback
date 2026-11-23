//! The link (components §4.5): an `<a>` to a page of the app or to another
//! site. [`rules`] decides where it goes and what it tells, host-tested; this
//! module is the glue that draws it.

use std::sync::Arc;

use l10n::{Localizer, msg};
use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use leptos_router::hooks::use_location;

use super::Icon;
use crate::text;
use rules::{Destination, LinkPlacement, LinkTone};

// Where a link goes and what it tells, decided on the host; `Link` is its
// glue.
pub(crate) mod rules;

/// A link (§4.5): an `<a>`, in Ladder Blue unless its page names
/// `LinkTone::Inherit`, the color of the text around it, as a link inside a
/// status sentence is. It has no size: it is the text around it. Its words
/// are its `children`, which name it. Where it stands is its `placement`,
/// which every link names ([`LinkPlacement`]): among words it is always
/// underlined; standing alone it is underlined only under the pointer or
/// pressed, and the page it names has a bar at its start (decision 152).
///
/// `to` is where it goes, parsed once ([`Destination`]):
///
/// - **A route** (a page of the app) is a plain `<a href>` inside the
///   router, which takes the press, so it is a route change: the page is
///   titled and focus moves to its heading, never a reload; a press with a
///   modifier key, or with another button than the first, is the browser's
///   own (a new tab or window). It is marked `aria-current="page"` and drawn
///   bold while the router's location is its page, as its [`Matching`]
///   says, so it can't drift from where the reader is. Not the router's
///   `A`, which sets its own `aria-current` from the path alone, so a link
///   to a page of a list (`?page=2`) would be current on every page of it,
///   and a second `aria-current` would race it. A route link stands inside
///   the router, whose location it reads.
/// - **An outside address** is an `<a href rel="external noreferrer">`,
///   never routed, even one that turns out to be the app's own, which opens
///   where the reader is: the reader's browser offers a new tab. It says it
///   goes to another site in words after its own, which a screen reader
///   hears and no one sees (`ui-link-outside`), and with Lucide's
///   external-link icon after its own words, which a screen reader skips,
///   so its name begins with the words a sighted reader sees (2.5.3).
///
/// What the caller owes WCAG 2.2 AA, since the link does none of it:
///
/// - **Words that say where it goes,** in their sentence or alone (2.4.4).
/// - **A link to do something is a button.** A link goes somewhere; an
///   action is `Button`'s.
#[component]
pub fn Link(
    to: Destination,
    localizer: Arc<Localizer>,
    #[prop(optional)] tone: LinkTone,
    placement: LinkPlacement,
    children: Children,
) -> impl IntoView {
    let class = rules::link_class(tone, placement);
    let rel = to.rel();
    let leaves = to.leaves();
    let href = match &to {
        Destination::Route(route) => route.href().to_owned(),
        Destination::Outside(outside) => outside.href().to_owned(),
    };
    // Only a route asks the router where the reader is: an outside link is
    // never the current page.
    let current = if leaves {
        Signal::stored(None)
    } else {
        let location = use_location();
        Signal::derive(move || to.current(&location.pathname.get(), &location.search.get()))
    };
    // The words that say it goes to another site, after its own: in its
    // name, for a screen reader, and out of sight, as the live regions are.
    let outside_note = leaves.then(|| {
        view! {
            <Icon glyph=LucideGlyph::ExternalLink />
            <span class="sr-only">{text(&localizer, msg::UI_LINK_OUTSIDE)}</span>
        }
    });
    view! {
        <a class=class href=href rel=rel aria-current=current>
            {children()}
            {outside_note}
        </a>
    }
    .into_any()
}
