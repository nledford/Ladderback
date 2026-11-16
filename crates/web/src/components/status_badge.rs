use super::Icon;
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

/// A status badge: a short labeled state on a colored pill, one of
/// `badge-loading`, `badge-empty`, `badge-error` or `badge-success`, whose
/// text token on its background token is pair-checked by
/// `scripts/check-contrast.ts`, with the state's own icon before the label
/// ([`BadgeState::glyph`]), so the state never rests on its color (A2). The
/// icon is an [`Icon`] of the default kind, decoration beside the text:
/// `aria-hidden`, so a screen reader reads the label once, never a tab stop,
/// in the Inherit tone, the text's color, so the contrast gate covers it too,
/// and as tall as the text. The icon never moves, a running badge's either:
/// an operation can run for hours while the page is in use, and nothing here
/// could stop a spinner (WCAG 2.2.2); the label tells that it runs. The state
/// and the label may be signals: the pill and its icon follow them in place,
/// as a live row's do.
#[component]
pub fn StatusBadge(
    #[prop(into)] state: Signal<BadgeState>,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    view! {
        <span class=move || format!("badge badge-{}", state.get().pill())>
            <Icon glyph=Signal::derive(move || state.get().glyph()) />
            {move || label.get()}
        </span>
    }
}
