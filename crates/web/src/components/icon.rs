//! The icon (components §4.5): one Lucide glyph, decoration unless its
//! caller gives it a label. [`rules`] decides what a screen reader meets and
//! how it is drawn, host-tested; this module is the glue that draws it.

use lepticons_data::{Glyph, LucideGlyph};
use leptos::prelude::*;

use crate::variants::{Size, Tone};

pub(crate) mod rules;

pub use rules::Meaning;

/// An icon (§4.5): one Lucide glyph from `lepticons-data`, its shapes (the
/// crate's own markup, never outside input) inside an `<svg>` root of the
/// icon's own, stroked in `currentColor`, which its tone sets.
///
/// By default it is decoration beside words that say what it shows, or
/// inside a control its own label names: `aria-hidden`, so a screen reader
/// reads the words once. Given `meaning=Meaning::Labelled(label)`, it is an
/// image (`role="img"`) named by the label, for an icon that says something
/// no words beside it say. [`rules`] decides both. It is never a tab stop
/// (`focusable="false"`).
///
/// Its `tone` and `size` come from the shared vocabulary (decision 108):
/// Inherit, the color of the text around it, and Small, as tall as the text,
/// unless the caller names others. The size is the drawing's length on the
/// shared scale (Small 1em, Medium 1.5em, Large 2em, XLarge 3em), the
/// spinner's; the root's own `width` and `height` of 1em keep it the text's
/// height without the stylesheet. It has no placement: it sits in its line
/// of text, centered on the text as an inline image is. The glyph may be a
/// signal: the drawing follows it in place, as a live status badge's does.
///
/// What the caller owes WCAG 2.2 AA, since the icon does none of it:
///
/// - **A label on every meaningful icon,** saying what it shows.
/// - **A labelled icon whose glyph changes is built anew** with the new
///   glyph's label: the label is fixed when the icon is built, so an icon
///   that only swapped its glyph would name what it no longer shows. A
///   decorative icon's glyph may follow a state in place.
/// - **Never a tone alone (1.4.1).** A tone tints the glyph; its shape or
///   the words beside it carry the meaning.
/// - **An icon-only control is named on the control** (`aria-label` on the
///   button), and the icon inside it stays decoration, so a screen reader
///   hears the control's name once and no image inside it.
/// - **Direction.** It draws its glyph as Lucide draws it in either
///   direction; a glyph whose meaning follows the reading direction would
///   need its caller to pick the one for the page's direction.
#[component]
pub fn Icon(
    #[prop(into)] glyph: Signal<LucideGlyph>,
    #[prop(optional)] meaning: Meaning,
    #[prop(default = rules::default_tone())] tone: Tone,
    #[prop(default = rules::default_size())] size: Size,
) -> impl IntoView {
    let (role, hidden) = (meaning.role(), meaning.hidden());
    let name = meaning.name().map(str::to_owned);
    view! {
        <svg
            class=rules::icon_class(tone, size)
            xmlns="http://www.w3.org/2000/svg"
            width="1em"
            height="1em"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            role=role
            aria-hidden=hidden
            aria-label=name
            focusable="false"
            inner_html=move || glyph.get().svg()
        />
    }
}
