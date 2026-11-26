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
use super::{Button, Icon, Tip, Tooltip, hand_focus_when_gone_to};
use crate::live::reply;
use crate::text;
use crate::variants::{Size, Tone};
pub use rules::ToastTone;
use rules::{Hold, Lifetime, Room, Timer};

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
    room: RwSignal<Room>,
}

impl Toasts {
    pub fn new() -> Self {
        Self {
            shown: RwSignal::new(None),
            life: StoredValue::new(Lifetime::default()),
            room: RwSignal::new(Room::default()),
        }
    }

    /// Shows `words` as a toast in `tone`, said once through the announcer,
    /// politely, or assertively for a failure; its going is not said. It
    /// never takes focus. Its slot keeps room for it while the page is open.
    pub fn show(self, tone: ToastTone, words: String) {
        reply(tone.politeness(), &words);
        self.room.maybe_update(|room| room.keep(&words));
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
/// whose ID `then_focus` holds then, which the page keeps on the page
/// (2.4.3), and nothing is said.
///
/// It lies in a slot that keeps room for it from the page's first paint,
/// one line of a toast tall, and grows to fit each toast shown without
/// shrinking back while the page is open, so a toast coming or going moves
/// nothing after it; only a toast taller than any before it, such as one
/// whose words wrap on a small screen with larger text, moves what follows,
/// once, by the lines it adds.
///
/// What the caller owes WCAG 2.2 AA, since the toast does none of it:
///
/// - **Keeping a failure on the page.** A failure stays where it happened,
///   an alert say, and a toast never is the only place it appears, unless
///   it is the failure of that press alone, which leaves nothing else on
///   the page wrong (a copy the browser refused): then an `Error` toast
///   beside the control, which stays until dismissed, is that place.
/// - **Words that say what happened,** in the page's language.
/// - **Keeping `then_focus` on the page,** or changing it to the control
///   that asked for the toast shown, when a page's toasts answer more than
///   one.
#[component]
pub fn Toast(
    localizer: Arc<Localizer>,
    toasts: Toasts,
    #[prop(into)] then_focus: Signal<String>,
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
    let room_name = name.clone();
    let toast = move || {
        showing.get().then(|| {
            let localizer = Arc::clone(&localizer);
            let name = name.clone();
            let button = NodeRef::<leptos::html::Button>::new();
            hand_focus_when_gone_to(button, move || then_focus.try_get_untracked());
            view! {
                <div
                    class=move || rules::toast_class(tone())
                    on:mouseenter=move |_| toasts.hold(Hold::Pointer)
                    on:mouseleave=move |_| toasts.release(Hold::Pointer)
                    on:focusin=move |_| toasts.hold(Hold::Focus)
                    on:focusout=move |_| toasts.release(Hold::Focus)
                >
                    <span class=ICON>
                        {move || {
                            let tone = tone();
                            let label = text(&localizer, tone.label());
                            view! { <Icon glyph=tone.glyph() meaning=Meaning::Labeled(label) /> }
                        }}
                    </span>
                    <p class=WORDS>
                        {move || {
                            toasts
                                .shown
                                .with(|shown| shown.as_ref().map(|shown| shown.words.clone()))
                        }}
                    </p>
                    <span class=DISMISS>
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
    };
    view! {
        // The room comes first, so the toast, drawn after it in the one
        // cell, lies above it.
        <div class=rules::SLOT>
            <For
                each=move || toasts.room.with(|room| room.words().to_vec())
                key=|words: &String| words.clone()
                let:words
            >
                {room(room_name.clone(), words)}
            </For>
            {toast}
        </div>
    }
}

// The parts of a toast, and of the room kept for one, so both lay out alike.
// The icon, centered on the first line of the words (one line tall: `1lh`,
// which no utility names), in the words' color, so the pair's contrast
// covers it.
const ICON: &str = "toast-icon flex flex-none items-center [min-block-size:1lh]";
// The words take the room left and wrap (The 320 Rule); `min-inline-size`
// lets them shrink below their longest word, which no logical utility names.
const WORDS: &str = "toast-words flex-auto m-0 [min-inline-size:0] wrap-anywhere";
// At the line's end, never squeezed by the words.
const DISMISS: &str = "toast-dismiss flex-none";

/// Room kept for a toast of `words` (or one empty line): an unseen copy of
/// one, laid out as it is at any width and text size. Hidden (`invisible`),
/// it is out of the accessibility tree, and `inert` keeps its button out of
/// reach. Its words are drawn by CSS from an attribute, so no search of the
/// page's text finds them twice. Its Dismiss button sits in the span a
/// [`Tooltip`] wraps a control in, with no tooltip: a closed one takes no
/// room, and a real one per copy would keep its listeners on the window for
/// the page's life.
fn room(name: String, words: String) -> impl IntoView {
    view! {
        <div class=rules::ROOM inert=true>
            <span class=ICON>
                <Icon glyph=ToastTone::default().glyph() />
            </span>
            <p class=format!("{WORDS} before:content-[attr(data-words)]") data-words=words></p>
            <span class=DISMISS>
                <span class="tooltip-anchor">
                    // Never pressed: it is unseen and inert.
                    <Button
                        content=Content::IconOnly {
                            glyph: LucideGlyph::X.into(),
                            label: name.into(),
                        }
                        appearance=Appearance::Ghost
                        tone=Tone::Inherit
                        size=Size::Small
                        on_press=Callback::new(|()| {})
                    />
                </span>
            </span>
        </div>
    }
}
