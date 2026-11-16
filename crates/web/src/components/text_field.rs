//! The text field (components §4.5). [`rules`] decides what each kind asks
//! of the browser and what a screen reader meets, host-tested.

pub(crate) mod rules;

pub use rules::{Access, Kind};
