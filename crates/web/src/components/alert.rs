//! The alert (components §4.5): an inline message about a page's state, an
//! icon and words. [`rules`] decides its tone's icon, the word that names
//! it and how it is drawn, host-tested; this module is the glue that draws
//! it.

use std::sync::Arc;

use l10n::Localizer;
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::button::{Appearance, Content};
use super::icon::Meaning;
use super::{Button, Icon, Tip, Tooltip, hand_focus_when_gone};
use crate::text;
use crate::variants::Size;
use rules::AlertTone;

// What an alert shows and how it is drawn, decided on the host; `Alert` is
// its glue.
pub(crate) mod rules;

/// How a dismissible alert goes: `on_dismiss` is called when its Dismiss
/// button is pressed, and its page then takes the alert off the page, the
/// page's own state being what says it is shown. Focus, on the button as it
/// goes, moves to the element with the ID `then_focus`, which the page keeps
/// on the page, so focus never falls to the page (2.4.3).
pub struct Dismissal {
    pub on_dismiss: Callback<()>,
    pub then_focus: String,
}

/// An alert (§4.5): an inline message about a page's state, its words its
/// `children`, after an icon that tells its tone by its shape and is named
/// by a word for it ("Error"), so a screen reader hears the tone before the
/// words and no tone is told by its color alone (1.4.1). Its `tone` comes
/// from the shared vocabulary ([`AlertTone`]): Primary, information, unless
/// its page names Success, Warning or Error. It has no size and no
/// placement: its words are the page's text, and it takes a line of its own
/// as wide as its room.
///
/// It is not a live region (no `role="alert"`, no `aria-live`) and never a
/// stop for Tab. Given a [`Dismissal`], it ends with a Ghost Small Dismiss
/// button, an icon-only `Button` whose tooltip shows its name.
///
/// What the caller owes WCAG 2.2 AA, since the alert does none of it:
///
/// - **Announcing what is urgent (4.1.3).** A failure that needs the reader
///   now is said through the announcer's assertive region as well
///   ([`crate::live::reply`]); the alert only shows it.
/// - **Saying a failure once.** One alert for one failure, never repeated
///   in red across the page (DESIGN.md).
/// - **Words that say what happened and what to do,** in the page's
///   language, the daemon's own words marked as English (3.1.2).
/// - **Taking a dismissed alert off the page** when `on_dismiss` is called,
///   and keeping `then_focus` on the page.
#[component]
pub fn Alert(
    localizer: Arc<Localizer>,
    #[prop(optional)] tone: AlertTone,
    #[prop(optional)] dismissal: Option<Dismissal>,
    children: Children,
) -> impl IntoView {
    let label = text(&localizer, tone.label());
    let dismiss = dismissal.map(
        |Dismissal {
             on_dismiss,
             then_focus,
         }| {
            let name = text(&localizer, rules::dismiss_name());
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone(button, then_focus);
            view! {
                <span class="alert-dismiss">
                    <Tooltip tip=Tip::Names(name.clone())>
                        <Button
                            content=Content::IconOnly {
                                glyph: LucideGlyph::X.into(),
                                label: name.into(),
                            }
                            appearance=Appearance::Ghost
                            size=Size::Small
                            node_ref=button
                            on_press=on_dismiss
                        />
                    </Tooltip>
                </span>
            }
        },
    );
    view! {
        <div class=rules::alert_class(tone)>
            <span class="alert-icon">
                <Icon glyph=tone.glyph() meaning=Meaning::Labelled(label) tone=tone.tone() />
            </span>
            <div class="alert-words">{children()}</div>
            {dismiss}
        </div>
    }
    .into_any()
}
