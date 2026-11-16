use leptos::prelude::*;

use crate::live::{pause, resume};

/// The control that pauses a page's live updates (§4.5, "Live updates"; WCAG
/// 2.2.2): a toggle button, `aria-pressed`, whose name `label` stays the same
/// pressed or not, so a screen reader says the state and never a new name.
/// While it is pressed, the banner (the text of `banner`, in the element
/// with the id `banner_id`) shows beside it and describes it
/// (`aria-describedby`): it is not a live region, so it is read with the
/// button and never by itself.
///
/// Pressing it to pause runs `on_pause`, where the page holds what it shows
/// ([`crate::pause::Pausable`]), then says `paused_said` and holds the
/// announcer's events. Pressing it again runs `on_resume`, where the page
/// shows its live state again and words what changed meanwhile, which is
/// said before the events that waited. The button is built once and stays,
/// so focus never moves (2.4.3). Leaving the page ends the pause: nothing is
/// stored, so a reload starts unpaused.
#[component]
pub(crate) fn PauseUpdates(
    label: String,
    paused_said: String,
    #[prop(into)] paused: Signal<bool>,
    #[prop(into)] banner: Signal<String>,
    banner_id: &'static str,
    on_pause: Callback<()>,
    on_resume: Callback<(), String>,
) -> impl IntoView {
    on_cleanup(|| resume(None));
    view! {
        <div class="pause-updates">
            <button
                type="button"
                aria-pressed=move || if paused.get() { "true" } else { "false" }
                aria-describedby=move || paused.get().then_some(banner_id)
                on:click=move |_| {
                    if paused.get_untracked() {
                        resume(Some(on_resume.run(())));
                    } else {
                        on_pause.run(());
                        pause(&paused_said);
                    }
                }
            >
                {label}
            </button>
            {move || {
                paused
                    .get()
                    .then(|| {
                        view! {
                            <p id=banner_id class="pause-banner">
                                {move || banner.get()}
                            </p>
                        }
                    })
            }}
        </div>
    }
}
