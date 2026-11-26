use super::*;

// The hook itself, in the browser bundle: wasm32 only, run by `just test-wasm`.
#[cfg(target_arch = "wasm32")]
mod browser;

/// The line names the panic's file, line and column, and says why its
/// message is missing.
#[test]
fn a_panic_is_reported_by_its_place() {
    let place = Location::caller();
    let line = line(Some(place));
    let named = format!(
        "Ladderback's web UI panicked at {}:{}:{};",
        place.file(),
        place.line(),
        place.column()
    );
    assert!(line.starts_with(&named), "{line}");
    assert!(line.ends_with("as it can quote a secret"), "{line}");
}

/// A panic with no place is still reported, without one.
#[test]
fn a_panic_with_no_place_is_reported_all_the_same() {
    let line = line(None);
    assert!(line.starts_with("Ladderback's web UI panicked;"), "{line}");
    assert!(line.ends_with("as it can quote a secret"), "{line}");
}
