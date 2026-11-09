//! The live regions of the page (technical design §4.5, "Live updates"): the
//! glue between the [`announcer`](crate::announcer) and the two regions of
//! `index.html`. It owns the page's one announcer and one timer, writes what
//! the announcer decides into the regions, and nothing else. The timing is the
//! announcer's, and is tested there; the `@ui` scenarios of
//! `features/ui/accessibility.feature` run this in a browser.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;
use leptos::wasm_bindgen::closure::Closure;

use crate::announcer::{Announcement, Announcer, Clock, Effect, Kind, Politeness};

/// The element ids of the two regions in `index.html`.
const POLITE_ID: &str = "live-polite";
const ASSERTIVE_ID: &str = "live-assertive";

/// The browser's monotonic clock.
struct PageClock;

impl Clock for PageClock {
    fn now(&self) -> Duration {
        let millis = web_sys::window()
            .and_then(|window| window.performance())
            .map_or(0.0, |performance| performance.now());
        Duration::try_from_secs_f64(millis / 1_000.0).unwrap_or_default()
    }
}

/// The one `setTimeout` the announcer needs, and the function it calls.
struct Timer {
    handle: Cell<Option<i32>>,
    tick: Closure<dyn FnMut()>,
}

thread_local! {
    static ANNOUNCER: RefCell<Announcer<PageClock>> = RefCell::new(Announcer::new(PageClock));
    static TIMER: Timer = Timer {
        handle: Cell::new(None),
        tick: Closure::new(tick),
    };
}

/// Tells the screen reader `text`, in the region of `politeness`.
pub(crate) fn say(politeness: Politeness, text: &str) {
    let announcement = Announcement {
        kind: Kind::Event,
        politeness,
        text: text.to_owned(),
    };
    step(|announcer| announcer.announce(announcement));
}

/// A modal dialog opened: polite messages wait until it closes.
pub(crate) fn modal_opened() {
    step(|announcer| {
        announcer.modal_opened();
        Vec::new()
    });
}

/// A modal dialog closed.
pub(crate) fn modal_closed() {
    step(Announcer::modal_closed);
}

/// The timer fired: writes what has come due.
fn tick() {
    TIMER.with(|timer| timer.handle.set(None));
    step(Announcer::poll);
}

/// Runs one step of the announcer, applies its writes to the regions, and
/// sets the timer for the next one. The announcer is not borrowed while the
/// DOM is written.
fn step(run: impl FnOnce(&mut Announcer<PageClock>) -> Vec<Effect>) {
    let (effects, wakeup) = ANNOUNCER.with_borrow_mut(|announcer| {
        let effects = run(announcer);
        (effects, announcer.next_wakeup())
    });
    for effect in effects {
        apply(&effect);
    }
    schedule(wakeup);
}

fn apply(effect: &Effect) {
    let (politeness, text) = match effect {
        Effect::Clear(politeness) => (*politeness, ""),
        Effect::Set(politeness, text) => (*politeness, text.as_str()),
    };
    let id = match politeness {
        Politeness::Polite => POLITE_ID,
        Politeness::Assertive => ASSERTIVE_ID,
    };
    let region = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id));
    if let Some(region) = region {
        region.set_text_content(Some(text));
    }
}

/// Replaces the timer with one that fires in `wakeup`, or with none.
fn schedule(wakeup: Option<Duration>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    TIMER.with(|timer| {
        if let Some(handle) = timer.handle.take() {
            window.clear_timeout_with_handle(handle);
        }
        if let Some(wait) = wakeup {
            // Rounded up, so the timer never fires a fraction early.
            let millis = i32::try_from(wait.as_micros().div_ceil(1_000)).unwrap_or(i32::MAX);
            let handle = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    timer.tick.as_ref().unchecked_ref(),
                    millis,
                )
                .ok();
            timer.handle.set(handle);
        }
    });
}

/// Opens the dialog that stands in for a modal and tells the announcer, so
/// messages wait while it is open. A dialog already open is not counted twice.
pub(crate) fn open_modal(dialog: &web_sys::HtmlDialogElement, counted: RwSignal<bool>) {
    if !dialog.open() && dialog.show_modal().is_ok() {
        counted.set(true);
        modal_opened();
    }
}

/// Tells the announcer the modal is gone, once however it went: the `close`
/// event, which Escape and the Close button both raise, or the page leaving.
pub(crate) fn modal_gone(counted: RwSignal<bool>) {
    if counted.try_get_untracked() == Some(true) {
        counted.set(false);
        modal_closed();
    }
}
