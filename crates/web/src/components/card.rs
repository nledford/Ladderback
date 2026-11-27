//! The card (components §4.5; DESIGN.md, The Summary Card Rule): the shared
//! frame of a summary of one thing that stands on its own, as a machine on
//! the hub's dashboard does.

use leptos::prelude::*;

use super::TitleLevel;

/// The card's frame and body in one: the page's own color showing through,
/// a 1px edge in Field Edge with the small corner, and no shadow (the Flat
/// Ledger Rule); one inner padding, its parts in a column at one gap. It
/// fills its row's height in the list of cards.
const FRAME: &str = "flex flex-col gap-2 [min-inline-size:0] block-full m-0 p-4 border border-solid \
     border-field-edge rounded-(--radius-small)";

/// The actions' row, at the card's foot: its controls at the inline end,
/// wrapping onto more lines where the card is narrow (1.4.10).
const ACTIONS: &str = "flex flex-wrap items-center justify-end gap-x-4 gap-y-2 mbs-auto";

/// The classes of the list cards stand in: a grid of columns at least 18rem
/// wide, or the page's whole width where it is narrower, which wraps by its
/// own width (1.4.10).
pub(crate) const CARDS: &str = "grid grid-cols-[repeat(auto-fill,minmax(min(100%,18rem),1fr))] \
     gap-4 m-0 p-0 list-none";

/// A card (§4.5, The Summary Card Rule): an `<article>` named by its title
/// (`aria-labelledby`), a heading at `level` whose words are `heading`, the
/// header's only part. Then, in this order:
///
/// - **The body**, `children`: what the card tells.
/// - **The actions**, while given: the caller's controls, in a row at the
///   card's foot and its inline end.
///
/// It is never a control: never a link, a button or a stop for Tab, so a
/// keyboard user passes only through the controls inside it. Its heading
/// takes focus from a script alone (`tabindex="-1"`), as when a part of
/// the card that held focus goes. It is flat: the page's color inside a 1px
/// Field Edge with the small corner, and no shadow. It takes no tone and no
/// size: a status is a badge's words inside it. Cards stand in a list the
/// caller lays out with [`CARDS`].
///
/// What the caller owes WCAG 2.2 AA: a `heading_id` unique on the page, the
/// heading's level in the page's outline (1.3.1), and controls named by the
/// thing they act on (2.4.6).
#[component]
pub fn Card(
    heading_id: String,
    level: TitleLevel,
    #[prop(into)] heading: ViewFnOnce,
    #[prop(optional, into)] actions: Option<ViewFnOnce>,
    children: Children,
) -> impl IntoView {
    let labeled_by = heading_id.clone();
    let title = match level {
        TitleLevel::Two => view! {
            <h2 id=heading_id class="m-0" tabindex="-1">
                {heading.run()}
            </h2>
        }
        .into_any(),
        TitleLevel::Three => view! {
            <h3 id=heading_id class="m-0" tabindex="-1">
                {heading.run()}
            </h3>
        }
        .into_any(),
    };
    let actions = actions.map(|actions| view! { <div class=ACTIONS>{actions.run()}</div> });
    view! {
        <article class=FRAME aria-labelledby=labeled_by>
            {title}
            {children()}
            {actions}
        </article>
    }
}
