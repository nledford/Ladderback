//! The toast (components §4.5): the answer to what the reader just did, an
//! icon, the words and a Dismiss button, said through the announcer.
//! [`rules`] decides its tone's icon, the word that names it, how it is said
//! and drawn and how long it stays, host-tested; this module is the glue
//! that draws it and runs its waits.

use std::sync::Arc;

use l10n::Localizer;
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::button::{Appearance, Content};
use super::icon::Meaning;
use super::{Button, Icon, Tip, Tooltip, hand_focus_when_gone};
use crate::live::reply;
use crate::text;
use crate::variants::{Size, Tone};
pub use rules::ToastTone;
use rules::{Hold, Lifetime, Timer};

// What a toast shows, how it is said and drawn and how long it stays,
// decided on the host; `Toast` is its glue.
pub(crate) mod rules;

/// The toast shown: its tone and its words.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    tone: ToastTone,
    words: String,
}

/// A page's toasts, shown one at a time in one place: the page keeps them,
/// gives them to the [`Toast`] it draws, and calls [`Toasts::show`] when the
/// reader's action has its answer. A new toast takes the place of the one
/// shown.
#[derive(Clone, Copy)]
pub struct Toasts {
    shown: RwSignal<Option<Shown>>,
    life: StoredValue<Lifetime>,
}

impl Toasts {
    pub fn new() -> Self {
        Self {
            shown: RwSignal::new(None),
            life: StoredValue::new(Lifetime::default()),
        }
    }

    /// Shows `words` as a toast in `tone`, said once through the announcer,
    /// politely, or assertively for a failure; its going is not said. It
    /// never takes focus.
    pub fn show(self, tone: ToastTone, words: String) {
        reply(tone.politeness(), &words);
        self.shown.set(Some(Shown { tone, words }));
        let timer = self.life.try_update_value(|life| life.arrive(tone));
        self.wait(timer.flatten());
    }

    /// Takes the toast away, whether its time is up or its reader dismissed
    /// it.
    fn dismiss(self) {
        self.life.try_update_value(Lifetime::gone);
        self.shown.set(None);
    }

    fn hold(self, hold: Hold) {
        self.life.try_update_value(|life| life.hold(hold));
    }

    fn release(self, hold: Hold) {
        let timer = self.life.try_update_value(|life| life.release(hold));
        self.wait(timer.flatten());
    }

    /// Starts `timer`'s wait, at whose end the toast goes, unless a newer
    /// wait, a hold or a dismissal has voided it.
    fn wait(self, timer: Option<Timer>) {
        let Some(Timer { token, after }) = timer else {
            return;
        };
        set_timeout(
            move || {
                if self.life.try_with_value(|life| life.expires(token)) == Some(true) {
                    self.dismiss();
                }
            },
            after,
        );
    }
}

impl Default for Toasts {
    fn default() -> Self {
        Self::new()
    }
}

/// A toast (§4.5) in the page's flow, where its page puts it, never fixed
/// over the page: an icon that tells its tone by its shape and is named by
/// a word for it, the words, then a Ghost Small Dismiss button whose tooltip
/// shows its name. Shown by [`Toasts::show`]; its tone is a
/// [`ToastTone`], information unless its page names Success, Warning or
/// Error.
///
/// It is no live region: the announcer says it, once. Information and
/// success go by themselves after 6 s, but never while the pointer rests on
/// the toast or focus is in it, and have their whole 6 s again once both
/// have left (2.2.1); a warning or a failure needs the reader and stays
/// until dismissed. Dismissed, focus on its button moves to the element
/// with the ID `then_focus`, which the page keeps on the page (2.4.3), and
/// nothing is said.
///
/// What the caller owes WCAG 2.2 AA, since the toast does none of it:
///
/// - **Keeping a failure on the page.** A toast is never the only place a
///   failure appears: it also stays where it happened, an alert say.
/// - **Words that say what happened,** in the page's language.
/// - **Keeping `then_focus` on the page.**
#[component]
pub fn Toast(
    localizer: Arc<Localizer>,
    toasts: Toasts,
    #[prop(into)] then_focus: String,
) -> impl IntoView {
    // The alert's word, so one Dismiss reads alike across the set.
    let name = text(&localizer, super::alert::rules::dismiss_name());
    // The box stays while toasts replace one another, so a hold on it and
    // focus on its Dismiss button carry over; it goes only with the last.
    let showing = Memo::new(move |_| toasts.shown.with(Option::is_some));
    let tone = move || {
        toasts
            .shown
            .with(|shown| shown.as_ref().map(|shown| shown.tone).unwrap_or_default())
    };
    move || {
        showing.get().then(|| {
            let localizer = Arc::clone(&localizer);
            let name = name.clone();
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone(button, then_focus.clone());
            view! {
                <div
                    class=move || rules::toast_class(tone())
                    on:mouseenter=move |_| toasts.hold(Hold::Pointer)
                    on:mouseleave=move |_| toasts.release(Hold::Pointer)
                    on:focusin=move |_| toasts.hold(Hold::Focus)
                    on:focusout=move |_| toasts.release(Hold::Focus)
                >
                    // The icon, centered on the first line of the words (one
                    // line tall: `1lh`, which no utility names), in the
                    // words' color, so the pair's contrast covers it.
                    <span class="toast-icon flex flex-none items-center [min-block-size:1lh]">
                        {move || {
                            let tone = tone();
                            let label = text(&localizer, tone.label());
                            view! { <Icon glyph=tone.glyph() meaning=Meaning::Labelled(label) /> }
                        }}
                    </span>
                    // The words take the room left and wrap (The 320 Rule);
                    // `min-inline-size` lets them shrink below their longest
                    // word, which no logical utility names.
                    <p class="toast-words flex-auto m-0 [min-inline-size:0] wrap-anywhere">
                        {move || {
                            toasts
                                .shown
                                .with(|shown| shown.as_ref().map(|shown| shown.words.clone()))
                        }}
                    </p>
                    // At the line's end, never squeezed by the words.
                    <span class="toast-dismiss flex-none">
                        <Tooltip tip=Tip::Names(name.clone())>
                            <Button
                                content=Content::IconOnly {
                                    glyph: LucideGlyph::X.into(),
                                    label: name.into(),
                                }
                                appearance=Appearance::Ghost
                                tone=Tone::Inherit
                                size=Size::Small
                                node_ref=button
                                on_press=Callback::new(move |()| toasts.dismiss())
                            />
                        </Tooltip>
                    </span>
                </div>
            }
        })
    }
}
