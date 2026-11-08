//! What the worker prints when it panics.
//!
//! The default hook writes the panic message to stderr as it is. A panic
//! message can quote anything, a repository string with its credentials
//! included, so the worker's own hook redacts it first (invariant I4).

use std::any::Any;
use std::io::{self, Write};
use std::panic::Location;

use super::events::Events;

/// Replaces the panic hook: every panic is written to stderr with the
/// secrets of the job removed. Stderr is the daemon's, so it matters.
pub(crate) fn install(events: Events) {
    std::panic::set_hook(Box::new(move |info| {
        let text = report(info.payload(), info.location(), &events);
        // A worker that can't write to stderr has nothing better to do.
        let _ = writeln!(io::stderr(), "{text}");
    }));
}

/// The one line a panic is reported as.
fn report(payload: &(dyn Any + Send), location: Option<&Location<'_>>, events: &Events) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the panic carries no text".to_owned());
    let place = location.map_or_else(String::new, |place| {
        format!(" at {}:{}:{}", place.file(), place.line(), place.column())
    });
    events.redact_text(&format!("steading worker panicked{place}: {message}"))
}

#[cfg(test)]
mod tests {
    use domain::secrets::{Redactor, SecretValue};

    use super::*;
    use crate::worker::events::testing::capture;

    fn events_that_know(secret: &str) -> Events {
        let (events, _capture) = capture();
        events.redact_with(Redactor::new(&[SecretValue::new(secret)]));
        events
    }

    #[test]
    fn a_panic_message_never_carries_a_secret() {
        let events = events_that_know("hunter2");
        let payloads: [Box<dyn Any + Send>; 2] = [
            Box::new("password hunter2 rejected"),
            Box::new(String::from("password hunter2 rejected")),
        ];
        for payload in &payloads {
            let text = report(payload.as_ref(), Some(Location::caller()), &events);
            assert!(!text.contains("hunter2"), "{text}");
            assert!(text.contains("password *** rejected"), "{text}");
        }
    }

    #[test]
    fn the_place_of_the_panic_is_part_of_the_report() {
        let events = events_that_know("hunter2");
        let text = report(&"boom", Some(Location::caller()), &events);
        assert!(text.contains(file!()), "{text}");
        assert!(text.ends_with(": boom"), "{text}");
    }

    #[test]
    fn a_panic_that_carries_no_text_is_still_reported() {
        let events = events_that_know("hunter2");
        let text = report(&42_u32, None, &events);
        assert!(text.contains("panicked"), "{text}");
    }
}
