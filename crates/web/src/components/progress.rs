use std::sync::Arc;

use crate::announcer::Clock;
use l10n::Localizer;
use leptos::prelude::*;

use super::hand_focus_when_gone;
use crate::live::PageClock;
use crate::progress::{
    Clear, Next, Teller, Told, Wake, percent, progress_text, value_text, wait_for,
};
use crate::routes::text_with;
use crate::text;

/// A progress bar (§4.5; §16, decision 83): `role="progressbar"`, named
/// `label`, and a stop in the tab order, so a keyboard user reaches it and
/// hears its value whenever she wants. What it draws follows every report
/// of `reading`: the step above the track while there is a percentage, the
/// fill, which moves smoothly to it unless the reader asks for less motion,
/// and the percentage, or the step in its place while there is none. What
/// it tells a screen reader, `aria-valuenow` and `aria-valuetext` (the step,
/// the percentage, the amounts and the time left), changes as
/// [`Teller`] decides: at most every 30 s while the step stays the same, at
/// once for a new step. It is never announced: nothing here calls the
/// announcer.
///
/// Everything it draws is inside the bar, whose children are presentational,
/// so its focus ring surrounds what a sighted reader sees and a screen reader
/// hears the value text alone. A reading without a total is indeterminate
/// (WAI-ARIA `progressbar`): no `aria-valuenow` and no track, and the step,
/// or that the operation is working, where the percentage stands; never 0%,
/// which would say that nothing has been done (#422). The one timer that
/// tells a held value is cleared when the bar goes.
///
/// A bar that goes while it holds focus, as one does when its run ends,
/// hands focus to the element with the ID `then_focus` (a row's header), so
/// focus never falls to the page (WCAG 2.4.3).
#[component]
pub fn Progress(
    localizer: Arc<Localizer>,
    label: String,
    #[prop(into)] reading: Signal<crate::backend::Progress>,
    #[prop(optional, into)] then_focus: Option<String>,
) -> impl IntoView {
    let teller = StoredValue::new(Teller::default());
    let wake = StoredValue::new(Wake::<TimeoutHandle>::default());
    let told = RwSignal::new(None::<Told>);
    let bar = NodeRef::<leptos::html::Div>::new();
    on_cleanup(move || {
        wake.try_update_value(Wake::clear);
    });
    if let Some(id) = then_focus {
        hand_focus_when_gone(bar, id);
    }
    // The first reading is told as the bar is built, so it is never without
    // a value; the effect tells what follows, and asks again of the same.
    let first = reading.get_untracked();
    act(
        teller.try_update_value(|teller| teller.report(first, PageClock.now())),
        teller,
        wake,
        told,
    );
    Effect::new(move |_| {
        let progress = reading.get();
        act(
            teller.try_update_value(|teller| teller.report(progress, PageClock.now())),
            teller,
            wake,
            told,
        );
    });
    let percent =
        Memo::new(move |_| reading.with(|progress| percent(progress.current, progress.total)));
    let known = Memo::new(move |_| percent.get().is_some());
    let said = Memo::new(move |_| reading.with(progress_text));
    let value_localizer = Arc::clone(&localizer);
    let step_localizer = Arc::clone(&localizer);
    view! {
        <div
            node_ref=bar
            role="progressbar"
            tabindex="0"
            class="progress"
            aria-label=label
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow=move || {
                told.get().and_then(|told| told.measure).map(|measure| measure.percent.to_string())
            }
            aria-valuetext=move || {
                told.get()
                    .map(|told| value_text(&told, |id, args| text_with(&value_localizer, id, args)))
            }
        >
            {move || {
                said.get()
                    .above
                    .map(|id| {
                        view! { <span class="progress-step">{text(&step_localizer, id)}</span> }
                    })
            }}
            {move || {
                known
                    .get()
                    .then(|| {
                        view! {
                            <div class="progress-track">
                                // A style property, which the page's content security policy
                                // allows (§10.1), not a `style` attribute, which it refuses.
                                <div
                                    class="progress-fill"
                                    style:inline-size=move || {
                                        format!("{}%", percent.get().unwrap_or_default())
                                    }
                                ></div>
                            </div>
                        }
                    })
            }}
            <span>
                {move || match percent.get() {
                    Some(percent) => format!("{percent}%"),
                    None => text(&localizer, said.get().unknown),
                }}
            </span>
        </div>
    }
}

impl Clear for TimeoutHandle {
    fn clear(self) {
        TimeoutHandle::clear(&self);
    }
}

/// Does what a bar's [`Teller`] answered, `None` once the bar has gone:
/// tells the value now, clearing the timer that waited to, or waits on one
/// timer until the hold is over and asks again.
fn act(
    next: Option<Next>,
    teller: StoredValue<Teller>,
    wake: StoredValue<Wake<TimeoutHandle>>,
    told: RwSignal<Option<Told>>,
) {
    match next {
        Some(Next::Tell(value)) => {
            wake.try_update_value(Wake::clear);
            told.set(Some(value));
        }
        Some(Next::WaitUntil(due)) => {
            let ask = move || {
                act(
                    teller.try_update_value(|teller| teller.wake(PageClock.now())),
                    teller,
                    wake,
                    told,
                );
            };
            if let Ok(handle) = set_timeout_with_handle(ask, wait_for(due, PageClock.now())) {
                wake.try_update_value(|wake| wake.set(handle));
            }
        }
        Some(Next::Keep) | None => {}
    }
}
