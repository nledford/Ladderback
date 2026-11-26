//! The banner (components §4.5): a page-wide notice above the content, an
//! icon and words, which the reader may dismiss for good in this browser.
//! [`rules`] decides its tone's icon, the word that names it, how it is drawn
//! and whether a remembered dismissal keeps it away, host-tested; this module
//! is the glue that draws it and reads and writes the browser's memory.

use std::sync::Arc;

use l10n::Localizer;
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::button::{Appearance, Content};
use super::icon::Meaning;
use super::{Button, Icon, Tip, Tooltip, hand_focus_when_gone};
use crate::page::storage;
use crate::text;
use crate::variants::Size;
use rules::BannerTone;

// What a banner shows and how it is drawn, decided on the host; `Banner` is
// its glue.
pub(crate) mod rules;

/// How a dismissible banner goes, and how the browser remembers it.
///
/// - `remember_as` names the banner in the browser's local storage, one name
///   per banner of the app ("update"), so dismissing one keeps no other away.
/// - `edition` is what this showing of it says, such as the version of a
///   pending update: the dismissal is remembered for this edition only, so a
///   newer one shows again.
/// - `then_focus` is the ID of the element focus moves to when the Dismiss
///   button had it, which the page keeps on the page, so focus never falls
///   to the page (2.4.3).
pub struct BannerDismissal {
    pub remember_as: &'static str,
    pub edition: String,
    pub then_focus: String,
}

/// A banner (§4.5): a page-wide notice above a page's content, its words its
/// `children`, after an icon that tells its tone by its shape and is named
/// by a word for it ("Warning"), so a screen reader hears the tone before
/// the words and no tone is told by its color alone (1.4.1). Its `tone` comes
/// from the shared vocabulary ([`BannerTone`]): Primary, information, unless
/// its page names Warning. It has no size and no placement: its words are
/// the page's text, and it takes the whole width of the content. Its `id`,
/// when the page gives one, lets another element point at it, as each
/// page's heading is described by the notice that sign-in is off on the
/// network.
///
/// It is not a live region (no `role="alert"`, no `aria-live`) and never a
/// stop for Tab. Given a [`BannerDismissal`], it ends with a Ghost Small
/// Dismiss button, an icon-only `Button` whose tooltip shows its name. A
/// press takes the banner off the page and keeps the dismissal of that
/// edition in the browser's local storage, so the banner stays away on every
/// later load until a newer edition comes; a browser that keeps nothing
/// shows it again on the next load.
///
/// What the caller owes WCAG 2.2 AA, since the banner does none of it:
///
/// - **Announcing what is urgent (4.1.3).** A change that needs the reader
///   now is said through the announcer as well ([`crate::live::say`]); the
///   banner only shows it.
/// - **Words that say what is so and what to do,** in the page's language,
///   the daemon's own words marked as English (3.1.2).
/// - **Keeping `then_focus` on the page.**
#[component]
pub fn Banner(
    localizer: Arc<Localizer>,
    #[prop(optional)] id: Option<&'static str>,
    #[prop(optional)] tone: BannerTone,
    #[prop(optional)] dismissal: Option<BannerDismissal>,
    children: Children,
) -> impl IntoView {
    let label = text(&localizer, tone.label());
    let name = text(&localizer, rules::dismiss_name());
    // Dismissed in this edition before: nothing to draw, now or later.
    let dismissed = RwSignal::new(dismissal.as_ref().is_some_and(|dismissal| {
        rules::stays_dismissed(kept(dismissal.remember_as).as_deref(), &dismissal.edition)
    }));
    // The words are built once, and only when drawn: a banner that stays
    // away builds none, and starts nothing they would. Once gone it never
    // comes back without its page drawing a new one.
    let mut words = Some(children);
    move || {
        if dismissed.get() {
            return None;
        }
        let words = words.take()?;
        // Drawn untracked: only the dismissal redraws this branch, which
        // takes the banner away.
        Some(untrack(|| {
            let dismiss = dismissal.as_ref().map(
                |BannerDismissal {
                     remember_as,
                     edition,
                     then_focus,
                 }| {
                    let button = NodeRef::<leptos::html::Button>::new();
                    // Runs as this branch goes, while the button is still on
                    // the page.
                    hand_focus_when_gone(button, then_focus.clone());
                    let (remember_as, edition) = (*remember_as, edition.clone());
                    let (tip, button_name) = (name.clone(), name.clone());
                    view! {
                        // At the line's end, never squeezed by the words.
                        <span class="banner-dismiss flex-none">
                            <Tooltip tip=Tip::Names(tip)>
                                <Button
                                    content=Content::IconOnly {
                                        glyph: LucideGlyph::X.into(),
                                        label: button_name.into(),
                                    }
                                    appearance=Appearance::Ghost
                                    size=Size::Small
                                    node_ref=button
                                    on_press=Callback::new(move |()| {
                                        remember(remember_as, &edition);
                                        dismissed.set(true);
                                    })
                                />
                            </Tooltip>
                        </span>
                    }
                },
            );
            view! {
                <div id=id class=rules::banner_class(tone)>
                    // The icon, centered on the first line of the words (one
                    // line tall: `1lh`, which no utility names).
                    <span class="banner-icon flex flex-none items-center [min-block-size:1lh]">
                        <Icon
                            glyph=tone.glyph()
                            meaning=Meaning::Labeled(label.clone())
                            tone=tone.tone()
                        />
                    </span>
                    // The words take the room left and wrap, a long path too
                    // (The 320 Rule); `min-inline-size` lets them shrink below
                    // their longest word, which no logical utility names.
                    <div class="banner-words flex-auto [min-inline-size:0] wrap-anywhere">
                        {words()}
                    </div>
                    {dismiss}
                </div>
            }
        }))
    }
}

/// The edition of the banner `banner` this browser remembers dismissing, if
/// it keeps one. Outside input: [`rules::stays_dismissed`] holds it to the
/// edition shown.
fn kept(banner: &str) -> Option<String> {
    storage()?
        .get_item(&rules::storage_key(banner))
        .ok()
        .flatten()
}

/// Remembers in this browser that the banner `banner` was dismissed in
/// `edition`. A browser that refuses keeps nothing: the banner still goes
/// now and shows again on the next load.
fn remember(banner: &str, edition: &str) {
    if let Some(storage) = storage() {
        let _ = storage.set_item(&rules::storage_key(banner), edition);
    }
}
