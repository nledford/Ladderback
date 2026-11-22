//! What the web UI writes to the browser console when it panics (#1411): one
//! fixed line with the place, never the panic's message.
//!
//! A message can quote anything a value held, and in the browser that is what
//! the person typed: a repository password, the admin password, a pairing
//! token. The console is on their own machine, yet it is what they paste into a
//! bug report, so the message is left out in every build, the `gallery` one
//! included.

use std::panic::Location;

use leptos::wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(wasm_bindgen = leptos::wasm_bindgen)]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(line: &str);
}

/// Replaces the panic hook with one that writes where a panic happened to the
/// console, and leaves its message out.
pub(crate) fn install() {
    std::panic::set_hook(Box::new(|info| console_error(&line(info.location()))));
}

/// The one line a panic is reported as.
fn line(location: Option<&Location<'_>>) -> String {
    let place = location.map_or_default(|place| {
        format!(" at {}:{}:{}", place.file(), place.line(), place.column())
    });
    format!(
        "Ladderback's web UI panicked{place}; its message is left out, as it can quote a secret"
    )
}

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests;
