use crate::badge::BadgeState;
use lepticons_data::Glyph;
use leptos::prelude::*;

/// A status badge: a short labeled state on a colored pill, one of
/// `badge-loading`, `badge-empty`, `badge-error` or `badge-success`, whose
/// text token on its background token is pair-checked by
/// `scripts/check-contrast.ts`, with the state's own icon before the label
/// ([`BadgeState::glyph`]), so the state never rests on its color (A2). The
/// icon is decoration beside the text: `aria-hidden`, so a screen reader reads
/// the label once, `focusable="false"`, and stroked in `currentColor`, the
/// text's color, so the contrast gate covers it too. The icon never moves, a
/// running badge's either: an operation can run for hours while the page is
/// in use, and nothing here could stop a spinner (WCAG 2.2.2); the label
/// tells that it runs. The state and the label may be signals: the pill and
/// its icon follow them in place, as a live row's do.
#[component]
pub fn StatusBadge(
    #[prop(into)] state: Signal<BadgeState>,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    view! {
        <span class=move || format!("badge badge-{}", state.get().pill())>
            // The glyph's shapes (`lepticons-data`'s own markup, never outside
            // input) inside an `<svg>` of the badge's own: `1em` keeps it
            // text-sized without the stylesheet.
            <svg
                class="badge-icon"
                xmlns="http://www.w3.org/2000/svg"
                width="1em"
                height="1em"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
                focusable="false"
                inner_html=move || state.get().glyph().svg()
            />
            {move || label.get()}
        </span>
    }
}
