use leptos::prelude::*;

use rules::Shape;

// What a skeleton draws, decided on the host; `Skeleton` is its glue.
pub(crate) mod rules;

/// A skeleton (§4.5): the still shape of content on its way, standing where
/// the content will, so the page doesn't jump when it arrives. Its `shape`
/// says what it stands for, lines of text or a block, sized from the
/// reader's text; [`rules`] decides the bars it draws. It has no tone (it is
/// not a status) and no size (it takes the content's).
///
/// It is hidden from a screen reader (`aria-hidden`), has no role and is
/// never a stop for Tab. It never moves, in any motion setting: it starts by
/// itself beside content the reader can use, may stay past 5 s on a slow
/// daemon, and nothing on the page could pause it (2.2.2).
///
/// What the caller owes, since the skeleton says nothing:
///
/// - **Words.** It is never a stand-in for words: the page says the wait in
///   words ("Loading…"), visible, beside it.
/// - **Busy.** The region it stands for carries `aria-busy="true"` until the
///   content arrives.
/// - **Arrival.** The content takes its place without moving focus and
///   without an announcement of its own: the announcer stays the page's.
#[component]
pub fn Skeleton(shape: Shape) -> impl IntoView {
    view! {
        <span class=shape.class() aria-hidden="true">
            {shape
                .bars()
                .into_iter()
                .map(|bar| {
                    // Style properties, which the page's content security
                    // policy allows (§10.1), not a `style` attribute, which it
                    // refuses.
                    view! {
                        <span
                            class="skeleton-bar"
                            style:inline-size=bar.inline
                            style:block-size=bar.block
                        ></span>
                    }
                })
                .collect_view()}
        </span>
    }
    .into_any()
}
