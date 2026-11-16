//! The button (components §4.5): a native `<button>` drawn from the shared
//! vocabulary's tokens. [`rules`] decides how it is drawn, what a screen
//! reader meets and whether it takes a press, host-tested; this module is
//! the glue that draws it.

use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::{Icon, Spinner};
use crate::components::spinner::rules::LabelShown;
use crate::variants::{Size, Tone};
use rules::{Leading, Shape};

// How a button is drawn and what it tells a screen reader, decided on the
// host; `Button` is its glue. Only the gallery shows a button yet, and the
// host tests use these decisions, so only a build of neither leaves them
// unused.
#[cfg_attr(
    all(not(feature = "gallery"), not(test)),
    expect(dead_code, reason = "only the gallery shows a button yet")
)]
pub(crate) mod rules;

pub use rules::{Appearance, Kind};

/// What a button shows, each with the label that names it, so a button
/// without a name can't be built: its label alone, an icon before its
/// label, or only an icon, the label then its name (`aria-label`). The icon
/// is decoration, the label saying what the button does; its glyph may
/// follow a state, as a toggle's may. The label must say something: an
/// empty one leaves a button without a name (4.1.2).
#[derive(Clone, Copy, Debug)]
pub enum Content {
    Text(Signal<String>),
    IconAndText {
        glyph: Signal<LucideGlyph>,
        label: Signal<String>,
    },
    IconOnly {
        glyph: Signal<LucideGlyph>,
        label: Signal<String>,
    },
}

impl Content {
    fn parts(self) -> (Shape, Option<Signal<LucideGlyph>>, Signal<String>) {
        match self {
            Self::Text(label) => (Shape::Text, None, label),
            Self::IconAndText { glyph, label } => (Shape::IconAndText, Some(glyph), label),
            Self::IconOnly { glyph, label } => (Shape::IconOnly, Some(glyph), label),
        }
    }
}

/// A button (§4.5): a native `<button>` of its page (`type="button"`), or
/// one that sends its form (`kind=Kind::Submit`), with the page's own font
/// (The Reader's Size Rule), named by its `content`'s label.
///
/// Its `appearance` is `Outlined` unless the page names `Filled` or
/// `Ghost`; its `tone` and `size` come from the shared vocabulary
/// (decision 108): Secondary and Medium unless the caller names others. A
/// `Filled` button in `Inherit` is drawn `Outlined`, since no label color
/// is checked against every color the text around it could take. The size
/// is the button's height, in `em` (Small 1.75em, Medium 2em, Large 2.5em,
/// XLarge 3em), its padding growing with it; its icon and spinner stay as
/// tall as the text (`Small`). [`rules`] decides all of it.
///
/// `busy` and `disabled` make it unavailable (`aria-disabled="true"`, never
/// the `disabled` attribute), still in the Tab order, its presses ignored:
/// the page's `on_press` is not called, and a submit button sends no form.
/// A busy button shows a spinner in its icon's place, or before a label
/// that has none, which a screen reader skips: the button keeps its name.
/// `pressed` makes it a toggle (`aria-pressed`), drawn bolder with an inset
/// edge while pressed, so the state is never color alone.
///
/// What the caller owes WCAG 2.2 AA, since the button does none of it:
///
/// - **One `Filled` `Primary` button per action row,** its main action.
/// - **Announcing a busy button's work (4.1.3).** The button is no live
///   region: the page says the wait, and its end, through the announcer
///   ([`crate::live::reply`]).
/// - **A label that says what the button does,** for an icon-only one too,
///   whose label is its only name.
/// - **Never a tone alone (1.4.1).** A tone tints the button; its words say
///   what it does.
#[component]
pub fn Button(
    content: Content,
    #[prop(optional)] appearance: Appearance,
    #[prop(default = rules::default_tone())] tone: Tone,
    #[prop(default = rules::default_size())] size: Size,
    #[prop(optional)] kind: Kind,
    #[prop(optional, into)] busy: Signal<bool>,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] pressed: Option<Signal<bool>>,
    #[prop(optional, into)] on_press: Option<Callback<()>>,
) -> impl IntoView {
    let (shape, glyph, label) = content.parts();
    let press = move |event: web_sys::MouseEvent| {
        if !rules::takes_press(disabled.get_untracked(), busy.get_untracked()) {
            // Unavailable: a submit button sends no form either.
            event.prevent_default();
            return;
        }
        if let Some(on_press) = on_press {
            on_press.run(());
        }
    };
    let leading = move || match (shape.leading(busy.get()), glyph) {
        (Leading::Icon, Some(glyph)) => Some(view! { <Icon glyph=glyph /> }.into_any()),
        // The button keeps its name: the spinner, named by the same label,
        // is hidden from a screen reader with all it holds.
        (Leading::Spinner, _) => Some(
            view! {
                <span class="button-spinner" aria-hidden="true">
                    <Spinner shown=LabelShown::Hidden tone=Tone::Inherit label=label />
                </span>
            }
            .into_any(),
        ),
        (Leading::Nothing | Leading::Icon, _) => None,
    };
    view! {
        <button
            type=kind.attribute()
            class=rules::button_class(appearance, tone, size, shape)
            aria-label=move || shape.name(&label.get())
            aria-disabled=move || rules::unavailable(disabled.get(), busy.get())
            aria-pressed=move || rules::pressed(pressed.map(|pressed| pressed.get()))
            on:click=press
        >
            {leading}
            {shape
                .shows_label()
                .then(|| view! { <span class="button-label">{move || label.get()}</span> })}
        </button>
    }
}
