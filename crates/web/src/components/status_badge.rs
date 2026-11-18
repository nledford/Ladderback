use super::Badge;
use leptos::prelude::*;
use rules::BadgeState;

// What a status badge shows for each state, decided on the host;
// `StatusBadge` is its glue.
pub(crate) mod rules;
// The badge as the browser build draws it, run by `just test-wasm`; declared
// as `main.rs` declares `formatting`, for the same reason.
#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests;

/// A status badge: a short labeled state, a [`Badge`] in its state's tone
/// ([`BadgeState::tone`]), whose words on its fill are pair-checked by
/// `scripts/check-contrast.ts`, with the state's own icon before the label
/// ([`BadgeState::glyph`]), so the state never rests on its color (A2). The
/// icon is the badge's, decoration beside the text:
/// `aria-hidden`, so a screen reader reads the label once, never a tab stop,
/// in the text's color, so the contrast gate covers it too, and as tall as
/// the text. The icon never moves, a running badge's either: an operation
/// can run for hours while the page is in use, and nothing here could stop
/// a spinner (WCAG 2.2.2); the label tells that it runs. The state and the
/// label may be signals: the pill and its icon follow them in place, as a
/// live row's do.
#[component]
pub fn StatusBadge(
    #[prop(into)] state: Signal<BadgeState>,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    view! {
        <Badge
            label=label
            tone=Signal::derive(move || state.get().tone())
            icon=Signal::derive(move || state.get().glyph())
        />
    }
}
