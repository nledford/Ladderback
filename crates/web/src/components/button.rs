//! The button (components §4.5): a native `<button>` drawn from the shared
//! vocabulary's tokens. [`rules`] decides how it is drawn, what a screen
//! reader meets and whether it takes a press, host-tested.

// Decided on the host; only its tests use it yet.
#[cfg_attr(not(test), expect(dead_code, reason = "only the tests use it yet"))]
pub(crate) mod rules;
