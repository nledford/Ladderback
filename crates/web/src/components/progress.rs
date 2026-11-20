use std::sync::Arc;

use crate::announcer::Clock;
use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::hand_focus_when_gone;
use crate::live::PageClock;
use crate::routes::text_with;
use crate::text;
use crate::variants::{Size, Tone};
use rules::{
    Clear, FILL, Next, REPORTED, STEP, TRACK, Teller, Told, WORDS, Wake, bar_class, default_size,
    fill_shift, percent, progress_text, value_text, wait_for,
};

// What a progress bar tells a screen reader, and when, decided on the host;
// `Progress` is its glue.
mod rules;

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
/// which would say that nothing has been done (#422). Below those words it
/// says when the run last reported, as the daemon heard it, read in `zone`
/// (UTC without one): an exact instant, which changes only when a report
/// arrives and is told like the rest of the value, so a stalled run is told
/// from a busy one without anything that ticks (WCAG 2.2.2). The one timer
/// that tells a held value is cleared when the bar goes.
///
/// A bar that goes while it holds focus, as one does when its run ends,
/// hands focus to the element with the ID `then_focus` (a row's header), so
/// focus never falls to the page (WCAG 2.4.3).
///
/// Its `tone` (the shared vocabulary, `Primary` unless its page names
/// another) fills the bar only: the track stays Ledger Gray and the words
/// the text's color, so how a run goes is never told by color alone
/// (1.4.1). Its `size` is its track's thickness, half the shared scale's
/// length, `Small` unless its page names another.
#[component]
pub fn Progress(
    localizer: Arc<Localizer>,
    label: String,
    #[prop(into)] reading: Signal<crate::backend::Progress>,
    #[prop(optional, into)] then_focus: Option<String>,
    #[prop(optional)] tone: Tone,
    #[prop(optional)] size: Option<Size>,
    #[prop(default = None)] zone: Option<api::IanaZone>,
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
    let reported_localizer = Arc::clone(&localizer);
    let value_zone = zone.clone();
    view! {
        <div
            node_ref=bar
            role="progressbar"
            tabindex="0"
            class=bar_class(tone, size.unwrap_or_else(default_size))
            aria-label=label
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow=move || {
                told.get().and_then(|told| told.measure).map(|measure| measure.percent.to_string())
            }
            aria-valuetext=move || {
                told.get()
                    .map(|told| {
                        value_text(
                            &told,
                            value_zone.as_ref(),
                            |id, args| text_with(&value_localizer, id, args),
                        )
                    })
            }
        >
            {move || {
                said.get()
                    .above
                    .map(|id| {
                        view! { <span class=STEP>{text(&step_localizer, id)}</span> }
                    })
            }}
            {move || {
                known
                    .get()
                    .then(|| {
                        view! {
                            <div class=TRACK>
                                // A style property, which the page's content security policy
                                // allows (§10.1), not a `style` attribute, which it refuses.
                                <div
                                    class=FILL
                                    style:transform=move || {
                                        fill_shift(percent.get().unwrap_or_default())
                                    }
                                ></div>
                            </div>
                        }
                    })
            }}
            <span class=WORDS>
                {move || match percent.get() {
                    Some(percent) => format!("{percent}%"),
                    None => text(&localizer, said.get().unknown),
                }}
            </span>
            {move || {
                said.get()
                    .reported
                    .map(|at| {
                        let shown = text_with(
                            &reported_localizer,
                            msg::UI_PROGRESS_REPORTED,
                            &[
                                (
                                    "when",
                                    MsgArg::Timestamp {
                                        utc_ms: at.as_millisecond(),
                                        zone: zone.clone(),
                                    },
                                ),
                            ],
                        );
                        view! {
                            <span class=REPORTED>
                                <time datetime=at.to_string()>{shown}</time>
                            </span>
                        }
                    })
            }}
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
