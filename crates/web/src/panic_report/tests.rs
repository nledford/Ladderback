use std::panic::PanicHookInfo;

use leptos::wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_test::wasm_bindgen_test;

use super::*;

/// What the panic's message carries, and the console line never may.
const CANARY: &str = "canary-1411-do-not-print";

// `console.error`, replaced until it is given one line, which is kept for
// `written_line` and not shown.
#[wasm_bindgen(wasm_bindgen = leptos::wasm_bindgen, inline_js = r#"
let written = "";
export function keep_the_next_console_line() {
    const original = console.error;
    console.error = (text) => {
        written = String(text);
        console.error = original;
    };
}
export function written_line() {
    return written;
}
"#)]
extern "C" {
    fn keep_the_next_console_line();
    fn written_line() -> String;
}

/// The hook `main` installs, in the browser bundle: the console line names
/// the panic's file and line, and never its message.
///
/// A wasm panic aborts, so nothing in the test runs after it. The check runs
/// in the panic hook instead, after the hook under test: only a right line
/// hands the panic on to the runner's own hook, which records the message
/// `expected` looks for. A wrong line, or none, leaves the panic unrecorded
/// and the test failed.
#[wasm_bindgen_test]
#[should_panic(expected = "the repository password is")]
fn a_panic_is_written_to_the_console_by_its_place_never_its_message() {
    let runner = std::panic::take_hook();
    install();
    let under_test = std::panic::take_hook();
    // The `panic!` is the last line of this function.
    let place = format!(" at {}:{}:", file!(), line!() + 9);
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        keep_the_next_console_line();
        under_test(info);
        let line = written_line();
        if line.contains(&place) && !line.contains(CANARY) {
            runner(info);
        }
    }));
    panic!("the repository password is {CANARY}");
}
